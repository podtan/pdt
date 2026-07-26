//! Collection service

use serde_json::json;

use crate::error::{ApiError, Result};
use crate::models::{AuditAction, Collection, CreateCollectionRequest, UpdateCollectionRequest};
use crate::service::Services;

/// Service for collection business logic
pub struct CollectionService;

impl CollectionService {
    /// Create a new collection with validation
    pub async fn create(
        services: &Services,
        request: CreateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        // Validate request
        if request.name.trim().is_empty() {
            return Err(ApiError::Validation("Name cannot be empty".to_string()));
        }

        // Create collection
        let collection = services.collections().create(request.clone(), user_id).await?;

        // Create audit entry
        services.audit().create(
            "collection",
            &collection.id,
            AuditAction::Create,
            json!({ "name": collection.name }),
            user_id,
        )
        .await?;

        Ok(collection)
    }

    /// Get collection by ID
    pub async fn get(services: &Services, id: &str) -> Result<Collection> {
        services.collections().get_by_id(id).await
    }

    /// Update a collection
    pub async fn update(
        services: &Services,
        id: &str,
        request: UpdateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        // Validate request
        if let Some(ref name) = request.name {
            if name.trim().is_empty() {
                return Err(ApiError::Validation("Name cannot be empty".to_string()));
            }
        }

        // Get current state for audit
        let old_collection = services.collections().get_by_id(id).await?;

        // Update collection
        let collection = services.collections().update(id, request.clone(), user_id).await?;

        // Create audit entry
        services.audit().create(
            "collection",
            id,
            AuditAction::Update,
            json!({
                "old": {
                    "name": old_collection.name,
                    "description": old_collection.description,
                },
                "new": {
                    "name": request.name,
                    "description": request.description,
                }
            }),
            user_id,
        )
        .await?;

        Ok(collection)
    }

    /// Delete a collection
    pub async fn delete(services: &Services, id: &str, user_id: &str) -> Result<()> {
        // Verify collection exists
        services.collections().get_by_id(id).await?;

        // Delete collection
        services.collections().delete(id).await?;

        // Create audit entry
        services.audit().create(
            "collection",
            id,
            AuditAction::Delete,
            json!({}),
            user_id,
        )
        .await?;

        Ok(())
    }

    /// List collections with pagination
    pub async fn list(
        services: &Services,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Collection>, Option<String>)> {
        services.collections().list(limit, cursor).await
    }

    /// Add an asset to a collection
    pub async fn add_asset(
        services: &Services,
        collection_id: &str,
        asset_id: &str,
        user_id: &str,
    ) -> Result<()> {
        // Verify asset exists
        services.assets().get_by_id(asset_id).await?;

        // Add asset to collection
        services.collections().add_asset(collection_id, asset_id).await?;

        // Create audit entry
        services.audit().create(
            "collection",
            collection_id,
            AuditAction::AddToCollection,
            json!({ "asset_id": asset_id }),
            user_id,
        )
        .await?;

        Ok(())
    }

    /// Remove an asset from a collection
    pub async fn remove_asset(
        services: &Services,
        collection_id: &str,
        asset_id: &str,
        user_id: &str,
    ) -> Result<()> {
        // Remove asset from collection
        services.collections().remove_asset(collection_id, asset_id).await?;

        // Create audit entry
        services.audit().create(
            "collection",
            collection_id,
            AuditAction::RemoveFromCollection,
            json!({ "asset_id": asset_id }),
            user_id,
        )
        .await?;

        Ok(())
    }
}
