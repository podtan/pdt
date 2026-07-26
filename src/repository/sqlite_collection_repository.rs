//! SQLite implementation of CollectionRepository

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::error::{ApiError, Result};
use crate::models::{Collection, CreateCollectionRequest, Tag, UpdateCollectionRequest};
use crate::repository::traits::CollectionRepository;

pub struct SqliteCollectionRepository {
    pool: SqlitePool,
}

impl SqliteCollectionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn hydrate_collection(&self, row: &CollectionRow) -> Result<Collection> {
        // Load tags
        let tag_rows: Vec<CollectionTagRow> = sqlx::query_as::<_, CollectionTagRow>(
            "SELECT id, category, value, added_by, added_at FROM collection_tags WHERE collection_id = ?",
        )
        .bind(&row.id)
        .fetch_all(&self.pool)
        .await?;

        let tags: Vec<Tag> = tag_rows
            .into_iter()
            .map(|t| Tag {
                id: t.id,
                category: t.category,
                value: t.value,
                added_by: t.added_by,
                added_at: chrono::DateTime::parse_from_rfc3339(&t.added_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
            .collect();

        // Load asset_ids
        let asset_id_rows: Vec<(String,)> =
            sqlx::query_as("SELECT asset_id FROM collection_assets WHERE collection_id = ?")
                .bind(&row.id)
                .fetch_all(&self.pool)
                .await?;

        let asset_ids: Vec<String> = asset_id_rows.into_iter().map(|r| r.0).collect();

        Ok(Collection {
            id: row.id.clone(),
            name: row.name.clone(),
            description: row.description.clone(),
            tags,
            asset_ids,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            created_by: row.created_by.clone(),
            updated_by: row.updated_by.clone(),
        })
    }
}

#[async_trait::async_trait]
impl CollectionRepository for SqliteCollectionRepository {
    async fn create(
        &self,
        request: CreateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO collections (id, name, description, created_at, updated_at, created_by, updated_by)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&request.name)
        .bind(&request.description)
        .bind(&now)
        .bind(&now)
        .bind(user_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        // Insert tags
        for tag_req in &request.tags {
            let tag_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO collection_tags (id, collection_id, category, value, added_by, added_at) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&tag_id)
            .bind(&id)
            .bind(&tag_req.category)
            .bind(&tag_req.value)
            .bind(user_id)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }

        let row = CollectionRow {
            id,
            name: request.name,
            description: request.description,
            created_at: now.clone(),
            updated_at: now,
            created_by: user_id.to_string(),
            updated_by: user_id.to_string(),
        };

        self.hydrate_collection(&row).await
    }

    async fn get_by_id(&self, id: &str) -> Result<Collection> {
        let row = sqlx::query_as::<_, CollectionRow>(
            "SELECT id, name, description, created_at, updated_at, created_by, updated_by
             FROM collections WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Collection not found: {}", id)))?;

        self.hydrate_collection(&row).await
    }

    async fn update(
        &self,
        id: &str,
        request: UpdateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        let now = Utc::now().to_rfc3339();

        // Verify exists
        let _existing = self.get_by_id(id).await?;

        if let Some(ref name) = request.name {
            sqlx::query("UPDATE collections SET name = ?, updated_at = ?, updated_by = ? WHERE id = ?")
                .bind(name)
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        if let Some(ref description) = request.description {
            sqlx::query("UPDATE collections SET description = ?, updated_at = ?, updated_by = ? WHERE id = ?")
                .bind(description)
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        if request.name.is_none() && request.description.is_none() {
            sqlx::query("UPDATE collections SET updated_at = ?, updated_by = ? WHERE id = ?")
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        self.get_by_id(id).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM collections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::NotFound(format!("Collection not found: {}", id)));
        }

        Ok(())
    }

    async fn list(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Collection>, Option<String>)> {
        let query_str = if cursor.is_some() {
            "SELECT id, name, description, created_at, updated_at, created_by, updated_by
             FROM collections WHERE id > ? ORDER BY id ASC LIMIT ?"
        } else {
            "SELECT id, name, description, created_at, updated_at, created_by, updated_by
             FROM collections ORDER BY id ASC LIMIT ?"
        };

        let mut q = sqlx::query_as::<_, CollectionRow>(query_str);

        if let Some(c) = cursor {
            q = q.bind(c);
        }

        let rows = q.bind(limit + 1).fetch_all(&self.pool).await?;

        let mut collections = Vec::with_capacity(rows.len());
        for row in rows {
            collections.push(self.hydrate_collection(&row).await?);
        }

        let next_cursor = if collections.len() > limit as usize {
            collections.last().map(|c| c.id.clone())
        } else {
            None
        };

        // Trim to limit
        if collections.len() > limit as usize {
            collections.truncate(limit as usize);
        }

        Ok((collections, next_cursor))
    }

    async fn add_asset(&self, collection_id: &str, asset_id: &str) -> Result<()> {
        // Verify collection exists
        let exists: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM collections WHERE id = ?")
                .bind(collection_id)
                .fetch_one(&self.pool)
                .await?;

        if exists.0 == 0 {
            return Err(ApiError::NotFound(format!(
                "Collection not found: {}",
                collection_id
            )));
        }

        sqlx::query("INSERT OR IGNORE INTO collection_assets (collection_id, asset_id) VALUES (?, ?)")
            .bind(collection_id)
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        sqlx::query("UPDATE collections SET updated_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(collection_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn remove_asset(&self, collection_id: &str, asset_id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM collection_assets WHERE collection_id = ? AND asset_id = ?")
            .bind(collection_id)
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        // Verify collection exists (even if no rows were deleted)
        if result.rows_affected() == 0 {
            let exists: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM collections WHERE id = ?")
                    .bind(collection_id)
                    .fetch_one(&self.pool)
                    .await?;

            if exists.0 == 0 {
                return Err(ApiError::NotFound(format!(
                    "Collection not found: {}",
                    collection_id
                )));
            }
        }

        sqlx::query("UPDATE collections SET updated_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(collection_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn remove_asset_from_all(&self, asset_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM collection_assets WHERE asset_id = ?")
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        sqlx::query("UPDATE collections SET updated_at = ? WHERE id IN (SELECT collection_id FROM collection_assets WHERE asset_id = ?)")
            .bind(Utc::now().to_rfc3339())
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct CollectionRow {
    id: String,
    name: String,
    description: Option<String>,
    created_at: String,
    updated_at: String,
    created_by: String,
    updated_by: String,
}

#[derive(sqlx::FromRow)]
struct CollectionTagRow {
    id: String,
    category: String,
    value: String,
    added_by: String,
    added_at: String,
}
