//! PDT Server Entry Point

use anyhow::Context;
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::net::SocketAddr;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use pdt::{auth::middleware::AuthLayer, config::Config, db::Database, handlers, openapi::ApiDoc, service::Services};
use pdt::repository::{
    MongoAssetRepository, MongoAuditRepository, MongoCollectionRepository,
    MongoRelationRepository,
};
use pdt::config::DatabaseBackend;
use std::sync::Arc;
use pep::oidc_resource_server::ResourceServerClient;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

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
    tracing::info!("Configuration loaded (backend: {:?})", config.database_backend);

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

            Services::from_repositories(
                asset_repo,
                relation_repo,
                collection_repo,
                audit_repo,
            )
        }
        #[cfg(feature = "sqlite-backend")]
        DatabaseBackend::Sqlite => {
            use pdt::repository::{
                SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
                SqliteRelationRepository,
            };

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
            sqlx::raw_sql(
                "INSERT INTO assets_fts(assets_fts) VALUES('rebuild');"
            )
            .execute(&pool)
            .await
            .map_err(|e| tracing::warn!("FTS rebuild skipped (non-fatal): {}", e))
            .ok();
            tracing::info!("FTS index rebuilt");

            let asset_repo = SqliteAssetRepository::new(pool.clone());
            let relation_repo = SqliteRelationRepository::new(pool.clone());
            let collection_repo = SqliteCollectionRepository::new(pool.clone());
            let audit_repo = SqliteAuditRepository::new(pool);

            Services::from_repositories(
                asset_repo,
                relation_repo,
                collection_repo,
                audit_repo,
            )
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
                tracing::error!("Failed to initialize Cedar authorizer: {}. Running without Cedar.", e);
                None
            }
        }
    } else {
        tracing::info!("Cedar authorization disabled");
        None
    };

    // Initialize auth
    let auth_client = ResourceServerClient::new();
    let auth_layer = AuthLayer::new(config.auth.clone(), auth_client);
    tracing::info!("Auth layer initialized (enabled: {})", config.auth.enabled);

    // Build router
    // Public routes (no auth)
    let public_routes = Router::new()
        .route("/health", get(handlers::health::health_check))
        // Cedar policy store endpoint — public so other services can fetch policies
        .route("/api/cedar/policies", get(handlers::cedar::get_cedar_policies));

    // Protected routes (auth + Cedar)
    let protected_routes = Router::new()
        // Asset routes
        .route("/api/assets", post(handlers::assets::create_asset))
        .route("/api/assets", get(handlers::assets::list_assets))
        .route("/api/assets/{id}", get(handlers::assets::get_asset))
        .route("/api/assets/{id}", put(handlers::assets::update_asset))
        .route("/api/assets/{id}", delete(handlers::assets::delete_asset))
        .route("/api/assets/{id}/tags", post(handlers::assets::add_tag))
        .route(
            "/api/assets/{id}/tags/{tag_id}",
            delete(handlers::assets::remove_tag),
        )
        // Auth context route (Cedar visibility/ownership management)
        .route(
            "/api/assets/{id}/auth-context",
            put(handlers::assets::update_auth_context),
        )
        // Relation routes
        .route("/api/relations", post(handlers::relations::create_relation))
        .route("/api/relations/{id}", get(handlers::relations::get_relation))
        .route(
            "/api/relations/{id}",
            delete(handlers::relations::delete_relation),
        )
        .route(
            "/api/assets/{id}/relations",
            get(handlers::relations::get_asset_relations),
        )
        .route(
            "/api/assets/{id}/graph",
            get(handlers::relations::traverse_graph),
        )
        // Collection routes (no Cedar — use Services directly)
        .route(
            "/api/collections",
            post(handlers::collections::create_collection),
        )
        .route(
            "/api/collections",
            get(handlers::collections::list_collections),
        )
        .route(
            "/api/collections/{id}",
            get(handlers::collections::get_collection),
        )
        .route(
            "/api/collections/{id}",
            put(handlers::collections::update_collection),
        )
        .route(
            "/api/collections/{id}",
            delete(handlers::collections::delete_collection),
        )
        .route(
            "/api/collections/{id}/assets",
            post(handlers::collections::add_asset),
        )
        .route(
            "/api/collections/{id}/assets/{asset_id}",
            delete(handlers::collections::remove_asset),
        )
        // Search routes (with Cedar filtering)
        .route("/api/search", get(handlers::search::search))
        // Audit routes
        .route("/api/audit", get(handlers::audit::list_audit_entries))
        .route(
            "/api/assets/{id}/history",
            get(handlers::audit::get_asset_history),
        )
        // Middleware (auth + tracing + CORS)
        .layer(TraceLayer::new_for_http())
        .layer(auth_layer)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        // OpenAPI spec + Swagger UI (public, no auth)
        .merge(
            SwaggerUi::new("/swagger-ui")
                .url("/api-docs/openapi.json", ApiDoc::openapi()),
        )
        .with_state((
            services,
            authorizer.map(Arc::new),
        ));

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
