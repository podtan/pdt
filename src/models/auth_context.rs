//! Authorization context embedded on assets for Cedar ABAC evaluation
//!
//! AuthContext contains **only** auth-specific fields that don't exist
//! anywhere else in the Asset document. Tag-derived attributes (status,
//! asset_type) are read from `asset.tags` directly by the Cedar entity
//! builder at eval time — no duplication, no sync needed.
//!
//! Assets without `auth_context` (existing 400+ docs) bypass Cedar
//! enforcement and default to open access (grandfathering).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Authorization context embedded on an asset.
///
/// Three fields that don't exist anywhere in Asset or Tags.
/// This struct is 1:1 with the Asset document (MongoDB-idiomatic).
#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct AuthContext {
    /// Authorization visibility level: "public", "team", "private", "org"
    #[serde(default)]
    pub visibility: String,

    /// Groups that have implicit access beyond the visibility rules
    #[serde(default)]
    pub owner_groups: Vec<String>,

    /// Security classification level: "internal", "confidential", "restricted"
    #[serde(default)]
    pub confidentiality: String,
}

impl AuthContext {
    /// Create a new AuthContext with default values
    pub fn new() -> Self {
        Self::default()
    }
}

/// WORKSPACE DEFAULT BAND (issue ca989667 — the pdt-side create cohort
/// grant, CPO GO 2026-11-23): an OPTIONAL, explicitly-configured default
/// authz band applied at create to BAND-LESS collaboration records in the
/// configured workspace store. This is the nghr caller-layer cohort grant
/// (ensure_multi_reader_cohort) extended to direct callers that have no
/// nghr layer — the workspace's collaboration policy, hosted in pdt config.
///
/// Precedence at birth (load-bearing, all four CPO guards):
/// 1. an explicit caller stamp ALWAYS wins (any provided visibility or
///    owner_groups — the default never merges over an explicit stamp);
/// 2. an unconfigured workspace keeps the ruled born-empty honest-private
///    semantics untouched (zero change where nobody asked for change);
/// 3. the band exists ONLY by explicit per-workspace admin action (the
///    admin-gated default-band endpoint writing the store-side config) —
///    never a global default, never implicit, never inherited from a
///    parent workspace to a child store;
/// 4. every application is recorded in the audit log (workspace + band).
#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema, PartialEq, Eq)]
pub struct WorkspaceDefaultBand {
    /// Default visibility for band-less collaboration creates. Restricted
    /// to "team": a "public" default would be a world-visible grant far
    /// beyond the ruled cohort shape (the nghr precedent stamps the cohort,
    /// never the world).
    pub visibility: String,

    /// Default owner_groups (the cohort). Group-membership forms only —
    /// bare account-SPNs are inert no matter how name-correct (the
    /// 0.9.1 dangling-forms class).
    #[serde(default)]
    pub owner_groups: Vec<String>,
}

impl WorkspaceDefaultBand {
    /// Validate the band shape. A default band that is not a team+cohort
    /// band is rejected at the WRITE surface (the admin endpoint) and
    /// defensively at every application site — an invalid band is treated
    /// as UNCONFIGURED (fail toward the ruled honest-private semantics).
    pub fn validate(&self) -> Result<(), String> {
        if self.visibility != "team" {
            return Err(format!(
                "default band visibility must be \"team\" (got {:?}) — the grant is a \
                 cohort band, never a world-visible default",
                self.visibility
            ));
        }
        if self.owner_groups.is_empty() {
            return Err(
                "default band owner_groups must be non-empty — a team visibility with no \
                 cohort grants creator+admin only (a lie)"
                    .to_string(),
            );
        }
        for g in &self.owner_groups {
            if g.trim().is_empty() {
                return Err("default band owner_groups entries must be non-empty".to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_auth_context() {
        let ctx = AuthContext::new();
        assert!(ctx.visibility.is_empty());
        assert!(ctx.owner_groups.is_empty());
        assert!(ctx.confidentiality.is_empty());
    }

    #[test]
    fn test_serde_roundtrip() {
        let ctx = AuthContext {
            visibility: "team".to_string(),
            owner_groups: vec!["developers".to_string(), "pdt-team".to_string()],
            confidentiality: "confidential".to_string(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: AuthContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.visibility, "team");
        assert_eq!(deserialized.owner_groups.len(), 2);
        assert_eq!(deserialized.confidentiality, "confidential");
    }
}
