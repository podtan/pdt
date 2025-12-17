//! Relation repository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use std::collections::{HashSet, VecDeque};
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{CreateRelationRequest, Relation};

/// Repository for relation operations
pub struct RelationRepository;

impl RelationRepository {
    /// Create a new relation
    pub async fn create(
        db: &Database,
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

        db.relations().insert_one(&relation).await?;

        Ok(relation)
    }

    /// Get relation by ID
    pub async fn get_by_id(db: &Database, id: &str) -> Result<Relation> {
        let filter = doc! { "_id": id };

        db.relations()
            .find_one(filter)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Relation not found: {}", id)))
    }

    /// Delete a relation
    pub async fn delete(db: &Database, id: &str) -> Result<()> {
        let filter = doc! { "_id": id };
        let result = db.relations().delete_one(filter).await?;

        if result.deleted_count == 0 {
            return Err(ApiError::NotFound(format!("Relation not found: {}", id)));
        }

        Ok(())
    }

    /// Get all relations for an asset (both directions)
    pub async fn get_asset_relations(db: &Database, asset_id: &str) -> Result<Vec<Relation>> {
        let filter = doc! {
            "$or": [
                { "from_asset_id": asset_id },
                { "to_asset_id": asset_id }
            ]
        };

        let mut cursor = db.relations().find(filter).await?;
        let mut relations = Vec::new();

        while let Some(relation) = cursor.try_next().await? {
            relations.push(relation);
        }

        Ok(relations)
    }

    /// Delete all relations involving an asset
    pub async fn delete_by_asset(db: &Database, asset_id: &str) -> Result<u64> {
        let filter = doc! {
            "$or": [
                { "from_asset_id": asset_id },
                { "to_asset_id": asset_id }
            ]
        };

        let result = db.relations().delete_many(filter).await?;
        Ok(result.deleted_count)
    }

    /// Traverse the relationship graph from an asset
    /// Returns all connected assets up to the specified depth
    pub async fn traverse_graph(
        db: &Database,
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

            let relations = Self::get_asset_relations(db, &current_id).await?;

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

    /// Check for cycles that would be created by adding a relation
    pub async fn would_create_cycle(
        db: &Database,
        from_id: &str,
        to_id: &str,
    ) -> Result<bool> {
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
            let mut cursor = db.relations().find(filter).await?;

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
