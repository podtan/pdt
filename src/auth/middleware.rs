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
                return inner.call(req).await;
            }

            let token = match extract_bearer_token(req.headers()) {
                Some(t) => t,
                None => {
                    if config.dev_mode {
                        inject_mock_claims(&mut req);
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
                Ok(claims) => {
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
        .and_then(|s| {
            if s.starts_with("Bearer ") {
                Some(s[7..].to_string())
            } else {
                None
            }
        })
}

fn inject_mock_claims(req: &mut Request) {
    let mut extra = std::collections::HashMap::new();
    extra.insert(
        "role".to_string(),
        serde_json::Value::String("admin".to_string()),
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
