//! MongoDB implementation of AuditRepository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use uuid::Uuid;

use crate::db::Database;
use crate::error::Result;
use crate::models::{AuditAction, AuditEntry};
use crate::repository::traits::AuditRepository;

/// MongoDB-backed audit repository
pub struct MongoAuditRepository {
    db: Database,
}

impl MongoAuditRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl AuditRepository for MongoAuditRepository {
    async fn create(
        &self,
        entity_type: &str,
        entity_id: &str,
        action: AuditAction,
        changes: serde_json::Value,
        user_id: &str,
    ) -> Result<AuditEntry> {
        let entry = AuditEntry {
            id: Uuid::new_v4().to_string(),
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            action,
            changes,
            user_id: user_id.to_string(),
            timestamp: Utc::now(),
        };

        self.db.audit().insert_one(&entry).await?;

        Ok(entry)
    }

    async fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        user_id: Option<&str>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<AuditEntry>, Option<String>)> {
        let mut filter = doc! {};

        if let Some(et) = entity_type {
            filter.insert("entity_type", et);
        }

        if let Some(ei) = entity_id {
            filter.insert("entity_id", ei);
        }

        if let Some(ui) = user_id {
            filter.insert("user_id", ui);
        }

        if let Some(c) = cursor {
            filter.insert("_id", doc! { "$lt": c });
        }

        let options = mongodb::options::FindOptions::builder()
            .sort(doc! { "timestamp": -1, "_id": -1 })
            .limit(limit + 1)
            .build();

        let mut cursor = self.db.audit().find(filter).with_options(options).await?;
        let mut entries = Vec::new();

        while let Some(entry) = cursor.try_next().await? {
            entries.push(entry);
        }

        let next_cursor = if entries.len() > limit as usize {
            entries.pop();
            entries.last().map(|e| e.id.clone())
        } else {
            None
        };

        Ok((entries, next_cursor))
    }

    async fn get_entity_history(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditEntry>> {
        let filter = doc! {
            "entity_type": entity_type,
            "entity_id": entity_id
        };

        let options = mongodb::options::FindOptions::builder()
            .sort(doc! { "timestamp": -1 })
            .limit(limit)
            .build();

        let mut cursor = self.db.audit().find(filter).with_options(options).await?;
        let mut entries = Vec::new();

        while let Some(entry) = cursor.try_next().await? {
            entries.push(entry);
        }

        Ok(entries)
    }
}
