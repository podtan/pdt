//! Audit-prune IT (issue 2c71e4e6 — the audit-entries retention verb,
//! ARCHIVE-THEN-DELETE): the 5 named tests sequenced in ONE #[tokio::test]
//! against the REAL router (`pdt::router::build_router` — the exact shape
//! the binary serves, dev-mode admin claims).
//!
//! WHY ONE TEST: the execution gate (PDT_PRUNE_ENABLED) and the policy path
//! (PDT_PRUNE_POLICY_PATH) are process-global env reads at invocation —
//! parallel tests would race them. The phases below set/unset env
//! sequentially and clean up at the end.
//!
//! Phases (the spec's 5 tests + fixture):
//!   1. GATE DEFAULT-OFF: PDT_PRUNE_ENABLED unset → execution 403, no archive.
//!   2. POLICY ABSENT: no PDT_PRUNE_POLICY_PATH → execution 403 (fail-closed).
//!   3. BELOW-WINDOW REFUSED: policy 90d, request 30d → 403.
//!   4. DRY-RUN ZERO-WRITES: numbers only — no archive file, rows intact.
//!   5. ARCHIVE-THEN-DELETE VERIFY: archive file with row-count verify,
//!      rows deleted, post-count 0, fresh rows survive, prune audited.
//!   6. NO-SILENT-DELETE PIN: every deleted row is IN the archive file;
//!      re-run sees 0 candidates.

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

/// Seed audit rows with BACKDATED timestamps directly into a band db (the
/// repository's create() stamps now — the prune window needs old rows).
async fn seed_audit_entries(db_path: &PathBuf, old_count: usize, fresh_count: usize) {
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
    let old_ts = (chrono::Utc::now() - chrono::Duration::days(400)).to_rfc3339();
    let fresh_ts = chrono::Utc::now().to_rfc3339();
    for i in 0..old_count {
        sqlx::query(
            "INSERT INTO audit_entries (id, entity_type, entity_id, action, changes_json, user_id, timestamp)
             VALUES (?, 'asset', ?, 'update', '{}', 'seed-old', ?)",
        )
        .bind(format!("00000000-0000-4000-8000-{:012}", i))
        .bind(format!("old-entity-{i}"))
        .bind(&old_ts)
        .execute(&pool)
        .await
        .expect("old audit row inserts");
    }
    for i in 0..fresh_count {
        sqlx::query(
            "INSERT INTO audit_entries (id, entity_type, entity_id, action, changes_json, user_id, timestamp)
             VALUES (?, 'asset', ?, 'update', '{}', 'seed-fresh', ?)",
        )
        .bind(format!("00000000-0000-4000-9000-{:012}", i))
        .bind(format!("fresh-entity-{i}"))
        .bind(&fresh_ts)
        .execute(&pool)
        .await
        .expect("fresh audit row inserts");
    }
    pool.close().await;
}

async fn count_rows(db_path: &PathBuf) -> i64 {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(db_path)
                .create_if_missing(false),
        )
        .await
        .expect("band db connects");
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_entries")
        .fetch_one(&pool)
        .await
        .expect("count");
    pool.close().await;
    count
}

/// POST /api/maintenance/audit-prune with dev-mode (admin) claims.
async fn post_audit_prune(
    app: axum::Router,
    band_id: &str,
    older_than_days: u32,
    dry_run: bool,
) -> (StatusCode, serde_json::Value) {
    let body = format!(
        r#"{{"older_than_days": {}, "dry_run": {}}}"#,
        older_than_days, dry_run
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/maintenance/audit-prune")
                .header("X-Instance-Id", band_id)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn audit_prune_end_to_end_gates_dry_run_and_archive_then_delete() {
    // ---- Fixture: temp instances dir + workspace + band + seeded rows ----
    let dir = std::env::temp_dir().join(format!("pdt_audit_prune_it_{}", uuid::Uuid::new_v4()));
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

    seed_audit_entries(&band_db, 25, 5).await;
    assert_eq!(count_rows(&band_db).await, 30, "30 seeded rows");

    // The policy file: the endorsed audit-entries class (90d hot →
    // archive-then-delete).
    let policy_path = dir.join("prune-policy.json");
    std::fs::write(
        &policy_path,
        r#"{"classes": [], "audit_entries": {"window_days": 90, "disposition": "archive_then_delete"}}"#,
    )
    .expect("policy file writes");

    // ---- Phase 1: GATE DEFAULT-OFF (PDT_PRUNE_ENABLED unset → 403) ----
    std::env::remove_var("PDT_PRUNE_ENABLED");
    std::env::set_var("PDT_PRUNE_POLICY_PATH", &policy_path);
    let (status, body) = post_audit_prune(app.clone(), &band, 90, false).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "execution must 403 with PDT_PRUNE_ENABLED unset, got: {body}"
    );
    assert!(
        body.to_string().contains("execution disabled"),
        "403 body names the disabled gate: {body}"
    );
    let archives: Vec<_> = std::fs::read_dir(dir.join(&workspace).join(&band))
        .expect("band dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("audit-archive-")
        })
        .collect();
    assert!(
        archives.is_empty(),
        "a refused execution must not create an archive file"
    );

    // ---- Phase 2: POLICY ABSENT (fail-closed) ----
    std::env::set_var("PDT_PRUNE_ENABLED", "1");
    std::env::remove_var("PDT_PRUNE_POLICY_PATH");
    let (status, body) = post_audit_prune(app.clone(), &band, 90, false).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "execution must 403 with no policy loaded, got: {body}"
    );
    assert!(
        body.to_string().contains("no retention policy"),
        "403 body names the missing policy: {body}"
    );

    // ---- Phase 3: BELOW-WINDOW REFUSED (policy 90d, request 30d) ----
    std::env::set_var("PDT_PRUNE_POLICY_PATH", &policy_path);
    let (status, body) = post_audit_prune(app.clone(), &band, 30, false).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a window below the policy window must 403, got: {body}"
    );
    assert!(
        body.to_string().contains("below the policy window"),
        "403 body names the window guard: {body}"
    );
    assert_eq!(count_rows(&band_db).await, 30, "refusals write nothing");

    // ---- Phase 4: DRY-RUN ZERO-WRITES ----
    let (status, body) = post_audit_prune(app.clone(), &band, 90, true).await;
    assert_eq!(status, StatusCode::OK, "dry-run answers 200: {body}");
    assert_eq!(body["dry_run"], serde_json::Value::Bool(true));
    assert_eq!(body["candidates"], serde_json::Value::from(25));
    assert_eq!(
        body["policy_verdict"],
        serde_json::Value::String("allowed".to_string()),
        "dry-run carries the policy verdict: {body}"
    );
    assert!(
        body["archive_dir"].is_string(),
        "dry-run shows the planned archive home: {body}"
    );
    let archives: Vec<_> = std::fs::read_dir(dir.join(&workspace).join(&band))
        .expect("band dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("audit-archive-")
        })
        .collect();
    assert!(
        archives.is_empty(),
        "DRY-RUN ZERO-WRITES: no archive file may exist"
    );
    assert_eq!(count_rows(&band_db).await, 30, "dry-run deletes nothing");

    // ---- Phase 5: ARCHIVE-THEN-DELETE VERIFY ----
    let (status, body) = post_audit_prune(app.clone(), &band, 90, false).await;
    assert_eq!(status, StatusCode::OK, "execution answers 200: {body}");
    assert_eq!(body["dry_run"], serde_json::Value::Bool(false));
    assert_eq!(body["candidates"], serde_json::Value::from(25));
    assert_eq!(body["archived"], serde_json::Value::from(25));
    assert_eq!(body["deleted"], serde_json::Value::from(25));
    assert_eq!(body["post_count"], serde_json::Value::from(0));
    assert_eq!(body["verified"], serde_json::Value::Bool(true));

    // The archive file exists, holds EVERY deleted row, each line parses.
    let archive_path = PathBuf::from(body["archive_path"].as_str().expect("archive_path"));
    assert!(archive_path.exists(), "the archive file exists");
    let archive_content = std::fs::read_to_string(&archive_path).expect("archive reads");
    let lines: Vec<&str> = archive_content.lines().collect();
    assert_eq!(
        lines.len(),
        25,
        "row-count verify: the archive holds every deleted row"
    );
    for line in &lines {
        let entry: serde_json::Value = serde_json::from_str(line).expect("each line parses");
        assert!(
            entry.get("_id").is_some(),
            "archived rows carry the _id convention: {line}"
        );
    }

    // The fresh rows SURVIVE; the prune itself rides one audit entry.
    assert_eq!(
        count_rows(&band_db).await,
        6,
        "5 fresh rows + the prune's own audit entry survive"
    );

    // ---- Phase 6: NO-SILENT-DELETE PIN (re-run sees 0 candidates) ----
    let (status, body) = post_audit_prune(app.clone(), &band, 90, true).await;
    assert_eq!(status, StatusCode::OK, "re-run dry-run answers: {body}");
    assert_eq!(
        body["candidates"],
        serde_json::Value::from(0),
        "nothing older than the window remains"
    );
    assert!(
        archive_path.exists(),
        "the archive file persists as the record after the delete"
    );

    // ---- Cleanup: env + temp dir ----
    std::env::remove_var("PDT_PRUNE_ENABLED");
    std::env::remove_var("PDT_PRUNE_POLICY_PATH");
    std::fs::remove_dir_all(&dir).ok();
}
