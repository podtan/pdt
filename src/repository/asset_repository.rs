//! Asset repository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{AddTagRequest, Asset, CreateAssetRequest, Tag, UpdateAssetRequest};

/// Repository for asset operations
pub struct AssetRepository;

impl AssetRepository {
    /// Create a new asset
    pub async fn create(
        db: &Database,
        request: CreateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
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

        let asset = Asset {
            id: id.clone(),
            title: request.title,
            content: request.content,
            tags,
            metadata: request.metadata,
            created_at: now,
            updated_at: now,
            created_by: user_id.to_string(),
            deleted_at: None,
        };

        db.assets().insert_one(&asset).await?;

        Ok(asset)
    }

    /// Get asset by ID
    pub async fn get_by_id(db: &Database, id: &str) -> Result<Asset> {
        let filter = doc! {
            "_id": id,
            "deleted_at": null
        };

        db.assets()
            .find_one(filter)
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Asset not found: {}", id)))
    }

    /// Update an asset
    pub async fn update(
        db: &Database,
        id: &str,
        request: UpdateAssetRequest,
    ) -> Result<Asset> {
        let mut update_doc = doc! {
            "$set": {
                "updated_at": Utc::now()
            }
        };

        if let Some(title) = request.title {
            update_doc
                .get_document_mut("$set")
                .unwrap()
                .insert("title", title);
        }

        if let Some(content) = request.content {
            update_doc
                .get_document_mut("$set")
                .unwrap()
                .insert("content", content);
        }

        if let Some(metadata) = request.metadata {
            let bson_metadata = bson::to_bson(&metadata)?;
            update_doc
                .get_document_mut("$set")
                .unwrap()
                .insert("metadata", bson_metadata);
        }

        let filter = doc! {
            "_id": id,
            "deleted_at": null
        };

        db.assets().update_one(filter, update_doc).await?;

        Self::get_by_id(db, id).await
    }

    /// Soft delete an asset
    pub async fn delete(db: &Database, id: &str) -> Result<()> {
        let filter = doc! {
            "_id": id,
            "deleted_at": null
        };

        let update = doc! {
            "$set": {
                "deleted_at": Utc::now()
            }
        };

        let result = db.assets().update_one(filter, update).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!("Asset not found: {}", id)));
        }

        Ok(())
    }

    /// List assets with pagination
    /// Optionally filter by asset type tag value
    pub async fn list(
        db: &Database,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        let mut filter = doc! { "deleted_at": null };

        // Filter by asset type tag if specified
        if let Some(at) = asset_type_tag {
            filter.insert(
                "tags",
                doc! {
                    "$elemMatch": {
                        "category": "asset_type",
                        "value": at
                    }
                },
            );
        }

        if let Some(c) = cursor {
            filter.insert("_id", doc! { "$gt": c });
        }

        let options = mongodb::options::FindOptions::builder()
            .sort(doc! { "_id": 1 })
            .limit(limit + 1)
            .build();

        let mut cursor = db.assets().find(filter).with_options(options).await?;
        let mut assets = Vec::new();

        while let Some(asset) = cursor.try_next().await? {
            assets.push(asset);
        }

        let next_cursor = if assets.len() > limit as usize {
            assets.pop();
            assets.last().map(|a| a.id.clone())
        } else {
            None
        };

        Ok((assets, next_cursor))
    }

    /// Add a tag to an asset
    pub async fn add_tag(
        db: &Database,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<Tag> {
        // Verify asset exists
        Self::get_by_id(db, asset_id).await?;

        let tag = Tag {
            id: Uuid::new_v4().to_string(),
            category: request.category,
            value: request.value,
            added_by: user_id.to_string(),
            added_at: Utc::now(),
        };

        let filter = doc! { "_id": asset_id };
        let update = doc! {
            "$push": { "tags": bson::to_bson(&tag)? },
            "$set": { "updated_at": Utc::now() }
        };

        db.assets().update_one(filter, update).await?;

        Ok(tag)
    }

    /// Remove a tag from an asset
    pub async fn remove_tag(db: &Database, asset_id: &str, tag_id: &str) -> Result<()> {
        let filter = doc! { "_id": asset_id };
        let update = doc! {
            "$pull": { "tags": { "id": tag_id } },
            "$set": { "updated_at": Utc::now() }
        };

        let result = db.assets().update_one(filter, update).await?;

        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!("Asset not found: {}", asset_id)));
        }

        Ok(())
    }

    /// Search assets by text and tags
    pub async fn search(
        db: &Database,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        let mut filter = doc! { "deleted_at": null };

        if let Some(q) = query {
            if !q.is_empty() {
                filter.insert("$text", doc! { "$search": q });
            }
        }

        for (category, value) in tag_filters {
            filter.insert(
                "tags",
                doc! {
                    "$elemMatch": {
                        "category": category,
                        "value": value
                    }
                },
            );
        }

        if let Some(c) = cursor {
            filter.insert("_id", doc! { "$gt": c });
        }

        let options = mongodb::options::FindOptions::builder()
            .sort(doc! { "_id": 1 })
            .limit(limit + 1)
            .build();

        let mut cursor = db.assets().find(filter).with_options(options).await?;
        let mut assets = Vec::new();

        while let Some(asset) = cursor.try_next().await? {
            assets.push(asset);
        }

        let next_cursor = if assets.len() > limit as usize {
            assets.pop();
            assets.last().map(|a| a.id.clone())
        } else {
            None
        };

        Ok((assets, next_cursor))
    }

    /// Check if asset exists (including deleted)
    pub async fn exists(db: &Database, id: &str) -> Result<bool> {
        let filter = doc! { "_id": id };
        let count = db.assets().count_documents(filter).await?;
        Ok(count > 0)
    }
}
