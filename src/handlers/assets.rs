//! Asset handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::Result;
use crate::models::{
    AddTagRequest, Asset, CreateAssetRequest, PaginatedResponse, PaginationParams, Tag,
    UpdateAssetRequest,
};
use crate::service::{AssetService, Services};

/// Query parameters for listing assets
#[derive(Debug, Deserialize)]
pub struct ListAssetsParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    /// Filter by asset type tag value (e.g., "document", "concept", "idea")
    pub asset_type: Option<String>,
}

/// Create a new asset
pub async fn create_asset(
    State(services): State<Services>,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<Asset>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

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
    Path(id): Path<String>,
    Json(request): Json<UpdateAssetRequest>,
) -> Result<Json<Asset>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

    let asset = AssetService::update(services.db(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Delete an asset
pub async fn delete_asset(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

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
        params.pagination.limit,
        params.pagination.cursor.as_deref(),
        params.asset_type.as_deref(),
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
    Path(id): Path<String>,
    Json(request): Json<AddTagRequest>,
) -> Result<Json<Tag>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

    let tag = AssetService::add_tag(services.db(), &id, request, user_id).await?;
    Ok(Json(tag))
}

/// Remove a tag from an asset
pub async fn remove_tag(
    State(services): State<Services>,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

    AssetService::remove_tag(services.db(), &id, &tag_id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
