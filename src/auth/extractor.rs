use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use pep::oidc::types::JwtClaims;
use serde_json::Value;

pub struct AuthenticatedUser {
    pub user_id: String,
    pub username: Option<String>,
    pub email: Option<String>,
    /// The full `extra` map from the original JWT claims (role, groups, etc.).
    /// This is `None` only when the AuthenticatedUser is constructed in test code
    /// without JWT claims; in that case callers should fall back to sensible defaults.
    pub claims_extra: Option<std::collections::HashMap<String, Value>>,
}

impl AuthenticatedUser {
    /// Build a `JwtClaims` suitable for Cedar evaluation from this user.
    ///
    /// Propagates `role` and `groups` from the original JWT `extra` map.
    /// When `claims_extra` is `None` (e.g. unit-test construction), falls back
    /// to `role = "viewer"` — a safe, least-privilege default.
    pub fn to_cedar_claims(&self) -> JwtClaims {
        let mut extra = std::collections::HashMap::new();

        match &self.claims_extra {
            Some(orig) => {
                // Propagate role if present
                if let Some(role) = orig.get("role") {
                    extra.insert("role".to_string(), role.clone());
                } else {
                    extra.insert(
                        "role".to_string(),
                        Value::String("viewer".to_string()),
                    );
                }
                // Propagate groups if present
                if let Some(groups) = orig.get("groups") {
                    extra.insert("groups".to_string(), groups.clone());
                }
                // Carry over any other fields (e.g. custom attributes)
                for (k, v) in orig {
                    if k != "role" && k != "groups" {
                        extra.insert(k.clone(), v.clone());
                    }
                }
            }
            None => {
                // No original claims — assume least privilege
                extra.insert(
                    "role".to_string(),
                    Value::String("viewer".to_string()),
                );
            }
        }

        JwtClaims {
            sub: self.user_id.clone(),
            iss: "pdt".to_string(),
            aud: None,
            exp: i64::MAX,
            iat: None,
            email: self.email.clone(),
            name: None,
            preferred_username: self.username.clone(),
            extra,
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let claims = parts
            .extensions
            .get::<JwtClaims>()
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let extra = Some(claims.extra.clone());

        Ok(AuthenticatedUser {
            user_id: claims.sub.clone(),
            username: claims.preferred_username.clone(),
            email: claims.email.clone(),
            claims_extra: extra,
        })
    }
}
