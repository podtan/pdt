//! Collection service

use serde_json::json;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{AuditAction, Collection, CreateCollectionRequest, UpdateCollectionRequest};
use crate::repository::{AssetRepository, AuditRepository, CollectionRepository};

/// Service for collection business logic
pub struct CollectionService;

impl CollectionService {
    /// Create a new collection with validation
    pub async fn create(
        db: &Database,
        request: CreateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection> {
        // Validate request
        if request.name.trim().is_empty() {
            return Err(ApiError::Validation("Name cannot be empty".to_string()));
        }

        // Create collection
        let collection = CollectionRepository::create(db, request.clone(), user_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
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
    pub async fn get(db: &Database, id: &str) -> Result<Collection> {
        CollectionRepository::get_by_id(db, id).await
    }

    /// Update a collection
    pub async fn update(
        db: &Database,
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
        let old_collection = CollectionRepository::get_by_id(db, id).await?;

        // Update collection
        let collection = CollectionRepository::update(db, id, request.clone()).await?;

        // Create audit entry
        AuditRepository::create(
            db,
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
    pub async fn delete(db: &Database, id: &str, user_id: &str) -> Result<()> {
        // Verify collection exists
        CollectionRepository::get_by_id(db, id).await?;

        // Delete collection
        CollectionRepository::delete(db, id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
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
        db: &Database,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Collection>, Option<String>)> {
        CollectionRepository::list(db, limit, cursor).await
    }

    /// Add an asset to a collection
    pub async fn add_asset(
        db: &Database,
        collection_id: &str,
        asset_id: &str,
        user_id: &str,
    ) -> Result<()> {
        // Verify asset exists
        AssetRepository::get_by_id(db, asset_id).await?;

        // Add asset to collection
        CollectionRepository::add_asset(db, collection_id, asset_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
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
        db: &Database,
        collection_id: &str,
        asset_id: &str,
        user_id: &str,
    ) -> Result<()> {
        // Remove asset from collection
        CollectionRepository::remove_asset(db, collection_id, asset_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
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
