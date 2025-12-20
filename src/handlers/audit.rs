//! Audit handlers

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::Result;
use crate::models::{AuditEntry, PaginatedResponse};
use crate::service::{AuditService, Services};

fn default_limit() -> i64 {
    20
}

/// Query parameters for audit list
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flatten breaks numeric deserialize)
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    #[serde(default = "default_history_limit")]
    pub limit: i64,
}

fn default_history_limit() -> i64 {
    50
}

/// List audit entries
pub async fn list_audit_entries(
    State(services): State<Services>,
    Query(params): Query<AuditListParams>,
) -> Result<Json<PaginatedResponse<AuditEntry>>> {
    let (entries, next_cursor) = AuditService::list(
        services.db(),
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
pub async fn get_asset_history(
    State(services): State<Services>,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<AuditEntry>>> {
    let entries = AuditService::get_asset_history(services.db(), &id, params.limit).await?;
    Ok(Json(entries))
}
