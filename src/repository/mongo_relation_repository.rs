//! MongoDB implementation of RelationRepository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use std::collections::{HashSet, VecDeque};
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{CreateRelationRequest, Relation};
use crate::repository::traits::RelationRepository;

/// MongoDB-backed relation repository
pub struct MongoRelationRepository {
    db: Database,
}

impl MongoRelationRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl RelationRepository for MongoRelationRepository {
    async fn create(
        &self,
        request: CreateRelationRequest,
        user_id: &str,
    ) -> Result<Relation> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();

        let relation = Relation {
            id: id.clone(),
            from_asset_id: request.from_asset_id,
            to_asset_id: request.to_asset_id,
            relation_type: request.relation_type,
            metadata: request.metadata,
            created_at: now,
            created_by: user_id.to_string(),
        };

        self.db.relations().insert_one(&relation).await?;

        Ok(relation)
    }

    async fn get_by_id(&self, id: &str) -> Result<Relation> {
        let filter = doc! { "_id": id };

        self.db
            .relations()
            .find_one(filter)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Relation not found: {}", id)))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let filter = doc! { "_id": id };
        let result = self.db.relations().delete_one(filter).await?;

        if result.deleted_count == 0 {
            return Err(ApiError::NotFound(format!("Relation not found: {}", id)));
        }

        Ok(())
    }

    async fn get_asset_relations(&self, asset_id: &str) -> Result<Vec<Relation>> {
        let filter = doc! {
            "$or": [
                { "from_asset_id": asset_id },
                { "to_asset_id": asset_id }
            ]
        };

        let mut cursor = self.db.relations().find(filter).await?;
        let mut relations = Vec::new();

        while let Some(relation) = cursor.try_next().await? {
            relations.push(relation);
        }

        Ok(relations)
    }

    async fn delete_by_asset(&self, asset_id: &str) -> Result<u64> {
        let filter = doc! {
            "$or": [
                { "from_asset_id": asset_id },
                { "to_asset_id": asset_id }
            ]
        };

        let result = self.db.relations().delete_many(filter).await?;
        Ok(result.deleted_count)
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
        // Check if there's already a path from to_id to from_id
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();

        queue.push_back(to_id.to_string());
        visited.insert(to_id.to_string());

        while let Some(current_id) = queue.pop_front() {
            if current_id == from_id {
                return Ok(true);
            }

            let filter = doc! { "from_asset_id": &current_id };
            let mut cursor = self.db.relations().find(filter).await?;

            while let Some(relation) = cursor.try_next().await? {
                if !visited.contains(&relation.to_asset_id) {
                    visited.insert(relation.to_asset_id.clone());
                    queue.push_back(relation.to_asset_id);
                }
            }
        }

        Ok(false)
    }
}
