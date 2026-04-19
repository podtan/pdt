//! Relation handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use pep::oidc::types::JwtClaims;
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::AppState;
use crate::error::Result;
use crate::models::{CreateRelationRequest, Relation};
use crate::service::{relation_service::GraphNode, RelationService};

/// Query parameters for graph traversal
#[derive(Debug, Deserialize)]
pub struct TraverseParams {
    #[serde(default = "default_depth")]
    pub depth: u32,
}

fn default_depth() -> u32 {
    3
}

/// Extract JwtClaims from AuthenticatedUser (reconstructed from auth layer).
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

/// Create a new relation
pub async fn create_relation(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(request): Json<CreateRelationRequest>,
) -> Result<Json<Relation>> {
    let user_id = &user.user_id;

    // Cedar enforcement: check permission on both assets
    if let Some(ref authorizer) = state.authorizer() {
        let claims = extract_claims(&user);
        crate::cedar::enforcement::check_relation_permission(
            state.services().db(),
            authorizer,
            &claims,
            "Relate",
            &request.from_asset_id,
            &request.to_asset_id,
        )
        .await?;
    }

    let relation = RelationService::create(state.services().db(), request, user_id).await?;
    Ok(Json(relation))
}

/// Get relation by ID
pub async fn get_relation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Relation>> {
    let relation = RelationService::get(state.services().db(), &id).await?;
    Ok(Json(relation))
}

/// Delete a relation
pub async fn delete_relation(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement: check permission on the relation's source asset
    if let Some(ref authorizer) = state.authorizer() {
        let relation = RelationService::get(state.services().db(), &id).await?;
        let claims = extract_claims(&user);
        crate::cedar::enforcement::check_asset_permission_by_id(
            state.services().db(),
            authorizer,
            &claims,
            "Relate",
            &relation.from_asset_id,
        )
        .await?;
    }

    RelationService::delete(state.services().db(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Get all relations for an asset
pub async fn get_asset_relations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Relation>>> {
    let relations = RelationService::get_asset_relations(state.services().db(), &id).await?;
    Ok(Json(relations))
}

/// Traverse the relationship graph from an asset
pub async fn traverse_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<TraverseParams>,
) -> Result<Json<Vec<GraphNode>>> {
    let nodes = RelationService::traverse_graph(state.services().db(), &id, params.depth).await?;
    Ok(Json(nodes))
}
