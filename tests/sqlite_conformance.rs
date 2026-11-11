//! SQLite backend conformance tests.
//!
//! These tests run the generic conformance test functions from `common/mod.rs`
//! against an in-memory SQLite database. Each test gets a fresh database
//! with migrations applied.
#![cfg(feature = "sqlite-backend")]

mod common;

use std::collections::HashMap;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use pdt::models::UpdateAssetRequest;
use pdt::repository::{
    AssetRepository, RelationRepository, SqliteAssetRepository, SqliteAuditRepository,
    SqliteCollectionRepository, SqliteGroupRepository, SqliteRelationRepository,
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
    let groups_sql = include_str!("../migrations/sqlite/002_groups.sql");
    sqlx::raw_sql(groups_sql)
        .execute(&pool)
        .await
        .expect("failed to run groups migrations");

    pool
}

/// Set up all 5 SQLite repositories backed by a single in-memory database.
async fn setup_repos() -> (
    SqliteAssetRepository,
    SqliteRelationRepository,
    SqliteCollectionRepository,
    SqliteAuditRepository,
    SqliteGroupRepository,
) {
    let pool = setup_sqlite().await;
    (
        SqliteAssetRepository::new(pool.clone()),
        SqliteRelationRepository::new(pool.clone()),
        SqliteCollectionRepository::new(pool.clone()),
        SqliteAuditRepository::new(pool.clone()),
        SqliteGroupRepository::new(pool),
    )
}

// ============================================================================
// ASSET REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_asset_create_and_get() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_create_and_get(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_by_id_not_found() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_get_by_id_not_found(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_by_id_prefix_resolves_unique() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_get_by_id_prefix_resolves_unique(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_by_id_prefix_absent() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_get_by_id_prefix_absent(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_by_id_prefix_wildcards_stay_literal() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_get_by_id_prefix_wildcards_stay_literal(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_update() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_update(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_soft_delete() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_soft_delete(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_exists_including_deleted() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_exists_including_deleted(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_list_with_pagination() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_list_with_pagination(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_list_filter_by_tag() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_list_filter_by_tag(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_by_text() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_search_by_text(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_no_results() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_search_no_results(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_no_query() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_search_no_query(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_with_tag_filter() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_search_with_tag_filter(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_search_cursor_pagination_and_created_at() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_search_cursor_pagination_and_created_at(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_add_and_remove_tag() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_add_and_remove_tag(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_update_auth_context() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_update_auth_context(&assets).await;
}

/// Regression (issue b32a486b): the metadata replace is DELETE-then-INSERT —
/// across separate pooled connections two concurrent updates used to
/// interleave (one writer's DELETE landing after the other's INSERTs,
/// silently reverting it — the lost-update class the Eynak leaf heartbeats
/// hit ~1-in-3-writes). The update now runs in ONE transaction: writers
/// serialize and the final metadata state is always exactly ONE writer's
/// complete set — never mixed, never partial.
#[tokio::test]
async fn sqlite_asset_concurrent_metadata_updates_never_interleave() {
    // A multi-connection pool on a SHARED file-backed db (the conformance
    // setup pins max_connections(1) — that would serialize trivially; and
    // :memory: gives each connection its own private db). WAL + file = the
    // prod shape.
    let db_path = std::env::temp_dir().join(format!(
        "pdt-interleave-test-{}-{}.db",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await
        .expect("failed to connect to temp SQLite");

    let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("failed to run migrations");
    let groups_sql = include_str!("../migrations/sqlite/002_groups.sql");
    sqlx::raw_sql(groups_sql)
        .execute(&pool)
        .await
        .expect("failed to run groups migrations");

    let assets = SqliteAssetRepository::new(pool.clone());
    let created = assets
        .create(common::make_asset_request("Racer", "content"), "user1")
        .await
        .unwrap();

    let mk = |k: &str, v: i64| UpdateAssetRequest {
        title: None,
        content: None,
        metadata: Some(HashMap::from([(k.to_string(), serde_json::json!(v))])),
    };

    // 25 rounds of concurrent metadata replaces: each round the final
    // metadata must be EXACTLY one writer's complete set — never mixed
    // (both keys), never partial (zero rows), never a stale value.
    for round in 0..25i64 {
        let ra = assets.update(&created.id, mk("writer_a", round), "userA");
        let rb = assets.update(&created.id, mk("writer_b", round), "userB");
        let (ra, rb) = tokio::join!(ra, rb);
        ra.expect("concurrent update A should succeed");
        rb.expect("concurrent update B should succeed");

        let final_asset = assets.get_by_id(&created.id).await.unwrap();
        assert_eq!(
            final_asset.metadata.len(),
            1,
            "round {round}: metadata must hold exactly ONE writer's key, got {:?}",
            final_asset.metadata
        );
        let (_key, value) = final_asset.metadata.iter().next().unwrap();
        assert_eq!(
            value.as_i64(),
            Some(round),
            "round {round}: the surviving write must carry THIS round's value, got {:?}",
            value
        );
    }

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
}

// ============================================================================
// RELATION REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_relation_create_and_get() {
    let (assets, relations, _, _, _) = setup_repos().await;
    common::relation_create_and_get(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_has_access_round_trip() {
    // tocpi workspace membership v1 rides the has_access relation type —
    // the sqlite parse mapping must round-trip it, never swallow it to
    // related_to (the silent-swallow class, parse_relation_type `_` arm).
    use pdt::models::{CreateAssetRequest, CreateRelationRequest, RelationType};
    let (assets, relations, _, _, _) = setup_repos().await;

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
    let (_, relations, _, _, _) = setup_repos().await;
    common::relation_get_by_id_not_found(&relations).await;
}

#[tokio::test]
async fn sqlite_relation_delete() {
    let (_, relations, _, _, _) = setup_repos().await;
    common::relation_delete(&relations).await;
}

#[tokio::test]
async fn sqlite_relation_get_asset_relations() {
    let (assets, relations, _, _, _) = setup_repos().await;
    common::relation_get_asset_relations(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_delete_by_asset() {
    let (assets, relations, _, _, _) = setup_repos().await;
    common::relation_delete_by_asset(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_traverse_graph() {
    let (assets, relations, _, _, _) = setup_repos().await;
    common::relation_traverse_graph(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_get_descendants() {
    let (assets, relations, _, _, _) = setup_repos().await;
    common::relation_get_descendants(&assets, &relations).await;
}

#[tokio::test]
async fn sqlite_relation_would_create_cycle() {
    let (assets, relations, _, _, _) = setup_repos().await;
    common::relation_would_create_cycle(&assets, &relations).await;
}

// ============================================================================
// COLLECTION REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_collection_create_and_get() {
    let (_, _, collections, _, _) = setup_repos().await;
    common::collection_create_and_get(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_get_by_id_not_found() {
    let (_, _, collections, _, _) = setup_repos().await;
    common::collection_get_by_id_not_found(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_update() {
    let (_, _, collections, _, _) = setup_repos().await;
    common::collection_update(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_delete() {
    let (_, _, collections, _, _) = setup_repos().await;
    common::collection_delete(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_list_with_pagination() {
    let (_, _, collections, _, _) = setup_repos().await;
    common::collection_list_with_pagination(&collections).await;
}

#[tokio::test]
async fn sqlite_collection_add_and_remove_asset() {
    let (assets, _, collections, _, _) = setup_repos().await;
    common::collection_add_and_remove_asset(&assets, &collections).await;
}

#[tokio::test]
async fn sqlite_collection_remove_asset_from_all() {
    let (assets, _, collections, _, _) = setup_repos().await;
    common::collection_remove_asset_from_all(&assets, &collections).await;
}

// ============================================================================
// AUDIT REPOSITORY TESTS
// ============================================================================

#[tokio::test]
async fn sqlite_audit_create_and_list() {
    let (_, _, _, audit, _) = setup_repos().await;
    common::audit_create_and_list(&audit).await;
}

#[tokio::test]
async fn sqlite_audit_list_with_filters() {
    let (_, _, _, audit, _) = setup_repos().await;
    common::audit_list_with_filters(&audit).await;
}

#[tokio::test]
async fn sqlite_audit_get_entity_history() {
    let (_, _, _, audit, _) = setup_repos().await;
    common::audit_get_entity_history(&audit).await;
}

// ============================================================================
// CROSS-REPOSITORY INTEGRATION TEST
// ============================================================================

#[tokio::test]
async fn sqlite_asset_delete_cascades() {
    let (assets, relations, collections, audit, groups) = setup_repos().await;
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
    let (assets, _, _, _, _) = setup_repos().await;
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
    let (assets, _, _, _, _) = setup_repos().await;
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
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_search_compact_omits_content(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_get_contents_batch() {
    let (assets, _, _, _, _) = setup_repos().await;
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
    let (assets, relations, collections, audit, groups) = setup_repos().await;
    Services::from_repositories(assets, relations, collections, audit, groups)
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
        SqliteGroupRepository::new(pool.clone()),
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

// ============================================================================
// COUNT / TRUE-TOTAL TESTS (issue b040d645)
// ============================================================================

#[tokio::test]
async fn sqlite_asset_count_search_matches_filter() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_count_search_matches_filter(&assets).await;
}

#[tokio::test]
async fn sqlite_asset_count_list_matches_filter() {
    let (assets, _, _, _, _) = setup_repos().await;
    common::asset_count_list_matches_filter(&assets).await;
}

// ============================================================================
// CASCADE AUDIT TEST (issue 40df9c5c — F15)
// ============================================================================

#[tokio::test]
async fn asset_delete_audit_carries_cascade_relations() {
    // Issue 40df9c5c: the relation cascade was unaudited — the asset-delete
    // audit entry carried an EMPTY changes object while the cascade
    // hard-deleted the relation rows (both backends), so a forensic walk
    // could not reconstruct which relations an asset delete removed. The
    // ONE audit entry must carry the removed relation summary (one entry,
    // not N — less is more).
    use pdt::models::{AuditAction, CreateRelationRequest, RelationType};
    use pdt::service::RelationService;
    use std::collections::HashMap;

    let services = setup_services().await;
    let user = "test-user";
    let parent = AssetService::create(
        &services,
        common::make_asset_request("parent", "body"),
        user,
        &[],
    )
    .await
    .expect("seed parent");
    let child = AssetService::create(
        &services,
        common::make_asset_request("child", "body"),
        user,
        &[],
    )
    .await
    .expect("seed child");
    let other = AssetService::create(
        &services,
        common::make_asset_request("other", "body"),
        user,
        &[],
    )
    .await
    .expect("seed unrelated");

    let rel1 = RelationService::create(
        &services,
        CreateRelationRequest {
            from_asset_id: parent.id.clone(),
            to_asset_id: child.id.clone(),
            relation_type: RelationType::Contains,
            metadata: HashMap::new(),
        },
        user,
    )
    .await
    .expect("seed relation 1");
    let rel2 = RelationService::create(
        &services,
        CreateRelationRequest {
            from_asset_id: other.id.clone(),
            to_asset_id: parent.id.clone(),
            relation_type: RelationType::References,
            metadata: HashMap::new(),
        },
        user,
    )
    .await
    .expect("seed relation 2");

    AssetService::delete(&services, &parent.id, user)
        .await
        .expect("delete parent");

    // Walk-verify: the cascade removed BOTH relations (from AND to legs).
    let remaining = services
        .relations()
        .get_asset_relations(&parent.id)
        .await
        .expect("post-delete walk");
    assert!(
        remaining.is_empty(),
        "cascade must remove relations touching the deleted asset"
    );

    // The ONE audit entry carries the removed relation summary.
    let (audits, _) = services
        .audit()
        .list(None, None, None, 100, None)
        .await
        .expect("audit list");
    let delete_entry = audits
        .iter()
        .find(|a| a.action == AuditAction::Delete && a.entity_id == parent.id)
        .expect("asset delete audit entry");
    assert_eq!(delete_entry.changes["cascade"], serde_json::json!(true));
    assert_eq!(
        delete_entry.changes["relations_removed"],
        serde_json::json!(2)
    );
    let relations = delete_entry.changes["relations"]
        .as_array()
        .expect("relations array in audit entry");
    let ids: Vec<&str> = relations
        .iter()
        .map(|r| r["id"].as_str().expect("relation id"))
        .collect();
    assert!(
        ids.contains(&rel1.id.as_str()),
        "audit must name removed relation {}",
        rel1.id
    );
    assert!(
        ids.contains(&rel2.id.as_str()),
        "audit must name removed relation {}",
        rel2.id
    );
    // Endpoints + type carried per relation — the rows are hard-deleted, so
    // the audit entry is the only reconstruction record.
    for r in relations {
        assert!(r["from_asset_id"].is_string(), "from endpoint carried");
        assert!(r["to_asset_id"].is_string(), "to endpoint carried");
        assert!(r["relation_type"].is_string(), "relation type carried");
    }
}

// ============================================================================
// ADOPT GUARDS (CPO ruling 6d86ee70 — the Path A adopt's first-class path)
// ============================================================================

mod adopt_guards {
    //! The ruling's two GUARDS + the happy path, standalone verbatim
    //! (release checklist v3):
    //! - duplicate-refusal: an adopt over an existing uuid refuses loudly
    //!   (409 class) and the store keeps exactly ONE copy;
    //! - audit-write-DENIES: a failed audit write rolls the whole adopt
    //!   back — the asset is ABSENT (unaudited materialization = silent
    //!   widening, machine-enforced by the transaction);
    //! - happy path: the adopt lands + the audit entry (action=adopt,
    //!   provenance in changes) exists;
    //! - provenance required.

    use pdt::error::ApiError;
    use pdt::models::{AdoptAssetRequest, AuditAction, CreateAssetRequest};
    use pdt::repository::SqliteAssetRepository;
    use pdt::service::AssetService;
    use pdt::service::Services;

    async fn setup_repos() -> (
        SqliteAssetRepository,
        SqliteAssetRepository,
        pdt::repository::SqliteAuditRepository,
        pdt::repository::SqliteRelationRepository,
        pdt::repository::SqliteCollectionRepository,
        pdt::repository::SqliteGroupRepository,
        sqlx::SqlitePool,
    ) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(":memory:")
                    .create_if_missing(true)
                    .foreign_keys(true),
            )
            .await
            .expect("in-memory sqlite");
        let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
        sqlx::raw_sql(migration_sql)
            .execute(&pool)
            .await
            .expect("migrations");
        let groups_sql = include_str!("../migrations/sqlite/002_groups.sql");
        sqlx::raw_sql(groups_sql)
            .execute(&pool)
            .await
            .expect("groups migrations");
        (
            SqliteAssetRepository::new(pool.clone()),
            SqliteAssetRepository::new(pool.clone()),
            pdt::repository::SqliteAuditRepository::new(pool.clone()),
            pdt::repository::SqliteRelationRepository::new(pool.clone()),
            pdt::repository::SqliteCollectionRepository::new(pool.clone()),
            pdt::repository::SqliteGroupRepository::new(pool.clone()),
            pool,
        )
    }

    fn adopt_request(title: &str, provenance: &str) -> AdoptAssetRequest {
        AdoptAssetRequest {
            title: title.to_string(),
            content: Some("adopted body".to_string()),
            tags: vec![],
            metadata: Default::default(),
            auth_context: None,
            provenance: provenance.to_string(),
        }
    }

    const LOCKED_ID: &str = "78398674-e25b-449a-9e7c-544511d398aa";
    const BANKED_SOURCE: &str = "pre-dirauth leaf backup /var/lib/pdt-backups/leaf-a4613285-pre-dirauth-20261101T0023Z.db (00:23Z Nov 1)";

    /// GUARD 1 — DUPLICATE-REFUSAL: an adopt over an existing uuid refuses
    /// loudly; the store keeps exactly ONE copy.
    #[tokio::test]
    async fn duplicate_refusal() {
        let (assets, _assets2, audit, relations, collections, groups, _pool) = setup_repos().await;
        let services = Services::from_repositories(assets, relations, collections, audit, groups);

        // First adopt lands.
        let adopted = AssetService::adopt(
            &services,
            LOCKED_ID,
            adopt_request("Profile: Ravand-nox", BANKED_SOURCE),
            "hafez-custodian",
        )
        .await
        .expect("first adopt lands");
        assert_eq!(
            adopted.id, LOCKED_ID,
            "the adopt materializes the SPECIFIED uuid"
        );

        // Second adopt over the SAME uuid refuses loudly (409 class).
        let err = AssetService::adopt(
            &services,
            LOCKED_ID,
            adopt_request("Profile: Ravand-nox", BANKED_SOURCE),
            "hafez-custodian",
        )
        .await
        .expect_err("duplicate adopt must refuse");
        assert!(
            matches!(err, ApiError::Conflict(_)),
            "duplicate-barred = loud 409, got {:?}",
            err
        );

        // The store holds exactly ONE copy.
        let asset = AssetService::get(&services, LOCKED_ID)
            .await
            .expect("the one copy resolves");
        assert_eq!(asset.id, LOCKED_ID);
    }

    /// GUARD 2 — AUDIT-WRITE DENIES: a failed audit write rolls the whole
    /// adopt back — the asset is ABSENT (the transaction is the
    /// machine-enforced deny).
    #[tokio::test]
    async fn audit_write_failure_denies_the_adopt() {
        let (assets, _assets2, audit, relations, collections, groups, pool) = setup_repos().await;
        let services = Services::from_repositories(assets, relations, collections, audit, groups);

        // Sabotage the audit surface: drop the audit_entries table — the
        // adopt's audit write will fail, and the ruling's deny must roll
        // the materialization back.
        sqlx::raw_sql("DROP TABLE audit_entries;")
            .execute(&pool)
            .await
            .expect("audit table dropped");

        let err = AssetService::adopt(
            &services,
            LOCKED_ID,
            adopt_request("Profile: Ravand-nox", BANKED_SOURCE),
            "hafez-custodian",
        )
        .await
        .expect_err("the adopt must be DENIED when the audit write fails");
        assert!(
            !matches!(err, ApiError::Conflict(_)),
            "the failure is the audit write, not the duplicate bar: {:?}",
            err
        );

        // THE ASSET IS ABSENT — the transaction rolled back; no unaudited
        // materialization exists.
        let get_err = AssetService::get(&services, LOCKED_ID)
            .await
            .expect_err("the adopted asset must NOT exist");
        assert!(matches!(get_err, ApiError::NotFound(_)));
    }

    /// HAPPY PATH: the adopt lands + the audit entry (action=adopt,
    /// provenance in changes) exists — every adopt is audited.
    #[tokio::test]
    async fn adopt_lands_and_audits() {
        let (assets, _assets2, audit, relations, collections, groups, _pool) = setup_repos().await;
        let services = Services::from_repositories(assets, relations, collections, audit, groups);

        let adopted = AssetService::adopt(
            &services,
            LOCKED_ID,
            adopt_request("Profile: Ravand-nox", BANKED_SOURCE),
            "hafez-custodian",
        )
        .await
        .expect("adopt lands");
        assert_eq!(adopted.id, LOCKED_ID);
        assert_eq!(adopted.title, "Profile: Ravand-nox");

        // The audit entry exists with the provenance.
        let entries = services
            .audit()
            .list(Some("asset"), Some(LOCKED_ID), None, 100, None)
            .await
            .expect("audit list")
            .0;
        let adopt_entry = entries
            .iter()
            .find(|e| e.action == AuditAction::Adopt)
            .expect("the adopt is audited");
        assert_eq!(
            adopt_entry.changes["provenance"],
            serde_json::json!(BANKED_SOURCE),
            "the audit entry carries the banked source (provenance required)"
        );
        assert_eq!(adopt_entry.user_id, "hafez-custodian");
    }

    /// PROVENANCE REQUIRED: an adopt without provenance refuses (400
    /// class) before any store write.
    #[tokio::test]
    async fn provenance_required() {
        let (assets, _assets2, audit, relations, collections, groups, _pool) = setup_repos().await;
        let services = Services::from_repositories(assets, relations, collections, audit, groups);

        let err = AssetService::adopt(
            &services,
            LOCKED_ID,
            adopt_request("Profile: Ravand-nox", "   "),
            "hafez-custodian",
        )
        .await
        .expect_err("empty provenance must refuse");
        assert!(
            matches!(err, ApiError::Validation(_)),
            "provenance required = 400 class, got {:?}",
            err
        );
        let _ = std::marker::PhantomData::<CreateAssetRequest>;
    }
}

// ============================================================================
// SHORT-ID PREFIX RESOLUTION (issue e9478d91) — service-level pins
// ============================================================================

#[tokio::test]
async fn sqlite_resolve_unique_prefix_serves_canonical_row() {
    let (assets, rel, col, aud, grp) = setup_repos().await;
    let created = assets
        .create(
            pdt::models::CreateAssetRequest {
                title: "Prefix specimen".to_string(),
                content: Some("content".to_string()),
                tags: vec![],
                metadata: std::collections::HashMap::new(),
                auth_context: None,
            },
            "user1",
        )
        .await
        .unwrap();
    let prefix = created.id[..8].to_string();
    let services = pdt::service::Services::from_repositories(assets, rel, col, aud, grp);
    let resolved = pdt::service::AssetService::resolve(&services, &prefix)
        .await
        .expect("unique prefix resolves");
    assert_eq!(
        resolved.id, created.id,
        "the response id is the canonical stored form"
    );
}

#[tokio::test]
async fn sqlite_resolve_ambiguous_prefix_answers_409_with_count_and_hint() {
    let pool = setup_sqlite().await;
    // Two crafted ids sharing a long prefix — impossible with random UUIDs,
    // so the rows are seeded directly.
    for tail in [
        "00000000000000000000000000000001",
        "00000000000000000000000000000002",
    ] {
        sqlx::query(
            "INSERT INTO assets (id, title, content, created_at, updated_at, created_by, updated_by, deleted_at, visibility, confidentiality, owner_groups)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, 'team', '', '[]')",
        )
        .bind(format!("aaaa0000-0000-0000-0000-{}", tail))
        .bind("Ambiguous specimen")
        .bind("")
        .bind("2026-11-08T00:00:00+00:00")
        .bind("2026-11-08T00:00:00+00:00")
        .bind("user1")
        .bind("user1")
        .execute(&pool)
        .await
        .expect("seed row");
    }
    let assets = pdt::repository::SqliteAssetRepository::new(pool.clone());
    let services = pdt::service::Services::from_repositories(
        assets,
        pdt::repository::SqliteRelationRepository::new(pool.clone()),
        pdt::repository::SqliteCollectionRepository::new(pool.clone()),
        pdt::repository::SqliteAuditRepository::new(pool.clone()),
        pdt::repository::SqliteGroupRepository::new(pool),
    );
    let err = pdt::service::AssetService::resolve(&services, "aaaa0000")
        .await
        .expect_err("ambiguous prefix answers 409");
    let msg = err.to_string();
    assert!(
        msg.contains("2 live assets"),
        "the body carries the match count: {msg}"
    );
    assert!(
        msg.contains("use at least"),
        "the body carries the min-length hint: {msg}"
    );
}

#[tokio::test]
async fn sqlite_resolve_malformed_non_hex_answers_422() {
    let (assets, rel, col, aud, grp) = setup_repos().await;
    let services = pdt::service::Services::from_repositories(assets, rel, col, aud, grp);
    let err = pdt::service::AssetService::resolve(&services, "ghost-id")
        .await
        .expect_err("non-hex id answers 422, never a lying 404");
    assert!(
        err.to_string().contains("Malformed id"),
        "the id-format error: {err}"
    );
}

#[tokio::test]
async fn sqlite_resolve_uppercase_prefix_resolves_case_insensitively() {
    let (assets, _, _, _, _) = setup_repos().await;
    let created = assets
        .create(
            pdt::models::CreateAssetRequest {
                title: "Case specimen".to_string(),
                content: Some("content".to_string()),
                tags: vec![],
                metadata: std::collections::HashMap::new(),
                auth_context: None,
            },
            "user1",
        )
        .await
        .unwrap();
    let prefix = created.id[..8].to_uppercase();
    let matches = assets.get_by_id_prefix(&prefix).await.unwrap();
    assert_eq!(
        matches.len(),
        1,
        "uppercase input resolves (case-insensitive)"
    );
    assert_eq!(matches[0].id, created.id, "the response id stays canonical");
}

#[tokio::test]
async fn sqlite_resolve_prefix_is_tenancy_scoped_cross_band_404() {
    // The tenancy mask applies to prefix queries (security-adjacent, tested
    // explicitly): an asset created in store A is unreachable by prefix
    // from store B — the prefix query scans the tenant's own DB only.
    let (assets_a, _, _, _, _) = setup_repos().await;
    let (assets_b, _, _, _, _) = setup_repos().await;
    let created = assets_a
        .create(
            pdt::models::CreateAssetRequest {
                title: "Band A specimen".to_string(),
                content: Some("content".to_string()),
                tags: vec![],
                metadata: std::collections::HashMap::new(),
                auth_context: None,
            },
            "user1",
        )
        .await
        .unwrap();
    let prefix = created.id[..8].to_string();
    let matches = assets_b.get_by_id_prefix(&prefix).await.unwrap();
    assert!(
        matches.is_empty(),
        "a cross-band prefix is absent in store B — 404, never a bypass"
    );
}
