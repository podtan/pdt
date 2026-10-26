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
//! - dry_run=false → 403 unless the class is policy-cleared (CPO
//!   precondition 2, routing 0ae73f46): archive-only classes refused
//!   (messages of record, audit trail), windows below the policy window
//!   refused, classes absent from the policy refused — the policy table
//!   loads from PDT_PRUNE_POLICY_PATH at invocation (data, not code);
//! - removal is soft-delete-only (archive-over-delete hard line — never a
//!   hard delete; recoverable at the store);
//! - scheduled+announced windows with pre-prune snapshots are procedural
//!   (the dry-run report exists to be sighted and announced).
//!
//! ADMIN GATE (both endpoints) — token role claim first, then the registry
//! fallback (CPO precondition 1, routing 0ae73f46): exchanged tokens (RFC
//! 8693, the 6c8efe55 no-claims class) carry NO role/groups, so the
//! token-claim gate alone 403s real admins. Fallback: RolesService resolves
//! the caller's global_role from the PDT registry (the data-backed source
//! the backfill syncs). Fail-closed preserved: no token role AND no registry
//! admin role = 403.

use axum::Json;
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::TenantState;
use crate::error::{ApiError, Result};
use crate::models::roles::GLOBAL_ROLE_ADMIN;
use crate::service::directory_service::DirectoryService;
use crate::service::prune_policy::{PrunePolicy, PruneVerdict};
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

/// The admin decision, pure for testability: token role claim first, then
/// the registry global_role (CPO precondition 1). Fail-closed: anything
/// else is deny.
fn admin_allowed(token_roles: &[String], registry_global_role: Option<&str>) -> bool {
    token_roles.iter().any(|r| r == "admin") || registry_global_role == Some(GLOBAL_ROLE_ADMIN)
}

/// Admin gate with registry fallback (CPO precondition 1, routing 0ae73f46):
/// exchanged tokens (RFC 8693, the 6c8efe55 no-claims class) carry NO
/// role/groups claims — the token-claim gate alone 403s real admins. When
/// the token carries no admin role, resolve the caller's global_role from
/// the PDT registry (RolesService — the data-backed source the backfill
/// syncs). Fail-closed preserved: no token role AND no registry admin role
/// = 403.
async fn admin_gate(tx: &TenantState, user: &AuthenticatedUser) -> Result<()> {
    let token_roles = role_strings(user);
    if token_roles.iter().any(|r| r == "admin") {
        return Ok(());
    }
    let answer = RolesService::resolve(tx.services(), &user.user_id).await?;
    if admin_allowed(&token_roles, Some(&answer.global_role)) {
        return Ok(());
    }
    Err(ApiError::Forbidden(
        "maintenance requires role=admin (token role claim or registry global_role)".to_string(),
    ))
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
    // Admin gate — fail-closed, with the registry fallback for claim-less
    // exchanged tokens (CPO precondition 1).
    admin_gate(&tx, &user).await?;

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
    /// soft-delete prune — REFUSED unless PDT_PRUNE_ENABLED is set AND the
    /// class is policy-cleared (PDT_PRUNE_POLICY_PATH).
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
        (status = 403, description = "Requires role=admin (token claim or registry global_role); execution refused when PDT_PRUNE_ENABLED unset, or the class is not policy-cleared (archive-only class, window below policy, or class absent from PDT_PRUNE_POLICY_PATH)"),
    ),
    tag = "maintenance",
)]
pub async fn prune(
    tx: TenantState,
    user: AuthenticatedUser,
    Json(req): Json<PruneRequest>,
) -> Result<Json<serde_json::Value>> {
    // Admin gate — fail-closed, with the registry fallback for claim-less
    // exchanged tokens (CPO precondition 1).
    admin_gate(&tx, &user).await?;

    // Policy gate (CPO precondition 2, routing 0ae73f46): the retention
    // policy loads from PDT_PRUNE_POLICY_PATH at invocation — data, not
    // code. No policy = execution refused for every class (fail-closed);
    // dry-run still answers — the numbers are the gate's input.
    let policy = PrunePolicy::load_from_env();

    // Wire gate: execution (dry_run=false) refused unless PDT_PRUNE_ENABLED.
    // Dry-run is ALWAYS available — the numbers are the gate's input.
    let execution_enabled = prune_execution_enabled();
    if !req.dry_run && !execution_enabled {
        return Err(ApiError::Forbidden(
            "prune execution disabled — set PDT_PRUNE_ENABLED after the CPO sights the dry-run numbers (ruling 8d591e85)".to_string(),
        ));
    }

    // Class gate: execution requires the class to be policy-cleared —
    // archive-only classes refused (messages of record, audit trail),
    // windows below the policy window refused, classes absent from the
    // policy refused. A raw (type,message,14d) call cannot soft-delete
    // messages of record (the paydar dry-run finding).
    if !req.dry_run {
        let verdict = match &policy {
            Some(p) => p.evaluate(&req.tag_category, &req.tag_value, req.older_than_days),
            None => PruneVerdict::Refused(
                "no retention policy loaded (PDT_PRUNE_POLICY_PATH unset or unreadable) — execution refused (fail-closed)".to_string(),
            ),
        };
        if let PruneVerdict::Refused(reason) = verdict {
            return Err(ApiError::Forbidden(reason));
        }
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

    // Envelope: report + the gate states, so every response is self-describing.
    let mut body = match report {
        serde_json::Value::Object(map) => map,
        other => return Ok(Json(other)),
    };
    body.insert(
        "execution_enabled".to_string(),
        serde_json::Value::Bool(execution_enabled),
    );
    body.insert(
        "policy_loaded".to_string(),
        serde_json::Value::Bool(policy.is_some()),
    );
    // Dry-run carries the policy verdict — the CPO sight reads whether
    // execution WOULD be allowed for this class/window, not just the counts.
    if req.dry_run {
        let verdict = match &policy {
            Some(p) => match p.evaluate(&req.tag_category, &req.tag_value, req.older_than_days) {
                PruneVerdict::Allowed => "allowed".to_string(),
                PruneVerdict::Refused(reason) => format!("refused: {reason}"),
            },
            None => "refused: no retention policy loaded (fail-closed)".to_string(),
        };
        body.insert(
            "policy_verdict".to_string(),
            serde_json::Value::String(verdict),
        );
    }
    Ok(Json(serde_json::Value::Object(body)))
}

#[cfg(test)]
mod prune_gate_tests {
    use super::{admin_allowed, prune_execution_enabled_from};

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

    /// Regression pins (CPO precondition 1, routing 0ae73f46): the admin
    /// decision is token role claim FIRST, then the registry global_role
    /// fallback — fail-closed (no token role AND no registry admin = deny).
    #[test]
    fn admin_gate_token_role_admin_allows() {
        assert!(admin_allowed(&["admin".to_string()], None));
        assert!(admin_allowed(
            &["member".to_string(), "admin".to_string()],
            Some("user")
        ));
    }

    #[test]
    fn admin_gate_registry_fallback_allows_claimless_admin() {
        // The 6c8efe55 exchanged-token shape: NO token role, registry says
        // admin → allowed (the fix the CPO precondition demands).
        assert!(admin_allowed(&[], Some("admin")));
    }

    #[test]
    fn admin_gate_fail_closed_without_either() {
        // No token role AND no registry admin role = deny, always.
        assert!(!admin_allowed(&[], None));
        assert!(!admin_allowed(&[], Some("user")));
        assert!(!admin_allowed(&[], Some("agent")));
        assert!(!admin_allowed(&["member".to_string()], Some("user")));
        // A lookalike role string never widens access.
        assert!(!admin_allowed(&["Admin".to_string()], Some("user")));
        assert!(!admin_allowed(&["administrator".to_string()], None));
    }
}
