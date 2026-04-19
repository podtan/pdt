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
    authorizer: Option<std::sync::Arc<pep::cedar::CedarAuthorizer>>,
}

impl Services {
    /// Create new services container (without Cedar)
    pub fn new(db: Database) -> Self {
        Services {
            db,
            authorizer: None,
        }
    }

    /// Create new services container with Cedar authorizer
    pub fn with_cedar(db: Database, authorizer: pep::cedar::CedarAuthorizer) -> Self {
        Services {
            db,
            authorizer: Some(std::sync::Arc::new(authorizer)),
        }
    }

    /// Get database reference
    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Get Cedar authorizer reference (if enabled)
    pub fn authorizer(&self) -> Option<&pep::cedar::CedarAuthorizer> {
        self.authorizer.as_deref()
    }

    /// Check whether Cedar authorization is enabled
    pub fn cedar_enabled(&self) -> bool {
        self.authorizer.is_some()
    }
}
