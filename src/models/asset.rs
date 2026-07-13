//! Asset model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use std::collections::HashMap;

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
    /// Optional auth_context to set at creation time (overrides defaults).
    /// If omitted, defaults to public/internal/empty.
    #[serde(default)]
    pub auth_context: Option<super::AuthContext>,
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
        truncated.rsplit_once(' ').map(|(w, _)| w).unwrap_or(truncated)
    ))
}
