//! Tag model
//!
//! Tags use a simple {category, value} key-value format (industry standard).
//! Categories are free-form strings - no enum restrictions.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use super::datetime_format;

/// Regex for validating category names: alphanumeric, hyphens, underscores, forward slashes
static CATEGORY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_/-]+$").unwrap());

/// Validate a tag category string
pub fn validate_category(category: &str) -> Result<(), String> {
    if category.is_empty() {
        return Err("Category cannot be empty".to_string());
    }
    if category.len() > 64 {
        return Err("Category cannot exceed 64 characters".to_string());
    }
    if !CATEGORY_REGEX.is_match(category) {
        return Err("Category can only contain alphanumeric characters, hyphens, underscores, and forward slashes".to_string());
    }
    Ok(())
}

/// Validate a tag value string
pub fn validate_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("Value cannot be empty".to_string());
    }
    if value.len() > 256 {
        return Err("Value cannot exceed 256 characters".to_string());
    }
    Ok(())
}

/// Tag attached to an asset
///
/// Uses industry-standard key-value format:
/// - `category`: Free-form string (e.g., "type", "status", "domain", "priority")
/// - `value`: The tag value (e.g., "document", "draft", "backend", "high")
///
/// Common categories (suggestions, not enforced):
/// - `type` - Asset type (document, idea, requirement, bug, task)
/// - `status` - Status (draft, review, approved, archived)
/// - `domain` - Business domain (backend, frontend, infra)
/// - `lang` - Language (en, fa)
/// - `priority` - Priority (high, medium, low)
/// - `scope` - Feature/component scope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: String,
    pub category: String,
    pub value: String,
    pub added_by: String,
    #[serde(with = "datetime_format")]
    pub added_at: DateTime<Utc>,
}

/// Request to add a tag to an asset
#[derive(Debug, Clone, Deserialize)]
pub struct AddTagRequest {
    pub category: String,
    pub value: String,
}

impl AddTagRequest {
    /// Validate the tag request
    pub fn validate(&self) -> Result<(), String> {
        validate_category(&self.category)?;
        validate_value(&self.value)?;
        Ok(())
    }
}
