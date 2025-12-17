//! Relation handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::Result;
use crate::models::{CreateRelationRequest, Relation};
use crate::service::{relation_service::GraphNode, RelationService, Services};

/// Query parameters for graph traversal
#[derive(Debug, Deserialize)]
pub struct TraverseParams {
    #[serde(default = "default_depth")]
    pub depth: u32,
}

fn default_depth() -> u32 {
    3
}

/// Create a new relation
pub async fn create_relation(
    State(services): State<Services>,
    Json(request): Json<CreateRelationRequest>,
) -> Result<Json<Relation>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

    let relation = RelationService::create(services.db(), request, user_id).await?;
    Ok(Json(relation))
}

/// Get relation by ID
pub async fn get_relation(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<Relation>> {
    let relation = RelationService::get(services.db(), &id).await?;
    Ok(Json(relation))
}

/// Delete a relation
pub async fn delete_relation(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // TODO: Get user_id from authenticated context
    let user_id = "system";

    RelationService::delete(services.db(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Get all relations for an asset
pub async fn get_asset_relations(
    State(services): State<Services>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Relation>>> {
    let relations = RelationService::get_asset_relations(services.db(), &id).await?;
    Ok(Json(relations))
}

/// Traverse the relationship graph from an asset
pub async fn traverse_graph(
    State(services): State<Services>,
    Path(id): Path<String>,
    Query(params): Query<TraverseParams>,
) -> Result<Json<Vec<GraphNode>>> {
    let nodes = RelationService::traverse_graph(services.db(), &id, params.depth).await?;
    Ok(Json(nodes))
}
