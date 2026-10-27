//! Gate-contract regression tests (ruled contract bbc5b79f — issue e0db0993,
//! F17): authenticated-by-default fleet policy-store contract.
//!
//! Pins the gate per surface at the REAL production router
//! (`pdt::router::build_router` — the exact shape the binary serves):
//! - `/health` stays public (load-bearing for health checks).
//! - `/api/cedar/policies` + `/swagger-ui` + `/api-docs/openapi.json` are
//!   GATED — 401 without a token when auth is enabled.
//! - Dev stacks (auth disabled + dev_mode) keep working — mock claims
//!   injected, every surface reachable (the local IT-gate contract,
//!   procedure d4ff6605).

mod common;

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use pdt::auth::config::AuthConfig;
use pdt::auth::middleware::AuthLayer;
use pdt::repository::{
    SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteRelationRepository,
};
use pdt::router::build_router;
use pdt::service::Services;
use pdt::tenant::TenantPoolManager;
use pep::oidc_resource_server::ResourceServerClient;
use tower::ServiceExt;

async fn sqlite_services() -> Services {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(":memory:")
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .expect("failed to connect to in-memory SQLite");

    let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("failed to run migrations");

    Services::from_repositories(
        SqliteAssetRepository::new(pool.clone()),
        SqliteRelationRepository::new(pool.clone()),
        SqliteCollectionRepository::new(pool.clone()),
        SqliteAuditRepository::new(pool),
    )
}

fn auth_config(enabled: bool, dev_mode: bool) -> AuthConfig {
    AuthConfig {
        enabled,
        // Unreachable issuer — the gate tests never reach JWT validation:
        // enabled + no token 401s at the bearer-extraction leg, and the
        // dev-mode router injects mock claims without validating.
        issuer_url: "http://127.0.0.1:1".to_string(),
        expected_audience: None,
        dev_mode,
        userinfo_url: None,
    }
}

async fn router_with(enabled: bool, dev_mode: bool) -> axum::Router {
    let services = sqlite_services().await;
    let tenant_manager =
        TenantPoolManager::new(services, PathBuf::from("/tmp/pdt-gate-test-instances"));
    let auth_layer = AuthLayer::new(auth_config(enabled, dev_mode), ResourceServerClient::new());
    build_router(tenant_manager, auth_layer, None)
}

async fn status_of(app: axum::Router, uri: &str) -> StatusCode {
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    response.status()
}

#[tokio::test]
async fn cedar_policies_gate_401_without_token() {
    // Ruled contract bbc5b79f: the authorization model must not be readable
    // by unauthenticated callers.
    let app = router_with(true, false).await;
    assert_eq!(
        status_of(app, "/api/cedar/policies").await,
        StatusCode::UNAUTHORIZED,
        "/api/cedar/policies must be gated"
    );
}

#[tokio::test]
async fn swagger_ui_gate_401_without_token() {
    // Ruled contract bbc5b79f: documentation UIs expose the full API
    // surface; they serve operators/agents, not the public.
    let app = router_with(true, false).await;
    assert_eq!(
        status_of(app.clone(), "/swagger-ui/").await,
        StatusCode::UNAUTHORIZED,
        "/swagger-ui must be gated"
    );
    assert_eq!(
        status_of(app, "/api-docs/openapi.json").await,
        StatusCode::UNAUTHORIZED,
        "/api-docs/openapi.json must be gated"
    );
}

#[tokio::test]
async fn health_stays_public_load_bearing() {
    // Ruled contract bbc5b79f: health checks are the allowlisted
    // unauthenticated flow.
    let app = router_with(true, false).await;
    assert_eq!(
        status_of(app, "/health").await,
        StatusCode::OK,
        "/health stays public (load-bearing)"
    );
}

#[tokio::test]
async fn dev_mode_router_keeps_every_surface_reachable() {
    // The local IT-gate contract (procedure d4ff6605): dev stacks run
    // AUTH_ENABLED=false AUTH_DEV_MODE=true — the gate must not break them.
    let app = router_with(false, true).await;
    assert_eq!(status_of(app, "/health").await, StatusCode::OK);
    let app = router_with(false, true).await;
    assert_eq!(
        status_of(app, "/api/cedar/policies").await,
        StatusCode::OK,
        "dev-mode cedar policies reachable"
    );
    let app = router_with(false, true).await;
    assert_eq!(
        status_of(app, "/api-docs/openapi.json").await,
        StatusCode::OK,
        "dev-mode openapi spec reachable"
    );
}
