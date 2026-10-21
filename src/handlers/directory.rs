//! Directory name-map handlers (4474a038)
//!
//! uuid → {name, kind, email/role} as PDT data, resolved for all surfaces
//! (payamui/torpi/tocpi display tasks). Decoupled from Kanidm (WS-A).
//! Unknown id → LOUD unresolved marker (404, uuid echoed) — never fake names.

use axum::{
    extract::{Path, Query},
    Json,
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::TenantState;
use crate::error::Result;
use crate::models::directory::{DirectoryEntry, UpsertDirectoryRequest};
use crate::service::directory_service::DirectoryService;

/// Query parameters for listing directory entries.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListDirectoryParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    100
}

/// Resolve a principal uuid to its directory entry.
#[utoipa::path(
    get,
    path = "/api/directory/{uuid}",
    params(
        ("uuid" = String, Path, description = "Principal uuid to resolve"),
    ),
    responses(
        (status = 200, description = "Directory entry resolved", body = DirectoryEntry),
        (status = 404, description = "Loud unresolved marker — no directory entry for this uuid (never fake names)"),
    ),
    tag = "directory",
)]
pub async fn get_directory_entry(
    tx: TenantState,
    Path(uuid): Path<String>,
) -> Result<Json<DirectoryEntry>> {
    let entry = DirectoryService::resolve(tx.services(), &uuid).await?;
    Ok(Json(entry))
}

/// Upsert a directory entry (import path for nghr employees, thq directory
/// rows, tocpi user records). Idempotent by uuid.
#[utoipa::path(
    put,
    path = "/api/directory/{uuid}",
    params(
        ("uuid" = String, Path, description = "Principal uuid to upsert"),
    ),
    request_body = UpsertDirectoryRequest,
    responses(
        (status = 200, description = "Directory entry upserted", body = DirectoryEntry),
        (status = 400, description = "Empty uuid"),
    ),
    tag = "directory",
)]
pub async fn upsert_directory_entry(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(uuid): Path<String>,
    Json(request): Json<UpsertDirectoryRequest>,
) -> Result<Json<DirectoryEntry>> {
    let entry = DirectoryService::upsert(tx.services(), &uuid, request, &user.user_id).await?;
    Ok(Json(entry))
}

/// List directory entries (paginated).
#[utoipa::path(
    get,
    path = "/api/directory",
    params(ListDirectoryParams),
    responses(
        (status = 200, description = "Directory entries (paginated)"),
    ),
    tag = "directory",
)]
pub async fn list_directory(
    tx: TenantState,
    Query(params): Query<ListDirectoryParams>,
) -> Result<Json<serde_json::Value>> {
    use crate::models::PaginatedResponse;
    let (entries, next_cursor) =
        DirectoryService::list(tx.services(), params.limit, params.cursor.as_deref()).await?;
    Ok(Json(serde_json::to_value(PaginatedResponse {
        data: entries,
        next_cursor,
        total: None,
    })?))
}
