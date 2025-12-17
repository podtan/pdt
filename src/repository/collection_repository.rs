//! Collection repository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{Collection, CreateCollectionRequest, Tag, UpdateCollectionRequest};

/// Repository for collection operations
pub struct CollectionRepository;

impl CollectionRepository {
    /// Create a new collection
    pub async fn create(
        db: &Database,
        request: CreateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();

        let tags: Vec<Tag> = request
            .tags
            .into_iter()
            .map(|t| Tag {
                id: Uuid::new_v4().to_string(),
                category: t.category,
                value: t.value,
                added_by: user_id.to_string(),
                added_at: now,
            })
            .collect();

        let collection = Collection {
            id: id.clone(),
            name: request.name,
            description: request.description,
            tags,
            asset_ids: Vec::new(),
            created_at: now,
            updated_at: now,
            created_by: user_id.to_string(),
        };

        db.collections().insert_one(&collection).await?;

        Ok(collection)
    }

    /// Get collection by ID
    pub async fn get_by_id(db: &Database, id: &str) -> Result<Collection> {
        let filter = doc! { "_id": id };

        db.collections()
            .find_one(filter)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Collection not found: {}", id)))
    }

    /// Update a collection
    pub async fn update(
        db: &Database,
        id: &str,
        request: UpdateCollectionRequest,
    ) -> Result<Collection> {
        let mut update_doc = doc! {
            "$set": {
                "updated_at": Utc::now()
            }
        };

        if let Some(name) = request.name {
            update_doc
                .get_document_mut("$set")
                .unwrap()
                .insert("name", name);
        }

        if let Some(description) = request.description {
            update_doc
                .get_document_mut("$set")
                .unwrap()
                .insert("description", description);
        }

        let filter = doc! { "_id": id };
        let result = db.collections().update_one(filter, update_doc).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!("Collection not found: {}", id)));
        }

        Self::get_by_id(db, id).await
    }

    /// Delete a collection
    pub async fn delete(db: &Database, id: &str) -> Result<()> {
        let filter = doc! { "_id": id };
        let result = db.collections().delete_one(filter).await?;

        if result.deleted_count == 0 {
            return Err(ApiError::NotFound(format!("Collection not found: {}", id)));
        }

        Ok(())
    }

    /// List collections with pagination
    pub async fn list(
        db: &Database,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Collection>, Option<String>)> {
        let mut filter = doc! {};

        if let Some(c) = cursor {
            filter.insert("_id", doc! { "$gt": c });
        }

        let options = mongodb::options::FindOptions::builder()
            .sort(doc! { "_id": 1 })
            .limit(limit + 1)
            .build();

        let mut cursor = db.collections().find(filter).with_options(options).await?;
        let mut collections = Vec::new();

        while let Some(collection) = cursor.try_next().await? {
            collections.push(collection);
        }

        let next_cursor = if collections.len() > limit as usize {
            collections.pop();
            collections.last().map(|c| c.id.clone())
        } else {
            None
        };

        Ok((collections, next_cursor))
    }

    /// Add an asset to a collection
    pub async fn add_asset(db: &Database, collection_id: &str, asset_id: &str) -> Result<()> {
        let filter = doc! { "_id": collection_id };
        let update = doc! {
            "$addToSet": { "asset_ids": asset_id },
            "$set": { "updated_at": Utc::now() }
        };

        let result = db.collections().update_one(filter, update).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!(
                "Collection not found: {}",
                collection_id
            )));
        }

        Ok(())
    }

    /// Remove an asset from a collection
    pub async fn remove_asset(db: &Database, collection_id: &str, asset_id: &str) -> Result<()> {
        let filter = doc! { "_id": collection_id };
        let update = doc! {
            "$pull": { "asset_ids": asset_id },
            "$set": { "updated_at": Utc::now() }
        };

        let result = db.collections().update_one(filter, update).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!(
                "Collection not found: {}",
                collection_id
            )));
        }

        Ok(())
    }

    /// Remove an asset from all collections
    pub async fn remove_asset_from_all(db: &Database, asset_id: &str) -> Result<()> {
        let filter = doc! { "asset_ids": asset_id };
        let update = doc! {
            "$pull": { "asset_ids": asset_id },
            "$set": { "updated_at": Utc::now() }
        };

        db.collections().update_many(filter, update).await?;

        Ok(())
    }
}
