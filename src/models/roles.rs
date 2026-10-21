//! Role model — workspace-membership roles + user-global roles (1552c098).
//!
//! Roles live as PDT data keyed by the token's stable subject (sub):
//! - Workspace-membership roles: `has_access` relation metadata `role`
//!   (owner | ws_admin | member). Legacy v1 rows carrying "admin" are
//!   normalized to ws_admin at RESOLUTION time (read-path, no write needed).
//! - User-global roles: `global_role` tag on the principal's user/agent
//!   asset (admin | agent | user).
//!
//! Least privilege (1552c098 AC): unknown/missing membership role = member;
//! unknown/missing global role = user. Tests pin both defaults.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Membership role: full control of the workspace (from `has_instance`).
pub const MEMBERSHIP_ROLE_OWNER: &str = "owner";
/// Membership role: manage members + workspace settings, no delete.
pub const MEMBERSHIP_ROLE_WS_ADMIN: &str = "ws_admin";
/// Membership role: use-only (least privilege).
pub const MEMBERSHIP_ROLE_MEMBER: &str = "member";
/// Least-privilege default membership role (1552c098 AC).
pub const DEFAULT_MEMBERSHIP_ROLE: &str = MEMBERSHIP_ROLE_MEMBER;

/// Global role: platform admin.
pub const GLOBAL_ROLE_ADMIN: &str = "admin";
/// Global role: agent service account.
pub const GLOBAL_ROLE_AGENT: &str = "agent";
/// Global role: plain human user (least privilege).
pub const GLOBAL_ROLE_USER: &str = "user";
/// Least-privilege default global role (1552c098 AC).
pub const DEFAULT_GLOBAL_ROLE: &str = GLOBAL_ROLE_USER;

/// Normalize a raw membership-role string from relation metadata.
///
/// Legacy v1 vocabulary mapped "admin" → ws_admin (three-tier ruling,
/// Farzan d130b9b5: WsAdmin excluded from delete, members use-only).
/// Unknown/missing → member (least privilege, 1552c098 AC).
pub fn normalize_membership_role(raw: Option<&str>) -> &'static str {
    match raw {
        Some("owner") => MEMBERSHIP_ROLE_OWNER,
        Some("ws_admin") | Some("admin") => MEMBERSHIP_ROLE_WS_ADMIN,
        Some("member") => MEMBERSHIP_ROLE_MEMBER,
        // Unknown or missing → least privilege. An unrecognized role string
        // must never widen access (fail-closed normalization).
        _ => DEFAULT_MEMBERSHIP_ROLE,
    }
}

/// One workspace-membership role row resolved for a principal.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MembershipRoleRow {
    pub workspace_id: String,
    /// Opaque role string (owner | ws_admin | member) — consumers treat
    /// role names opaquely (spec of record 2fea489c, shape decision).
    pub role: String,
}

/// Sub-keyed role answer — BOTH role families in ONE lookup (Bonyan rider
/// point 1, spec of record 2fea489c). pep caches the whole answer per sub.
///
/// Wire contract (rider point 2): a sub with zero roles — known or unknown —
/// answers 200 with the least-privilege defaults (authorize-nothing deny);
/// only transport/5xx is a hard error that pep fails closed on. The two
/// states stay distinguishable on the wire. No time-of-check data in the
/// answer (rider point 4 — TTL honesty).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RoleAnswer {
    /// The sub the lookup was keyed by (echoed verbatim).
    pub sub: String,
    /// Platform-level role (admin | agent | user). Defaults to user.
    pub global_role: String,
    /// Per-workspace membership rows (has_access metadata + has_instance
    /// ownership). Empty = no membership rows.
    pub membership: Vec<MembershipRoleRow>,
}

#[cfg(test)]
mod least_privilege_tests {
    use super::*;

    #[test]
    fn missing_membership_role_defaults_to_member() {
        // 1552c098 AC: tests pin the least-privilege default
        assert_eq!(normalize_membership_role(None), "member");
    }

    #[test]
    fn unknown_membership_role_fails_closed_to_member() {
        // An unrecognized role string must never widen access
        assert_eq!(normalize_membership_role(Some("superadmin")), "member");
        assert_eq!(normalize_membership_role(Some("")), "member");
    }

    #[test]
    fn legacy_admin_normalizes_to_ws_admin() {
        // v1 rows carry "admin" — read-path normalization, no write needed
        assert_eq!(normalize_membership_role(Some("admin")), "ws_admin");
    }

    #[test]
    fn known_roles_pass_through() {
        assert_eq!(normalize_membership_role(Some("owner")), "owner");
        assert_eq!(normalize_membership_role(Some("ws_admin")), "ws_admin");
        assert_eq!(normalize_membership_role(Some("member")), "member");
    }

    #[test]
    fn default_global_role_is_user() {
        // Least privilege at platform level
        assert_eq!(DEFAULT_GLOBAL_ROLE, "user");
    }

    #[test]
    fn default_membership_role_is_member() {
        assert_eq!(DEFAULT_MEMBERSHIP_ROLE, "member");
    }
}
