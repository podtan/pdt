//! Role resolution handlers (1552c098)
//!
//! GET /api/roles/{sub} — the sub-keyed single-lookup role answer for the
//! PEP authz path (Bonyan rider, spec of record 2fea489c). One wire call
//! returns BOTH role families; a sub with zero roles answers 200 with the
//! least-privilege defaults (authorize-nothing deny) — only transport/5xx is
//! a hard error. No Cedar enforcement on this route: it IS the authz data
//! source the PEP consumes (enforcing authz on it would be circular); the
//! auth layer (valid token) still applies.

use axum::{extract::Path, Json};

use crate::cedar::enforcement::TenantState;
use crate::error::Result;
use crate::models::roles::RoleAnswer;
use crate::service::roles_service::RolesService;

/// Resolve roles for a sub — one lookup, both families.
#[utoipa::path(
    get,
    path = "/api/roles/{sub}",
    params(
        ("sub" = String, Path, description = "The token's stable subject (sub) to resolve"),
    ),
    responses(
        (status = 200, description = "Role answer (both families; empty membership + default global_role = authorize-nothing deny)", body = RoleAnswer),
        (status = 400, description = "Empty sub"),
        (status = 500, description = "Transport/DB failure — consumers fail closed"),
    ),
    tag = "roles",
)]
pub async fn get_roles(tx: TenantState, Path(sub): Path<String>) -> Result<Json<RoleAnswer>> {
    let answer = RolesService::resolve(tx.services(), &sub).await?;
    Ok(Json(answer))
}
