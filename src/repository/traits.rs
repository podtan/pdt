//! Repository trait abstractions for database-agnostic data access.
//!
//! Each trait defines the contract for a repository that can be backed
//! by any database (MongoDB, SQLite, PostgreSQL, etc.).

use crate::error::Result;
use crate::models::{
    AddTagRequest, Asset, AuditAction, AuditEntry, AuthContext, Collection,
    CreateAssetRequest, CreateCollectionRequest, CreateRelationRequest, Relation,
    UpdateAssetRequest, UpdateCollectionRequest,
};
use async_trait::async_trait;

/// Repository for asset operations.
#[async_trait]
pub trait AssetRepository: Send + Sync {
    /// Create a new asset.
    async fn create(&self, request: CreateAssetRequest, user_id: &str) -> Result<Asset>;

    /// Get asset by ID (excludes soft-deleted).
    async fn get_by_id(&self, id: &str) -> Result<Asset>;

    /// Update an asset's mutable fields.
    async fn update(
        &self,
        id: &str,
        request: UpdateAssetRequest,
        user_id: &str,
    ) -> Result<Asset>;

    /// Soft delete an asset.
    async fn soft_delete(&self, id: &str) -> Result<()>;

    /// List assets with cursor-based pagination, filtering, and sorting.
    async fn list(
        &self,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<Asset>, Option<String>)>;

    /// Search assets by text query and/or tag filters.
    async fn search(
        &self,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)>;

    /// Add a tag to an asset.
    async fn add_tag(
        &self,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<crate::models::Tag>;

    /// Remove a tag from an asset.
    async fn remove_tag(&self, asset_id: &str, tag_id: &str) -> Result<()>;

    /// Check if an asset exists (including soft-deleted).
    async fn exists(&self, id: &str) -> Result<bool>;

    /// Update the auth_context of an asset.
    async fn update_auth_context(
        &self,
        id: &str,
        auth_context: &AuthContext,
    ) -> Result<Asset>;
}

/// Repository for relation operations.
#[async_trait]
pub trait RelationRepository: Send + Sync {
    /// Create a new relation.
    async fn create(
        &self,
        request: CreateRelationRequest,
        user_id: &str,
    ) -> Result<Relation>;

    /// Get relation by ID.
    async fn get_by_id(&self, id: &str) -> Result<Relation>;

    /// Delete a relation.
    async fn delete(&self, id: &str) -> Result<()>;

    /// Get all relations for an asset (both directions).
    async fn get_asset_relations(&self, asset_id: &str) -> Result<Vec<Relation>>;

    /// Delete all relations involving an asset.
    async fn delete_by_asset(&self, asset_id: &str) -> Result<u64>;

    /// Traverse the relationship graph from an asset.
    /// Returns (asset_id, depth, relations) tuples.
    async fn traverse_graph(
        &self,
        start_asset_id: &str,
        max_depth: u32,
    ) -> Result<Vec<(String, u32, Vec<Relation>)>>;

    /// Get all descendant asset IDs reachable from a root via outgoing relations.
    async fn get_descendants(&self, root_id: &str) -> Result<Vec<String>>;

    /// Check if adding a relation would create a cycle.
    async fn would_create_cycle(&self, from_id: &str, to_id: &str) -> Result<bool>;
}

/// Repository for collection operations.
#[async_trait]
pub trait CollectionRepository: Send + Sync {
    /// Create a new collection.
    async fn create(
        &self,
        request: CreateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection>;

    /// Get collection by ID.
    async fn get_by_id(&self, id: &str) -> Result<Collection>;

    /// Update a collection.
    async fn update(
        &self,
        id: &str,
        request: UpdateCollectionRequest,
        user_id: &str,
    ) -> Result<Collection>;

    /// Delete a collection.
    async fn delete(&self, id: &str) -> Result<()>;

    /// List collections with cursor-based pagination.
    async fn list(
        &self,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Collection>, Option<String>)>;

    /// Add an asset to a collection.
    async fn add_asset(&self, collection_id: &str, asset_id: &str) -> Result<()>;

    /// Remove an asset from a collection.
    async fn remove_asset(&self, collection_id: &str, asset_id: &str) -> Result<()>;

    /// Remove an asset from all collections.
    async fn remove_asset_from_all(&self, asset_id: &str) -> Result<()>;
}

/// Repository for audit operations.
#[async_trait]
pub trait AuditRepository: Send + Sync {
    /// Create a new audit entry.
    async fn create(
        &self,
        entity_type: &str,
        entity_id: &str,
        action: AuditAction,
        changes: serde_json::Value,
        user_id: &str,
    ) -> Result<AuditEntry>;

    /// List audit entries with filters and cursor-based pagination.
    async fn list(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        user_id: Option<&str>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<AuditEntry>, Option<String>)>;

    /// Get history for a specific entity.
    async fn get_entity_history(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditEntry>>;
}
