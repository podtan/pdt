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
                let mut resp = (e.status_code(), self.to_string()).into_response();
                // Contract (b82a1925 guard-1): EVERY 401 this service emits
                // carries the challenge. pep-sourced JWT validation failures
                // (garbage/malformed bearer → "Invalid JWT header") surface
                // here as PepError(401) and were emitted bare — found in the
                // v0.3.4 prod close-smoke (Paydar, 2026-09-08).
                if resp.status() == StatusCode::UNAUTHORIZED {
                    resp.headers_mut().insert(
                        header::WWW_AUTHENTICATE,
                        Self::www_authenticate("invalid bearer token")
                            .parse()
                            .expect("static header value"),
                    );
                }
                resp
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

// NOTE: appended tests — see also the guard-1 pins above.
// Contract completion (v0.3.4 close-smoke finding, Paydar 2026-09-08):
// EVERY 401 carries the challenge, including pep-sourced ones.
#[cfg(test)]
mod pep_401_challenge_tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn pep_sourced_401_carries_challenge() {
        let resp = AuthError::PepError(pep::error::PepError::JwtValidation(
            "Invalid JWT header".to_string(),
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .expect("WWW-Authenticate required on pep-sourced 401")
            .to_str()
            .unwrap();
        assert!(www.starts_with("Bearer error=\"invalid_token\""), "{www}");
    }

    #[test]
    fn pep_non_401_errors_do_not_carry_challenge() {
        let resp = AuthError::PepError(pep::error::PepError::AuthorizationFailed(
            "deny".to_string(),
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(
            resp.headers().get(header::WWW_AUTHENTICATE).is_none(),
            "challenge belongs on authn 401s only — never on 403"
        );
    }
}
