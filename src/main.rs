//! PDT Server Entry Point

use anyhow::Context;
use std::net::SocketAddr;
use std::path::PathBuf;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use pdt::auth::middleware::AuthLayer;
use pdt::config::{Config, DatabaseBackend};
use pdt::db::Database;
use pdt::repository::{
    MongoAssetRepository, MongoAuditRepository, MongoCollectionRepository, MongoRelationRepository,
};
use pdt::router::build_router;
use pdt::service::Services;
use pdt::tenant::TenantPoolManager;
use pep::oidc_resource_server::ResourceServerClient;

#[cfg(feature = "sqlite-backend")]
use pdt::repository::{
    SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteRelationRepository,
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
            let audit_repo = MongoAuditRepository::new(db);

            Services::from_repositories(asset_repo, relation_repo, collection_repo, audit_repo)
        }
        #[cfg(feature = "sqlite-backend")]
        DatabaseBackend::Sqlite => {
            let db_path = &config.sqlite_path;
            tracing::info!("Connecting to SQLite: {}", db_path);

            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(5)
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
            let audit_repo = SqliteAuditRepository::new(pool);

            Services::from_repositories(asset_repo, relation_repo, collection_repo, audit_repo)
        }
    };

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
    let tenant_manager = TenantPoolManager::new(services, PathBuf::from(&instances_dir));
    tracing::info!(
        "Multi-tenant routing enabled (instances dir: {})",
        instances_dir
    );

    // Initialize auth
    let auth_client = ResourceServerClient::new();
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
