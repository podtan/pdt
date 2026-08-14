//! Instance (multi-tenant) management handlers
//!
//! These endpoints allow TOCPI (or other control-plane services) to:
//! - Provision a new instance SQLite database (workspace or nested child)
//! - Delete an instance and its children recursively
//! - List active instances (flat and tree views)

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::tenant::TenantPoolManager;

/// Response after provisioning an instance.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProvisionResponse {
    pub instance_id: String,
    pub status: String,
    pub message: String,
}

/// Optional parent workspace for provisioning.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct ProvisionQuery {
    /// Parent workspace UUID. When present, the instance is provisioned as a
    /// nested child at `{instances_dir}/{parent}/{instance_id}/pdt.db`.
    /// When absent, the instance is a top-level workspace.
    pub parent: Option<String>,
}

/// Provision a new instance database.
///
/// Without `?parent=`: creates a top-level workspace at
/// `{instances_dir}/{instance_id}/pdt.db`.
///
/// With `?parent={workspace-uuid}`: creates a child instance at
/// `{instances_dir}/{parent}/{instance_id}/pdt.db`. The parent must already
/// be provisioned.
#[utoipa::path(
    post,
    path = "/api/instances/{instance_id}/provision",
    params(
        ("instance_id" = String, Path, description = "Instance ID (UUID) to provision"),
        ("parent" = Option<String>, Query, description = "Parent workspace UUID for nested instances"),
    ),
    request_body = (),
    responses(
        (status = 200, description = "Instance provisioned successfully", body = ProvisionResponse),
        (status = 400, description = "Invalid instance ID or missing parent"),
        (status = 500, description = "Provisioning failed"),
    ),
    tag = "instances",
)]
pub async fn provision_instance(
    State(manager): State<TenantPoolManager>,
    Path(instance_id): Path<String>,
    Query(query): Query<ProvisionQuery>,
) -> Result<Json<ProvisionResponse>, (axum::http::StatusCode, String)> {
    manager
        .provision_instance(&instance_id, query.parent.as_deref())
        .await
        .map_err(|e| {
            let status = if e.to_string().contains("invalid instance id")
                || e.to_string().contains("not provisioned")
            {
                axum::http::StatusCode::BAD_REQUEST
            } else {
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            tracing::error!("Failed to provision instance {}: {}", instance_id, e);
            (status, e.to_string())
        })?;

    let nested = query.parent.is_some();
    Ok(Json(ProvisionResponse {
        instance_id,
        status: "provisioned".to_string(),
        message: if nested {
            "Nested instance database created and migrated".to_string()
        } else {
            "Instance database created and migrated".to_string()
        },
    }))
}

/// Delete an instance and all of its nested children (recursive).
///
/// Deleting a workspace removes `{instances_dir}/{workspace}/` including all
/// child databases. Deleting a child removes only that child's subtree.
#[utoipa::path(
    delete,
    path = "/api/instances/{instance_id}",
    params(
        ("instance_id" = String, Path, description = "Instance ID (UUID) to delete"),
    ),
    responses(
        (status = 200, description = "Instance deleted"),
        (status = 400, description = "Invalid instance ID"),
        (status = 404, description = "Instance not found"),
    ),
    tag = "instances",
)]
pub async fn delete_instance(
    State(manager): State<TenantPoolManager>,
    Path(instance_id): Path<String>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    manager
        .delete_instance(&instance_id)
        .await
        .map_err(|e| {
            let status = if e.to_string().contains("invalid instance id") {
                axum::http::StatusCode::BAD_REQUEST
            } else if e.to_string().contains("not found") {
                axum::http::StatusCode::NOT_FOUND
            } else {
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            tracing::error!("Failed to delete instance {}: {}", instance_id, e);
            (status, e.to_string())
        })?;

    Ok(Json(serde_json::json!({ "deleted": true, "instance_id": instance_id })))
}

/// Simple response for instance list.
#[derive(Debug, Serialize, ToSchema)]
pub struct InstanceInfo {
    pub instance_id: String,
}

/// List all active instances (workspaces and children, flattened).
#[utoipa::path(
    get,
    path = "/api/instances",
    responses(
        (status = 200, description = "List of active instances", body = [InstanceInfo]),
    ),
    tag = "instances",
)]
pub async fn list_instances(
    State(manager): State<TenantPoolManager>,
) -> Json<Vec<InstanceInfo>> {
    let instances = manager
        .list_instances()
        .into_iter()
        .map(|id| InstanceInfo { instance_id: id })
        .collect();
    Json(instances)
}

/// A node in the instance tree.
#[derive(Debug, Serialize, ToSchema)]
pub struct InstanceTreeNode {
    pub workspace_id: String,
    /// Child instance IDs nested under this workspace (companies, agents).
    pub children: Vec<String>,
}

/// List all provisioned instances as a workspace tree.
#[utoipa::path(
    get,
    path = "/api/instances/tree",
    responses(
        (status = 200, description = "Workspace tree of instances", body = [InstanceTreeNode]),
    ),
    tag = "instances",
)]
pub async fn list_instance_tree(
    State(manager): State<TenantPoolManager>,
) -> Json<Vec<InstanceTreeNode>> {
    let mut nodes: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for (ws, child) in manager.list_instance_tree() {
        let entry = nodes.entry(ws).or_default();
        if let Some(c) = child {
            entry.push(c);
        }
    }

    Json(
        nodes
            .into_iter()
            .map(|(workspace_id, mut children)| {
                children.sort();
                InstanceTreeNode { workspace_id, children }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect(),
    )
}
