//! Asset handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use pep::oidc::types::JwtClaims;
use serde::Deserialize;
use utoipa::IntoParams;

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
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
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
#[utoipa::path(
    post,
    path = "/api/assets",
    request_body = CreateAssetRequest,
    responses(
        (status = 200, description = "Asset created successfully", body = Asset),
        (status = 400, description = "Validation error"),
    ),
    tag = "assets",
)]
pub async fn create_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::create(state.services(), request, user_id).await?;
    Ok(Json(asset))
}

/// Get asset by ID
#[utoipa::path(
    get,
    path = "/api/assets/{id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    responses(
        (status = 200, description = "Asset found", body = Asset),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn get_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<Asset>> {
    let asset = AssetService::get(state.services(), &id).await?;

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
#[utoipa::path(
    put,
    path = "/api/assets/{id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    request_body = UpdateAssetRequest,
    responses(
        (status = 200, description = "Asset updated successfully", body = Asset),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn update_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    // Cedar enforcement on the current state of the asset
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Edit", &asset)?;
        }
    }

    let asset = AssetService::update(state.services(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Delete an asset (soft delete)
#[utoipa::path(
    delete,
    path = "/api/assets/{id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    responses(
        (status = 200, description = "Asset deleted successfully"),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn delete_asset(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Delete", &asset)?;
        }
    }

    AssetService::delete(state.services(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// List assets with pagination
#[utoipa::path(
    get,
    path = "/api/assets",
    params(ListAssetsParams),
    responses(
        (status = 200, description = "List of assets", body = PaginatedResponse<Asset>),
    ),
    tag = "assets",
)]
pub async fn list_assets(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Query(params): Query<ListAssetsParams>,
) -> Result<Json<PaginatedResponse<Asset>>> {
    let (assets, next_cursor) = AssetService::list(
        state.services(),
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
#[utoipa::path(
    post,
    path = "/api/assets/{id}/tags",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    request_body = AddTagRequest,
    responses(
        (status = 200, description = "Tag added successfully", body = Tag),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn add_tag(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AddTagRequest>,
) -> Result<Json<Tag>> {
    let user_id = &user.user_id;

    // Cedar enforcement for Tag action
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Tag", &asset)?;
        }
    }

    let tag = AssetService::add_tag(state.services(), &id, request, user_id).await?;
    Ok(Json(tag))
}

/// Remove a tag from an asset
#[utoipa::path(
    delete,
    path = "/api/assets/{id}/tags/{tag_id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
        ("tag_id" = String, Path, description = "Tag ID to remove"),
    ),
    responses(
        (status = 200, description = "Tag removed successfully"),
        (status = 404, description = "Asset or tag not found"),
    ),
    tag = "assets",
)]
pub async fn remove_tag(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement for Tag action
    if let Some(authorizer) = state.authorizer() {
        let asset = AssetService::get(state.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            crate::cedar::enforcement::check_permission(authorizer, &claims, "Tag", &asset)?;
        }
    }

    AssetService::remove_tag(state.services(), &id, &tag_id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Update the authorization context of an asset (Cedar visibility/ownership management)
#[utoipa::path(
    put,
    path = "/api/assets/{id}/auth-context",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    request_body = UpdateAuthContextRequest,
    responses(
        (status = 200, description = "Auth context updated successfully", body = Asset),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn update_auth_context(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAuthContextRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::update_auth_context(state.services(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Extract JwtClaims from the AuthenticatedUser.
///
/// Reconstructs JwtClaims preserving the original `extra` map (role, groups, etc.)
/// that was extracted from the real JWT by the auth middleware.
fn extract_claims(user: &AuthenticatedUser) -> JwtClaims {
    user.to_cedar_claims()
}
