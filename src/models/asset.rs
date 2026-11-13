//! Asset model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

use super::auth_context::AuthContext;
use super::datetime_format;
use super::Tag;

// Note: Asset types are handled through the tagging system using TagCategory::AssetType
// rather than a separate enum. This provides more flexibility and consistency.
// Common asset type tag values: document, concept, idea, data_entity, reference

/// Knowledge asset document
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Asset {
    #[serde(rename = "_id")]
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Vec<Tag>, // Asset type is determined by tags with category AssetType
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(with = "datetime_format")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "datetime_format")]
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// Cedar authorization context — when present, Cedar policies are enforced.
    /// When absent (grandfathered assets), access is open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_context: Option<AuthContext>,
}

/// Request to create a new asset
/// Note: Asset type should be specified via tags with category "asset_type"
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateAssetRequest {
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Vec<super::AddTagRequest>, // Should include at least one AssetType tag
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Optional auth_context to set at creation time.
    /// GUARD (0e583f06): an omitted or empty band never stores — creates
    /// default to the caller's cohort band (visibility="team" + caller's
    /// groups), or "private" (creator-only, loud WARN) when the caller has
    /// no groups claim. Explicit non-empty visibility is honored as-is.
    #[serde(default)]
    pub auth_context: Option<super::AuthContext>,
}

/// Request body: ADOPT an asset with a SPECIFIED id (same-uuid
/// materialization — CPO ruling 6d86ee70, the Path A adopt's first-class
/// path; replaces the barred direct-sqlite restore).
///
/// Additive: plain create keeps minting new uuids — this operation exists
/// ONLY for restoring a record whose uuid is locked elsewhere (the live
/// store holds zero copies; the row survives in a banked backup).
/// Duplicate-barred at the repository layer (refuses loudly if the uuid
/// exists live or soft-deleted). Provenance is REQUIRED — the call names
/// the banked source; it lands in the audit entry.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AdoptAssetRequest {
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Vec<super::AddTagRequest>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub auth_context: Option<super::AuthContext>,
    /// REQUIRED: the banked source the row is materialized from (e.g. the
    /// frozen pre-dirauth leaf backup / the pre-mutation full-store
    /// backup). Materialization needs custody + path, not recovery.
    pub provenance: String,
}

/// Request to update an existing asset
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateAssetRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Request to MERGE metadata keys into an asset (issue a5daf31d) — upsert
/// per key, never a full-map replace. The delta-write surface for writers
/// that own a few keys (the runtime state report owns observed_state +
/// last_synced_at); keys not in the request are untouched.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct MergeMetadataRequest {
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Request to update the authorization context of an asset
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateAuthContextRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_groups: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidentiality: Option<String>,
    /// When true, cascade auth_context to all descendant assets via relation graph
    #[serde(default)]
    pub cascade: bool,
}

/// Search-index reconcile report — drift between the FTS index and the
/// live asset rows (the list-shows/GET-404 family: issues e410455a,
/// 92b7a94e, 49fe8037). The FTS index is DERIVED data keyed by
/// `assets.rowid`; the assets table is the truth. Drift classes:
/// - `orphan` — an indexed rowid with NO assets row at all (the asset was
///   hard-removed outside the trigger contract; a query matching it serves
///   nothing while the index still advertises it).
/// - `unindexed` — a live (non-deleted) assets row the FTS index does not
///   know: text search can never surface it.
/// `soft_deleted_indexed` is EXPECTED state (soft-delete keeps the index
/// entry; the read path filters `deleted_at IS NULL`) — informational, not
/// drift.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchIndexDrift {
    /// Rowids the FTS index knows (external-content index scan).
    pub indexed_rowids: u64,
    /// Live (non-deleted) asset rows — the truth the index must mirror.
    pub live_assets: u64,
    /// Indexed rowids with no assets row (drift).
    pub orphan_count: u64,
    /// Live assets missing from the index (drift).
    pub unindexed_count: u64,
    /// Soft-deleted rows still indexed — expected, informational.
    pub soft_deleted_indexed: u64,
    /// Bounded specimen of orphan ROWIDS (the asset ids are gone with the
    /// rows — rowids are the only handle that survives).
    pub orphan_samples: Vec<String>,
    /// Bounded specimen of unindexed ASSET ids.
    pub unindexed_samples: Vec<String>,
    /// true = the repair ran (full FTS rebuild) and these numbers are the
    /// POST-repair state (both drift classes must be 0).
    pub repaired: bool,
}

/// Compact search result — returned by /api/search by default
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SearchResult {
    #[serde(rename = "_id")]
    pub id: String,
    pub title: String,
    /// Auto-generated snippet (first 200 chars, markdown-stripped)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Compact tags without noise fields (id, added_by, added_at)
    pub tags: Vec<TagSummary>,
    #[serde(with = "datetime_format")]
    pub updated_at: DateTime<Utc>,
    /// Birth-stamp — added v0.3.10 (fame bbb4882c): list consumers project
    /// created_at from search results; the compact shape previously dropped
    /// it, forcing downstream projections to fabricate updated_at into the
    /// created_at field.
    #[serde(with = "datetime_format")]
    pub created_at: DateTime<Utc>,
}

/// Tag without internal metadata — suitable for list/search views
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TagSummary {
    pub category: String,
    pub value: String,
}

/// Generate a compact snippet from markdown content.
///
/// Strips markdown syntax, collapses whitespace, and truncates at word boundary.
pub fn generate_snippet(content: &str, max_length: usize) -> Option<String> {
    if content.is_empty() {
        return None;
    }

    // Strip markdown syntax
    let text: String = content
        .lines()
        .map(|l| l.trim_start_matches('#').trim_start())
        .collect::<Vec<_>>()
        .join(" ")
        .replace("**", "")
        .replace("*", "")
        .replace("`", "")
        .replace("```", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if text.is_empty() {
        return None;
    }

    if text.len() <= max_length {
        return Some(text);
    }

    // Find a safe char boundary at or before max_length
    let mut end = max_length;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        return Some(text);
    }

    // Truncate at word boundary within the safe range
    let truncated = &text[..end];
    Some(format!(
        "{}...",
        truncated
            .rsplit_once(' ')
            .map(|(w, _)| w)
            .unwrap_or(truncated)
    ))
}
