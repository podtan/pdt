//! Collection model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::datetime_format;
use super::Tag;

/// Named collection of assets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub asset_ids: Vec<String>,
    #[serde(with = "datetime_format")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "datetime_format")]
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_by: String,
}

/// Request to create a new collection
#[derive(Debug, Clone, Deserialize)]
pub struct CreateCollectionRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<super::AddTagRequest>,
}

/// Request to update a collection
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCollectionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request to add an asset to a collection
#[derive(Debug, Clone, Deserialize)]
pub struct AddAssetRequest {
    pub asset_id: String,
}
