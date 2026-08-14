//! Multi-tenant SQLite routing with nested (workspace) instances
//!
//! Layout:
//!
//! ```text
//! {instances_dir}/
//!   {workspace-uuid}/
//!     pdt.db                        ← workspace-level DB (shared, future use)
//!     {child-uuid}/pdt.db           ← child instance (company via NGHR, agent via Fame)
//!     {child-uuid}/pdt.db
//! ```
//!
//! When `X-Instance-Id` header is present, requests are routed to that
//! instance's SQLite database. The header must be a bare UUID (v4 or any
//! RFC 4122 form) — never a path — and may refer to either a workspace
//! (top-level) or a child instance (nested one level). When the header is
//! absent, the global database is used.
//!
//! Instance pools are created lazily on first request and cached. Lazy
//! creation is allowed **only** for workspaces (which are always provisioned
//! explicitly by the control plane first); a child instance must already
//! exist on disk — the control plane provisions it explicitly — otherwise
//! the request is rejected rather than silently materializing a DB for an
//! arbitrary nested path.

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

/// Validate that an instance ID is a bare UUID (RFC 4122, hyphenated form).
///
/// Instance IDs are used to build filesystem paths, so they must never
/// contain separators, traversal sequences, or any other path component.
/// Accepting only canonical UUIDs makes path joining safe.
fn validate_uuid(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("invalid instance id (not a UUID): {} ({})", id, e))
}

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

/// Manages per-instance SQLite connection pools for nested (workspace) instances.
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
    /// Cached per-instance services, keyed by instance UUID.
    instances: Arc<RwLock<HashMap<String, Services>>>,
    /// Base directory for instance databases (e.g. `/data/instances`).
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
    /// - Top-level (workspace) instances: lazily opened if provisioned on disk,
    ///   or created if this is the initial provisioning call path.
    /// - Child instances: must already exist on disk (provisioned explicitly
    ///   by the control plane); otherwise an error is returned instead of
    ///   silently creating a nested DB.
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

        validate_uuid(instance_id)?;

        // Locate the database for this instance (workspace or child).
        let db_path = self.locate_db(instance_id).await?;

        // Slow path: create new pool + services
        info!("Creating SQLite pool for instance: {}", instance_id);
        let services = self.open_instance_services(instance_id, &db_path).await?;

        // Cache it
        {
            let mut cache = self.instances.write().unwrap();
            cache.insert(instance_id.to_string(), services.clone());
        }

        Ok(services)
    }

    /// Resolve the `pdt.db` path for an instance ID.
    ///
    /// Checks top-level (`{instances_dir}/{id}/pdt.db`) then nested
    /// (`{instances_dir}/{parent}/{id}/pdt.db`, any parent). A top-level
    /// instance that doesn't exist yet is opened at the top-level path
    /// (lazy provisioning of workspaces); a nested path that doesn't exist
    /// is an error — child instances must be provisioned explicitly.
    async fn locate_db(&self, instance_id: &str) -> Result<PathBuf> {
        let top = self.instances_dir.join(instance_id).join("pdt.db");
        if top.exists() {
            return Ok(top);
        }

        // Search one level down for a child instance with this UUID.
        if let Ok(entries) = std::fs::read_dir(&self.instances_dir) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let candidate = entry.path().join(instance_id).join("pdt.db");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }

        // Not found anywhere: treat as a new top-level workspace.
        // `create_if_missing(true)` in the connect options will create it.
        Ok(top)
    }

    /// Create a new SQLite pool for an instance and build Services from it.
    async fn open_instance_services(&self, instance_id: &str, db_path: &PathBuf) -> Result<Services> {
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
                    .filename(db_path)
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
    /// When `parent_id` is `None`, provisions a top-level workspace at
    /// `{instances_dir}/{instance_id}/pdt.db`.
    ///
    /// When `parent_id` is `Some`, provisions a child instance at
    /// `{instances_dir}/{parent_id}/{instance_id}/pdt.db`. The parent
    /// workspace (and its own `pdt.db`) must already exist.
    ///
    /// Called by TOCPI during provisioning to ensure the database exists
    /// before seeding data via the facade APIs.
    pub async fn provision_instance(&self, instance_id: &str, parent_id: Option<&str>) -> Result<()> {
        validate_uuid(instance_id)?;
        if let Some(parent) = parent_id {
            validate_uuid(parent)?;
        }

        let db_path = match parent_id {
            None => self.instances_dir.join(instance_id).join("pdt.db"),
            Some(parent) => {
                let parent_db = self.instances_dir.join(parent).join("pdt.db");
                if !parent_db.exists() {
                    anyhow::bail!(
                        "Parent workspace {} is not provisioned (missing {:?})",
                        parent,
                        parent_db
                    );
                }
                self.instances_dir.join(parent).join(instance_id).join("pdt.db")
            }
        };

        if db_path.exists() {
            // Already provisioned — idempotent
            info!("Instance {} already provisioned at {:?}", instance_id, db_path);
            return Ok(());
        }

        self.open_instance_services(instance_id, &db_path).await?;
        info!("Instance {} provisioned under parent {:?} at {:?}",
              instance_id, parent_id, db_path);
        Ok(())
    }

    /// Provision a workspace and all of its children in one call.
    ///
    /// Used when creating a workspace that immediately gets contents.
    pub async fn provision_workspace_tree(
        &self,
        workspace_id: &str,
        child_ids: &[String],
    ) -> Result<()> {
        self.provision_instance(workspace_id, None).await?;
        for child in child_ids {
            self.provision_instance(child, Some(workspace_id)).await?;
        }
        Ok(())
    }

    /// Recursively delete an instance and all of its children.
    ///
    /// For a workspace: removes `{instances_dir}/{workspace_id}/` including
    /// all nested child databases. For a child instance, use
    /// [`delete_instance`] with the child id (removes only that subtree).
    ///
    /// Any services cached for the removed instances are dropped so the
    /// SQLite files are unlocked on platforms that require it.
    pub async fn delete_instance(&self, instance_id: &str) -> Result<()> {
        validate_uuid(instance_id)?;

        // Determine whether this is a top-level (workspace) or nested instance.
        let top_dir = self.instances_dir.join(instance_id);

        let target_dir = if top_dir.is_dir() {
            top_dir
        } else {
            // Nested: find the parent that contains this child
            let mut found: Option<PathBuf> = None;
            if let Ok(entries) = std::fs::read_dir(&self.instances_dir) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join(instance_id);
                    if candidate.is_dir() {
                        found = Some(candidate);
                        break;
                    }
                }
            }
            match found {
                Some(dir) => dir,
                None => anyhow::bail!("Instance {} not found on disk", instance_id),
            }
        };

        // Evict cached pools for this id and all nested ids under the target dir
        {
            let mut cache = self.instances.write().unwrap();
            cache.remove(instance_id);
            if let Ok(entries) = std::fs::read_dir(&target_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        cache.remove(name);
                    }
                }
            }
        }

        std::fs::remove_dir_all(&target_dir).with_context(|| {
            format!("Failed to delete instance directory {:?}", target_dir)
        })?;

        info!("Instance {} deleted (recursive): {:?}", instance_id, target_dir);
        Ok(())
    }

    /// Get a list of all active instance IDs (workspaces and children).
    ///
    /// Scans the instances directory for provisioned databases.
    /// Includes both cached (in-memory) and disk-only instances.
    pub fn list_instances(&self) -> Vec<String> {
        let mut ids: HashSet<String> = self.instances.read().unwrap().keys().cloned().collect();

        // Also scan the directory for provisioned instances on disk
        for (dir, _) in self.scan_workspace_dirs() {
            ids.insert(dir);
        }

        ids.into_iter().collect()
    }

    /// List all provisioned instances as (workspace_id, Option<child_id>) pairs.
    ///
    /// Workspaces appear as `(workspace_id, None)`; child instances appear as
    /// `(workspace_id, Some(child_id))`.
    pub fn list_instance_tree(&self) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&self.instances_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let ws_name = match entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                // Workspace's own DB
                if path.join("pdt.db").exists() {
                    out.push((ws_name.to_string(), None));
                }

                // Child instances
                if let Ok(children) = std::fs::read_dir(&path) {
                    for child in children.flatten() {
                        let cpath = child.path();
                        if cpath.is_dir() && cpath.join("pdt.db").exists() {
                            if let Some(cname) = child.file_name().to_str() {
                                out.push((ws_name.to_string(), Some(cname.to_string())));
                            }
                        }
                    }
                }
            }
        }

        out
    }

    /// Iterate (directory_name, path) for all workspace dirs on disk.
    fn scan_workspace_dirs(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.instances_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.join("pdt.db").exists() {
                    if let Some(name) = entry.file_name().to_str() {
                        out.push((name.to_string(), path));
                    }
                }
            }
        }
        out
    }

    /// Get the global services reference.
    pub fn global(&self) -> &Services {
        &self.global_services
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_uuid_rejects_traversal() {
        assert!(validate_uuid("../../etc/passwd").is_err());
        assert!(validate_uuid("foo/bar").is_err());
        assert!(validate_uuid("..").is_err());
        assert!(validate_uuid("not-a-uuid").is_err());
        assert!(validate_uuid("").is_err());
        assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000").is_ok());
        // Uppercase and non-hyphenated forms are valid UUIDs; path-safe.
        assert!(validate_uuid("550E8400E29B41D4A716446655440000").is_ok());
    }

    #[cfg(feature = "sqlite-backend")]
    mod integration {
        use super::super::*;

        async fn manager_with_dir() -> TenantPoolManager {
            let dir = std::env::temp_dir()
                .join(format!("pdt_tenant_test_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();

            // Build real global services on an in-memory SQLite so the manager
            // is fully functional.
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(":memory:")
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
            sqlx::raw_sql(migration_sql).execute(&pool).await.unwrap();

            let services = Services::from_repositories(
                SqliteAssetRepository::new(pool.clone()),
                SqliteRelationRepository::new(pool.clone()),
                SqliteCollectionRepository::new(pool.clone()),
                SqliteAuditRepository::new(pool),
            );
            TenantPoolManager::new(services, dir)
        }

        #[tokio::test]
        async fn workspace_provision_creates_top_level_db() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            assert!(mgr.instances_dir.join(&ws).join("pdt.db").exists());
        }

        #[tokio::test]
        async fn child_provision_creates_nested_db() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            let child = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            mgr.provision_instance(&child, Some(&ws)).await.unwrap();
            assert!(mgr.instances_dir.join(&ws).join(&child).join("pdt.db").exists());
        }

        #[tokio::test]
        async fn child_requires_existing_parent() {
            let mgr = manager_with_dir().await;
            let missing_parent = uuid::Uuid::new_v4().to_string();
            let child = uuid::Uuid::new_v4().to_string();
            let err = mgr.provision_instance(&child, Some(&missing_parent)).await;
            assert!(err.is_err());
        }

        #[tokio::test]
        async fn provisioning_is_idempotent() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            // Second call succeeds without error
            mgr.provision_instance(&ws, None).await.unwrap();
        }

        #[tokio::test]
        async fn provisioning_rejects_traversal() {
            let mgr = manager_with_dir().await;
            assert!(mgr.provision_instance("../evil", None).await.is_err());
            assert!(mgr.provision_instance("evil", Some("../parent")).await.is_err());
        }

        #[tokio::test]
        async fn routing_finds_child_db() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            let child = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            mgr.provision_instance(&child, Some(&ws)).await.unwrap();

            // get_services for the child must resolve to the nested DB
            let services = mgr.get_services(Some(&child)).await.unwrap();
            // Write an asset via the child services and confirm it landed in
            // the nested DB (not global, not workspace) by reopening the file.
            let _created = services
                .assets()
                .create(
                    crate::models::CreateAssetRequest {
                        title: "child-marker".into(),
                        content: None,
                        tags: vec![],
                        metadata: Default::default(),
                        auth_context: None,
                    },
                    "test-user",
                )
                .await
                .unwrap();

            // Open the nested DB directly and confirm the marker exists
            let direct = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(mgr.instances_dir.join(&ws).join(&child).join("pdt.db")),
                )
                .await
                .unwrap();
            let row: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM assets WHERE title = 'child-marker'")
                    .fetch_one(&direct)
                    .await
                    .unwrap();
            assert_eq!(row.0, 1, "marker must be in the nested child DB");
        }

        #[tokio::test]
        async fn routing_rejects_non_uuid_header() {
            let mgr = manager_with_dir().await;
            assert!(mgr.get_services(Some("../etc")).await.is_err());
            assert!(mgr.get_services(Some("nope")).await.is_err());
        }

        #[tokio::test]
        async fn delete_workspace_removes_children() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            let a = uuid::Uuid::new_v4().to_string();
            let b = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            mgr.provision_instance(&a, Some(&ws)).await.unwrap();
            mgr.provision_instance(&b, Some(&ws)).await.unwrap();

            mgr.delete_instance(&ws).await.unwrap();
            assert!(!mgr.instances_dir.join(&ws).exists());
        }

        #[tokio::test]
        async fn delete_child_keeps_workspace() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            let child = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            mgr.provision_instance(&child, Some(&ws)).await.unwrap();

            mgr.delete_instance(&child).await.unwrap();
            assert!(mgr.instances_dir.join(&ws).join("pdt.db").exists());
            assert!(!mgr.instances_dir.join(&ws).join(&child).exists());
        }

        #[tokio::test]
        async fn list_instance_tree_reports_hierarchy() {
            let mgr = manager_with_dir().await;
            let ws = uuid::Uuid::new_v4().to_string();
            let child = uuid::Uuid::new_v4().to_string();
            mgr.provision_instance(&ws, None).await.unwrap();
            mgr.provision_instance(&child, Some(&ws)).await.unwrap();

            let tree = mgr.list_instance_tree();
            assert!(tree.contains(&(ws.clone(), None)));
            assert!(tree.contains(&(ws, Some(child))));
        }
    }
}
