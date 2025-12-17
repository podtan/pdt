//! Relation model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types of relationships between assets
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// Asset includes other assets
    Contains,
    /// Asset cites or links to another
    References,
    /// General associations between assets
    RelatedTo,
    /// Required relationships for asset validity
    DependsOn,
    /// Asset replaces or updates another
    Supersedes,
    /// Assets enhance each other's value
    Complements,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::Contains => write!(f, "contains"),
            RelationType::References => write!(f, "references"),
            RelationType::RelatedTo => write!(f, "related_to"),
            RelationType::DependsOn => write!(f, "depends_on"),
            RelationType::Supersedes => write!(f, "supersedes"),
            RelationType::Complements => write!(f, "complements"),
        }
    }
}

/// Relationship between two assets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    #[serde(rename = "_id")]
    pub id: String,
    pub from_asset_id: String,
    pub to_asset_id: String,
    pub relation_type: RelationType,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

/// Request to create a new relation
#[derive(Debug, Clone, Deserialize)]
pub struct CreateRelationRequest {
    pub from_asset_id: String,
    pub to_asset_id: String,
    pub relation_type: RelationType,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}
