//! Cedar enforcement logic for PDT
//!
//! This module provides the core enforcement functions that decide whether
//! Cedar authorization should be applied to a given asset, and evaluates
//! authorization requests against Cedar policies.
//!
//! ## Grandfathering Strategy
//!
//! Assets without `auth_context` (400+ existing docs) bypass Cedar enforcement
//! and default to open access. Only assets with `auth_context` go through Cedar.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use cedar_policy::{Context, Entities, Request};
use pep::cedar::CedarAuthorizer;
use pep::oidc::types::JwtClaims;

use crate::auth::AuthenticatedUser;
use crate::error::ApiError;
use crate::models::Asset;
use crate::service::Services;
use crate::tenant::TenantPoolManager;

/// The raw application state type passed to `Router::with_state()`.
/// This is a tuple so axum can extract different parts via `FromRef`.
pub type AppRawState = (TenantPoolManager, Option<Arc<CedarAuthorizer>>);

/// Application state shared across all handlers.
/// Implements `FromRef` so axum can extract it from the tuple state.
///
/// NOTE: This resolves to the GLOBAL services. For tenant-aware services,
/// use the `TenantState` extractor instead.
#[derive(Clone)]
pub struct AppState {
    pub services: Services,
    pub authorizer: Option<Arc<CedarAuthorizer>>,
}

impl AppState {
    /// Get a reference to the services
    pub fn services(&self) -> &Services {
        &self.services
    }

    /// Get a reference to the Cedar authorizer (if enabled)
    pub fn authorizer(&self) -> Option<&CedarAuthorizer> {
        self.authorizer.as_ref().map(|v| v.as_ref())
    }
}

/// Tenant-aware state extractor.
///
/// Reads the `X-Instance-Id` header from the request and resolves the correct
/// `Services` (per-instance SQLite or global). Use this in handlers that need
/// tenant routing:
///
/// ```rust,ignore
/// pub async fn create_asset(
///     tx: TenantState,
///     user: AuthenticatedUser,
///     Json(request): Json<CreateAssetRequest>,
/// ) -> Result<Json<Asset>> {
///     let services = &tx.services;
///     // ...
/// }
/// ```
#[derive(Clone)]
pub struct TenantState {
    pub services: Services,
    pub authorizer: Option<Arc<CedarAuthorizer>>,
    pub instance_id: Option<String>,
}

impl TenantState {
    pub fn services(&self) -> &Services {
        &self.services
    }

    pub fn authorizer(&self) -> Option<&CedarAuthorizer> {
        self.authorizer.as_ref().map(|v| v.as_ref())
    }

    pub fn instance_id(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }
}

/// Implement FromRequestParts on the concrete AppRawState type.
impl<S> FromRequestParts<S> for TenantState
where
    S: Send + Sync,
    TenantPoolManager: FromRef<S>,
    AuthorizerRef: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        use axum::http::StatusCode;

        // Extract X-Instance-Id header
        let instance_id = parts
            .headers
            .get("X-Instance-Id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let manager = TenantPoolManager::from_ref(state);
        let auth_ref = AuthorizerRef::from_ref(state);

        let services = manager
            .get_services(instance_id.as_deref())
            .await
            .map_err(|e| {
                tracing::error!("Failed to resolve tenant services: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to resolve tenant: {}", e),
                )
                    .into_response()
            })?;

        Ok(TenantState {
            services,
            authorizer: auth_ref.0,
            instance_id,
        })
    }
}

/// Wrapper for the optional Cedar authorizer so we can implement `FromRef`.
#[derive(Clone)]
pub struct AuthorizerRef(pub Option<Arc<CedarAuthorizer>>);

impl AuthorizerRef {
    pub fn as_ref(&self) -> Option<&CedarAuthorizer> {
        self.0.as_ref().map(|v| v.as_ref())
    }
}

/// Implement FromRef for backward-compatible AppState (global services).
impl FromRef<AppRawState> for AppState {
    fn from_ref(state: &AppRawState) -> Self {
        AppState {
            services: state.0.global().clone(),
            authorizer: state.1.clone(),
        }
    }
}

impl FromRef<AppRawState> for TenantPoolManager {
    fn from_ref(state: &AppRawState) -> Self {
        state.0.clone()
    }
}

impl FromRef<AppRawState> for AuthorizerRef {
    fn from_ref(state: &AppRawState) -> Self {
        AuthorizerRef(state.1.clone())
    }
}

/// Build JwtClaims from AuthenticatedUser for Cedar evaluation.
/// This is used by handlers that need claims for Cedar enforcement
/// (e.g., search, list filtering) but use AuthenticatedUser for extraction.
///
/// Propagates the original `extra` map (role, groups, etc.) from the real JWT.
pub fn extract_claims_for_cedar(user: &AuthenticatedUser) -> JwtClaims {
    user.to_cedar_claims()
}

/// Check whether Cedar authorization should be enforced for this asset.
///
/// Assets with `auth_context` present are Cedar-enforced.
/// Assets without `auth_context` (grandfathered) bypass enforcement entirely.
pub fn should_enforce_cedar(asset: &Asset) -> bool {
    asset.auth_context.is_some()
}

/// Check Cedar permission — returns 403 if denied.
///
/// This is the main enforcement function used by handlers.
/// For grandfathered assets (no auth_context), always allows.
/// For Cedar-enforced assets, evaluates the policy.
pub fn check_permission(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    asset: &Asset,
) -> Result<(), ApiError> {
    if !should_enforce_cedar(asset) {
        return Ok(()); // Grandfathered — open access
    }

    let allowed = evaluate_permission(authorizer, claims, action, asset)?;
    if !allowed {
        tracing::warn!(
            "Cedar denied {} on asset {} for user {}",
            action,
            asset.id,
            claims.sub
        );
        return Err(ApiError::Forbidden(format!(
            "Access denied: you do not have '{}' permission on asset '{}'",
            action, asset.id
        )));
    }
    Ok(())
}

/// Evaluate a Cedar authorization request for a specific asset.
///
/// Returns `Ok(true)` if the action is allowed, `Ok(false)` if denied.
/// Returns `Err` only if there is an entity-building error.
pub fn evaluate_permission(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    asset: &Asset,
) -> Result<bool, ApiError> {
    let principal = pep::cedar::build_principal_uid(claims).map_err(|e| {
        tracing::warn!("Cedar principal build failed: {}", e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    let action_uid = pep::cedar::build_action_uid(action).map_err(|e| {
        tracing::warn!("Cedar action build failed: {}", e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    let resource = crate::cedar::entity::build_asset_resource_uid(asset).map_err(|e| {
        tracing::warn!("Cedar resource build failed for asset {}: {}", asset.id, e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    // Build entities so Cedar can evaluate attributes
    let asset_entity = crate::cedar::entity::asset_to_cedar_entity(asset);
    let principal_entity = crate::cedar::entity::user_to_cedar_principal(claims);

    let entities = Entities::from_entities(
        [asset_entity, principal_entity],
        None,
    )
    .map_err(|e| {
        tracing::warn!("Cedar entities build failed: {}", e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    let request = Request::new(principal, action_uid, resource, Context::empty(), None)
        .map_err(|e| {
            tracing::warn!("Cedar request build failed: {}", e);
            ApiError::Forbidden(format!("Authorization error: {}", e))
        })?;

    let response = authorizer.is_allowed_with_entities(&request, &entities);

    Ok(response.allowed())
}

/// Check if the current user can perform an action on an asset (by ID, fetching from DB).
///
/// Used by relation handlers that need to verify permission on related assets
/// but don't already have the full asset loaded.
pub async fn check_asset_permission_by_id(
    services: &Services,
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    asset_id: &str,
) -> Result<(), ApiError> {
    let asset = services.assets().get_by_id(asset_id).await?;
    if let Some(authz) = check_permission(authorizer, claims, action, &asset).err() {
        return Err(authz);
    }
    Ok(())
}

/// Check if user can perform an action involving two assets (e.g., creating a relation).
///
/// Both the source and target assets must be readable by the user.
pub async fn check_relation_permission(
    services: &Services,
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    from_asset_id: &str,
    to_asset_id: &str,
) -> Result<(), ApiError> {
    check_asset_permission_by_id(services, authorizer, claims, "View", from_asset_id).await?;
    check_asset_permission_by_id(services, authorizer, claims, action, to_asset_id).await?;
    Ok(())
}

/// Evaluate Cedar authorization for a list of assets, filtering out denied ones.
///
/// Returns only the assets that the given user is allowed to perform the action on.
/// Grandfathered assets (no `auth_context`) are always included.
pub fn filter_by_permission(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    assets: Vec<Asset>,
) -> Vec<Asset> {
    assets
        .into_iter()
        .filter(|asset| {
            if !should_enforce_cedar(asset) {
                return true; // Grandfathered — always include
            }
            match evaluate_permission(authorizer, claims, action, asset) {
                Ok(allowed) => allowed,
                Err(e) => {
                    tracing::warn!(
                        "Cedar evaluation error for asset {}: {}, denying access",
                        asset.id,
                        e
                    );
                    false
                }
            }
        })
        .collect()
}
