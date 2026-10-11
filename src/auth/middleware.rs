use super::{AuthConfig, AuthError};
use axum::{
    extract::Request,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use futures::future::BoxFuture;
use pep::{
    oidc::types::{JwtClaims, JwtValidationOptions},
    oidc_resource_server::ResourceServerClient,
};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tower::{Layer, Service};

use serde::{Deserialize, Serialize};

/// Outcome of post-validation claims enrichment, carried on the request so
/// visibility-filtered read endpoints can refuse to answer when authorization
/// material is missing (issue 365be03f).
///
/// The lie this prevents: a principal whose enrichment silently failed is
/// evaluated by Cedar as zero-attribute; visibility-filtered search/list then
/// drop every row and answer 200-with-empty — indistinguishable from genuine
/// absence. With this marker, those endpoints answer 503 auth_degraded
/// instead. Zero-rows must mean zero-rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthEnrichment {
    /// Authorization material (groups and/or role) is present — Cedar
    /// evaluation is honest whatever it decides.
    Enriched,
    /// Enrichment produced NO authorization material. On this platform's
    /// Kanidm IdP access tokens carry no identity claims (tocpi 0.7.2
    /// finding), so groups/role can only arrive via the userinfo merge; a
    /// zero-attribute principal after an enrichment attempt means that
    /// merge failed (pep treats userinfo HTTP errors as non-fatal Ok —
    /// the 2026-10-07 payam F1 window). Filtered reads must not answer 200.
    Degraded,
}

/// Classify claims that have just been through enrichment.
///
/// Degraded ⇔ neither `groups` nor `role` landed. Any landed attribute makes
/// Cedar's deny/allow decision truthful (it evaluates real data the principal
/// verifiably holds); the lie requires a fully zero-attribute principal.
pub fn classify_enrichment(claims: &JwtClaims) -> AuthEnrichment {
    if claims.extra.contains_key("groups") || claims.extra.contains_key("role") {
        AuthEnrichment::Enriched
    } else {
        AuthEnrichment::Degraded
    }
}

#[derive(Clone)]
pub struct AuthLayer {
    config: AuthConfig,
    client: Arc<ResourceServerClient>,
}

impl AuthLayer {
    pub fn new(config: AuthConfig, client: ResourceServerClient) -> Self {
        Self {
            config,
            client: Arc::new(client),
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            config: self.config.clone(),
            client: self.client.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    config: AuthConfig,
    client: Arc<ResourceServerClient>,
}

impl<S> Service<Request> for AuthMiddleware<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let config = self.config.clone();
        let client = self.client.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if !config.enabled {
                // If auth is disabled, we inject mock claims if dev_mode is true
                if config.dev_mode {
                    inject_mock_claims(&mut req);
                }
                req.extensions_mut().insert(AuthEnrichment::Enriched);
                return inner.call(req).await;
            }

            let token = match extract_bearer_token(req.headers()) {
                Some(t) => t,
                None => {
                    if config.dev_mode {
                        inject_mock_claims(&mut req);
                        req.extensions_mut().insert(AuthEnrichment::Enriched);
                        return inner.call(req).await;
                    }
                    return Ok(AuthError::MissingToken.into_response());
                }
            };

            let mut options = JwtValidationOptions::default();
            let audience = if let Some(aud) = &config.expected_audience {
                aud.as_str()
            } else {
                options.skip_audience_validation = true;
                ""
            };

            match client
                .validate_jwt_with_options(&token, &config.issuer_url, audience, &options)
                .await
            {
                Ok(mut claims) => {
                    // Adaptive claims enrichment: fill missing groups/role from OIDC userinfo
                    // This makes PDT work with Kanidm (no groups in AT) and Keycloak/Auth0 (groups in AT)
                    //
                    // b82a1925 guard-1 port (class ruling 2026-09-07; fame 219717b):
                    // enrichment failure = AUTHN failure → 401 with retry signal,
                    // journaled at WARN. The previous "non-fatal" swallow let a
                    // role-less principal fall through to the viewer default →
                    // default-deny → a silent 403 that lied about authz.
                    if let Err(e) = client
                        .enrich_claims_with_userinfo(
                            &mut claims,
                            &token,
                            &config.issuer_url,
                            config.userinfo_url.as_deref(),
                        )
                        .await
                    {
                        // pep >= 0.5.8 fails closed: userinfo failures surface as
                        // PepError::AuthDegraded instead of the old Ok-swallow.
                        // Two honest shapes (issue 365be03f contract):
                        //  - retryable degraded (IdP 5xx/429/transport) → 503
                        //    auth_degraded, the marker filtered reads key on;
                        //    same request may succeed on retry.
                        //  - everything else (4xx invalid_token = terminal for
                        //    this token, pep parse failures) → the standing
                        //    b82a1925 401-with-retry contract; the caller's
                        //    reactive refresh cures it in one request.
                        if e.is_auth_degraded() && e.retryable() {
                            tracing::warn!(
                                "Auth: enrichment AUTH-DEGRADED (transient upstream: {}) — \
                                 answering 503 auth_degraded (issue 365be03f)",
                                e
                            );
                            return Ok(crate::error::ApiError::AuthDegraded.into_response());
                        }
                        tracing::warn!(
                            "Auth: userinfo enrichment FAILED — answering 401 with retry \
                             signal (default-role fallback removed, request fails loudly): {}",
                            e
                        );
                        return Ok(AuthError::EnrichmentFailed(format!("{}", e)).into_response());
                    }
                    // Layer 2 (defense in depth): pep >= 0.5.8 fails closed on
                    // userinfo HTTP errors, but a 200 response carrying no
                    // groups/role still returns Ok — classify what actually
                    // landed (issue 365be03f).
                    let enrichment = classify_enrichment(&claims);
                    if enrichment == AuthEnrichment::Degraded {
                        tracing::warn!(
                            "Auth: enrichment landed EMPTY (no groups/role after userinfo) \
                             — marking request AUTH-DEGRADED; visibility-filtered reads \
                             will answer 503 auth_degraded (issue 365be03f)"
                        );
                    }
                    req.extensions_mut().insert(enrichment);
                    req.extensions_mut().insert(claims);
                    inner.call(req).await
                }
                Err(e) => Ok(AuthError::PepError(e).into_response()),
            }
        })
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(|token| token.to_string()))
}

fn inject_mock_claims(req: &mut Request) {
    let mut extra = std::collections::HashMap::new();
    extra.insert(
        "role".to_string(),
        serde_json::Value::String("admin".to_string()),
    );
    extra.insert(
        "groups".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::String("developers".to_string())]),
    );

    let claims = JwtClaims {
        sub: "dev-user-id".to_string(),
        iss: "dev-issuer".to_string(),
        aud: Some("dev-audience".to_string()),
        exp: 9999999999,
        iat: Some(0),
        email: Some("dev@example.com".to_string()),
        name: Some("Dev User".to_string()),
        preferred_username: Some("dev-user".to_string()),
        extra,
    };
    req.extensions_mut().insert(claims);
}
