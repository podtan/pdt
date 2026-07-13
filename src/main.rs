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
    tracing::info!("Configuration loaded");

    // Connect to database
    let db = Database::connect(&config.database).await?;
    tracing::info!("Connected to database: {}", config.database.database);

    // Create indices
    db.create_indices().await?;
    tracing::info!("Database indices created");

    // Initialize services
    let services = Services::new(db);

    // Initialize Cedar authorization (optional — gracefully disabled when CEDAR_ENABLED=false)
    let authorizer = if config.cedar.enabled {
        match pep::cedar::CedarAuthorizer::new(config.cedar.clone().into()) {
            Ok(authorizer) => {
                tracing::info!("Cedar authorizer initialized (policy_path: {:?})", config.cedar.policy_path);
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
        .route("/health", get(handlers::health::health_check));

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
