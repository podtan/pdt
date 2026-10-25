//! Maintenance handlers — admin-gated, idempotent backfills (1552c098 + 4474a038).
//!
//! POST /api/maintenance/backfill — runs BOTH backfills (roles + directory)
//! against the live registry. Idempotent: re-runs clean (every step skips
//! work already done). Admin-gated on the caller's role claim — fail-closed:
//! anything but role=admin is 403.
//!
//! POST /api/maintenance/prune — retention prune (CPO ruling 8d591e85,
//! retention policy b78efcd): class filter (tag category+value) + age window
//! + dry-run mode. BINDING GATES, mechanically enforced where the wire can:
//! - dry_run=true → the numbers only, zero writes (always available);
//! - dry_run=false → 403 unless PDT_PRUNE_ENABLED is set — FIRST EXECUTION
//!   is gated on the CPO's sight of the dry-run numbers, and the gate is a
//!   wire guarantee, not a promise;
//! - removal is soft-delete-only (archive-over-delete hard line — never a
//!   hard delete; recoverable at the store);
//! - scheduled+announced windows with pre-prune snapshots are procedural
//!   (the dry-run report exists to be sighted and announced).

use axum::Json;
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::TenantState;
use crate::error::{ApiError, Result};
use crate::service::directory_service::DirectoryService;
use crate::service::roles_service::RolesService;
use crate::service::AssetService;

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

/// Retention prune request — the policy lives in DATA (the retention policy
/// asset supplies these values at invocation time); the endpoint encodes
/// only the mechanics (data must not leak into code).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PruneRequest {
    /// Tag CATEGORY to match (e.g. "type").
    pub tag_category: String,
    /// Tag VALUE to match (e.g. "report").
    pub tag_value: String,
    /// Age window in days — assets whose created_at is older are candidates.
    pub older_than_days: u32,
    /// true = report the numbers only (zero writes). false = execute the
    /// soft-delete prune — REFUSED unless PDT_PRUNE_ENABLED is set.
    pub dry_run: bool,
}

/// Mechanical execution gate (CPO ruling 8d591e85 — "build only, no deletion
/// moves until then"): prune execution is REFUSED at the wire unless
/// PDT_PRUNE_ENABLED is explicitly set. Default OFF — a fresh deploy cannot
/// prune until the flag is a deliberate act.
fn prune_execution_enabled() -> bool {
    prune_execution_enabled_from(std::env::var_os("PDT_PRUNE_ENABLED"))
}

/// The gate logic, env-injected for testability (env is process-global —
/// parallel tests must not race it).
fn prune_execution_enabled_from(env: Option<std::ffi::OsString>) -> bool {
    env.map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Retention prune — class filter + age window + dry-run mode.
#[utoipa::path(
    post,
    path = "/api/maintenance/prune",
    request_body = PruneRequest,
    responses(
        (status = 200, description = "Prune report (dry-run numbers or execution summary)"),
        (status = 403, description = "Requires role=admin, or execution refused (PDT_PRUNE_ENABLED unset)"),
    ),
    tag = "maintenance",
)]
pub async fn prune(
    tx: TenantState,
    user: AuthenticatedUser,
    Json(req): Json<PruneRequest>,
) -> Result<Json<serde_json::Value>> {
    // Admin gate — fail-closed: a missing/unrecognized role claim is 403.
    if !role_strings(&user).iter().any(|r| r == "admin") {
        return Err(ApiError::Forbidden(
            "maintenance prune requires role=admin".to_string(),
        ));
    }

    // Wire gate: execution (dry_run=false) refused unless PDT_PRUNE_ENABLED.
    // Dry-run is ALWAYS available — the numbers are the gate's input.
    let execution_enabled = prune_execution_enabled();
    if !req.dry_run && !execution_enabled {
        return Err(ApiError::Forbidden(
            "prune execution disabled — set PDT_PRUNE_ENABLED after the CPO sights the dry-run numbers (ruling 8d591e85)".to_string(),
        ));
    }

    let report = AssetService::prune(
        tx.services(),
        &req.tag_category,
        &req.tag_value,
        req.older_than_days,
        req.dry_run,
        &user.user_id,
    )
    .await?;

    // Envelope: report + the gate state, so every response is self-describing.
    let mut body = match report {
        serde_json::Value::Object(map) => map,
        other => return Ok(Json(other)),
    };
    body.insert(
        "execution_enabled".to_string(),
        serde_json::Value::Bool(execution_enabled),
    );
    Ok(Json(serde_json::Value::Object(body)))
}

#[cfg(test)]
mod prune_gate_tests {
    use super::prune_execution_enabled_from;

    /// Regression pin (CPO ruling 8d591e85 — "build only, no deletion moves
    /// until then"): the prune execution gate is DEFAULT-OFF. A fresh deploy
    /// without PDT_PRUNE_ENABLED refuses execution at the wire; dry-run is
    /// always available. Only an explicit "1"/"true" opens the gate.
    #[test]
    fn prune_execution_gate_is_default_off() {
        assert!(!prune_execution_enabled_from(None), "unset flag must gate");
        assert!(!prune_execution_enabled_from(Some("0".into())));
        assert!(!prune_execution_enabled_from(Some("false".into())));
        assert!(!prune_execution_enabled_from(Some("".into())));
        assert!(!prune_execution_enabled_from(Some("yes".into())));
    }

    #[test]
    fn prune_execution_gate_opens_on_explicit_flag() {
        assert!(prune_execution_enabled_from(Some("1".into())));
        assert!(prune_execution_enabled_from(Some("true".into())));
        assert!(prune_execution_enabled_from(Some("TRUE".into())));
    }
}
