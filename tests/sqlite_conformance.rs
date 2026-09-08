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
    AssetRepository, SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteRelationRepository,
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
        .create(common::make_asset_request("Unrelated", "nothing here"), "user1")
        .await
        .unwrap();

    // Single tokens, multi-word queries, and unicode all keep matching.
    for q in ["integration", "Integration Probe", "dispatch wiring", "بانک"] {
        let (res, _) = assets
            .search(Some(q), vec![], 10, None)
            .await
            .unwrap_or_else(|e| panic!("search {:?} must not fail: {}", q, e));
        assert!(!res.is_empty(), "query {:?} should find the seeded asset", q);
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
