//! Cedar policy store endpoint
//!
//! Serves PDT's Cedar policies over HTTP so other services (NGHR, Torpi) can
//! fetch them centrally via the `policy_store_url` config option.

use axum::{Json, extract::State};
use serde::Serialize;
use utoipa::ToSchema;

use crate::cedar::enforcement::AppState;
use crate::error::Result;

/// Response from the Cedar policy store endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct CedarPolicyResponse {
    /// Cedar policy text (one or more `permit`/`forbid` statements).
    pub policy: String,
    /// Cedar schema text (entity and action declarations).
    pub schema: String,
    /// Version identifier (content hash) for change detection.
    pub version: String,
}

/// Get the current Cedar policies and schema.
///
/// This endpoint allows other services to fetch PDT's policies via HTTP.
/// The response includes the policy text, schema text, and a version string.
#[utoipa::path(
    get,
    path = "/api/cedar/policies",
    responses(
        (status = 200, description = "Cedar policies and schema", body = CedarPolicyResponse),
    ),
    tag = "cedar"
)]
pub async fn get_cedar_policies(
    State(_state): State<AppState>,
) -> Result<Json<CedarPolicyResponse>> {
    // Serve the embedded policies (same as what the binary was compiled with)
    let policy = include_str!("../../policies/rbac.cedar");
    let schema = include_str!("../../policies/schema.cedarschema");

    // Version is derived from the policy content hash for change detection
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    policy.hash(&mut hasher);
    let version = format!("{:016x}", hasher.finish());

    Ok(Json(CedarPolicyResponse {
        policy: policy.to_string(),
        schema: schema.to_string(),
        version,
    }))
}
