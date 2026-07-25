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
