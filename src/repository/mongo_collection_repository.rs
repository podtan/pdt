//! MongoDB implementation of CollectionRepository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{Collection, CreateCollectionRequest, Tag, UpdateCollectionRequest};
use crate::repository::traits::CollectionRepository;

/// MongoDB-backed collection repository
pub struct MongoCollectionRepository {
    db: Database,
}

impl MongoCollectionRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl CollectionRepository for MongoCollectionRepository {
    async fn create(
        &self,
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
            updated_by: user_id.to_string(),
        };

        self.db.collections().insert_one(&collection).await?;

        Ok(collection)
    }

    async fn get_by_id(&self, id: &str) -> Result<Collection> {
        let filter = doc! { "_id": id };

        self.db
            .collections()
            .find_one(filter)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Collection not found: {}", id)))
    }

    async fn update(
        &self,
        id: &str,
        request: UpdateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        let mut update_doc = doc! {
            "$set": {
                "updated_at": Utc::now(),
                "updated_by": user_id
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
        let result = self.db.collections().update_one(filter, update_doc).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!("Collection not found: {}", id)));
        }

        self.get_by_id(id).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let filter = doc! { "_id": id };
        let result = self.db.collections().delete_one(filter).await?;

        if result.deleted_count == 0 {
            return Err(ApiError::NotFound(format!("Collection not found: {}", id)));
        }

        Ok(())
    }

    async fn list(
        &self,
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

        let mut cursor = self.db.collections().find(filter).with_options(options).await?;
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

    async fn add_asset(&self, collection_id: &str, asset_id: &str) -> Result<()> {
        let filter = doc! { "_id": collection_id };
        let update = doc! {
            "$addToSet": { "asset_ids": asset_id },
            "$set": { "updated_at": Utc::now() }
        };

        let result = self.db.collections().update_one(filter, update).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!(
                "Collection not found: {}",
                collection_id
            )));
        }

        Ok(())
    }

    async fn remove_asset(&self, collection_id: &str, asset_id: &str) -> Result<()> {
        let filter = doc! { "_id": collection_id };
        let update = doc! {
            "$pull": { "asset_ids": asset_id },
            "$set": { "updated_at": Utc::now() }
        };

        let result = self.db.collections().update_one(filter, update).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!(
                "Collection not found: {}",
                collection_id
            )));
        }

        Ok(())
    }

    async fn remove_asset_from_all(&self, asset_id: &str) -> Result<()> {
        let filter = doc! { "asset_ids": asset_id };
        let update = doc! {
            "$pull": { "asset_ids": asset_id },
            "$set": { "updated_at": Utc::now() }
        };

        self.db.collections().update_many(filter, update).await?;

        Ok(())
    }
}
