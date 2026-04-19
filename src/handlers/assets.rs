//! Asset handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use pep::oidc::types::JwtClaims;
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::AppState;
use crate::error::Result;
use crate::models::{
    AddTagRequest, Asset, CreateAssetRequest, PaginatedResponse, Tag,
    UpdateAssetRequest, UpdateAuthContextRequest,
};
use crate::service::AssetService;

fn default_limit() -> i64 {
    20
}

fn default_sort_by() -> String {
    "updated_at".to_string()
}

fn default_order() -> String {
    "desc".to_string()
}

/// Query parameters for listing assets
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flatten breaks numeric deserialize)
#[derive(Debug, Deserialize)]
pub struct ListAssetsParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
    /// Filter by asset type tag value (e.g., "document", "concept", "idea")
    pub asset_type: Option<String>,
    /// Sort field: "created_at" or "updated_at" (default: "updated_at")
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    /// Sort order: "asc" or "desc" (default: "desc")
    #[serde(default = "default_order")]
    pub order: String,
}

/// Create a new asset
pub async fn create_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::create(state.services().db(), request, user_id).await?;
    Ok(Json(asset))
}

/// Get asset by ID
pub async fn get_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<Asset>> {
    let asset = AssetService::get(state.services().db(), &id).await?;

    // Cedar enforcement: skip if no authorizer or asset has no auth_context
    if let Some(authorizer) = state.authorizer() {
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "View", &asset)?;
        }
    }

    Ok(Json(asset))
}

/// Update an asset
pub async fn update_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    // Cedar enforcement on the current state of the asset
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services().db(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Edit", &asset)?;
        }
    }

    let asset = AssetService::update(state.services().db(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Delete an asset
pub async fn delete_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services().db(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Delete", &asset)?;
        }
    }

    AssetService::delete(state.services().db(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// List assets
pub async fn list_assets(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Query(params): Query<ListAssetsParams>,
) -> Result<Json<PaginatedResponse<Asset>>> {
    let (assets, next_cursor) = AssetService::list(
        state.services().db(),
        params.limit,
        params.cursor.as_deref(),
        params.asset_type.as_deref(),
        &params.sort_by,
        &params.order,
    )
    .await?;

    // Cedar enforcement: filter out assets the user cannot view
    let filtered = if let Some(authorizer) = state.authorizer() {
        let claims = extract_claims(&user);
        crate::cedar::enforcement::filter_by_permission(authorizer, &claims, "View", assets)
    } else {
        assets
    };

    Ok(Json(PaginatedResponse {
        data: filtered,
        next_cursor,
        total: None,
    }))
}

/// Add a tag to an asset
pub async fn add_tag(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AddTagRequest>,
) -> Result<Json<Tag>> {
    let user_id = &user.user_id;

    // Cedar enforcement for Tag action
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services().db(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Tag", &asset)?;
        }
    }

    let tag = AssetService::add_tag(state.services().db(), &id, request, user_id).await?;
    Ok(Json(tag))
}

/// Remove a tag from an asset
pub async fn remove_tag(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement for Tag action
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services().db(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Tag", &asset)?;
        }
    }

    AssetService::remove_tag(state.services().db(), &id, &tag_id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Update the authorization context of an asset
pub async fn update_auth_context(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAuthContextRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::update_auth_context(state.services().db(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Extract JwtClaims from the request extensions (inserted by AuthLayer).
///
/// Falls back to a default admin claims when running in dev mode (no real JWT).
fn extract_claims(user: &AuthenticatedUser) -> JwtClaims {
    let mut extra = std::collections::HashMap::new();
    extra.insert("role".to_string(), serde_json::Value::String("admin".to_string()));

    JwtClaims {
        sub: user.user_id.clone(),
        iss: "pdt".to_string(),
        aud: None,
        exp: i64::MAX,
        iat: None,
        email: user.email.clone(),
        name: None,
        preferred_username: user.username.clone(),
        extra,
    }
}
