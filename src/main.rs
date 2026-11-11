//! PDT Server Entry Point

use anyhow::Context;
use std::net::SocketAddr;
use std::path::PathBuf;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use pdt::auth::middleware::AuthLayer;
use pdt::config::{Config, DatabaseBackend};
use pdt::db::Database;
use pdt::repository::{
    MongoAssetRepository, MongoAuditRepository, MongoCollectionRepository, MongoGroupRepository,
    MongoRelationRepository,
};
use pdt::router::build_router;
use pdt::service::Services;
use pdt::tenant::TenantPoolManager;

#[cfg(feature = "sqlite-backend")]
use pdt::repository::{
    SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteGroupRepository, SqliteRelationRepository,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Config::from_env()?;
    tracing::info!(
        "Configuration loaded (backend: {:?})",
        config.database_backend
    );

    // Initialize services based on configured backend
    let services = match config.database_backend {
        DatabaseBackend::Mongodb => {
            // Connect to MongoDB/DocumentDB
            let db = Database::connect(&config.database).await?;
            tracing::info!("Connected to MongoDB: {}", config.database.database);
            db.create_indices().await?;
            tracing::info!("Database indices created");

            let asset_repo = MongoAssetRepository::new(db.clone());
            let relation_repo = MongoRelationRepository::new(db.clone());
            let collection_repo = MongoCollectionRepository::new(db.clone());
            let audit_repo = MongoAuditRepository::new(db.clone());
            let group_repo = MongoGroupRepository::new(db);

            Services::from_repositories(
                asset_repo,
                relation_repo,
                collection_repo,
                audit_repo,
                group_repo,
            )
        }
        #[cfg(feature = "sqlite-backend")]
        DatabaseBackend::Sqlite => {
            let db_path = &config.sqlite_path;
            tracing::info!("Connecting to SQLite: {}", db_path);

            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(5)
                // Pool hygiene (issue b32a486b): a pooled connection that
                // ends up in a stale WAL-snapshot state (leaked transaction
                // from a DATABASE_ERROR storm) serves pre-edit reads until
                // process restart. max_lifetime recycles such a connection
                // within 30 min of first use; idle_timeout reaps never-
                // reused idle connections after 5 min — the class self-
                // heals instead of persisting.
                .max_lifetime(std::time::Duration::from_secs(1800))
                .idle_timeout(std::time::Duration::from_secs(300))
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(db_path)
                        .create_if_missing(true)
                        .foreign_keys(true)
                        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
                )
                .await
                .context("Failed to connect to SQLite")?;

            // Run migrations
            let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
            sqlx::raw_sql(migration_sql)
                .execute(&pool)
                .await
                .context("Failed to run SQLite migrations")?;
            let groups_sql = include_str!("../migrations/sqlite/002_groups.sql");
            sqlx::raw_sql(groups_sql)
                .execute(&pool)
                .await
                .context("Failed to run groups migrations (002_groups)")?;
            tracing::info!("SQLite migrations applied");

            // Rebuild FTS index for any pre-existing data (e.g. from migration script)
            sqlx::raw_sql("INSERT INTO assets_fts(assets_fts) VALUES('rebuild');")
                .execute(&pool)
                .await
                .map_err(|e| tracing::warn!("FTS rebuild skipped (non-fatal): {}", e))
                .ok();
            tracing::info!("FTS index rebuilt");

            let asset_repo = SqliteAssetRepository::new(pool.clone());
            let relation_repo = SqliteRelationRepository::new(pool.clone());
            let collection_repo = SqliteCollectionRepository::new(pool.clone());
            let audit_repo = SqliteAuditRepository::new(pool.clone());
            let group_repo = SqliteGroupRepository::new(pool);

            Services::from_repositories(
                asset_repo,
                relation_repo,
                collection_repo,
                audit_repo,
                group_repo,
            )
        }
    };

    // Backend-agnostic bootstrap: the distinguished instance-admins group
    // exists in EVERY store (workstream 69b4bb4c, a9f91cfb) — the sqlite
    // migration seeds it too; this covers every backend.
    pdt::service::group_service::GroupService::ensure_instance_admins_group(&services)
        .await
        .context("Failed to ensure the instance-admins group")?;
    // The DESIGNED seed mechanism (CPO ruling on defect c0c606c6, option
    // (b)): bootstrap principals (PDT_SEED_INSTANCE_ADMINS) land in the
    // global store's instance-admins at boot — audited, idempotent,
    // per-sub non-fatal.
    pdt::service::group_service::GroupService::seed_instance_admins(
        &services,
        &config.seed_instance_admins,
        "system",
    )
    .await
    .context("Failed to seed instance-admins bootstrap principals")?;

    // Initialize Cedar authorization (optional — gracefully disabled when CEDAR_ENABLED=false)
    let authorizer = if config.cedar.enabled {
        let cedar_config: pep::cedar::CedarConfig = config.cedar.clone().into();
        match pep::cedar::CedarAuthorizer::new_with_policy_store(cedar_config).await {
            Ok(authorizer) => {
                tracing::info!("Cedar authorizer initialized (embedded policies loaded)");
                Some(authorizer)
            }
            Err(e) => {
                tracing::error!(
                    "Failed to initialize Cedar authorizer: {}. Running without Cedar.",
                    e
                );
                None
            }
        }
    } else {
        tracing::info!("Cedar authorization disabled");
        None
    };

    // Initialize multi-tenant pool manager
    let instances_dir =
        std::env::var("PDT_INSTANCES_DIR").unwrap_or_else(|_| "/data/instances".to_string());
    let tenant_manager = TenantPoolManager::new(
        services,
        PathBuf::from(&instances_dir),
        config.seed_instance_admins.clone(),
    );
    tracing::info!(
        "Multi-tenant routing enabled (instances dir: {})",
        instances_dir
    );

    // Initialize auth
    // Trusted-issuer allowlist (issue 895cbcea, the 0.3.23 knob): the
    // assembly lives in the lib (pdt::auth::build_auth_client) so the gate
    // tests exercise the exact production shape — unset/empty = the client
    // is constructed WITHOUT the knob, byte-identical strict single-issuer
    // validation (fail-closed preserved, the pep law).
    let auth_client = pdt::auth::build_auth_client(&config.auth);
    match config.auth.trusted_issuers.as_deref() {
        Some(list) if !list.is_empty() => tracing::info!(
            "Trusted-issuer allowlist armed: {} issuer(s) beyond the configured issuer",
            list.len()
        ),
        _ => tracing::info!("Trusted-issuer allowlist unset — strict single-issuer validation"),
    }
    let auth_layer = AuthLayer::new(config.auth.clone(), auth_client);
    tracing::info!("Auth layer initialized (enabled: {})", config.auth.enabled);

    // Build router — assembly lives in the lib (pdt::router::build_router)
    // so the gate tests exercise the exact production shape (issue e0db0993).
    let app = build_router(tenant_manager, auth_layer, authorizer);

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.port)
        .parse::<SocketAddr>()
        .context("Invalid listen address")?;
    tracing::info!("Starting PDT server on {}", addr);
    tracing::info!("Swagger UI: http://{}/swagger-ui", addr);
    tracing::info!("OpenAPI spec: http://{}/api-docs/openapi.json", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
