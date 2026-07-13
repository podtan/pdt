//! Asset service

use serde_json::json;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, Asset, AuditAction, CreateAssetRequest, Tag,
    UpdateAssetRequest, UpdateAuthContextRequest,
};
use crate::repository::{
    AssetRepository, AuditRepository, CollectionRepository, RelationRepository,
};

/// Service for asset business logic
pub struct AssetService;

impl AssetService {
    /// Create a new asset with validation and default auth_context
    pub async fn create(
        db: &Database,
        request: CreateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Validate request
        if request.title.trim().is_empty() {
            return Err(ApiError::Validation("Title cannot be empty".to_string()));
        }

        // Create asset (repository auto-populates default auth_context)
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
        let asset = AssetRepository::update(db, id, request.clone(), user_id).await?;

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
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        AssetRepository::list(db, limit, cursor, asset_type_tag, sort_by, order).await
    }

    /// Add a tag to an asset
    pub async fn add_tag(
        db: &Database,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<Tag> {
        // Validate the tag request
        request.validate().map_err(ApiError::Validation)?;

        let tag = AssetRepository::add_tag(db, asset_id, request.clone(), user_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "asset",
            asset_id,
            AuditAction::AddTag,
            json!({
                "tag_id": tag.id,
                "category": &tag.category,
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

    /// Update the authorization context of an asset
    pub async fn update_auth_context(
        db: &Database,
        id: &str,
        request: UpdateAuthContextRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Get current asset
        let asset = AssetRepository::get_by_id(db, id).await?;

        // Merge request fields into existing auth_context (or create default)
        let mut ctx = asset.auth_context.unwrap_or_default();
        if let Some(ref visibility) = request.visibility {
            ctx.visibility = visibility.clone();
        }
        if let Some(ref owner_groups) = request.owner_groups {
            ctx.owner_groups = owner_groups.clone();
        }
        if let Some(ref confidentiality) = request.confidentiality {
            ctx.confidentiality = confidentiality.clone();
        }

        let updated = AssetRepository::update_auth_context(db, id, &ctx).await?;

        // Cascade to descendants via relation graph if requested
        let cascaded_count = if request.cascade {
            let descendants = RelationRepository::get_descendants(db, id).await?;
            let count = descendants.len();
            for child_id in &descendants {
                if let Ok(child) = AssetRepository::get_by_id(db, child_id).await {
                    // Merge: only update fields that were explicitly set in the request,
                    // preserving child-specific overrides for unset fields
                    let mut child_ctx = child.auth_context.unwrap_or_default();
                    if request.visibility.is_some() {
                        child_ctx.visibility = ctx.visibility.clone();
                    }
                    if request.owner_groups.is_some() {
                        child_ctx.owner_groups = ctx.owner_groups.clone();
                    }
                    if request.confidentiality.is_some() {
                        child_ctx.confidentiality = ctx.confidentiality.clone();
                    }
                    let _ = AssetRepository::update_auth_context(db, child_id, &child_ctx).await;
                }
            }
            count
        } else {
            0
        };

        // Audit
        AuditRepository::create(
            db,
            "asset",
            id,
            AuditAction::Update,
            json!({
                "action": "update_auth_context",
                "auth_context": &ctx,
                "cascade": request.cascade,
                "cascaded_count": cascaded_count,
            }),
            user_id,
        )
        .await?;

        Ok(updated)
    }
}
