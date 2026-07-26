//! Shared generic conformance tests for repository trait implementations.
//!
//! These functions are generic over the repository traits and can be
//! invoked from any backend's test file (sqlite, postgres, mongo, etc.).
//!
//! Usage from a test file:
//!   mod common;
//!   #[tokio::test]
//!   async fn sqlite_asset_crud() {
//!       let repos = common::setup_sqlite().await;
//!       common::asset_create_and_get(&repos).await;
//!   }

#![allow(dead_code)]

use std::collections::HashMap;

use pdt::models::{
    AddTagRequest, Asset, AuditAction, CreateAssetRequest, CreateCollectionRequest,
    CreateRelationRequest, RelationType, UpdateAssetRequest, UpdateAuthContextRequest,
    UpdateCollectionRequest, AuthContext,
};
use pdt::repository::{
    AssetRepository, AuditRepository, CollectionRepository, RelationRepository,
};

// ============================================================================
// TEST FIXTURE: A bundle of all 4 repositories for integration testing.
// ============================================================================

pub struct TestRepos<A, R, C, U>
where
    A: AssetRepository,
    R: RelationRepository,
    C: CollectionRepository,
    U: AuditRepository,
{
    pub assets: A,
    pub relations: R,
    pub collections: C,
    pub audit: U,
}

// ============================================================================
// HELPER: Build a CreateAssetRequest with sensible defaults.
// ============================================================================

pub fn make_asset_request(title: &str, content: &str) -> CreateAssetRequest {
    CreateAssetRequest {
        title: title.to_string(),
        content: Some(content.to_string()),
        tags: vec![],
        metadata: HashMap::new(),
        auth_context: None,
    }
}

pub fn make_asset_request_with_tags(
    title: &str,
    content: &str,
    tags: Vec<(&str, &str)>,
) -> CreateAssetRequest {
    CreateAssetRequest {
        title: title.to_string(),
        content: Some(content.to_string()),
        tags: tags
            .iter()
            .map(|(c, v)| AddTagRequest {
                category: c.to_string(),
                value: v.to_string(),
            })
            .collect(),
        metadata: HashMap::new(),
        auth_context: None,
    }
}

// ============================================================================
// ASSET REPOSITORY CONFORMANCE TESTS
// ============================================================================

pub async fn asset_create_and_get<A: AssetRepository>(assets: &A) {
    let req = make_asset_request("Test Asset", "Some content");
    let created = assets
        .create(req, "user1")
        .await
        .expect("create should succeed");

    assert!(!created.id.is_empty());
    assert_eq!(created.title, "Test Asset");
    assert_eq!(created.content.as_deref(), Some("Some content"));
    assert_eq!(created.created_by, "user1");
    assert_eq!(created.updated_by, "user1");
    assert!(created.deleted_at.is_none());

    let fetched = assets
        .get_by_id(&created.id)
        .await
        .expect("get_by_id should succeed");

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.title, "Test Asset");
    assert_eq!(fetched.content.as_deref(), Some("Some content"));
}

pub async fn asset_get_by_id_not_found<A: AssetRepository>(assets: &A) {
    let result = assets.get_by_id("nonexistent-id-12345").await;
    assert!(result.is_err(), "should return error for missing asset");
}

pub async fn asset_update<A: AssetRepository>(assets: &A) {
    let created = assets
        .create(make_asset_request("Original", "Original content"), "user1")
        .await
        .unwrap();

    let update_req = UpdateAssetRequest {
        title: Some("Updated Title".to_string()),
        content: Some("Updated content".to_string()),
        metadata: None,
    };

    let updated = assets
        .update(&created.id, update_req, "user2")
        .await
        .expect("update should succeed");

    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.content.as_deref(), Some("Updated content"));
    assert_eq!(updated.updated_by, "user2");
    assert!(updated.updated_at > created.updated_at);
}

pub async fn asset_soft_delete<A: AssetRepository>(assets: &A) {
    let created = assets
        .create(make_asset_request("To Delete", "Content"), "user1")
        .await
        .unwrap();

    assets
        .soft_delete(&created.id)
        .await
        .expect("soft_delete should succeed");

    // Should not be findable by get_by_id
    let result = assets.get_by_id(&created.id).await;
    assert!(
        result.is_err(),
        "soft-deleted asset should not be returned by get_by_id"
    );
}

pub async fn asset_exists_including_deleted<A: AssetRepository>(assets: &A) {
    let created = assets
        .create(make_asset_request("Exists Test", "Content"), "user1")
        .await
        .unwrap();

    assert!(
        assets.exists(&created.id).await.unwrap(),
        "asset should exist before delete"
    );

    assets.soft_delete(&created.id).await.unwrap();

    assert!(
        assets.exists(&created.id).await.unwrap(),
        "asset should still exist after soft delete (exists includes deleted)"
    );

    assert!(
        !assets.exists("totally-nonexistent").await.unwrap(),
        "nonexistent asset should not exist"
    );
}

pub async fn asset_list_with_pagination<A: AssetRepository>(assets: &A) {
    // Create 5 assets
    for i in 0..5 {
        assets
            .create(
                make_asset_request(&format!("Page Asset {}", i), &format!("Content {}", i)),
                "user1",
            )
            .await
            .unwrap();
    }

    // Request limit=2
    let (page1, next_cursor) = assets
        .list(2, None, None, "created_at", "desc")
        .await
        .expect("list should succeed");

    assert_eq!(page1.len(), 2, "first page should have 2 assets");
    assert!(next_cursor.is_some(), "should have a next cursor");

    // Fetch second page
    let (page2, next_cursor2) = assets
        .list(2, next_cursor.as_deref(), None, "created_at", "desc")
        .await
        .expect("list page 2 should succeed");

    assert_eq!(page2.len(), 2, "second page should have 2 assets");
    assert!(next_cursor2.is_some(), "should have another cursor");

    // Fetch third page
    let (page3, next_cursor3) = assets
        .list(2, next_cursor2.as_deref(), None, "created_at", "desc")
        .await
        .expect("list page 3 should succeed");

    assert_eq!(page3.len(), 1, "third page should have 1 asset");
    assert!(next_cursor3.is_none(), "no more pages");
}

pub async fn asset_list_filter_by_tag<A: AssetRepository>(assets: &A) {
    // Create assets with specific tags
    assets
        .create(
            make_asset_request_with_tags(
                "Doc Asset",
                "Document content",
                vec![("asset_type", "document")],
            ),
            "user1",
        )
        .await
        .unwrap();

    assets
        .create(
            make_asset_request_with_tags(
                "Idea Asset",
                "Idea content",
                vec![("asset_type", "idea")],
            ),
            "user1",
        )
        .await
        .unwrap();

    // Filter by asset_type=document
    let (results, _) = assets
        .list(10, None, Some("document"), "created_at", "desc")
        .await
        .expect("list with tag filter should succeed");

    assert!(
        results.iter().all(|a| a.title == "Doc Asset"),
        "should only return assets with asset_type=document"
    );
}

pub async fn asset_search_by_text<A: AssetRepository>(assets: &A) {
    assets
        .create(
            make_asset_request("SQLite Guide", "Learn SQLite FTS5 search"),
            "user1",
        )
        .await
        .unwrap();

    assets
        .create(
            make_asset_request("Postgres Guide", "Learn PostgreSQL search"),
            "user1",
        )
        .await
        .unwrap();

    let (results, _) = assets
        .search(Some("sqlite"), vec![], 10, None)
        .await
        .expect("search should succeed");

    assert!(
        !results.is_empty(),
        "search for 'sqlite' should find at least one result"
    );
    assert!(
        results.iter().any(|a| a.title.contains("SQLite")),
        "should find the SQLite Guide asset"
    );
}

pub async fn asset_search_no_results<A: AssetRepository>(assets: &A) {
    let (results, _) = assets
        .search(Some("zzzznonexistentzzzz"), vec![], 10, None)
        .await
        .expect("search should succeed");

    assert!(results.is_empty(), "should find no results");
}

pub async fn asset_search_no_query<A: AssetRepository>(assets: &A) {
    // Create at least one asset
    assets
        .create(make_asset_request("List All", "Some content"), "user1")
        .await
        .unwrap();

    let (results, _) = assets
        .search(None, vec![], 10, None)
        .await
        .expect("search with no query should succeed");

    assert!(
        !results.is_empty(),
        "search with no query should return all assets"
    );
}

pub async fn asset_search_with_tag_filter<A: AssetRepository>(assets: &A) {
    assets
        .create(
            make_asset_request_with_tags(
                "Tagged Search Doc",
                "Searchable content here",
                vec![("type", "document"), ("status", "published")],
            ),
            "user1",
        )
        .await
        .unwrap();

    let (results, _) = assets
        .search(
            None,
            vec![("type".to_string(), "document".to_string())],
            10,
            None,
        )
        .await
        .expect("search with tag filter should succeed");

    assert!(
        !results.is_empty(),
        "tag-filtered search should find results"
    );
    assert!(
        results.iter().any(|a| a.title == "Tagged Search Doc"),
        "should find the tagged asset"
    );
}

pub async fn asset_add_and_remove_tag<A: AssetRepository>(assets: &A) {
    let created = assets
        .create(make_asset_request("Tag Test", "Content"), "user1")
        .await
        .unwrap();

    assert!(created.tags.is_empty(), "asset should start with no tags");

    let tag = assets
        .add_tag(
            &created.id,
            AddTagRequest {
                category: "domain".to_string(),
                value: "backend".to_string(),
            },
            "user2",
        )
        .await
        .expect("add_tag should succeed");

    assert_eq!(tag.category, "domain");
    assert_eq!(tag.value, "backend");
    assert_eq!(tag.added_by, "user2");

    // Verify tag is persisted
    let fetched = assets.get_by_id(&created.id).await.unwrap();
    assert_eq!(fetched.tags.len(), 1);
    assert_eq!(fetched.tags[0].category, "domain");
    assert_eq!(fetched.tags[0].value, "backend");

    // Remove the tag
    assets
        .remove_tag(&created.id, &tag.id)
        .await
        .expect("remove_tag should succeed");

    let fetched = assets.get_by_id(&created.id).await.unwrap();
    assert!(
        fetched.tags.is_empty(),
        "asset should have no tags after removal"
    );
}

pub async fn asset_update_auth_context<A: AssetRepository>(assets: &A) {
    let created = assets
        .create(make_asset_request("Auth Ctx Test", "Content"), "user1")
        .await
        .unwrap();

    let ctx = AuthContext {
        visibility: "private".to_string(),
        owner_groups: vec!["admins".to_string()],
        confidentiality: "confidential".to_string(),
    };

    let updated = assets
        .update_auth_context(&created.id, &ctx)
        .await
        .expect("update_auth_context should succeed");

    let fetched_ctx = updated.auth_context.expect("should have auth_context");
    assert_eq!(fetched_ctx.visibility, "private");
    assert_eq!(fetched_ctx.confidentiality, "confidential");
    assert!(fetched_ctx.owner_groups.contains(&"admins".to_string()));
}

// ============================================================================
// RELATION REPOSITORY CONFORMANCE TESTS
// ============================================================================

pub async fn relation_create_and_get<A: AssetRepository, R: RelationRepository>(
    assets: &A,
    relations: &R,
) {
    let a1 = assets
        .create(make_asset_request("Asset A", "Content A"), "user1")
        .await
        .unwrap();
    let a2 = assets
        .create(make_asset_request("Asset B", "Content B"), "user1")
        .await
        .unwrap();

    let req = CreateRelationRequest {
        from_asset_id: a1.id.clone(),
        to_asset_id: a2.id.clone(),
        relation_type: RelationType::RelatedTo,
        metadata: HashMap::new(),
    };

    let created = relations
        .create(req, "user1")
        .await
        .expect("create relation should succeed");

    assert!(!created.id.is_empty());
    assert_eq!(created.from_asset_id, a1.id);
    assert_eq!(created.to_asset_id, a2.id);
    assert_eq!(created.relation_type, RelationType::RelatedTo);

    let fetched = relations
        .get_by_id(&created.id)
        .await
        .expect("get_by_id should succeed");

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.from_asset_id, a1.id);
}

pub async fn relation_get_by_id_not_found<R: RelationRepository>(relations: &R) {
    let result = relations.get_by_id("nonexistent-rel-id").await;
    assert!(result.is_err(), "should error for missing relation");
}

pub async fn relation_delete<R: RelationRepository>(relations: &R) {
    // This test needs assets to exist — caller should pre-seed
    // We test delete on a nonexistent ID
    let result = relations.delete("nonexistent-rel-id").await;
    assert!(result.is_err(), "should error deleting missing relation");
}

pub async fn relation_get_asset_relations<A: AssetRepository, R: RelationRepository>(
    assets: &A,
    relations: &R,
) {
    let a1 = assets
        .create(make_asset_request("Hub Asset", "Hub content"), "user1")
        .await
        .unwrap();
    let a2 = assets
        .create(make_asset_request("Connected Asset", "Connected"), "user1")
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: a1.id.clone(),
                to_asset_id: a2.id.clone(),
                relation_type: RelationType::RelatedTo,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    let asset1_relations = relations
        .get_asset_relations(&a1.id)
        .await
        .expect("get_asset_relations for a1 should succeed");

    assert_eq!(
        asset1_relations.len(),
        1,
        "a1 should have 1 relation"
    );

    let asset2_relations = relations
        .get_asset_relations(&a2.id)
        .await
        .expect("get_asset_relations for a2 should succeed");

    assert_eq!(
        asset2_relations.len(),
        1,
        "a2 should see the relation from its side too"
    );
}

pub async fn relation_delete_by_asset<A: AssetRepository, R: RelationRepository>(
    assets: &A,
    relations: &R,
) {
    let a1 = assets
        .create(make_asset_request("Delete By Asset 1", "Content"), "user1")
        .await
        .unwrap();
    let a2 = assets
        .create(make_asset_request("Delete By Asset 2", "Content"), "user1")
        .await
        .unwrap();
    let a3 = assets
        .create(make_asset_request("Delete By Asset 3", "Content"), "user1")
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: a1.id.clone(),
                to_asset_id: a2.id.clone(),
                relation_type: RelationType::RelatedTo,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: a1.id.clone(),
                to_asset_id: a3.id.clone(),
                relation_type: RelationType::DependsOn,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    let count = relations
        .delete_by_asset(&a1.id)
        .await
        .expect("delete_by_asset should succeed");

    assert_eq!(count, 2, "should have deleted 2 relations");

    let remaining = relations.get_asset_relations(&a1.id).await.unwrap();
    assert!(remaining.is_empty(), "no relations should remain");
}

pub async fn relation_traverse_graph<A: AssetRepository, R: RelationRepository>(
    assets: &A,
    relations: &R,
) {
    // Build: A -> B -> C (all RelatedTo)
    let a = assets
        .create(make_asset_request("Graph Root", "Root"), "user1")
        .await
        .unwrap();
    let b = assets
        .create(make_asset_request("Graph Middle", "Middle"), "user1")
        .await
        .unwrap();
    let c = assets
        .create(make_asset_request("Graph Leaf", "Leaf"), "user1")
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: a.id.clone(),
                to_asset_id: b.id.clone(),
                relation_type: RelationType::RelatedTo,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: b.id.clone(),
                to_asset_id: c.id.clone(),
                relation_type: RelationType::RelatedTo,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    let traversal = relations
        .traverse_graph(&a.id, 3)
        .await
        .expect("traverse_graph should succeed");

    assert_eq!(traversal.len(), 3, "should visit 3 nodes: a, b, c");

    // Root should be at depth 0
    let root = traversal.iter().find(|(id, _, _)| id == &a.id);
    assert!(root.is_some(), "root should be in traversal");
    assert_eq!(root.unwrap().1, 0, "root should be at depth 0");

    // Leaf should be at depth 2 (A->B is 1, B->C is 2)
    let leaf = traversal.iter().find(|(id, _, _)| id == &c.id);
    assert!(leaf.is_some(), "leaf should be in traversal");
    assert_eq!(leaf.unwrap().1, 2, "leaf should be at depth 2");
}

pub async fn relation_get_descendants<A: AssetRepository, R: RelationRepository>(
    assets: &A,
    relations: &R,
) {
    // Build: Root -> Child1, Root -> Child2, Child1 -> Grandchild
    let root = assets
        .create(make_asset_request("Desc Root", "Root"), "user1")
        .await
        .unwrap();
    let child1 = assets
        .create(make_asset_request("Desc Child 1", "Child"), "user1")
        .await
        .unwrap();
    let child2 = assets
        .create(make_asset_request("Desc Child 2", "Child"), "user1")
        .await
        .unwrap();
    let grandchild = assets
        .create(make_asset_request("Desc Grandchild", "Grand"), "user1")
        .await
        .unwrap();

    for (from, to) in [
        (root.id.clone(), child1.id.clone()),
        (root.id.clone(), child2.id.clone()),
        (child1.id.clone(), grandchild.id.clone()),
    ] {
        relations
            .create(
                CreateRelationRequest {
                    from_asset_id: from,
                    to_asset_id: to,
                    relation_type: RelationType::Contains,
                    metadata: HashMap::new(),
                },
                "user1",
            )
            .await
            .unwrap();
    }

    let descendants = relations
        .get_descendants(&root.id)
        .await
        .expect("get_descendants should succeed");

    assert_eq!(
        descendants.len(),
        3,
        "should have 3 descendants: child1, child2, grandchild"
    );
}

pub async fn relation_would_create_cycle<A: AssetRepository, R: RelationRepository>(
    assets: &A,
    relations: &R,
) {
    // Build: A -> B -> C
    let a = assets
        .create(make_asset_request("Cycle A", "A"), "user1")
        .await
        .unwrap();
    let b = assets
        .create(make_asset_request("Cycle B", "B"), "user1")
        .await
        .unwrap();
    let c = assets
        .create(make_asset_request("Cycle C", "C"), "user1")
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: a.id.clone(),
                to_asset_id: b.id.clone(),
                relation_type: RelationType::DependsOn,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: b.id.clone(),
                to_asset_id: c.id.clone(),
                relation_type: RelationType::DependsOn,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    // Adding C -> A would create a cycle (A->B->C->A)
    let creates_cycle = relations
        .would_create_cycle(&c.id, &a.id)
        .await
        .expect("would_create_cycle should succeed");

    assert!(
        creates_cycle,
        "C -> A should be detected as creating a cycle"
    );

    // Adding A -> C should NOT create a cycle
    let no_cycle = relations
        .would_create_cycle(&a.id, &c.id)
        .await
        .expect("would_create_cycle should succeed");

    assert!(
        !no_cycle,
        "A -> C should not be detected as creating a cycle"
    );
}

// ============================================================================
// COLLECTION REPOSITORY CONFORMANCE TESTS
// ============================================================================

pub async fn collection_create_and_get<C: CollectionRepository>(collections: &C) {
    let req = CreateCollectionRequest {
        name: "Test Collection".to_string(),
        description: Some("A test collection".to_string()),
        tags: vec![],
    };

    let created = collections
        .create(req, "user1")
        .await
        .expect("create collection should succeed");

    assert!(!created.id.is_empty());
    assert_eq!(created.name, "Test Collection");
    assert_eq!(created.description.as_deref(), Some("A test collection"));
    assert_eq!(created.created_by, "user1");
    assert!(created.asset_ids.is_empty());

    let fetched = collections
        .get_by_id(&created.id)
        .await
        .expect("get_by_id should succeed");

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.name, "Test Collection");
}

pub async fn collection_get_by_id_not_found<C: CollectionRepository>(collections: &C) {
    let result = collections.get_by_id("nonexistent-coll-id").await;
    assert!(result.is_err(), "should error for missing collection");
}

pub async fn collection_update<C: CollectionRepository>(collections: &C) {
    let created = collections
        .create(
            CreateCollectionRequest {
                name: "Original Coll".to_string(),
                description: Some("Original desc".to_string()),
                tags: vec![],
            },
            "user1",
        )
        .await
        .unwrap();

    let updated = collections
        .update(
            &created.id,
            UpdateCollectionRequest {
                name: Some("Updated Coll".to_string()),
                description: Some("Updated desc".to_string()),
            },
            "user2",
        )
        .await
        .expect("update should succeed");

    assert_eq!(updated.name, "Updated Coll");
    assert_eq!(updated.description.as_deref(), Some("Updated desc"));
    assert_eq!(updated.updated_by, "user2");
}

pub async fn collection_delete<C: CollectionRepository>(collections: &C) {
    let created = collections
        .create(
            CreateCollectionRequest {
                name: "To Delete Coll".to_string(),
                description: None,
                tags: vec![],
            },
            "user1",
        )
        .await
        .unwrap();

    collections
        .delete(&created.id)
        .await
        .expect("delete should succeed");

    let result = collections.get_by_id(&created.id).await;
    assert!(result.is_err(), "deleted collection should not be found");
}

pub async fn collection_list_with_pagination<C: CollectionRepository>(collections: &C) {
    for i in 0..5 {
        collections
            .create(
                CreateCollectionRequest {
                    name: format!("Coll {}", i),
                    description: None,
                    tags: vec![],
                },
                "user1",
            )
            .await
            .unwrap();
    }

    let (page1, next_cursor) = collections
        .list(2, None)
        .await
        .expect("list should succeed");

    assert_eq!(page1.len(), 2, "first page should have 2 collections");
    assert!(next_cursor.is_some(), "should have a next cursor");

    let (page2, _) = collections
        .list(2, next_cursor.as_deref())
        .await
        .expect("list page 2 should succeed");

    assert_eq!(page2.len(), 2, "second page should have 2 collections");
}

pub async fn collection_add_and_remove_asset<A: AssetRepository, C: CollectionRepository>(
    assets: &A,
    collections: &C,
) {
    let asset = assets
        .create(make_asset_request("Coll Asset", "Content"), "user1")
        .await
        .unwrap();

    let coll = collections
        .create(
            CreateCollectionRequest {
                name: "Asset Coll".to_string(),
                description: None,
                tags: vec![],
            },
            "user1",
        )
        .await
        .unwrap();

    // Add asset
    collections
        .add_asset(&coll.id, &asset.id)
        .await
        .expect("add_asset should succeed");

    let fetched = collections.get_by_id(&coll.id).await.unwrap();
    assert!(
        fetched.asset_ids.contains(&asset.id),
        "collection should contain the asset"
    );

    // Remove asset
    collections
        .remove_asset(&coll.id, &asset.id)
        .await
        .expect("remove_asset should succeed");

    let fetched = collections.get_by_id(&coll.id).await.unwrap();
    assert!(
        !fetched.asset_ids.contains(&asset.id),
        "collection should not contain the asset after removal"
    );
}

pub async fn collection_remove_asset_from_all<A: AssetRepository, C: CollectionRepository>(
    assets: &A,
    collections: &C,
) {
    let asset = assets
        .create(make_asset_request("Shared Asset", "Content"), "user1")
        .await
        .unwrap();

    let coll1 = collections
        .create(
            CreateCollectionRequest {
                name: "Coll 1".to_string(),
                description: None,
                tags: vec![],
            },
            "user1",
        )
        .await
        .unwrap();

    let coll2 = collections
        .create(
            CreateCollectionRequest {
                name: "Coll 2".to_string(),
                description: None,
                tags: vec![],
            },
            "user1",
        )
        .await
        .unwrap();

    collections.add_asset(&coll1.id, &asset.id).await.unwrap();
    collections.add_asset(&coll2.id, &asset.id).await.unwrap();

    // Verify it's in both
    assert!(collections.get_by_id(&coll1.id).await.unwrap().asset_ids.contains(&asset.id));
    assert!(collections.get_by_id(&coll2.id).await.unwrap().asset_ids.contains(&asset.id));

    // Remove from all
    collections
        .remove_asset_from_all(&asset.id)
        .await
        .expect("remove_asset_from_all should succeed");

    // Verify it's gone from both
    assert!(!collections.get_by_id(&coll1.id).await.unwrap().asset_ids.contains(&asset.id));
    assert!(!collections.get_by_id(&coll2.id).await.unwrap().asset_ids.contains(&asset.id));
}

// ============================================================================
// AUDIT REPOSITORY CONFORMANCE TESTS
// ============================================================================

pub async fn audit_create_and_list<U: AuditRepository>(audit: &U) {
    audit
        .create(
            "asset",
            "asset-123",
            AuditAction::Create,
            serde_json::json!({"title": "Test"}),
            "user1",
        )
        .await
        .expect("audit create should succeed");

    audit
        .create(
            "asset",
            "asset-123",
            AuditAction::Update,
            serde_json::json!({"field": "title"}),
            "user2",
        )
        .await
        .expect("audit create should succeed");

    let (entries, next_cursor) = audit
        .list(None, None, None, 10, None)
        .await
        .expect("audit list should succeed");

    assert!(entries.len() >= 2, "should have at least 2 audit entries");
    assert!(next_cursor.is_none(), "should not need pagination");

    // Entries should be sorted newest-first
    assert!(
        entries[0].timestamp >= entries[1].timestamp,
        "entries should be sorted by timestamp descending"
    );
}

pub async fn audit_list_with_filters<U: AuditRepository>(audit: &U) {
    audit
        .create(
            "asset",
            "filter-test-1",
            AuditAction::Create,
            serde_json::json!({}),
            "user_a",
        )
        .await
        .unwrap();

    audit
        .create(
            "collection",
            "filter-test-2",
            AuditAction::Create,
            serde_json::json!({}),
            "user_b",
        )
        .await
        .unwrap();

    // Filter by entity_type
    let (asset_entries, _) = audit
        .list(Some("asset"), None, None, 10, None)
        .await
        .expect("filter by entity_type should succeed");

    assert!(
        asset_entries.iter().all(|e| e.entity_type == "asset"),
        "all entries should be entity_type=asset"
    );

    // Filter by user_id
    let (user_entries, _) = audit
        .list(None, None, Some("user_a"), 10, None)
        .await
        .expect("filter by user_id should succeed");

    assert!(
        user_entries.iter().all(|e| e.user_id == "user_a"),
        "all entries should be from user_a"
    );
}

pub async fn audit_get_entity_history<U: AuditRepository>(audit: &U) {
    audit
        .create(
            "asset",
            "history-test",
            AuditAction::Create,
            serde_json::json!({"v": 1}),
            "user1",
        )
        .await
        .unwrap();

    audit
        .create(
            "asset",
            "history-test",
            AuditAction::Update,
            serde_json::json!({"v": 2}),
            "user1",
        )
        .await
        .unwrap();

    audit
        .create(
            "asset",
            "history-test",
            AuditAction::Delete,
            serde_json::json!({}),
            "user1",
        )
        .await
        .unwrap();

    let history = audit
        .get_entity_history("asset", "history-test", 10)
        .await
        .expect("get_entity_history should succeed");

    assert_eq!(
        history.len(),
        3,
        "should have 3 history entries for the entity"
    );

    // Should be sorted newest-first
    assert_eq!(history[0].action, AuditAction::Delete);
    assert_eq!(history[1].action, AuditAction::Update);
    assert_eq!(history[2].action, AuditAction::Create);
}

// ============================================================================
// CROSS-REPOSITORY INTEGRATION: Asset delete cascade
// ============================================================================

pub async fn asset_delete_cascades<
    A: AssetRepository,
    R: RelationRepository,
    C: CollectionRepository,
    U: AuditRepository,
>(
    assets: &A,
    relations: &R,
    collections: &C,
    audit: &U,
) {
    // Create asset, add to collection, create relation
    let a1 = assets
        .create(make_asset_request("Cascade Root", "Content"), "user1")
        .await
        .unwrap();
    let a2 = assets
        .create(make_asset_request("Cascade Target", "Content"), "user1")
        .await
        .unwrap();

    let coll = collections
        .create(
            CreateCollectionRequest {
                name: "Cascade Coll".to_string(),
                description: None,
                tags: vec![],
            },
            "user1",
        )
        .await
        .unwrap();

    collections.add_asset(&coll.id, &a1.id).await.unwrap();

    relations
        .create(
            CreateRelationRequest {
                from_asset_id: a1.id.clone(),
                to_asset_id: a2.id.clone(),
                relation_type: RelationType::RelatedTo,
                metadata: HashMap::new(),
            },
            "user1",
        )
        .await
        .unwrap();

    // Delete a1
    assets.soft_delete(&a1.id).await.unwrap();
    relations.delete_by_asset(&a1.id).await.unwrap();
    collections.remove_asset_from_all(&a1.id).await.unwrap();
    audit
        .create("asset", &a1.id, AuditAction::Delete, serde_json::json!({}), "user1")
        .await
        .unwrap();

    // Verify cascades
    assert!(
        relations.get_asset_relations(&a1.id).await.unwrap().is_empty(),
        "relations should be cleaned up"
    );

    let fetched_coll = collections.get_by_id(&coll.id).await.unwrap();
    assert!(
        !fetched_coll.asset_ids.contains(&a1.id),
        "asset should be removed from collection"
    );

    // Verify audit entry
    let history = audit.get_entity_history("asset", &a1.id, 10).await.unwrap();
    assert!(
        history.iter().any(|e| e.action == AuditAction::Delete),
        "audit should contain delete action"
    );
}
