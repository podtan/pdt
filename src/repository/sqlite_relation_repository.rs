//! SQLite implementation of RelationRepository

use std::collections::{HashSet, VecDeque};

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::error::{ApiError, Result};
use crate::models::{CreateRelationRequest, Relation, RelationType};
use crate::repository::traits::RelationRepository;

pub struct SqliteRelationRepository {
    pool: SqlitePool,
}

impl SqliteRelationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn parse_relation_type(s: &str) -> RelationType {
        match s {
            "contains" => RelationType::Contains,
            "references" => RelationType::References,
            "related_to" => RelationType::RelatedTo,
            "depends_on" => RelationType::DependsOn,
            "supersedes" => RelationType::Supersedes,
            "complements" => RelationType::Complements,
            "has_instance" => RelationType::HasInstance,
            _ => RelationType::RelatedTo,
        }
    }

    async fn hydrate_relation(&self, row: &RelationRow) -> Result<Relation> {
        let metadata: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(&row.metadata_json).unwrap_or_default();

        Ok(Relation {
            id: row.id.clone(),
            from_asset_id: row.from_asset_id.clone(),
            to_asset_id: row.to_asset_id.clone(),
            relation_type: Self::parse_relation_type(&row.relation_type),
            metadata,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            created_by: row.created_by.clone(),
        })
    }
}

#[async_trait::async_trait]
impl RelationRepository for SqliteRelationRepository {
    async fn create(
        &self,
        request: CreateRelationRequest,
        user_id: &str,
    ) -> Result<Relation> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let metadata_json = serde_json::to_string(&request.metadata).unwrap_or_else(|_| "{}".to_string());
        let rel_type = request.relation_type.to_string();

        sqlx::query(
            "INSERT INTO relations (id, from_asset_id, to_asset_id, relation_type, metadata_json, created_at, created_by)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&request.from_asset_id)
        .bind(&request.to_asset_id)
        .bind(&rel_type)
        .bind(&metadata_json)
        .bind(&now)
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        let row = RelationRow {
            id,
            from_asset_id: request.from_asset_id,
            to_asset_id: request.to_asset_id,
            relation_type: rel_type,
            metadata_json,
            created_at: now,
            created_by: user_id.to_string(),
        };

        self.hydrate_relation(&row).await
    }

    async fn get_by_id(&self, id: &str) -> Result<Relation> {
        let row = sqlx::query_as::<_, RelationRow>(
            "SELECT id, from_asset_id, to_asset_id, relation_type, metadata_json, created_at, created_by
             FROM relations WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Relation not found: {}", id)))?;

        self.hydrate_relation(&row).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM relations WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::NotFound(format!("Relation not found: {}", id)));
        }

        Ok(())
    }

    async fn get_asset_relations(&self, asset_id: &str) -> Result<Vec<Relation>> {
        let rows = sqlx::query_as::<_, RelationRow>(
            "SELECT id, from_asset_id, to_asset_id, relation_type, metadata_json, created_at, created_by
             FROM relations WHERE from_asset_id = ? OR to_asset_id = ?",
        )
        .bind(asset_id)
        .bind(asset_id)
        .fetch_all(&self.pool)
        .await?;

        let mut relations = Vec::with_capacity(rows.len());
        for row in rows {
            relations.push(self.hydrate_relation(&row).await?);
        }
        Ok(relations)
    }

    async fn delete_by_asset(&self, asset_id: &str) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM relations WHERE from_asset_id = ? OR to_asset_id = ?",
        )
        .bind(asset_id)
        .bind(asset_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    async fn traverse_graph(
        &self,
        start_asset_id: &str,
        max_depth: u32,
    ) -> Result<Vec<(String, u32, Vec<Relation>)>> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();
        let mut results: Vec<(String, u32, Vec<Relation>)> = Vec::new();

        queue.push_back((start_asset_id.to_string(), 0));
        visited.insert(start_asset_id.to_string());

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth > max_depth {
                continue;
            }

            let relations = self.get_asset_relations(&current_id).await?;

            if !relations.is_empty() || current_id == start_asset_id {
                results.push((current_id.clone(), depth, relations.clone()));
            }

            if depth < max_depth {
                for relation in relations {
                    let next_id = if relation.from_asset_id == current_id {
                        &relation.to_asset_id
                    } else {
                        &relation.from_asset_id
                    };

                    if !visited.contains(next_id) {
                        visited.insert(next_id.clone());
                        queue.push_back((next_id.clone(), depth + 1));
                    }
                }
            }
        }

        Ok(results)
    }

    async fn get_descendants(&self, root_id: &str) -> Result<Vec<String>> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        let mut results: Vec<String> = Vec::new();

        queue.push_back(root_id.to_string());
        visited.insert(root_id.to_string());

        while let Some(current_id) = queue.pop_front() {
            let relations = self.get_asset_relations(&current_id).await?;
            for relation in relations {
                if relation.from_asset_id == current_id && !visited.contains(&relation.to_asset_id) {
                    visited.insert(relation.to_asset_id.clone());
                    results.push(relation.to_asset_id.clone());
                    queue.push_back(relation.to_asset_id);
                }
            }
        }

        Ok(results)
    }

    async fn would_create_cycle(&self, from_id: &str, to_id: &str) -> Result<bool> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();

        queue.push_back(to_id.to_string());
        visited.insert(to_id.to_string());

        while let Some(current_id) = queue.pop_front() {
            if current_id == *from_id {
                return Ok(true);
            }

            let rows = sqlx::query_as::<_, RelationRow>(
                "SELECT id, from_asset_id, to_asset_id, relation_type, metadata_json, created_at, created_by
                 FROM relations WHERE from_asset_id = ?",
            )
            .bind(&current_id)
            .fetch_all(&self.pool)
            .await?;

            for row in rows {
                if !visited.contains(&row.to_asset_id) {
                    visited.insert(row.to_asset_id.clone());
                    queue.push_back(row.to_asset_id);
                }
            }
        }

        Ok(false)
    }
}

#[derive(sqlx::FromRow)]
struct RelationRow {
    id: String,
    from_asset_id: String,
    to_asset_id: String,
    relation_type: String,
    metadata_json: String,
    created_at: String,
    created_by: String,
}
