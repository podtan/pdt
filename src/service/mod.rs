//! Service layer for business logic

mod asset_service;
mod audit_service;
mod collection_service;
pub mod relation_service;
mod search_service;

pub use asset_service::AssetService;
pub use audit_service::AuditService;
pub use collection_service::CollectionService;
pub use relation_service::RelationService;
pub use search_service::SearchService;

use crate::db::Database;

/// Container for all services
#[derive(Clone)]
pub struct Services {
    db: Database,
}

impl Services {
    /// Create new services container
    pub fn new(db: Database) -> Self {
        Services { db }
    }

    /// Get database reference
    pub fn db(&self) -> &Database {
        &self.db
    }
}
