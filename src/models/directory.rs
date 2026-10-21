//! Directory name-map — uuid → {name, kind, email/role} (4474a038).
//!
//! Directory entries are PDT data (assets tagged `type=directory_entry` with
//! a unique `principal={uuid}` tag), populated from nghr employees, thq
//! directory rows, and tocpi user records via the upsert API or the
//! idempotent backfill. Resolution is decoupled from Kanidm (WS-A ruling).
//!
//! AC (4474a038): every known principal resolves; unknown id → LOUD
//! unresolved marker (never fake names); backfill idempotent.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Asset-type tag value for directory entries.
pub const DIRECTORY_ENTRY_TYPE: &str = "directory_entry";
/// Tag category carrying the principal uuid (unique lookup key).
pub const PRINCIPAL_TAG: &str = "principal";

/// A resolved directory entry: display data for a principal uuid.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DirectoryEntry {
    /// The principal uuid this entry resolves.
    pub uuid: String,
    /// Display name. `None` when no honest name is known — NEVER a fake name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Principal kind: "human" | "agent" | "service".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Email (or the raw identifier when no email exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Role string (opaque; e.g. "developer", "platform-services").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Request to upsert a directory entry (import path for nghr employees,
/// thq directory rows, tocpi user records).
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpsertDirectoryRequest {
    #[serde(default)]
    pub name: Option<String>,
    /// Principal kind: "human" | "agent" | "service".
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[cfg(test)]
mod directory_tests {
    use super::*;

    #[test]
    fn entry_serializes_without_fake_names() {
        // A known principal with no display name serializes name-absent —
        // honest absence, never a fabricated name (4474a038 AC).
        let entry = DirectoryEntry {
            uuid: "9f2a3bed-0000".to_string(),
            name: None,
            kind: Some("agent".to_string()),
            email: None,
            role: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json.get("name").is_none());
        assert_eq!(json["kind"], "agent");
    }

    #[test]
    fn entry_serializes_full_shape() {
        let entry = DirectoryEntry {
            uuid: "u-1".to_string(),
            name: Some("Saman".to_string()),
            kind: Some("agent".to_string()),
            email: Some("saman@tanbal.ir".to_string()),
            role: Some("platform-services".to_string()),
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["name"], "Saman");
        assert_eq!(json["uuid"], "u-1");
    }
}
