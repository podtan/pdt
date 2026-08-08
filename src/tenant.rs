//! Multi-tenant SQLite routing
//!
//! When `X-Instance-Id` header is present, requests are routed to a per-instance
//! SQLite database at `{instances_dir}/{instance_id}/pdt.db`. When absent, the
//! global database is used.
//!
//! Instance pools are created lazily on first request and cached.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tracing::{info, warn};

use crate::repository::{
    SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteRelationRepository,
};
use crate::service::Services;

/// Default max connections per instance pool.
const INSTANCE_POOL_MAX_CONN: u32 = 3;

/// Extracts the `X-Instance-Id` header from the request.
///
/// When present, handlers use this to route data operations to the correct
/// per-instance SQLite database. When absent, the global database is used.
#[derive(Debug, Clone, Default)]
pub struct TenantContext {
    pub instance_id: Option<String>,
}

impl TenantContext {
    /// Returns the instance ID if set.
    pub fn instance_id(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }

    /// Whether this request is for a specific instance.
    pub fn is_tenant(&self) -> bool {
        self.instance_id.is_some()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for TenantContext {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let instance_id = parts
            .headers
            .get("X-Instance-Id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        Ok(TenantContext { instance_id })
    }
}

/// Manages per-instance SQLite connection pools.
///
/// Holds a reference to the global `Services` and lazily creates + caches
/// `Services` for each tenant instance.
///
/// Thread-safe via `std::sync::RwLock` — critical sections are trivially fast
/// (HashMap lookup), so blocking is negligible.
#[derive(Clone)]
pub struct TenantPoolManager {
    /// Global services (used when no X-Instance-Id header).
    global_services: Services,
    /// Cached per-instance services.
    instances: Arc<RwLock<HashMap<String, Services>>>,
    /// Base directory for instance databases (e.g. `/data/instances`).
    /// Each instance gets `{instances_dir}/{instance_id}/pdt.db`.
    instances_dir: PathBuf,
}

impl TenantPoolManager {
    /// Create a new manager with the given global services and instances directory.
    pub fn new(global_services: Services, instances_dir: PathBuf) -> Self {
        Self {
            global_services,
            instances: Arc::new(RwLock::new(HashMap::new())),
            instances_dir,
        }
    }

    /// Get services for a given tenant. Falls back to global when instance_id is None.
    ///
    /// Creates the instance pool and runs migrations lazily on first access.
    pub async fn get_services(&self, instance_id: Option<&str>) -> Result<Services> {
        let Some(instance_id) = instance_id else {
            return Ok(self.global_services.clone());
        };

        // Fast path: check cache
        {
            let cache = self.instances.read().unwrap();
            if let Some(services) = cache.get(instance_id) {
                return Ok(services.clone());
            }
        }

        // Slow path: create new pool + services
        info!("Creating SQLite pool for instance: {}", instance_id);
        let services = self.create_instance_services(instance_id).await?;

        // Cache it
        {
            let mut cache = self.instances.write().unwrap();
            cache.insert(instance_id.to_string(), services.clone());
        }

        Ok(services)
    }

    /// Create a new SQLite pool for an instance and build Services from it.
    async fn create_instance_services(&self, instance_id: &str) -> Result<Services> {
        let instance_dir = self.instances_dir.join(instance_id);
        let db_path = instance_dir.join("pdt.db");

        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create instance directory: {:?}", parent)
            })?;
        }

        info!("Instance SQLite path: {:?}", db_path);

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(INSTANCE_POOL_MAX_CONN)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&db_path)
                    .create_if_missing(true)
                    .foreign_keys(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
            )
            .await
            .context("Failed to connect to instance SQLite")?;

        // Run migrations
        let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
        sqlx::raw_sql(migration_sql)
            .execute(&pool)
            .await
            .context("Failed to run instance SQLite migrations")?;

        // Rebuild FTS index
        sqlx::raw_sql("INSERT INTO assets_fts(assets_fts) VALUES('rebuild');")
            .execute(&pool)
            .await
            .map_err(|e| warn!("Instance FTS rebuild skipped (non-fatal): {}", e))
            .ok();

        info!("Instance {} migrations applied", instance_id);

        // Build services from the instance pool
        let asset_repo = SqliteAssetRepository::new(pool.clone());
        let relation_repo = SqliteRelationRepository::new(pool.clone());
        let collection_repo = SqliteCollectionRepository::new(pool.clone());
        let audit_repo = SqliteAuditRepository::new(pool);

        Ok(Services::from_repositories(
            asset_repo,
            relation_repo,
            collection_repo,
            audit_repo,
        ))
    }

    /// Register a new instance (pre-create its database).
    ///
    /// Called by TOCPI during provisioning to ensure the database exists
    /// before seeding data via the facade API.
    pub async fn provision_instance(&self, instance_id: &str) -> Result<()> {
        self.create_instance_services(instance_id).await?;
        // The services are created and cached, but we don't need to hold them
        // — subsequent requests will find them in the cache.
        info!("Instance {} provisioned", instance_id);
        Ok(())
    }

    /// Get a list of all active instance IDs.
    ///
    /// Scans the instances directory for provisioned databases.
    /// Includes both cached (in-memory) and disk-only instances.
    pub fn list_instances(&self) -> Vec<String> {
        let mut ids: HashSet<String> = self.instances.read().unwrap().keys().cloned().collect();

        // Also scan the directory for provisioned instances on disk
        if let Ok(entries) = std::fs::read_dir(&self.instances_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if entry.path().is_dir() {
                        // Check if it has a pdt.db file
                        if entry.path().join("pdt.db").exists() {
                            ids.insert(name.to_string());
                        }
                    }
                }
            }
        }

        ids.into_iter().collect()
    }

    /// Get the global services reference.
    pub fn global(&self) -> &Services {
        &self.global_services
    }
}
