//! SQLite implementation of AssetRepository

use std::collections::HashMap;

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, Asset, AuthContext, CreateAssetRequest, Tag, UpdateAssetRequest,
};
use crate::repository::traits::AssetRepository;

pub struct SqliteAssetRepository {
    pool: SqlitePool,
}

impl SqliteAssetRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Hydrate an Asset from the base row + tags + metadata.
    async fn hydrate_asset(
        &self,
        row: &AssetRow,
    ) -> Result<Asset> {
        // Load tags
        let tag_rows: Vec<TagRow> = sqlx::query_as::<_, TagRow>(
            "SELECT id, category, value, added_by, added_at FROM tags WHERE asset_id = ?",
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
                added_at: parse_datetime(&t.added_at),
            })
            .collect();

        // Load metadata
        let meta_rows: Vec<MetadataRow> = sqlx::query_as::<_, MetadataRow>(
            "SELECT key, value_json FROM asset_metadata WHERE asset_id = ?",
        )
        .bind(&row.id)
        .fetch_all(&self.pool)
        .await?;

        let metadata: HashMap<String, serde_json::Value> = meta_rows
            .into_iter()
            .filter_map(|m| {
                serde_json::from_str(&m.value_json)
                    .ok()
                    .map(|v| (m.key, v))
            })
            .collect();

        // Build auth_context from flattened columns
        let auth_context = if row.visibility.is_some()
            || row.confidentiality.is_some()
            || row.owner_groups.is_some()
        {
            Some(AuthContext {
                visibility: row.visibility.clone().unwrap_or_default(),
                owner_groups: row
                    .owner_groups
                    .as_ref()
                    .and_then(|g| serde_json::from_str(g).ok())
                    .unwrap_or_default(),
                confidentiality: row.confidentiality.clone().unwrap_or_default(),
            })
        } else {
            None
        };

        Ok(Asset {
            id: row.id.clone(),
            title: row.title.clone(),
            content: row.content.clone(),
            tags,
            metadata,
            created_at: parse_datetime(&row.created_at),
            updated_at: parse_datetime(&row.updated_at),
            created_by: row.created_by.clone(),
            updated_by: row.updated_by.clone(),
            deleted_at: row.deleted_at.as_ref().map(|s| parse_datetime(s)),
            auth_context,
        })
    }

    async fn hydrate_assets(&self, rows: Vec<AssetRow>) -> Result<Vec<Asset>> {
        let mut assets = Vec::with_capacity(rows.len());
        for row in rows {
            assets.push(self.hydrate_asset(&row).await?);
        }
        Ok(assets)
    }
}

/// Reduce a raw user search string to a safe FTS5 MATCH expression.
///
/// Every non-alphanumeric-separated token is emitted as a double-quoted FTS5
/// phrase (adjacent phrases = implicit AND). This strips ALL FTS5 query
/// syntax from user input — column filters (`token:` → `no such column:`
/// failures), boolean operators, NEAR, `*` prefixes, parentheses — while
/// preserving plain multi-word matching. `char::is_alphanumeric` is
/// unicode-aware, so Persian and other non-ASCII titles stay searchable.
/// Quoted tokens can never contain a quote (the splitter removes them), so
/// no escaping is needed. Empty/whitespace/symbol-only input yields an empty
/// string; the caller skips the MATCH clause entirely in that case.
fn build_fts_match_query(raw: &str) -> String {
    raw.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t))
        .collect::<Vec<_>>()
        .join(" ")
}

#[async_trait::async_trait]
impl AssetRepository for SqliteAssetRepository {
    async fn create(&self, request: CreateAssetRequest, user_id: &str) -> Result<Asset> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let auth_ctx = request.auth_context.or_else(|| Some(AuthContext::default()));

        // Insert asset row
        sqlx::query(
            "INSERT INTO assets (id, title, content, created_at, updated_at, created_by, updated_by, deleted_at, visibility, confidentiality, owner_groups)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&request.title)
        .bind(&request.content)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(user_id)
        .bind(user_id)
        .bind(auth_ctx.as_ref().map(|c| c.visibility.clone()).unwrap_or_default())
        .bind(auth_ctx.as_ref().map(|c| c.confidentiality.clone()).unwrap_or_default())
        .bind(auth_ctx.as_ref().map(|c| serde_json::to_string(&c.owner_groups).unwrap_or_default()))
        .execute(&self.pool)
        .await?;

        // Insert tags
        for tag_req in &request.tags {
            let tag_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO tags (id, asset_id, category, value, added_by, added_at) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&tag_id)
            .bind(&id)
            .bind(&tag_req.category)
            .bind(&tag_req.value)
            .bind(user_id)
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await?;
        }

        // Insert metadata
        for (key, value) in &request.metadata {
            let value_json = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
            sqlx::query(
                "INSERT INTO asset_metadata (asset_id, key, value_json) VALUES (?, ?, ?)",
            )
            .bind(&id)
            .bind(key)
            .bind(&value_json)
            .execute(&self.pool)
            .await?;
        }

        // Re-fetch the created asset
        self.get_by_id(&id).await
    }

    async fn get_by_id(&self, id: &str) -> Result<Asset> {
        let row = sqlx::query_as::<_, AssetRow>(
            "SELECT id, title, content, created_at, updated_at, created_by, updated_by, deleted_at, visibility, confidentiality, owner_groups
             FROM assets WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ApiError::GenericDatabase(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("Asset not found: {}", id)))?;

        self.hydrate_asset(&row).await
    }

    async fn update(
        &self,
        id: &str,
        request: UpdateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        let now = Utc::now().to_rfc3339();

        // Verify exists and not deleted
        let existing = self.get_by_id(id).await?;

        // Update title
        if let Some(ref title) = request.title {
            sqlx::query("UPDATE assets SET title = ?, updated_at = ?, updated_by = ? WHERE id = ? AND deleted_at IS NULL")
                .bind(title)
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        // Update content
        if let Some(ref content) = request.content {
            sqlx::query("UPDATE assets SET content = ?, updated_at = ?, updated_by = ? WHERE id = ? AND deleted_at IS NULL")
                .bind(content)
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        // Update metadata
        if let Some(ref metadata) = request.metadata {
            // Clear existing metadata
            sqlx::query("DELETE FROM asset_metadata WHERE asset_id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;

            // Insert new metadata
            for (key, value) in metadata {
                let value_json = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
                sqlx::query("INSERT INTO asset_metadata (asset_id, key, value_json) VALUES (?, ?, ?)")
                    .bind(id)
                    .bind(key)
                    .bind(&value_json)
                    .execute(&self.pool)
                    .await?;
            }

            sqlx::query("UPDATE assets SET updated_at = ?, updated_by = ? WHERE id = ? AND deleted_at IS NULL")
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        // If nothing was set, just bump updated_at
        if request.title.is_none() && request.content.is_none() && request.metadata.is_none() {
            sqlx::query("UPDATE assets SET updated_at = ?, updated_by = ? WHERE id = ? AND deleted_at IS NULL")
                .bind(&now)
                .bind(user_id)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        self.get_by_id(id).await
    }

    async fn soft_delete(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query("UPDATE assets SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::NotFound(format!("Asset not found: {}", id)));
        }

        Ok(())
    }

    async fn list(
        &self,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        let sort_field = match sort_by {
            "created_at" => "created_at",
            _ => "updated_at",
        };

        let sort_order = if order.eq_ignore_ascii_case("asc") {
            "ASC"
        } else {
            "DESC"
        };

        let comparator = if sort_order == "DESC" { "<" } else { ">" };

        // Build query dynamically
        let mut query_str = format!(
            "SELECT a.id, a.title, a.content, a.created_at, a.updated_at, a.created_by, a.updated_by, a.deleted_at, a.visibility, a.confidentiality, a.owner_groups
             FROM assets a
             WHERE a.deleted_at IS NULL"
        );

        // Filter by asset type tag
        if asset_type_tag.is_some() {
            query_str.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM tags t WHERE t.asset_id = a.id AND t.category = 'asset_type' AND t.value = ?)")
            );
        }

        // Cursor pagination
        if let Some(c) = cursor {
            if let Some((date_str, id_part)) = c.split_once('|') {
                query_str.push_str(&format!(
                    " AND (a.{sf} {cmp} ? OR (a.{sf} = ? AND a.id {cmp} ?))",
                    sf = sort_field,
                    cmp = comparator
                ));
                // We'll bind date_str, date_str, id_part
            } else {
                query_str.push_str(&format!(" AND a.id {} ?", comparator));
            }
        }

        query_str.push_str(&format!(
            " ORDER BY a.{} {}, a.id {} LIMIT ?",
            sort_field, sort_order, sort_order
        ));

        let mut q = sqlx::query_as::<_, AssetRow>(&query_str);

        if let Some(at) = asset_type_tag {
            q = q.bind(at);
        }

        if let Some(c) = cursor {
            if let Some((date_str, id_part)) = c.split_once('|') {
                q = q.bind(date_str).bind(date_str).bind(id_part);
            } else {
                q = q.bind(c);
            }
        }

        let rows = q.bind(limit + 1).fetch_all(&self.pool).await?;

        let assets = self.hydrate_assets(rows).await?;

        let next_cursor = if assets.len() > limit as usize {
            let assets = &assets[..limit as usize];
            assets.last().map(|a| {
                let date_val = if sort_field == "created_at" {
                    a.created_at.to_rfc3339()
                } else {
                    a.updated_at.to_rfc3339()
                };
                format!("{}|{}", date_val, a.id)
            })
        } else {
            None
        };

        // Trim to limit
        let assets = if assets.len() > limit as usize {
            assets[..limit as usize].to_vec()
        } else {
            assets
        };

        Ok((assets, next_cursor))
    }

    async fn search(
        &self,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        // Build query
        let mut query_str = String::from(
            "SELECT DISTINCT a.id, a.title, a.content, a.created_at, a.updated_at, a.created_by, a.updated_by, a.deleted_at, a.visibility, a.confidentiality, a.owner_groups
             FROM assets a
             WHERE a.deleted_at IS NULL",
        );

        // Full-text search via FTS5. The user string is reduced to quoted
        // phrases BEFORE binding (see build_fts_match_query): FTS5 parses the
        // MATCH string as an expression, where a bare `token:` reads as a
        // column filter and fails the whole request with `no such column:
        // <token>` (SQLx code 1) — the Sep 7-8 prod incident (5 events, 3
        // seats: hyphenated, multi-word, and numeric queries all 500'd).
        // The SQL parameter binding was always safe; the FTS5 expression was
        // not.
        let fts_query: Option<String> = query
            .filter(|q| !q.is_empty())
            .map(build_fts_match_query)
            .filter(|q| !q.is_empty());
        if fts_query.is_some() {
            query_str.push_str(
                " AND a.rowid IN (SELECT assets_fts.rowid FROM assets_fts WHERE assets_fts MATCH ?)",
            );
        }

        // Tag filters
        for (i, _) in tag_filters.iter().enumerate() {
            query_str.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM tags t WHERE t.asset_id = a.id AND t.category = ? AND t.value = ?)"
            ));
            let _ = i;
        }

        // Cursor
        if let Some(c) = cursor {
            if let Some((date_str, id_part)) = c.split_once('|') {
                query_str.push_str(
                    " AND (a.updated_at < ? OR (a.updated_at = ? AND a.id < ?))",
                );
                let _ = (date_str, id_part);
            } else {
                query_str.push_str(" AND a.id < ?");
            }
        }

        query_str.push_str(" ORDER BY a.updated_at DESC, a.id DESC LIMIT ?");

        let mut q = sqlx::query_as::<_, AssetRow>(&query_str);

        if let Some(q_text) = &fts_query {
            q = q.bind(q_text);
        }

        for (category, value) in &tag_filters {
            q = q.bind(category).bind(value);
        }

        if let Some(c) = cursor {
            if let Some((date_str, id_part)) = c.split_once('|') {
                q = q.bind(date_str).bind(date_str).bind(id_part);
            } else {
                q = q.bind(c);
            }
        }

        let rows = q.bind(limit + 1).fetch_all(&self.pool).await?;

        let assets = self.hydrate_assets(rows).await?;

        let next_cursor = if assets.len() > limit as usize {
            let trimmed = &assets[..limit as usize];
            trimmed.last().map(|a| {
                format!("{}|{}", a.updated_at.to_rfc3339(), a.id)
            })
        } else {
            None
        };

        let assets = if assets.len() > limit as usize {
            assets[..limit as usize].to_vec()
        } else {
            assets
        };

        Ok((assets, next_cursor))
    }

    async fn add_tag(
        &self,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<Tag> {
        // Verify asset exists
        self.get_by_id(asset_id).await?;

        let tag = Tag {
            id: Uuid::new_v4().to_string(),
            category: request.category,
            value: request.value,
            added_by: user_id.to_string(),
            added_at: Utc::now(),
        };

        sqlx::query(
            "INSERT INTO tags (id, asset_id, category, value, added_by, added_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&tag.id)
        .bind(asset_id)
        .bind(&tag.category)
        .bind(&tag.value)
        .bind(&tag.added_by)
        .bind(tag.added_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE assets SET updated_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        Ok(tag)
    }

    async fn remove_tag(&self, asset_id: &str, tag_id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM tags WHERE id = ? AND asset_id = ?")
            .bind(tag_id)
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            // Could be tag not found or asset not found
            self.get_by_id(asset_id).await?; // Will 404 if asset doesn't exist
        }

        sqlx::query("UPDATE assets SET updated_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(asset_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn exists(&self, id: &str) -> Result<bool> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM assets WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(row.0 > 0)
    }

    async fn update_auth_context(
        &self,
        id: &str,
        auth_context: &AuthContext,
    ) -> Result<Asset> {
        let result = sqlx::query(
            "UPDATE assets SET visibility = ?, confidentiality = ?, owner_groups = ?, updated_at = ?
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(&auth_context.visibility)
        .bind(&auth_context.confidentiality)
        .bind(serde_json::to_string(&auth_context.owner_groups).unwrap_or_default())
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::NotFound(format!("Asset not found: {}", id)));
        }

        self.get_by_id(id).await
    }
}

// --- Row types for sqlx ---

#[derive(sqlx::FromRow)]
struct AssetRow {
    id: String,
    title: String,
    content: Option<String>,
    created_at: String,
    updated_at: String,
    created_by: String,
    updated_by: String,
    deleted_at: Option<String>,
    visibility: Option<String>,
    confidentiality: Option<String>,
    owner_groups: Option<String>,
}

#[derive(sqlx::FromRow)]
struct TagRow {
    id: String,
    category: String,
    value: String,
    added_by: String,
    added_at: String,
}

#[derive(sqlx::FromRow)]
struct MetadataRow {
    key: String,
    value_json: String,
}

fn parse_datetime(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}
