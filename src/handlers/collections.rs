//! Collection handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use utoipa::IntoParams;

use crate::auth::AuthenticatedUser;
use crate::error::Result;
use crate::models::{
    AddAssetRequest, Collection, CreateCollectionRequest, PaginatedResponse, PaginationParams,
    UpdateCollectionRequest,
};
use crate::service::{CollectionService, Services};

/// Query parameters for collection list pagination
#[derive(Debug, serde::Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListCollectionsParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

/// Create a new collection
#[utoipa::path(
    post,
    path = "/api/collections",
    request_body = CreateCollectionRequest,
    responses(
        (status = 200, description = "Collection created successfully", body = Collection),
    ),
    tag = "collections",
)]
pub async fn create_collection(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Json(request): Json<CreateCollectionRequest>,
) -> Result<Json<Collection>> {
    let user_id = &user.user_id;

    let collection = CollectionService::create(services.db(), request, user_id).await?;
    Ok(Json(collection))
}

/// Get collection by ID
#[utoipa::path(
    get,
    path = "/api/collections/{id}",
    params(
        ("id" = String, Path, description = "Collection ID"),
    ),
    responses(
        (status = 200, description = "Collection found", body = Collection),
        (status = 404, description = "Collection not found"),
    ),
    tag = "collections",
)]
pub async fn get_collection(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<Collection>> {
    let collection = CollectionService::get(services.db(), &id).await?;
    Ok(Json(collection))
}

/// Update a collection
#[utoipa::path(
    put,
    path = "/api/collections/{id}",
    params(
        ("id" = String, Path, description = "Collection ID"),
    ),
    request_body = UpdateCollectionRequest,
    responses(
        (status = 200, description = "Collection updated successfully", body = Collection),
        (status = 404, description = "Collection not found"),
    ),
    tag = "collections",
)]
pub async fn update_collection(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateCollectionRequest>,
) -> Result<Json<Collection>> {
    let user_id = &user.user_id;

    let collection = CollectionService::update(services.db(), &id, request, user_id).await?;
    Ok(Json(collection))
}

/// Delete a collection
#[utoipa::path(
    delete,
    path = "/api/collections/{id}",
    params(
        ("id" = String, Path, description = "Collection ID"),
    ),
    responses(
        (status = 200, description = "Collection deleted successfully"),
        (status = 404, description = "Collection not found"),
    ),
    tag = "collections",
)]
pub async fn delete_collection(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    CollectionService::delete(services.db(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// List collections with pagination
#[utoipa::path(
    get,
    path = "/api/collections",
    params(ListCollectionsParams),
    responses(
        (status = 200, description = "List of collections", body = PaginatedResponse<Collection>),
    ),
    tag = "collections",
)]
pub async fn list_collections(
    State(services): State<Services>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<Collection>>> {
    let (collections, next_cursor) =
        CollectionService::list(services.db(), params.limit, params.cursor.as_deref()).await?;

    Ok(Json(PaginatedResponse {
        data: collections,
        next_cursor,
        total: None,
    }))
}

/// Add an asset to a collection
#[utoipa::path(
    post,
    path = "/api/collections/{id}/assets",
    params(
        ("id" = String, Path, description = "Collection ID"),
    ),
    request_body = AddAssetRequest,
    responses(
        (status = 200, description = "Asset added to collection successfully"),
        (status = 404, description = "Collection or asset not found"),
    ),
    tag = "collections",
)]
pub async fn add_asset(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AddAssetRequest>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    CollectionService::add_asset(services.db(), &id, &request.asset_id, user_id).await?;
    Ok(Json(serde_json::json!({ "added": true })))
}

/// Remove an asset from a collection
#[utoipa::path(
    delete,
    path = "/api/collections/{id}/assets/{asset_id}",
    params(
        ("id" = String, Path, description = "Collection ID"),
        ("asset_id" = String, Path, description = "Asset ID to remove"),
    ),
    responses(
        (status = 200, description = "Asset removed from collection successfully"),
        (status = 404, description = "Collection or asset not found"),
    ),
    tag = "collections",
)]
pub async fn remove_asset(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path((id, asset_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    CollectionService::remove_asset(services.db(), &id, &asset_id, user_id).await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}
