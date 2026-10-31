//! Relation handlers

use axum::{
    extract::{Path, Query},
    Json,
};
use pep::oidc::types::JwtClaims;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::TenantState;
use crate::error::Result;
use crate::models::{CreateRelationRequest, Relation, RelationType};
use crate::service::{relation_service::GraphNode, RelationService};

/// Query parameters for graph traversal
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TraverseParams {
    #[serde(default = "default_depth")]
    pub depth: u32,
}

fn default_depth() -> u32 {
    3
}

/// Extract JwtClaims from AuthenticatedUser.
///
/// Propagates the original `extra` map (role, groups, etc.) from the real JWT.
fn extract_claims(user: &AuthenticatedUser) -> JwtClaims {
    user.to_cedar_claims()
}

/// Create a new relation
#[utoipa::path(
    post,
    path = "/api/relations",
    request_body = CreateRelationRequest,
    responses(
        (status = 200, description = "Relation created successfully", body = Relation),
        (status = 404, description = "Source or target asset not found"),
    ),
    tag = "relations",
)]
pub async fn create_relation(
    tx: TenantState,
    user: AuthenticatedUser,
    Json(request): Json<CreateRelationRequest>,
) -> Result<Json<Relation>> {
    let user_id = &user.user_id;

    // Cedar enforcement. Membership data (has_access) and workspace agent
    // links (has_agent) are the ROLE LADDER — they enforce the dedicated
    // WS-B actions (99035dee) with the caller's resolved seat role in the
    // context, NEVER the generic Relate permit (which would let any
    // authenticated user self-escalate via a direct has_access write).
    if let Some(ref authorizer) = tx.authorizer() {
        let claims = extract_claims(&user);
        match request.relation_type {
            RelationType::HasAccess => {
                // has_access: User → Workspace — the workspace is the
                // target; the "target user" is the relation source.
                let requested_role = request
                    .metadata
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                crate::cedar::enforcement::check_membership_permission(
                    tx.services(),
                    authorizer,
                    &claims,
                    "ManageMembers",
                    &request.to_asset_id,
                    &request.from_asset_id,
                    requested_role.as_deref(),
                )
                .await?;
            }
            RelationType::HasAgent => {
                // has_agent: Workspace → Agent — the workspace is the source.
                crate::cedar::enforcement::check_membership_permission(
                    tx.services(),
                    authorizer,
                    &claims,
                    "ManageWorkspaceAgents",
                    &request.from_asset_id,
                    &request.from_asset_id,
                    None,
                )
                .await?;
            }
            _ => {
                crate::cedar::enforcement::check_relation_permission(
                    tx.services(),
                    authorizer,
                    &claims,
                    "Relate",
                    &request.from_asset_id,
                    &request.to_asset_id,
                )
                .await?;
            }
        }
    }

    let relation = RelationService::create(tx.services(), request, user_id).await?;
    Ok(Json(relation))
}

/// Get relation by ID
#[utoipa::path(
    get,
    path = "/api/relations/{id}",
    params(
        ("id" = String, Path, description = "Relation ID"),
    ),
    responses(
        (status = 200, description = "Relation found", body = Relation),
        (status = 404, description = "Relation not found"),
    ),
    tag = "relations",
)]
pub async fn get_relation(tx: TenantState, Path(id): Path<String>) -> Result<Json<Relation>> {
    let relation = RelationService::get(tx.services(), &id).await?;
    Ok(Json(relation))
}

/// Delete a relation
#[utoipa::path(
    delete,
    path = "/api/relations/{id}",
    params(
        ("id" = String, Path, description = "Relation ID"),
    ),
    responses(
        (status = 200, description = "Relation deleted successfully"),
        (status = 404, description = "Relation not found"),
    ),
    tag = "relations",
)]
pub async fn delete_relation(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement. Membership (has_access) and agent-link (has_agent)
    // deletes are role-ladder writes — the dedicated WS-B actions with the
    // caller's resolved seat role (99035dee), never the generic Relate
    // permit. A delete carries no requested_role: the ws_admin arm permits
    // removing PLAIN members only (target_seat_role == "member").
    if let Some(ref authorizer) = tx.authorizer() {
        let relation = RelationService::get(tx.services(), &id).await?;
        let claims = extract_claims(&user);
        match relation.relation_type {
            RelationType::HasAccess => {
                crate::cedar::enforcement::check_membership_permission(
                    tx.services(),
                    authorizer,
                    &claims,
                    "ManageMembers",
                    &relation.to_asset_id,
                    &relation.from_asset_id,
                    None,
                )
                .await?;
            }
            RelationType::HasAgent => {
                crate::cedar::enforcement::check_membership_permission(
                    tx.services(),
                    authorizer,
                    &claims,
                    "ManageWorkspaceAgents",
                    &relation.from_asset_id,
                    &relation.from_asset_id,
                    None,
                )
                .await?;
            }
            _ => {
                crate::cedar::enforcement::check_asset_permission_by_id(
                    tx.services(),
                    authorizer,
                    &claims,
                    "Relate",
                    &relation.from_asset_id,
                )
                .await?;
            }
        }
    }

    RelationService::delete(tx.services(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Get all relations for an asset
#[utoipa::path(
    get,
    path = "/api/assets/{id}/relations",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    responses(
        (status = 200, description = "List of relations", body = [Relation]),
        (status = 404, description = "Asset not found"),
    ),
    tag = "relations",
)]
pub async fn get_asset_relations(
    tx: TenantState,
    Path(id): Path<String>,
) -> Result<Json<Vec<Relation>>> {
    let relations = RelationService::get_asset_relations(tx.services(), &id).await?;
    Ok(Json(relations))
}

/// Traverse the relationship graph from an asset
#[utoipa::path(
    get,
    path = "/api/assets/{id}/graph",
    params(
        ("id" = String, Path, description = "Asset ID"),
        TraverseParams,
    ),
    responses(
        (status = 200, description = "Graph traversal results", body = [GraphNode]),
        (status = 404, description = "Asset not found"),
    ),
    tag = "relations",
)]
pub async fn traverse_graph(
    tx: TenantState,
    Path(id): Path<String>,
    Query(params): Query<TraverseParams>,
) -> Result<Json<Vec<GraphNode>>> {
    let nodes = RelationService::traverse_graph(tx.services(), &id, params.depth).await?;
    Ok(Json(nodes))
}
