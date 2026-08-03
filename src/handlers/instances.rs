//! Instance (multi-tenant) management handlers
//!
//! These endpoints allow TOCPI (or other control-plane services) to:
//! - Provision a new instance SQLite database
//! - List active instances

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::tenant::TenantPoolManager;

/// Response after provisioning an instance.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProvisionResponse {
    pub instance_id: String,
    pub status: String,
    pub message: String,
}

/// Provision a new instance database.
///
/// Creates the SQLite database at `{instances_dir}/{instance_id}/pdt.db`,
/// runs migrations, and caches the connection pool.
#[utoipa::path(
    post,
    path = "/api/instances/{instance_id}/provision",
    params(
        ("instance_id" = String, Path, description = "Instance ID to provision"),
    ),
    responses(
        (status = 200, description = "Instance provisioned successfully", body = ProvisionResponse),
        (status = 500, description = "Provisioning failed"),
    ),
    tag = "instances",
)]
pub async fn provision_instance(
    State(manager): State<TenantPoolManager>,
    Path(instance_id): Path<String>,
) -> Result<Json<ProvisionResponse>, axum::http::StatusCode> {
    manager
        .provision_instance(&instance_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to provision instance {}: {}", instance_id, e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(ProvisionResponse {
        instance_id,
        status: "provisioned".to_string(),
        message: "Instance database created and migrated".to_string(),
    }))
}

/// Simple response for instance list.
#[derive(Debug, Serialize, ToSchema)]
pub struct InstanceInfo {
    pub instance_id: String,
}

/// List all active (cached) instances.
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
