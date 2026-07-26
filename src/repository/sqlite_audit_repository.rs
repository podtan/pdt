//! SQLite implementation of AuditRepository

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::error::Result;
use crate::models::{AuditAction, AuditEntry};
use crate::repository::traits::AuditRepository;

pub struct SqliteAuditRepository {
    pool: SqlitePool,
}

impl SqliteAuditRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn parse_action(s: &str) -> AuditAction {
        match s {
            "create" => AuditAction::Create,
            "update" => AuditAction::Update,
            "delete" => AuditAction::Delete,
            "add_tag" => AuditAction::AddTag,
            "remove_tag" => AuditAction::RemoveTag,
            "add_relation" => AuditAction::AddRelation,
            "remove_relation" => AuditAction::RemoveRelation,
            "add_to_collection" => AuditAction::AddToCollection,
            "remove_from_collection" => AuditAction::RemoveFromCollection,
            _ => AuditAction::Update,
        }
    }
}

#[async_trait::async_trait]
impl AuditRepository for SqliteAuditRepository {
    async fn create(
        &self,
        entity_type: &str,
        entity_id: &str,
        action: AuditAction,
        changes: serde_json::Value,
        user_id: &str,
    ) -> Result<AuditEntry> {
        let id = Uuid::new_v4().to_string();
        let timestamp = Utc::now();
        let changes_json = serde_json::to_string(&changes).unwrap_or_else(|_| "{}".to_string());
        let action_str = action.to_string();

        sqlx::query(
            "INSERT INTO audit_entries (id, entity_type, entity_id, action, changes_json, user_id, timestamp)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(entity_type)
        .bind(entity_id)
        .bind(&action_str)
        .bind(&changes_json)
        .bind(user_id)
        .bind(timestamp.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(AuditEntry {
            id,
            entity_type: entity_type.to_string(),
            entity_id: entity_id.to_string(),
            action,
            changes,
            user_id: user_id.to_string(),
            timestamp,
        })
    }

    async fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        user_id: Option<&str>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<AuditEntry>, Option<String>)> {
        let mut query_str = String::from(
            "SELECT id, entity_type, entity_id, action, changes_json, user_id, timestamp
             FROM audit_entries WHERE 1=1",
        );

        if entity_type.is_some() {
            query_str.push_str(" AND entity_type = ?");
        }
        if entity_id.is_some() {
            query_str.push_str(" AND entity_id = ?");
        }
        if user_id.is_some() {
            query_str.push_str(" AND user_id = ?");
        }
        if cursor.is_some() {
            query_str.push_str(" AND id < ?");
        }

        query_str.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ?");

        let mut q = sqlx::query_as::<_, AuditRow>(&query_str);

        if let Some(et) = entity_type {
            q = q.bind(et);
        }
        if let Some(ei) = entity_id {
            q = q.bind(ei);
        }
        if let Some(ui) = user_id {
            q = q.bind(ui);
        }
        if let Some(c) = cursor {
            q = q.bind(c);
        }

        let rows = q.bind(limit + 1).fetch_all(&self.pool).await?;

        let mut entries: Vec<AuditEntry> = rows
            .into_iter()
            .map(|r| AuditEntry {
                id: r.id,
                entity_type: r.entity_type,
                entity_id: r.entity_id,
                action: SqliteAuditRepository::parse_action(&r.action),
                changes: serde_json::from_str(&r.changes_json).unwrap_or_default(),
                user_id: r.user_id,
                timestamp: chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
            .collect();

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
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, entity_type, entity_id, action, changes_json, user_id, timestamp
             FROM audit_entries WHERE entity_type = ? AND entity_id = ?
             ORDER BY timestamp DESC LIMIT ?",
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let entries = rows
            .into_iter()
            .map(|r| AuditEntry {
                id: r.id,
                entity_type: r.entity_type,
                entity_id: r.entity_id,
                action: SqliteAuditRepository::parse_action(&r.action),
                changes: serde_json::from_str(&r.changes_json).unwrap_or_default(),
                user_id: r.user_id,
                timestamp: chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
            .collect();

        Ok(entries)
    }
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    id: String,
    entity_type: String,
    entity_id: String,
    action: String,
    changes_json: String,
    user_id: String,
    timestamp: String,
}
