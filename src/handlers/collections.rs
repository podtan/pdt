//! Collection handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};

use crate::auth::AuthenticatedUser;
use crate::error::Result;
use crate::models::{
    AddAssetRequest, Collection, CreateCollectionRequest, PaginatedResponse, PaginationParams,
    UpdateCollectionRequest,
};
use crate::service::{CollectionService, Services};

/// Create a new collection
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
pub async fn get_collection(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<Collection>> {
    let collection = CollectionService::get(services.db(), &id).await?;
    Ok(Json(collection))
}

/// Update a collection
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
pub async fn delete_collection(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    CollectionService::delete(services.db(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// List collections
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
pub async fn remove_asset(
    State(services): State<Services>,
    user: AuthenticatedUser,
    Path((id, asset_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    CollectionService::remove_asset(services.db(), &id, &asset_id, user_id).await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}
