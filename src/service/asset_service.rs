//! Asset service

use serde_json::json;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, Asset, AuditAction, CreateAssetRequest, Tag, UpdateAssetRequest,
};
use crate::repository::{AssetRepository, AuditRepository, CollectionRepository, RelationRepository};

/// Service for asset business logic
pub struct AssetService;

impl AssetService {
    /// Create a new asset with validation
    pub async fn create(
        db: &Database,
        request: CreateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Validate request
        if request.title.trim().is_empty() {
            return Err(ApiError::Validation("Title cannot be empty".to_string()));
        }

        // Create asset
        let asset = AssetRepository::create(db, request.clone(), user_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "asset",
            &asset.id,
            AuditAction::Create,
            json!({
                "title": asset.title,
                "tags": asset.tags.len(),
            }),
            user_id,
        )
        .await?;

        Ok(asset)
    }

    /// Get asset by ID
    pub async fn get(db: &Database, id: &str) -> Result<Asset> {
        AssetRepository::get_by_id(db, id).await
    }

    /// Update an asset
    pub async fn update(
        db: &Database,
        id: &str,
        request: UpdateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Validate request
        if let Some(ref title) = request.title {
            if title.trim().is_empty() {
                return Err(ApiError::Validation("Title cannot be empty".to_string()));
            }
        }

        // Get current state for audit
        let old_asset = AssetRepository::get_by_id(db, id).await?;

        // Update asset
        let asset = AssetRepository::update(db, id, request.clone()).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "asset",
            id,
            AuditAction::Update,
            json!({
                "old": {
                    "title": old_asset.title,
                    "content": old_asset.content,
                },
                "new": {
                    "title": request.title,
                    "content": request.content,
                }
            }),
            user_id,
        )
        .await?;

        Ok(asset)
    }

    /// Delete an asset (soft delete)
    pub async fn delete(db: &Database, id: &str, user_id: &str) -> Result<()> {
        // Verify asset exists
        AssetRepository::get_by_id(db, id).await?;

        // Delete asset
        AssetRepository::soft_delete(db, id).await?;

        // Delete related relations
        RelationRepository::delete_by_asset(db, id).await?;

        // Remove from collections
        CollectionRepository::remove_asset_from_all(db, id).await?;

        // Create audit entry
        AuditRepository::create(db, "asset", id, AuditAction::Delete, json!({}), user_id).await?;

        Ok(())
    }

    /// List assets with pagination
    /// Filter by asset type tag if specified
    pub async fn list(
        db: &Database,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        AssetRepository::list(db, limit, cursor, asset_type_tag).await
    }

    /// Add a tag to an asset
    pub async fn add_tag(
        db: &Database,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<Tag> {
        let tag = AssetRepository::add_tag(db, asset_id, request.clone(), user_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "asset",
            asset_id,
            AuditAction::AddTag,
            json!({
                "tag_id": tag.id,
                "category": format!("{}", tag.category),
                "value": tag.value,
            }),
            user_id,
        )
        .await?;

        Ok(tag)
    }

    /// Remove a tag from an asset
    pub async fn remove_tag(
        db: &Database,
        asset_id: &str,
        tag_id: &str,
        user_id: &str,
    ) -> Result<()> {
        AssetRepository::remove_tag(db, asset_id, tag_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "asset",
            asset_id,
            AuditAction::RemoveTag,
            json!({ "tag_id": tag_id }),
            user_id,
        )
        .await?;

        Ok(())
    }
}
