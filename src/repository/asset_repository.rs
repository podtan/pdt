//! Asset repository

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{AddTagRequest, Asset, AuthContext, CreateAssetRequest, Tag, UpdateAssetRequest};

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
            updated_by: user_id.to_string(),
            deleted_at: None,
            auth_context: request.auth_context.map(Some).unwrap_or_else(|| Some(AuthContext::default())),
        };

        db.assets().insert_one(&asset).await?;

        Ok(asset)
    }

    /// Get asset by ID
    pub async fn get_by_id(db: &Database, id: &str) -> Result<Asset> {
        let filter = doc! {
            "_id": id,
            "deleted_at": { "$exists": false }
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
        user_id: &str,
    ) -> Result<Asset> {
        let mut update_doc = doc! {
            "$set": {
                "updated_at": Utc::now(),
                "updated_by": user_id
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
            "deleted_at": { "$exists": false }
        };

        db.assets().update_one(filter, update_doc).await?;

        Self::get_by_id(db, id).await
    }

    /// Soft delete an asset
    pub async fn soft_delete(db: &Database, id: &str) -> Result<()> {
        let filter = doc! {
            "_id": id,
            "deleted_at": { "$exists": false }
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

    /// List assets with pagination and sorting
    /// Optionally filter by asset type tag value
    ///
    /// Cursor format: when sorting by a date field, the cursor is
    /// `<ISO8601_datetime>|<_id>` to correctly paginate across
    /// non-unique sort fields. For `_id` sorting, the cursor is just the `_id`.
    pub async fn list(
        db: &Database,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        let mut filter = doc! { "deleted_at": { "$exists": false } };

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

        // Build sort document
        let sort_order = match order.to_lowercase().as_str() {
            "asc" | "ascending" => 1,
            _ => -1, // Default to descending
        };

        let sort_field = match sort_by {
            "created_at" => "created_at",
            _ => "updated_at", // Default to updated_at
        };

        // Parse cursor and apply proper seek-based pagination.
        // The cursor encodes `<sort_field_value>|<_id>` so we can
        // seek past the last seen item even when the sort field is not unique.
        if let Some(c) = cursor {
            if let Some((date_str, id)) = c.split_once('|') {
                // Compound cursor: seek past (sort_field, _id)
                use chrono::DateTime;
                if let Ok(dt) = date_str.parse::<DateTime<Utc>>() {
                    let bson_dt = mongodb::bson::DateTime::from_millis(dt.timestamp_millis());
                    let comparator = if sort_order == -1 { "$lt" } else { "$gt" };
                    // Items that have a strictly "further" sort value, OR same sort value but further _id
                    filter.insert(
                        "$or",
                        bson::bson!([
                            { sort_field: { comparator: bson_dt } },
                            { sort_field: bson_dt, "_id": { comparator: id } }
                        ]),
                    );
                } else {
                    // Fallback: treat entire cursor as _id (legacy format)
                    let comparator = if sort_order == -1 { "$lt" } else { "$gt" };
                    filter.insert("_id", doc! { comparator: c });
                }
            } else {
                // Simple _id cursor (legacy format)
                let comparator = if sort_order == -1 { "$lt" } else { "$gt" };
                filter.insert("_id", doc! { comparator: c });
            }
        }

        let options = mongodb::options::FindOptions::builder()
            .sort(doc! { sort_field: sort_order, "_id": sort_order })
            .limit(limit + 1)
            .build();

        let mut cursor = db.assets().find(filter).with_options(options).await?;
        let mut assets = Vec::new();

        while let Some(asset) = cursor.try_next().await? {
            assets.push(asset);
        }

        let next_cursor = if assets.len() > limit as usize {
            assets.pop();
            // Encode cursor as `<sort_field_value>|<_id>`
            assets.last().map(|a| {
                let date_val = match sort_field {
                    "created_at" => a.created_at.to_rfc3339(),
                    _ => a.updated_at.to_rfc3339(),
                };
                format!("{}|{}", date_val, a.id)
            })
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
        let mut filter = doc! { "deleted_at": { "$exists": false } };

        if let Some(q) = query {
            if !q.is_empty() {
                // Improve search precision:
                // 1. If it's multiple words and not quoted, try to make it an AND search
                //    by prefixing each word with '+'.
                // 2. If it's already quoted, leave it as a phrase search.
                let search_query = if q.contains(' ') && !q.starts_with('"') {
                    q.split_whitespace()
                        .map(|w| {
                            if w.starts_with('+') || w.starts_with('-') {
                                w.to_string()
                            } else {
                                format!("+{}", w)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    q.to_string()
                };
                filter.insert("$text", doc! { "$search": search_query });
            }
        }

        if !tag_filters.is_empty() {
            let mut tag_conditions = Vec::new();
            for (category, value) in tag_filters {
                tag_conditions.push(doc! {
                    "tags": {
                        "$elemMatch": {
                            "category": category,
                            "value": value
                        }
                    }
                });
            }
            filter.insert("$and", tag_conditions);
        }

        if let Some(c) = cursor {
            if let Some((date_str, id)) = c.split_once('|') {
                // Compound cursor: seek past (updated_at, _id)
                use chrono::DateTime;
                if let Ok(dt) = date_str.parse::<DateTime<Utc>>() {
                    let bson_dt = mongodb::bson::DateTime::from_millis(dt.timestamp_millis());
                    filter.insert(
                        "$or",
                        bson::bson!([
                            { "updated_at": { "$lt": bson_dt } },
                            { "updated_at": bson_dt, "_id": { "$lt": id } }
                        ]),
                    );
                } else {
                    // Fallback: treat entire cursor as _id
                    filter.insert("_id", doc! { "$lt": c });
                }
            } else {
                // Simple _id cursor (legacy format)
                filter.insert("_id", doc! { "$lt": c });
            }
        }

        let options = if query.is_some() {
            // Sort by relevance (text score) when a search query is provided
            mongodb::options::FindOptions::builder()
                .sort(doc! { "score": { "$meta": "textScore" } })
                .limit(limit + 1)
                .build()
        } else {
            // Otherwise sort by updated_at descending for most recent first
            mongodb::options::FindOptions::builder()
                .sort(doc! { "updated_at": -1 })
                .limit(limit + 1)
                .build()
        };

        let mut cursor = db.assets().find(filter).with_options(options).await?;
        let mut assets = Vec::new();

        while let Some(asset) = cursor.try_next().await? {
            assets.push(asset);
        }

        let next_cursor = if assets.len() > limit as usize {
            assets.pop();
            // Encode cursor as `<updated_at>|<_id>` for consistent pagination
            assets.last().map(|a| format!("{}|{}", a.updated_at.to_rfc3339(), a.id))
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

    /// Update the auth_context of an asset
    pub async fn update_auth_context(
        db: &Database,
        id: &str,
        auth_context: &AuthContext,
    ) -> Result<Asset> {
        let filter = doc! {
            "_id": id,
            "deleted_at": { "$exists": false }
        };

        let update = doc! {
            "$set": {
                "auth_context": bson::to_bson(auth_context)?,
                "updated_at": Utc::now()
            }
        };

        let result = db.assets().update_one(filter, update).await?;
        if result.matched_count == 0 {
            return Err(ApiError::NotFound(format!("Asset not found: {}", id)));
        }

        Self::get_by_id(db, id).await
    }
}
