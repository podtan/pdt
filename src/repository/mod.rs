//! Repository layer for database operations
//!
//! This module defines trait abstractions for all repositories and provides
//! concrete implementations for different database backends.

pub mod traits;

mod mongo_asset_repository;
mod mongo_audit_repository;
mod mongo_collection_repository;
mod mongo_relation_repository;

pub use traits::{AssetRepository, AuditRepository, CollectionRepository, RelationRepository};
pub use mongo_asset_repository::MongoAssetRepository;
pub use mongo_audit_repository::MongoAuditRepository;
pub use mongo_collection_repository::MongoCollectionRepository;
pub use mongo_relation_repository::MongoRelationRepository;

// SQLite repositories are UNCONDITIONALLY compiled: sqlx is non-optional and
// tenant.rs instantiates these for every instance leaf regardless of which
// backend backs the ROOT database. The `sqlite-backend` feature gates only
// the ROOT-database SELECTION (config.rs DatabaseBackend + main.rs wiring).
mod sqlite_asset_repository;
mod sqlite_audit_repository;
mod sqlite_collection_repository;
mod sqlite_relation_repository;

pub use sqlite_asset_repository::SqliteAssetRepository;
pub use sqlite_audit_repository::SqliteAuditRepository;
pub use sqlite_collection_repository::SqliteCollectionRepository;
pub use sqlite_relation_repository::SqliteRelationRepository;
