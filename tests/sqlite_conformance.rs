//! SQLite backend conformance tests.
//!
//! These tests run the generic conformance test functions from `common/mod.rs`
//! against an in-memory SQLite database. Each test gets a fresh database
//! with migrations applied.
#![cfg(feature = "sqlite-backend")]

mod common;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use pdt::repository::{
    AssetRepository, RelationRepository, SqliteAssetRepository, SqliteAuditRepository,
    SqliteCollectionRepository, SqliteRelationRepository,
};

/// Create a fresh in-memory SQLite database with migrations applied.
async fn setup_sqlite() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
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

    pool
}

/// Set up all 4 SQLite repositories backed by a single in-memory database.
async fn setup_repos() -> (
    SqliteAssetRepository,
    SqliteRelationRepository,
    SqliteCollectionRepository,
    SqliteAuditRepository,
) {
    let pool = setup_sqlite().await;
    (
        SqliteAssetRepository::new(pool.clone()),
        SqliteRelationRepository::new(pool.clone()),
        SqliteCollectionRepository::new(pool.clone()),
        SqliteAuditRepository::new(pool),
    )
}

// ============================================================================
// ASSET REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_asset_create_and_get() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_create_and_get(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_by_id_not_found() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_get_by_id_not_found(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_update() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_update(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_soft_delete() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_soft_delete(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_exists_including_deleted() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_exists_including_deleted(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_list_with_pagination() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_list_with_pagination(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_list_filter_by_tag() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_list_filter_by_tag(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_by_text() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_search_by_text(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_no_results() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_search_no_results(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_no_query() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_search_no_query(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_with_tag_filter() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_search_with_tag_filter(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_cursor_pagination_and_created_at() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_search_cursor_pagination_and_created_at(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_add_and_remove_tag() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_add_and_remove_tag(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_update_auth_context() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_update_auth_context(&assets).await;
}

// ============================================================================
// RELATION REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_relation_create_and_get() {
    let (assets, relations, _, _) = setup_repos().await;
    common::relation_create_and_get(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_has_access_round_trip() {
    // tocpi workspace membership v1 rides the has_access relation type —
    // the sqlite parse mapping must round-trip it, never swallow it to
    // related_to (the silent-swallow class, parse_relation_type `_` arm).
    use pdt::models::{CreateAssetRequest, CreateRelationRequest, RelationType};
    let (assets, relations, _, _) = setup_repos().await;

    // FK-constrained: both endpoints must exist as assets first; create
    // returns the server-assigned id.
    let user = assets
        .create(
            CreateAssetRequest {
                title: "Test User".to_string(),
                content: None,
                tags: vec![],
                metadata: std::collections::HashMap::new(),
                auth_context: None,
            },
            "creator",
        )
        .await
        .expect("user asset seeds");
    let ws = assets
        .create(
            CreateAssetRequest {
                title: "Test Workspace".to_string(),
                content: None,
                tags: vec![],
                metadata: std::collections::HashMap::new(),
                auth_context: None,
            },
            "creator",
        )
        .await
        .expect("workspace asset seeds");

    let created = relations
        .create(
            CreateRelationRequest {
                from_asset_id: user.id.clone(),
                to_asset_id: ws.id.clone(),
                relation_type: RelationType::HasAccess,
                metadata: std::collections::HashMap::new(),
            },
            "creator",
        )
        .await
        .expect("has_access relation creates");
    assert_eq!(created.relation_type, RelationType::HasAccess);

    let read_back = relations
        .get_asset_relations(&user.id)
        .await
        .expect("relations read back");
    assert!(
        read_back
            .iter()
            .any(|r| r.relation_type == RelationType::HasAccess && r.to_asset_id == ws.id),
        "has_access must survive the sqlite round trip, not degrade to related_to"
    );
}

#[tokio::test]
async fn sqlite_relation_get_by_id_not_found() {
    let (_, relations, _, _) = setup_repos().await;
    common::relation_get_by_id_not_found(&relations).await;
}

#[tokio::test]
async fn sqlite_relation_delete() {
    let (_, relations, _, _) = setup_repos().await;
    common::relation_delete(&relations).await;
}

#[tokio::test]
async fn sqlite_relation_get_asset_relations() {
    let (assets, relations, _, _) = setup_repos().await;
    common::relation_get_asset_relations(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_delete_by_asset() {
    let (assets, relations, _, _) = setup_repos().await;
    common::relation_delete_by_asset(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_traverse_graph() {
    let (assets, relations, _, _) = setup_repos().await;
    common::relation_traverse_graph(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_get_descendants() {
    let (assets, relations, _, _) = setup_repos().await;
    common::relation_get_descendants(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_would_create_cycle() {
    let (assets, relations, _, _) = setup_repos().await;
    common::relation_would_create_cycle(&assets, &relations).await;
}

// ============================================================================
// COLLECTION REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_collection_create_and_get() {
    let (_, _, collections, _) = setup_repos().await;
    common::collection_create_and_get(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_get_by_id_not_found() {
    let (_, _, collections, _) = setup_repos().await;
    common::collection_get_by_id_not_found(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_update() {
    let (_, _, collections, _) = setup_repos().await;
    common::collection_update(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_delete() {
    let (_, _, collections, _) = setup_repos().await;
    common::collection_delete(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_list_with_pagination() {
    let (_, _, collections, _) = setup_repos().await;
    common::collection_list_with_pagination(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_add_and_remove_asset() {
    let (assets, _, collections, _) = setup_repos().await;
    common::collection_add_and_remove_asset(&assets, &collections).await;
}

#[tokio::test]
async fn sqlite_collection_remove_asset_from_all() {
    let (assets, _, collections, _) = setup_repos().await;
    common::collection_remove_asset_from_all(&assets, &collections).await;
}

// ============================================================================
// AUDIT REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_audit_create_and_list() {
    let (_, _, _, audit) = setup_repos().await;
    common::audit_create_and_list(&audit).await;
}

#[tokio::test]
async fn sqlite_audit_list_with_filters() {
    let (_, _, _, audit) = setup_repos().await;
    common::audit_list_with_filters(&audit).await;
}

#[tokio::test]
async fn sqlite_audit_get_entity_history() {
    let (_, _, _, audit) = setup_repos().await;
    common::audit_get_entity_history(&audit).await;
}

// ============================================================================
// CROSS-REPOSITORY INTEGRATION TEST
// ============================================================================

#[tokio::test]
async fn sqlite_asset_delete_cascades() {
    let (assets, relations, collections, audit) = setup_repos().await;
    common::asset_delete_cascades(&assets, &relations, &collections, &audit).await;
}

// ---------------------------------------------------------------------------
// FTS5 expression-safety pins (Sep 7-8 prod incident).
//
// Raw user queries reached FTS5 MATCH as EXPRESSIONS: a bare `token:` reads
// as a column filter and failed the whole request with `no such column:
// <token>` (SQLx code 1) — 5 journal events, 3 seats, ~18h (Paydar's journal
// receipt 3123190a on issue 09245606). The sanitizer reduces every query to
// quoted phrases; these pins hold the shape closed.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sqlite_search_survives_fts5_metacharacter_queries() {
    let (assets, _, _, _) = setup_repos().await;
    assets
        .create(
            common::make_asset_request("Dispatch Guide", "rotate trustee creds"),
            "user1",
        )
        .await
        .unwrap();

    // Every shape below 500'd in prod (or is the journal's exact token class):
    // hyphen-split tokens, `col:term` column filters, numeric fragments,
    // operator soup.
    for q in [
        "instance-type",                 // occ 3 — hyphen split → `no such column: type`
        "type:dispatch",                 // column-filter class
        "5063f004",                      // occ 1 — numeric fragment
        "2",                             // occ 1 journal token
        "mint:",                         // occ 2 class
        "51d1",                          // occ 0 journal token
        "a OR b NEAR(c",                 // operator soup
        "TokenProvider client contract", // occ 2 shape — multi-word
    ] {
        let (res, _) = assets
            .search(Some(q), vec![], 10, None)
            .await
            .unwrap_or_else(|e| panic!("search {:?} must not fail: {}", q, e));
        assert_eq!(
            res.len(),
            0,
            "query {:?} matches nothing in this seed set — but must not error",
            q
        );
    }
}

#[tokio::test]
async fn sqlite_search_still_finds_assets_through_sanitized_phrases() {
    let (assets, _, _, _) = setup_repos().await;
    assets
        .create(
            common::make_asset_request("Integration Probe", "dispatch wiring بانک"),
            "user1",
        )
        .await
        .unwrap();
    assets
        .create(
            common::make_asset_request("Unrelated", "nothing here"),
            "user1",
        )
        .await
        .unwrap();

    // Single tokens, multi-word queries, and unicode all keep matching.
    for q in [
        "integration",
        "Integration Probe",
        "dispatch wiring",
        "بانک",
    ] {
        let (res, _) = assets
            .search(Some(q), vec![], 10, None)
            .await
            .unwrap_or_else(|e| panic!("search {:?} must not fail: {}", q, e));
        assert!(
            !res.is_empty(),
            "query {:?} should find the seeded asset",
            q
        );
    }

    let (res, _) = assets
        .search(Some("integration"), vec![], 10, None)
        .await
        .unwrap();
    assert!(
        res.iter().all(|a| a.title == "Integration Probe"),
        "sanitized match must stay precise, not widen to everything"
    );
}

#[tokio::test]
async fn sqlite_asset_search_compact_omits_content() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_search_compact_omits_content(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_contents_batch() {
    let (assets, _, _, _) = setup_repos().await;
    common::asset_get_contents_batch(&assets).await;
}

#[tokio::test]
async fn sqlite_tags_covering_index_exists() {
    let pool = setup_sqlite().await;
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_tags_asset_id_category_value'",
    )
    .fetch_one(&pool)
    .await
    .expect("index lookup should succeed");
    assert_eq!(
        row.0, 1,
        "the tags covering index must exist after migrations (slow-tag-query fix)"
    );
}

// ============================================================================
// RETENTION PRUNE SERVICE TESTS (CPO ruling 8d591e85 — build-only deliverable,
// first execution gated on the CPO's sight of the dry-run numbers)
// ============================================================================

use pdt::service::AssetService;
use pdt::service::Services;

async fn setup_services() -> Services {
    let (assets, relations, collections, audit) = setup_repos().await;
    Services::from_repositories(assets, relations, collections, audit)
}

#[tokio::test]
async fn prune_dry_run_reports_numbers_zero_writes() {
    let services = setup_services().await;
    let user = "test-user";
    for i in 0..3 {
        AssetService::create(
            &services,
            common::make_asset_request_with_tags(
                &format!("old report {}", i),
                "body",
                vec![("type", "report")],
            ),
            user,
            &[],
        )
        .await
        .expect("seed asset");
    }
    AssetService::create(
        &services,
        common::make_asset_request_with_tags("keep me", "body", vec![("type", "invoice")]),
        user,
        &[],
    )
    .await
    .expect("seed other-class asset");

    let report = AssetService::prune(&services, "type", "report", 0, true, user)
        .await
        .expect("dry-run prune");
    assert_eq!(report["dry_run"], serde_json::json!(true));
    assert_eq!(report["matched"], serde_json::json!(3));
    assert_eq!(report["filter"]["tag_category"], serde_json::json!("type"));
    assert_eq!(report["filter"]["tag_value"], serde_json::json!("report"));

    // Zero writes: every candidate still live on the read surface.
    let (listed, _) = services
        .assets()
        .search(None, vec![("type".into(), "report".into())], 100, None)
        .await
        .expect("post-dry-run search");
    assert_eq!(listed.len(), 3, "dry-run must not write");
    // Zero NEW audits: the seed creates audited (4 entries) — the dry-run
    // itself must add none.
    let (audits_before, _) = services
        .audit()
        .list(None, None, None, 100, None)
        .await
        .expect("audit list before");
    let report2 = AssetService::prune(&services, "type", "report", 0, true, user)
        .await
        .expect("second dry-run prune");
    assert_eq!(report2["matched"], serde_json::json!(3));
    let (audits_after, _) = services
        .audit()
        .list(None, None, None, 100, None)
        .await
        .expect("audit list after");
    assert_eq!(
        audits_after.len(),
        audits_before.len(),
        "dry-run must not audit"
    );
}

#[tokio::test]
async fn prune_execute_is_soft_delete_only_with_audit() {
    let services = setup_services().await;
    let user = "test-user";
    let a = AssetService::create(
        &services,
        common::make_asset_request_with_tags("old report", "body", vec![("type", "report")]),
        user,
        &[],
    )
    .await
    .expect("seed candidate");
    let b = AssetService::create(
        &services,
        common::make_asset_request_with_tags("keep me", "body", vec![("type", "invoice")]),
        user,
        &[],
    )
    .await
    .expect("seed other-class asset");

    let report = AssetService::prune(&services, "type", "report", 0, false, user)
        .await
        .expect("execute prune");
    assert_eq!(report["dry_run"], serde_json::json!(false));
    assert_eq!(report["matched"], serde_json::json!(1));
    assert_eq!(report["pruned"], serde_json::json!(1));
    assert_eq!(report["failed"], serde_json::json!([]));

    // Gone from the live surface...
    assert!(
        services.assets().get_by_id(&a.id).await.is_err(),
        "pruned asset must leave the live read surface"
    );
    // ...but recoverable at the store (archive-over-delete hard line —
    // soft-delete stamp, never a hard delete).
    assert!(
        services.assets().exists(&a.id).await.expect("exists probe"),
        "pruned asset must remain at the store (soft-delete-only)"
    );
    // Other-class asset untouched.
    assert!(services.assets().get_by_id(&b.id).await.is_ok());

    // Audit trail: the delete is on the record (full cascade path).
    let (audits, _) = services
        .audit()
        .list(Some("asset"), Some(&a.id), None, 100, None)
        .await
        .expect("audit list");
    assert!(
        audits
            .iter()
            .any(|e| e.action == pdt::models::AuditAction::Delete),
        "prune execution must audit each removal"
    );
}

#[tokio::test]
async fn prune_age_filter_matches_only_older_than_window() {
    let pool = setup_sqlite().await;
    let services = Services::from_repositories(
        SqliteAssetRepository::new(pool.clone()),
        SqliteRelationRepository::new(pool.clone()),
        SqliteCollectionRepository::new(pool.clone()),
        SqliteAuditRepository::new(pool.clone()),
    );
    let user = "test-user";
    let old = AssetService::create(
        &services,
        common::make_asset_request_with_tags("old report", "body", vec![("type", "report")]),
        user,
        &[],
    )
    .await
    .expect("seed old asset");
    let fresh = AssetService::create(
        &services,
        common::make_asset_request_with_tags("fresh report", "body", vec![("type", "report")]),
        user,
        &[],
    )
    .await
    .expect("seed fresh asset");

    // Backdate the old asset directly — the test owns the pool.
    let backdated = (chrono::Utc::now() - chrono::Duration::days(45)).to_rfc3339();
    sqlx::query("UPDATE assets SET created_at = ? WHERE id = ?")
        .bind(&backdated)
        .bind(&old.id)
        .execute(&pool)
        .await
        .expect("backdate created_at");

    // 30-day window: only the backdated asset is a candidate.
    let report = AssetService::prune(&services, "type", "report", 30, true, user)
        .await
        .expect("dry-run with age window");
    assert_eq!(report["matched"], serde_json::json!(1));
    let candidates = report["candidates"].as_array().expect("candidates array");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["id"], serde_json::json!(old.id));
    assert_ne!(candidates[0]["id"], serde_json::json!(fresh.id));
}
