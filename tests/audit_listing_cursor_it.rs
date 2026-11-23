//! Audit-listing cursor IT (the sort/cursor mismatch found 2026-11-23 —
//! the measurement artifact behind the false "fame write-path coverage
//! gap" premise, issue 3efb7982): the audit `list` sorted
//! `timestamp DESC, id DESC` but paged with `id < cursor` — a
//! lexicographic UUID compare that does not track the timestamp order.
//! Random ids made every page an arbitrary subset (duplicate rows across
//! pages, gaps) and produced SPURIOUS EARLY EXHAUSTION when the cursor id
//! sorted low — an unfiltered audit walk silently truncated while
//! claiming completeness (the lying-absence class on a listing plane).
//!
//! THE FIX (mirrors the asset list's established keyset form): the cursor
//! carries `timestamp|id` and the filter is the compound
//! `(timestamp < ? OR (timestamp = ? AND id < ?))`. Bare-id cursors keep
//! the legacy filter for in-flight paginations across a deploy.
//!
//! Guards (against the REAL router, dev-mode admin claims):
//!   1. FULL-COVERAGE WALK: 25 rows seeded in 5 clusters of 5 IDENTICAL
//!      timestamps (the dense-duplicate killer for any timestamp-only or
//!      id-only cursor), limit=4 → the walk must see all 25 unique ids,
//!      zero duplicates, and terminate only after full coverage.
//!   2. ORDER PIN: timestamps non-increasing across the whole walk.
//!   3. LEGACY-CURSOR COMPAT: a bare-id cursor still returns 200 + rows
//!      (in-flight paginations across a deploy don't break).

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use pdt::auth::config::AuthConfig;
use pdt::auth::middleware::AuthLayer;
use pdt::repository::{
    SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteGroupRepository, SqliteRelationRepository,
};
use pdt::router::build_router;
use pdt::service::Services;
use pdt::tenant::TenantPoolManager;
use pep::oidc_resource_server::ResourceServerClient;
use serde_json::Value;
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
        SqliteAuditRepository::new(pool.clone()),
        SqliteGroupRepository::new(pool),
    )
}

/// Seed 25 audit rows in 5 clusters of 5 IDENTICAL timestamps. Identical
/// timestamps inside a cluster are the exact shape that breaks any cursor
/// not keyed on the full sort key: a timestamp-only cursor skips the
/// cluster tail, an id-only cursor (the old bug) subsets arbitrarily.
/// Ids are random UUIDv4s (the production shape — sequential ids would
/// accidentally make `id < cursor` track the sort).
async fn seed_clustered_audit_entries(db_path: &PathBuf) -> Vec<(String, String)> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(db_path)
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .expect("band db connects");

    let base = chrono::Utc::now() - chrono::Duration::hours(1);
    let mut seeded = Vec::new();
    for cluster in 0..5usize {
        let ts = (base - chrono::Duration::minutes(cluster as i64 * 7)).to_rfc3339();
        for _row in 0..5usize {
            let id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO audit_entries (id, entity_type, entity_id, action, changes_json, user_id, timestamp)
                 VALUES (?, 'asset', ?, 'update', '{}', 'seed-cursor-it', ?)",
            )
            .bind(&id)
            .bind(format!("entity-{cluster}"))
            .bind(&ts)
            .execute(&pool)
            .await
            .expect("clustered audit row inserts");
            seeded.push((id, ts.clone()));
        }
    }
    pool.close().await;
    seeded
}

async fn get_audit_page(
    app: axum::Router,
    band_id: &str,
    limit: usize,
    cursor: Option<&str>,
) -> (StatusCode, Value) {
    let mut uri = format!("/api/audit?limit={limit}");
    if let Some(c) = cursor {
        uri.push_str(&format!("&cursor={}", urlencoding_lite(c)));
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&uri)
                .header("X-Instance-Id", band_id)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 << 20)
        .await
        .expect("body");
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Minimal query-component encoder (the cursor carries `|`, `+`, `:`).
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tokio::test]
async fn audit_listing_walk_covers_all_rows_exactly_once_across_duplicate_timestamps() {
    // ---- Fixture: workspace + band + 25 clustered rows ----
    let dir = std::env::temp_dir().join(format!("pdt_audit_cursor_it_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("tmpdir");

    let services = sqlite_services().await;
    let tenant_manager = TenantPoolManager::new(services, dir.clone(), Vec::new());
    let auth_layer = AuthLayer::new(
        AuthConfig {
            enabled: false,
            issuer_url: "http://127.0.0.1:1".to_string(),
            expected_audience: None,
            dev_mode: true,
            userinfo_url: None,
            trusted_issuers: None,
        },
        ResourceServerClient::new(),
    );
    let app = build_router(tenant_manager.clone(), auth_layer, None);

    let workspace = uuid::Uuid::new_v4().to_string();
    let band = uuid::Uuid::new_v4().to_string();
    tenant_manager
        .provision_instance(&workspace, None)
        .await
        .expect("workspace provisions");
    tenant_manager
        .provision_instance(&band, Some(&workspace))
        .await
        .expect("band provisions");
    let band_db = dir.join(&workspace).join(&band).join("pdt.db");
    assert!(band_db.exists(), "band db exists after provisioning");

    let seeded = seed_clustered_audit_entries(&band_db).await;
    assert_eq!(seeded.len(), 25, "25 seeded rows");
    let seeded_ids: std::collections::HashSet<String> =
        seeded.iter().map(|(id, _)| id.clone()).collect();

    // ---- Guard 1: full-coverage walk at limit=4 (7 pages) ----
    let mut cursor: Option<String> = None;
    let mut seen: Vec<String> = Vec::new();
    let mut last_ts: Option<String> = None;
    for _page in 0..20 {
        let (status, json) = get_audit_page(app.clone(), &band, 4, cursor.as_deref()).await;
        assert_eq!(status, StatusCode::OK, "walk page must 200: {json}");
        let data = json["data"].as_array().expect("data array").clone();
        if data.is_empty() {
            break;
        }
        for e in &data {
            seen.push(e["_id"].as_str().expect("entry id").to_string());
            let ts = e["timestamp"]
                .as_str()
                .expect("entry timestamp")
                .to_string();
            if let Some(prev) = &last_ts {
                assert!(
                    prev.as_str() >= ts.as_str(),
                    "timestamps must be non-increasing across the walk: {prev} then {ts}"
                );
            }
            last_ts = Some(ts);
        }
        match json["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_string()),
            None => break,
        }
    }

    let unique: std::collections::HashSet<&String> = seen.iter().collect();
    assert_eq!(
        unique.len(),
        seen.len(),
        "walk must not repeat rows (got {} rows, {} unique)",
        seen.len(),
        unique.len()
    );
    assert_eq!(
        unique.len(),
        seeded_ids.len(),
        "walk must cover ALL seeded rows before exhausting (covered {} of {})",
        unique.len(),
        seeded_ids.len()
    );
    for id in &seen {
        assert!(seeded_ids.contains(id), "walk must return only seeded rows");
    }

    // ---- Guard 3: legacy bare-id cursor stays compatible (200 + rows) ----
    let legacy_cursor = &seen[0];
    let (status, json) = get_audit_page(app.clone(), &band, 4, Some(legacy_cursor)).await;
    assert_eq!(status, StatusCode::OK, "legacy bare-id cursor must 200");
    assert!(
        json["data"].as_array().expect("data array").len() > 0 || json["next_cursor"].is_null(),
        "legacy cursor returns rows or an honest empty page: {json}"
    );

    // Teardown: no stray state.
    std::fs::remove_dir_all(&dir).expect("tmpdir removes");
}
