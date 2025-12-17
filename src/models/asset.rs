//! Asset model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::Tag;

// Note: Asset types are handled through the tagging system using TagCategory::AssetType
// rather than a separate enum. This provides more flexibility and consistency.
// Common asset type tag values: document, concept, idea, data_entity, reference

/// Knowledge asset document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    #[serde(rename = "_id")]
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Vec<Tag>,          // Asset type is determined by tags with category AssetType
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Request to create a new asset
/// Note: Asset type should be specified via tags with category "asset_type"
#[derive(Debug, Clone, Deserialize)]
pub struct CreateAssetRequest {
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tags: Vec<super::AddTagRequest>,  // Should include at least one AssetType tag
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Request to update an existing asset
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAssetRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}
