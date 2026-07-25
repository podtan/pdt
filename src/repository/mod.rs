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

#[cfg(feature = "sqlite-backend")]
mod sqlite_asset_repository;
#[cfg(feature = "sqlite-backend")]
mod sqlite_audit_repository;
#[cfg(feature = "sqlite-backend")]
mod sqlite_collection_repository;
#[cfg(feature = "sqlite-backend")]
mod sqlite_relation_repository;

#[cfg(feature = "sqlite-backend")]
pub use sqlite_asset_repository::SqliteAssetRepository;
#[cfg(feature = "sqlite-backend")]
pub use sqlite_audit_repository::SqliteAuditRepository;
#[cfg(feature = "sqlite-backend")]
pub use sqlite_collection_repository::SqliteCollectionRepository;
#[cfg(feature = "sqlite-backend")]
pub use sqlite_relation_repository::SqliteRelationRepository;
