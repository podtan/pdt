use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use pep::oidc::types::JwtClaims;

pub struct AuthenticatedUser {
    pub user_id: String,
    pub username: Option<String>,
    pub email: Option<String>,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let claims = parts
            .extensions
            .get::<JwtClaims>()
            .ok_or(StatusCode::UNAUTHORIZED)?;

        Ok(AuthenticatedUser {
            user_id: claims.sub.clone(),
            username: claims.preferred_username.clone(),
            email: claims.email.clone(),
        })
    }
}
