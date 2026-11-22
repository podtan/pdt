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
    /// Authorization material (role) is present — Cedar evaluation is honest
    /// whatever it decides. (Claim GROUPS retired as authorization material
    /// by the pep 0.5.21 authz drain — they can no longer certify
    /// enrichment.)
    Enriched,
    /// Enrichment produced NO role. On this platform's Kanidm IdP access
    /// tokens carry no identity claims (tocpi 0.7.2 finding), so role can
    /// only arrive via the userinfo merge; a role-less principal after an
    /// enrichment attempt means that merge failed or landed empty (pep
    /// treats userinfo HTTP errors as non-fatal Ok — the 2026-10-07 payam
    /// F1 window). Filtered reads must not answer 200.
    ///
    /// Classifier re-key per the CPO ruling of 2026-11-22 (fame semantic
    /// cb11a8e0 / payam 8c7305c8): Degraded iff no ROLE landed. The groups
    /// arm retired with the pep 0.5.21 drain — under it no principal
    /// carries claim groups, so a groups disjunct would classify every
    /// principal Degraded. The 365be03f invariant survives byte-for-byte: a
    /// zero-attribute principal never answers 200-empty.
    Degraded,
}

/// Classify claims that have just been through enrichment.
///
/// Re-keyed 2026-11-22 per CPO ruling (cb11a8e0 / 8c7305c8) — see the
/// comment inside for the full rationale and the standing note.
pub fn classify_enrichment(claims: &JwtClaims) -> AuthEnrichment {
    // Degraded iff no `role` landed (CPO ruling 2026-11-22, fame semantic
    // cb11a8e0 / payam 8c7305c8): the pep 0.5.21 authz drain retires claim
    // GROUPS as authorization material, so a landed-groups disjunct would
    // classify every principal Degraded. A landed role makes Cedar's
    // deny/allow decision truthful (it evaluates real data the principal
    // verifiably holds); the lie requires a role-less principal whose
    // filtered reads would answer 200-empty. The 365be03f invariant is
    // byte-for-byte: a zero-attribute principal (no role, no groups anywhere
    // -- the handler gates also require empty PDT-native memberships) never
    // answers 200-empty.
    //
    // STANDING NOTE (ruling guard 2): if pep ever drains ROLES too, this
    // classifier needs a REDESIGN (a new source of truth for "material
    // landed"), never a silent pass-through.
    if claims.extra.contains_key("role") {
        AuthEnrichment::Enriched
    } else {
        AuthEnrichment::Degraded
    }
}

/// Issuer equality with trailing-slash normalization (mirrors pep's private
/// `issuer_matches` — string-level comparison only).
fn issuer_matches(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Enrichment-issuer selection (issue 895cbcea cut-over leg — paydar's
/// finding 2026-11-01, journaled 04:18:01Z): `validate_jwt_with_options` can
/// return claims minted by a TRUSTED non-configured issuer (the per-token
/// fallback validated the token against its own issuer). Enrichment against
/// the CONFIGURED issuer then hits the wrong IdP with a foreign token →
/// 400 invalid_request → fail-closed 401: podai pdt-api tokens validate
/// cryptographically but stop at enrichment.
///
/// Returns `(issuer, userinfo_override)` for the enrichment call:
/// - Primary path (validated iss == configured issuer, modulo trailing
///   slash): byte-identical — the configured issuer + the operator's
///   `userinfo_url` override as-is.
/// - Fallback path: enrichment follows the VALIDATED issuer (claims.iss,
///   enforced by the fallback to the token's own issuer). The
///   `userinfo_url` override pins the CONFIGURED issuer's endpoint — it
///   must not hijack a fallback-validated request, so it is dropped here.
/// - Empty iss (never post-validation, guarded anyway): configured issuer.
pub(crate) fn enrichment_target(
    config: &AuthConfig,
    claims: &JwtClaims,
) -> (String, Option<String>) {
    let validated = claims.iss.trim();
    if validated.is_empty() || issuer_matches(validated, &config.issuer_url) {
        (config.issuer_url.clone(), config.userinfo_url.clone())
    } else {
        (claims.iss.clone(), None)
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
                    // Enrichment follows the VALIDATED issuer (issue 895cbcea
                    // cut-over leg): after the trusted-issuer fallback
                    // validates a token minted by a non-configured issuer,
                    // enrichment must hit THAT issuer's userinfo — the
                    // configured issuer rejects the foreign token
                    // (400 invalid_request → fail-closed 401). No-op on the
                    // primary path (validation enforced iss == configured).
                    let (enrichment_issuer, userinfo_override) =
                        enrichment_target(&config, &claims);
                    if let Err(e) = client
                        .enrich_claims_with_userinfo(
                            &mut claims,
                            &token,
                            &enrichment_issuer,
                            userinfo_override.as_deref(),
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
                            "Auth: enrichment landed EMPTY (no role after userinfo; claim \
                             groups retired by the pep 0.5.21 drain) \
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
