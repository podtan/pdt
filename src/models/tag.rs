//! Tag model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tag categories for multi-dimensional classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TagCategory {
    /// Asset type (document, concept, idea, data_entity, reference, or custom)
    AssetType,
    /// Business domain (Marketing, Finance, Legal, etc.)
    BusinessDomain,
    /// Language (English, Farsi, etc.)
    Language,
    /// Content format (markdown, structured data, etc.)
    ContentFormat,
    /// Sensitivity level (public, internal, confidential)
    SensitivityLevel,
    /// Source system or origin
    SourceSystem,
    /// Structural type (structured, semi-structured, unstructured)
    StructuralType,
    /// Target user type (analysts, developers, executives)
    TargetUserType,
    /// Quality status (draft, reviewed, approved)
    QualityStatus,
    /// Custom category
    Custom(String),
}

impl std::fmt::Display for TagCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagCategory::AssetType => write!(f, "asset_type"),
            TagCategory::BusinessDomain => write!(f, "business_domain"),
            TagCategory::Language => write!(f, "language"),
            TagCategory::ContentFormat => write!(f, "content_format"),
            TagCategory::SensitivityLevel => write!(f, "sensitivity_level"),
            TagCategory::SourceSystem => write!(f, "source_system"),
            TagCategory::StructuralType => write!(f, "structural_type"),
            TagCategory::TargetUserType => write!(f, "target_user_type"),
            TagCategory::QualityStatus => write!(f, "quality_status"),
            TagCategory::Custom(s) => write!(f, "custom:{}", s),
        }
    }
}

/// Tag attached to an asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: String,
    pub category: TagCategory,
    pub value: String,
    pub added_by: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub added_at: DateTime<Utc>,
}

/// Request to add a tag to an asset
#[derive(Debug, Clone, Deserialize)]
pub struct AddTagRequest {
    pub category: TagCategory,
    pub value: String,
}
