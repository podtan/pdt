//! Maintenance handlers — admin-gated, idempotent backfills (1552c098 + 4474a038).
//!
//! POST /api/maintenance/backfill — runs BOTH backfills (roles + directory)
//! against the live registry. Idempotent: re-runs clean (every step skips
//! work already done). Admin-gated on the caller's role claim — fail-closed:
//! anything but role=admin is 403.

use axum::Json;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::TenantState;
use crate::error::{ApiError, Result};
use crate::service::directory_service::DirectoryService;
use crate::service::roles_service::RolesService;

/// Extract role strings from the caller's claims — Kanidm returns `role` as
/// a JSON array; some IdPs return a single string (cedar/entity.rs nuance).
fn role_strings(user: &AuthenticatedUser) -> Vec<String> {
    user.claims_extra
        .as_ref()
        .and_then(|extra| extra.get("role"))
        .map(|v| match v {
            serde_json::Value::String(s) => vec![s.clone()],
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
}

/// Run the idempotent backfills (roles + directory) against the live registry.
#[utoipa::path(
    post,
    path = "/api/maintenance/backfill",
    responses(
        (status = 200, description = "Backfill summary (idempotent — re-runs clean)"),
        (status = 403, description = "Requires role=admin (fail-closed)"),
    ),
    tag = "maintenance",
)]
pub async fn backfill(tx: TenantState, user: AuthenticatedUser) -> Result<Json<serde_json::Value>> {
    // Admin gate — fail-closed: a missing/unrecognized role claim is 403.
    if !role_strings(&user).iter().any(|r| r == "admin") {
        return Err(ApiError::Forbidden(
            "maintenance backfill requires role=admin".to_string(),
        ));
    }

    let roles = RolesService::backfill(tx.services(), &user.user_id).await?;
    let directory = DirectoryService::backfill(tx.services(), &user.user_id).await?;

    // Explicit Map construction — the json! macro with non-literal values
    // trips the never-type-fallback future-compat error on this toolchain.
    let mut summary = serde_json::Map::new();
    summary.insert("roles".to_string(), roles);
    summary.insert("directory".to_string(), directory);
    Ok(Json(serde_json::Value::Object(summary)))
}
