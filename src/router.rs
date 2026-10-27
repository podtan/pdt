//! Router assembly — shared by the binary entry point and the gate tests.
//!
//! GATE SHAPE (ruled contract bbc5b79f — authenticated-by-default fleet
//! policy-store contract, CPO ruling 2026-10-25 recorded in issue e0db0993
//! response 3de69cb8):
//! - `/health` stays public — load-bearing for health checks.
//! - EVERYTHING else sits behind the auth layer, including
//!   `/api/cedar/policies` (the authorization model must not be readable by
//!   unauthenticated callers — consumers carry `policy_store_token` via
//!   `CedarConfig`) and `/swagger-ui` + `/api-docs/openapi.json`
//!   (documentation UIs expose the full API surface; they serve
//!   operators/agents, not the public).
//! - The explicit-public design intent these routes carried before the
//!   ruling is corrected here; an undocumented gate is a silent 401, so the
//!   comments state the gate (fail-loud posture).

use std::sync::Arc;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::middleware::AuthLayer;
use crate::handlers;
use crate::openapi::ApiDoc;
use crate::tenant::TenantPoolManager;
use pep::cedar::CedarAuthorizer;

/// Build the PDT router: public health route + authenticated-by-default
/// API surface (ruled contract bbc5b79f).
pub fn build_router(
    tenant_manager: TenantPoolManager,
    auth_layer: AuthLayer,
    authorizer: Option<CedarAuthorizer>,
) -> Router {
    // Public routes (no auth) — load-bearing unauthenticated flows ONLY
    // (health checks). Ruled contract bbc5b79f: no other unauthenticated
    // endpoint.
    let public_routes = Router::new().route("/health", get(handlers::health::health_check));

    // Protected routes (auth + Cedar)
    let protected_routes = Router::new()
        // Cedar policy store endpoint — GATED (ruled contract bbc5b79f):
        // the authorization model must not be readable by unauthenticated
        // callers. The previous explicit-public design intent ("public so
        // other services can fetch policies") is corrected by ruling;
        // authenticated policy fetches work today via CedarConfig's
        // policy_store_token.
        .route(
            "/api/cedar/policies",
            get(handlers::cedar::get_cedar_policies),
        )
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
        .route(
            "/api/relations/{id}",
            get(handlers::relations::get_relation),
        )
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
        // Role model (1552c098) — sub-keyed resolution for the PEP authz path
        .route("/api/roles/{sub}", get(handlers::roles::get_roles))
        // Directory name-map (4474a038) — uuid → {name, kind, email/role}
        .route("/api/directory", get(handlers::directory::list_directory))
        .route(
            "/api/directory/{uuid}",
            get(handlers::directory::get_directory_entry),
        )
        .route(
            "/api/directory/{uuid}",
            put(handlers::directory::upsert_directory_entry),
        )
        // Maintenance (admin-gated, idempotent backfills)
        .route(
            "/api/maintenance/backfill",
            post(handlers::maintenance::backfill),
        )
        // Retention prune (admin-gated; dry-run default, execution wire-gated
        // on PDT_PRUNE_ENABLED — CPO ruling 8d591e85)
        .route("/api/maintenance/prune", post(handlers::maintenance::prune))
        // Audit routes
        .route("/api/audit", get(handlers::audit::list_audit_entries))
        .route(
            "/api/assets/{id}/history",
            get(handlers::audit::get_asset_history),
        )
        // Tenant management endpoints
        .route(
            "/api/instances/{instance_id}/provision",
            post(handlers::instances::provision_instance),
        )
        .route(
            "/api/instances/{instance_id}",
            axum::routing::delete(handlers::instances::delete_instance),
        )
        .route("/api/instances", get(handlers::instances::list_instances))
        .route(
            "/api/instances/tree",
            get(handlers::instances::list_instance_tree),
        )
        // Middleware (auth + tracing + CORS)
        .layer(TraceLayer::new_for_http())
        .layer(auth_layer.clone())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // OpenAPI spec + Swagger UI — GATED (ruled contract bbc5b79f):
    // documentation UIs expose the full API surface; they serve
    // operators/agents, not the public. The previous "public, no auth"
    // design intent is corrected by ruling.
    let swagger_routes = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .layer(auth_layer)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(swagger_routes)
        .with_state((tenant_manager, authorizer.map(Arc::new)))
}
