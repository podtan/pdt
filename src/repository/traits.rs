//! Repository trait abstractions for database-agnostic data access.
//!
//! Each trait defines the contract for a repository that can be backed
//! by any database (MongoDB, SQLite, PostgreSQL, etc.).

use crate::error::Result;
use crate::models::{
    AddTagRequest, Asset, AuditAction, AuditEntry, AuthContext, Collection, CreateAssetRequest,
    CreateCollectionRequest, CreateRelationRequest, Relation, UpdateAssetRequest,
    UpdateCollectionRequest,
};
use async_trait::async_trait;
use std::collections::HashMap;

/// Repository for asset operations.
#[async_trait]
pub trait AssetRepository: Send + Sync {
    /// Create a new asset.
    async fn create(&self, request: CreateAssetRequest, user_id: &str) -> Result<Asset>;

    /// Get asset by ID (excludes soft-deleted).
    async fn get_by_id(&self, id: &str) -> Result<Asset>;

    /// Update an asset's mutable fields.
    async fn update(&self, id: &str, request: UpdateAssetRequest, user_id: &str) -> Result<Asset>;

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

    /// Compact search: same matching, ordering, and pagination as `search`,
    /// but the page rows carry NO content. The compact response path only
    /// needs a snippet, which the caller fetches for the visible page via
    /// `get_contents` in a second phase (issue eea5bcff — the full-body
    /// SELECT read every content blob off disk per search, then the response
    /// discarded it).
    ///
    /// Default: delegate to `search` (backends without a content-less
    /// projection return full rows; the second-phase fetch is then a no-op
    /// overwrite with identical values).
    async fn search_compact(
        &self,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        self.search(query, tag_filters, limit, cursor).await
    }

    /// Count assets matching the search filter (text query + tag filters) —
    /// the true total for PaginatedResponse (issue b040d645: the field
    /// existed but was never populated; boards rendered page-local counts
    /// as if they were the whole set). Cursor-independent: counts ALL
    /// matches in the store, not just this page. Pre-visibility: the count
    /// reflects the filter, not the caller's Cedar view (documented in the
    /// spec; the visibility guard protects content, and the pagination
    /// envelope is already cursor-over-the-store).
    async fn count_search(
        &self,
        query: Option<&str>,
        tag_filters: &[(String, String)],
    ) -> Result<u64>;

    /// Count assets matching the list filter (asset_type tag) — the true
    /// total for /api/assets PaginatedResponse (issue b040d645).
    /// Cursor-independent, pre-visibility — same semantics as count_search.
    async fn count_list(&self, asset_type_tag: Option<&str>) -> Result<u64>;

    /// Batch content fetch for a set of asset ids (compact-path second
    /// phase). Absent and soft-deleted ids are simply missing from the map.
    async fn get_contents(&self, ids: &[String]) -> Result<HashMap<String, String>>;

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
    async fn update_auth_context(&self, id: &str, auth_context: &AuthContext) -> Result<Asset>;

    /// ADOPT: materialize an asset with a SPECIFIED id (same-uuid adopt —
    /// CPO ruling 6d86ee70, the Path A adopt's first-class path; replaces
    /// the barred direct-sqlite restore).
    ///
    /// - duplicate-barred: refuses loudly if the id exists live OR
    ///   soft-deleted (the PK is the hard bar — an adopt over a
    ///   soft-deleted row would be a resurrection, a different operation).
    /// - audited ATOMICALLY: the audit entry (action=adopt, provenance in
    ///   changes) and the asset land in ONE transaction — a failed audit
    ///   write DENIES the adopt (unaudited materialization = silent
    ///   widening, fail-loud).
    /// - additive: plain create keeps minting new uuids; birth semantics
    ///   untouched.
    async fn adopt(
        &self,
        id: &str,
        request: CreateAssetRequest,
        user_id: &str,
        provenance: &str,
    ) -> Result<Asset>;
}

/// Repository for relation operations.
#[async_trait]
pub trait RelationRepository: Send + Sync {
    /// Create a new relation.
    async fn create(&self, request: CreateRelationRequest, user_id: &str) -> Result<Relation>;

    /// Get relation by ID.
    async fn get_by_id(&self, id: &str) -> Result<Relation>;

    /// Delete a relation.
    async fn delete(&self, id: &str) -> Result<()>;

    /// Get all relations for an asset (both directions).
    async fn get_asset_relations(&self, asset_id: &str) -> Result<Vec<Relation>>;

    /// List all relations of a given type (backfill scans — 1552c098).
    async fn list_by_type(&self, relation_type: &str) -> Result<Vec<Relation>>;

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
    async fn create(&self, request: CreateCollectionRequest, user_id: &str) -> Result<Collection>;

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

/// Repository for group operations (workstream 69b4bb4c, task a9f91cfb —
/// PDT-native per-instance groups). Groups live per-instance by
/// construction: the store IS the instance.
#[async_trait]
pub trait GroupRepository: Send + Sync {
    /// Create a group. Name is unique per instance store.
    async fn create_group(
        &self,
        name: &str,
        description: &str,
        user_id: &str,
    ) -> Result<crate::models::Group>;

    /// Get a group by id.
    async fn get_group(&self, id: &str) -> Result<crate::models::Group>;

    /// Find a group by its canonical name.
    async fn get_group_by_name(&self, name: &str) -> Result<Option<crate::models::Group>>;

    /// List all groups in this instance store.
    async fn list_groups(&self) -> Result<Vec<crate::models::Group>>;

    /// Delete a group (its memberships cascade).
    async fn delete_group(&self, id: &str) -> Result<()>;

    /// Add a member (principal id) to a group.
    async fn add_member(
        &self,
        group_id: &str,
        member_id: &str,
        user_id: &str,
    ) -> Result<crate::models::GroupMember>;

    /// Remove a member from a group.
    async fn remove_member(&self, group_id: &str, member_id: &str) -> Result<()>;

    /// List a group's members.
    async fn list_members(&self, group_id: &str) -> Result<Vec<crate::models::GroupMember>>;

    /// Resolve a principal's group NAMES at read time (Phase 1b: the
    /// cedar evaluator merges these into effective_groups per request).
    async fn memberships_for(&self, member_id: &str) -> Result<Vec<String>>;

    /// Is the principal a member of the distinguished instance-admins
    /// group (the admin bypass gate)? Fail-closed on absence.
    async fn is_instance_admin(&self, member_id: &str) -> Result<bool>;
}

/// Repository for audit operations.
// clippy::double_must_use fires on the async_trait expansion (the macro
// adds #[must_use] to the returned Box<dyn Future>, already must_use) —
// repo-wide class on this toolchain (45 instances on HEAD across all five
// traits); scoped here so the audit-prune additions (2c71e4e6) add no new
// instances.
#[async_trait]
#[allow(clippy::double_must_use)]
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

    /// Count audit entries older than the cutoff — the audit-prune candidate
    /// count (issue 2c71e4e6: the audit-entries retention verb).
    async fn count_before(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<i64>;

    /// Sample of the newest audit entries older than the cutoff — the
    /// audit-prune DRY-RUN report's sample (issue 2c71e4e6). Timestamp-DESC
    /// order ON PURPOSE: the sample rides the timestamp index (a plain
    /// index-ordered read, bounded by LIMIT for ANY match count), where the
    /// id-ordered shape full-scans the table when the cutoff matches few or
    /// zero rows (the >120s band-class hang). This is a REPORT, not the
    /// archive walk — the exactness contract belongs to the materialized-id
    /// walk (`ids_before` + `list_by_ids`) alone.
    async fn sample_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: i64,
    ) -> Result<Vec<AuditEntry>>;

    /// The candidate id set for the audit-prune archive walk — ONE
    /// index-driven pass (`WHERE timestamp < ?`, served by
    /// idx_audit_timestamp; no ORDER BY, so the planner never falls back to
    /// a sort or a rowid scan). The candidate set is STABLE during the
    /// walk: rows created mid-walk carry timestamps newer than the cutoff,
    /// so they are never candidates (issue 2c71e4e6).
    async fn ids_before(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<Vec<String>>;

    /// Full rows for an EXPLICIT id set — the archive walk's fetch phase.
    /// PK-driven (`id IN (...)`): exact, no re-scan, no cursor. The
    /// per-chunk id-ASC re-scan shape it replaces (`WHERE timestamp < ? AND
    /// id > ? ORDER BY id`) made every 500-row page re-scan forward through
    /// the table on band-class volume — 238,958 candidates ≈ 160 pages ≈
    /// 25–40 min on prod (paydar's 2026-11-07 decode; the materialized-id
    /// shape did the same volume in ~90s manually). The CALLER chunks the
    /// id set and archives every returned row; the row-count verify refuses
    /// a partial archive (issue 2c71e4e6).
    async fn list_by_ids(&self, ids: &[String]) -> Result<Vec<AuditEntry>>;

    /// Delete audit entries older than the cutoff — the audit-prune delete
    /// verb. Returns the rows affected. The CALLER archives + verifies the
    /// archive FIRST (archive-then-delete; the archive is the record — no
    /// silent deletes, issue 2c71e4e6).
    async fn delete_before(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<u64>;
}
