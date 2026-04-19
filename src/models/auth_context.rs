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

/// Authorization context embedded on an asset.
///
/// Three fields that don't exist anywhere in Asset or Tags.
/// This struct is 1:1 with the Asset document (MongoDB-idiomatic).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
