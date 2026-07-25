//! Asset service

use serde_json::json;

use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, Asset, AuditAction, CreateAssetRequest, Tag,
    UpdateAssetRequest, UpdateAuthContextRequest,
};
use crate::service::Services;

/// Service for asset business logic
pub struct AssetService;

impl AssetService {
    /// Create a new asset with validation and default auth_context
    pub async fn create(
        services: &Services,
        request: CreateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Validate request
        if request.title.trim().is_empty() {
            return Err(ApiError::Validation("Title cannot be empty".to_string()));
        }

        // Create asset (repository auto-populates default auth_context)
        let asset = services.assets().create(request.clone(), user_id).await?;

        // Create audit entry
        services.audit().create(
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
    pub async fn get(services: &Services, id: &str) -> Result<Asset> {
        services.assets().get_by_id(id).await
    }

    /// Update an asset
    pub async fn update(
        services: &Services,
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
        let old_asset = services.assets().get_by_id(id).await?;

        // Update asset
        let asset = services.assets().update(id, request.clone(), user_id).await?;

        // Create audit entry
        services.audit().create(
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
    pub async fn delete(services: &Services, id: &str, user_id: &str) -> Result<()> {
        // Verify asset exists
        services.assets().get_by_id(id).await?;

        // Delete asset
        services.assets().soft_delete(id).await?;

        // Delete related relations
        services.relations().delete_by_asset(id).await?;

        // Remove from collections
        services.collections().remove_asset_from_all(id).await?;

        // Create audit entry
        services.audit().create("asset", id, AuditAction::Delete, json!({}), user_id).await?;

        Ok(())
    }

    /// List assets with pagination
    /// Filter by asset type tag if specified
    pub async fn list(
        services: &Services,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        services.assets().list(limit, cursor, asset_type_tag, sort_by, order).await
    }

    /// Add a tag to an asset
    pub async fn add_tag(
        services: &Services,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<Tag> {
        // Validate the tag request
        request.validate().map_err(ApiError::Validation)?;

        let tag = services.assets().add_tag(asset_id, request.clone(), user_id).await?;

        // Create audit entry
        services.audit().create(
            "asset",
            asset_id,
            AuditAction::AddTag,
            json!({
                "tag_id": tag.id,
                "category": &tag.category,
                "value": &tag.value,
            }),
            user_id,
        )
        .await?;

        Ok(tag)
    }

    /// Remove a tag from an asset
    pub async fn remove_tag(
        services: &Services,
        asset_id: &str,
        tag_id: &str,
        user_id: &str,
    ) -> Result<()> {
        services.assets().remove_tag(asset_id, tag_id).await?;

        // Create audit entry
        services.audit().create(
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
        services: &Services,
        id: &str,
        request: UpdateAuthContextRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Get current asset
        let asset = services.assets().get_by_id(id).await?;

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

        let updated = services.assets().update_auth_context(id, &ctx).await?;

        // Cascade to descendants via relation graph if requested
        let cascaded_count = if request.cascade {
            let descendants = services.relations().get_descendants(id).await?;
            let count = descendants.len();
            for child_id in &descendants {
                if let Ok(child) = services.assets().get_by_id(child_id).await {
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
                    let _ = services.assets().update_auth_context(child_id, &child_ctx).await;
                }
            }
            count
        } else {
            0
        };

        // Audit
        services.audit().create(
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
