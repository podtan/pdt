//! Audit handlers

use axum::{
    extract::{Path, Query},
    Json,
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::Result;
use crate::models::{AuditEntry, PaginatedResponse};
use crate::service::AuditService;

fn default_limit() -> i64 {
    20
}

/// Query parameters for audit list
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flatten breaks numeric deserialize)
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditListParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub user_id: Option<String>,
}

/// Query parameters for asset history
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryParams {
    #[serde(default = "default_history_limit")]
    pub limit: i64,
}

fn default_history_limit() -> i64 {
    50
}

/// List audit entries
#[utoipa::path(
    get,
    path = "/api/audit",
    params(AuditListParams),
    responses(
        (status = 200, description = "List of audit entries", body = PaginatedResponse<AuditEntry>),
    ),
    tag = "audit",
)]
pub async fn list_audit_entries(
    tx: crate::cedar::enforcement::TenantState,
    Query(params): Query<AuditListParams>,
) -> Result<Json<PaginatedResponse<AuditEntry>>> {
    let (entries, next_cursor) = AuditService::list(
        tx.services(),
        params.entity_type.as_deref(),
        params.entity_id.as_deref(),
        params.user_id.as_deref(),
        params.limit,
        params.cursor.as_deref(),
    )
    .await?;

    Ok(Json(PaginatedResponse {
        data: entries,
        next_cursor,
        total: None,
    }))
}

/// Get history for a specific asset
#[utoipa::path(
    get,
    path = "/api/assets/{id}/history",
    params(
        ("id" = String, Path, description = "Asset ID"),
        HistoryParams,
    ),
    responses(
        (status = 200, description = "Asset change history", body = [AuditEntry]),
        (status = 404, description = "Asset not found"),
    ),
    tag = "audit",
)]
pub async fn get_asset_history(
    tx: crate::cedar::enforcement::TenantState,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<AuditEntry>>> {
    let entries = AuditService::get_asset_history(tx.services(), &id, params.limit).await?;
    Ok(Json(entries))
}
