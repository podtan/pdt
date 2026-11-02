//! Service layer for business logic
//!
//! Services hold trait-based repository objects so the database backend
//! can be swapped at runtime (MongoDB, SQLite, PostgreSQL, etc.).

mod asset_service;
mod audit_service;
mod collection_service;
pub mod directory_service;
pub mod group_service;
pub mod prune_policy;
pub mod relation_service;
pub mod roles_service;
mod search_service;

pub use asset_service::AssetService;
pub use audit_service::AuditService;
pub use collection_service::CollectionService;
pub use directory_service::DirectoryService;
pub use group_service::GroupService;
pub use prune_policy::PrunePolicy;
pub use prune_policy::PruneVerdict;
pub use relation_service::RelationService;
pub use roles_service::RolesService;
pub use search_service::SearchService;

use std::sync::Arc;

use crate::repository::{
    AssetRepository, AuditRepository, CollectionRepository, GroupRepository, RelationRepository,
};

/// Container for all services with trait-based repositories.
#[derive(Clone)]
pub struct Services {
    assets: Arc<dyn AssetRepository>,
    relations: Arc<dyn RelationRepository>,
    collections: Arc<dyn CollectionRepository>,
    audit: Arc<dyn AuditRepository>,
    groups: Arc<dyn GroupRepository>,
    authorizer: Option<std::sync::Arc<pep::cedar::CedarAuthorizer>>,
}

impl Services {
    /// Create new services container from individual repositories.
    pub fn from_repositories(
        assets: impl AssetRepository + 'static,
        relations: impl RelationRepository + 'static,
        collections: impl CollectionRepository + 'static,
        audit: impl AuditRepository + 'static,
        groups: impl GroupRepository + 'static,
    ) -> Self {
        Services {
            assets: Arc::new(assets),
            relations: Arc::new(relations),
            collections: Arc::new(collections),
            audit: Arc::new(audit),
            groups: Arc::new(groups),
            authorizer: None,
        }
    }

    /// Create new services container with Cedar authorizer.
    pub fn with_cedar_repos(
        assets: impl AssetRepository + 'static,
        relations: impl RelationRepository + 'static,
        collections: impl CollectionRepository + 'static,
        audit: impl AuditRepository + 'static,
        groups: impl GroupRepository + 'static,
        authorizer: pep::cedar::CedarAuthorizer,
    ) -> Self {
        Services {
            assets: Arc::new(assets),
            relations: Arc::new(relations),
            collections: Arc::new(collections),
            audit: Arc::new(audit),
            groups: Arc::new(groups),
            authorizer: Some(std::sync::Arc::new(authorizer)),
        }
    }

    /// Get asset repository reference.
    pub fn assets(&self) -> &dyn AssetRepository {
        self.assets.as_ref()
    }

    /// Get relation repository reference.
    pub fn relations(&self) -> &dyn RelationRepository {
        self.relations.as_ref()
    }

    /// Get collection repository reference.
    pub fn collections(&self) -> &dyn CollectionRepository {
        self.collections.as_ref()
    }

    /// Get audit repository reference.
    pub fn audit(&self) -> &dyn AuditRepository {
        self.audit.as_ref()
    }

    /// Get group repository reference (PDT-native per-instance groups).
    pub fn groups(&self) -> &dyn GroupRepository {
        self.groups.as_ref()
    }

    /// Get Cedar authorizer reference (if enabled).
    pub fn authorizer(&self) -> Option<&pep::cedar::CedarAuthorizer> {
        self.authorizer.as_deref()
    }

    /// Check whether Cedar authorization is enabled.
    pub fn cedar_enabled(&self) -> bool {
        self.authorizer.is_some()
    }
}
