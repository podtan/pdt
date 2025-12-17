//! Audit model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Types of audited actions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    AddTag,
    RemoveTag,
    AddRelation,
    RemoveRelation,
    AddToCollection,
    RemoveFromCollection,
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditAction::Create => write!(f, "create"),
            AuditAction::Update => write!(f, "update"),
            AuditAction::Delete => write!(f, "delete"),
            AuditAction::AddTag => write!(f, "add_tag"),
            AuditAction::RemoveTag => write!(f, "remove_tag"),
            AuditAction::AddRelation => write!(f, "add_relation"),
            AuditAction::RemoveRelation => write!(f, "remove_relation"),
            AuditAction::AddToCollection => write!(f, "add_to_collection"),
            AuditAction::RemoveFromCollection => write!(f, "remove_from_collection"),
        }
    }
}

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    #[serde(rename = "_id")]
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub action: AuditAction,
    pub changes: serde_json::Value,
    pub user_id: String,
    pub timestamp: DateTime<Utc>,
}
