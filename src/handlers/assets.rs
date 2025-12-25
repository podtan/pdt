//! Asset handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::error::Result;
use crate::models::{
    AddTagRequest, Asset, CreateAssetRequest, PaginatedResponse, Tag, UpdateAssetRequest,
};
use crate::service::{AssetService, Services};

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
    State(services): State<Services>,
    user: AuthenticatedUser,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::create(services.db(), request, user_id).await?;
    Ok(Json(asset))
}

/// Get asset by ID
pub async fn get_asset(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<Asset>> {
    let asset = AssetService::get(services.db(), &id).await?;
    Ok(Json(asset))
}

/// Update an asset
pub async fn update_asset(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::update(services.db(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Delete an asset
pub async fn delete_asset(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    AssetService::delete(services.db(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// List assets
pub async fn list_assets(
    State(services): State<Services>,
    Query(params): Query<ListAssetsParams>,
) -> Result<Json<PaginatedResponse<Asset>>> {
    let (assets, next_cursor) = AssetService::list(
        services.db(),
        params.limit,
        params.cursor.as_deref(),
        params.asset_type.as_deref(),
        &params.sort_by,
        &params.order,
    )
    .await?;

    Ok(Json(PaginatedResponse {
        data: assets,
        next_cursor,
        total: None,
    }))
}

/// Add a tag to an asset
pub async fn add_tag(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AddTagRequest>,
) -> Result<Json<Tag>> {
    let user_id = &user.user_id;

    let tag = AssetService::add_tag(services.db(), &id, request, user_id).await?;
    Ok(Json(tag))
}

/// Remove a tag from an asset
pub async fn remove_tag(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    AssetService::remove_tag(services.db(), &id, &tag_id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
