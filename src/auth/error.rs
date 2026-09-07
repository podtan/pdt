use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Missing authentication token")]
    MissingToken,
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    #[error("Invalid claims: {0}")]
    InvalidClaims(String),
    /// Claims enrichment against the IdP userinfo endpoint failed — the
    /// presented token cannot be resolved to a principal with authorization
    /// attributes. This is an AUTHENTICATION failure and must answer 401
    /// with an explicit retry signal, never 403-with-defaults.
    ///
    /// b82a1925 guard-1 port (class ruling 2026-09-07; fame reference
    /// 219717b): the previous behavior logged the enrichment error as
    /// "non-fatal" and let a role-less principal fall through to a
    /// `role="viewer"` default — a silent downgrade that default-denied as
    /// a lying 403 and hid recurring token age-out windows.
    ///
    /// Wire contract: 401 + `WWW-Authenticate: Bearer error="invalid_token"`
    /// + JSON `{"error":"token_expired","retryable":true,"detail":…}`.
    #[error("Token enrichment failed: {0}")]
    EnrichmentFailed(String),
    #[error("PEP error: {0}")]
    PepError(#[from] pep::error::PepError),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl AuthError {
    /// `WWW-Authenticate` challenge carried by every 401 this service emits.
    fn www_authenticate(reason: &str) -> String {
        format!(
            "Bearer error=\"invalid_token\", error_description=\"{}\"",
            reason.replace('"', "'")
        )
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::EnrichmentFailed(reason) => {
                let body = serde_json::json!({
                    // Uniform machine-readable contract (b82a1925 S1/S3):
                    // any 401 is retryable-exactly-once; 403 is NEVER
                    // retryable. `detail` preserves the cause for operators.
                    "error": "token_expired",
                    "retryable": true,
                    "detail": reason,
                });
                let mut resp = (StatusCode::UNAUTHORIZED, Json(body)).into_response();
                resp.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    Self::www_authenticate("token enrichment failed")
                        .parse()
                        .expect("static header value"),
                );
                resp
            }
            AuthError::MissingToken | AuthError::InvalidToken(_) => {
                let mut resp = (StatusCode::UNAUTHORIZED, self.to_string()).into_response();
                resp.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    Self::www_authenticate("missing or invalid bearer token")
                        .parse()
                        .expect("static header value"),
                );
                resp
            }
            AuthError::InvalidClaims(_) => {
                (StatusCode::FORBIDDEN, self.to_string()).into_response()
            }
            AuthError::PepError(ref e) => {
                (e.status_code(), self.to_string()).into_response()
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// b82a1925 guard-1 wire contract: enrichment failure answers
    /// 401 + WWW-Authenticate + machine-readable retryable body.
    #[test]
    fn enrichment_failure_is_401_with_retry_signal() {
        let resp = AuthError::EnrichmentFailed(
            "Userinfo endpoint returned error: {\"error\":\"invalid_token\"} status=400"
                .to_string(),
        )
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .expect("WWW-Authenticate required")
            .to_str()
            .unwrap();
        assert!(www.starts_with("Bearer error=\"invalid_token\""), "{www}");
        let bytes = futures::executor::block_on(axum::body::to_bytes(resp.into_body(), 4096))
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON body");
        assert_eq!(v["error"], "token_expired");
        assert_eq!(v["retryable"], true);
    }

    /// Enrichment failure must never be 403 or 500 (b82a1925 class).
    #[test]
    fn enrichment_failure_is_never_403_or_500() {
        let resp = AuthError::EnrichmentFailed("x".to_string()).into_response();
        assert_ne!(resp.status(), StatusCode::FORBIDDEN);
        assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
