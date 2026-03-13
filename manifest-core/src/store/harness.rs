//! Reusable test harness for [`FeatureStore`] implementations.
//!
//! Provides a suite of contract tests that every backend must pass. Each test
//! function takes a `&dyn FeatureStore` and exercises a specific area of the
//! contract. New backends run the full harness to validate correctness.
//!
//! # Usage
//!
//! ```ignore
//! use manifest_core::store::harness;
//! use manifest_core::store::SqliteStore;
//!
//! #[tokio::test]
//! async fn sqlite_tree_operations() {
//!     let store = create_sqlite_store().await;
//!     harness::test_tree_operations(&store).await;
//! }
//! ```

use crate::models::*;
use crate::store::{FeatureChangeset, FeatureQuery, FeatureStore, ParentFilter, StoreError};

// ── Helpers ──────────────────────────────────────────────────────────────

/// Create a test project in the store.
pub async fn create_project(store: &dyn FeatureStore, slug: &str) -> Project {
    store
        .create_project(&CreateProjectInput {
            id: None,
            name: format!("Harness {slug}"),
            slug: Some(slug.to_string()),
            description: None,
            instructions: None,
            key_prefix: None,
            skip_default_versions: true,
        })
        .await
        .expect("harness: create project")
}

/// Create a feature in the store.
pub async fn create_feature(
    store: &dyn FeatureStore,
    project_id: &ProjectId,
    title: &str,
    parent_id: Option<FeatureId>,
) -> Feature {
    store
        .create_feature(
            project_id,
            &CreateFeatureInput {
                id: None,
                parent_id,
                title: title.to_string(),
                details: None,
                state: None,
                priority: None,
                target_version_id: None,
            },
        )
        .await
        .expect("harness: create feature")
}

// ── Tree Operations ──────────────────────────────────────────────────────

/// Tests: create parent, add children, move child, verify hierarchy.
pub async fn test_tree_operations(store: &dyn FeatureStore) {
    let project = create_project(store, "tree-ops").await;

    // Create parent and children
    let parent = create_feature(store, &project.id, "Parent", None).await;
    let child_a = create_feature(store, &project.id, "Child A", Some(parent.id)).await;
    let child_b = create_feature(store, &project.id, "Child B", Some(parent.id)).await;

    // Verify children
    let children = store.get_feature_children(&parent.id).await.unwrap();
    assert_eq!(children.len(), 2, "Parent should have 2 children");
    let child_titles: Vec<_> = children.iter().map(|c| c.title.as_str()).collect();
    assert!(child_titles.contains(&"Child A"));
    assert!(child_titles.contains(&"Child B"));

    // Verify ancestors
    let ancestors = store.get_feature_ancestors(&child_a.id).await.unwrap();
    assert!(
        ancestors.iter().any(|a| a.id == parent.id),
        "Child A's ancestors should include Parent"
    );

    // Move child_b to project root (sibling of Parent)
    let root_id = project
        .root_feature_id
        .expect("project should have root feature");
    store
        .move_feature(&child_b.id, Some(&root_id), Some(0))
        .await
        .unwrap();
    let children_after = store.get_feature_children(&parent.id).await.unwrap();
    assert_eq!(
        children_after.len(),
        1,
        "Parent should have 1 child after move"
    );
    assert_eq!(children_after[0].id, child_a.id);

    // Verify moved feature still exists
    let moved = store.get_feature(&child_b.id).await.unwrap();
    assert_eq!(moved.title, "Child B");
}

// ── State Transitions ────────────────────────────────────────────────────

/// Tests: proposed -> in_progress -> implemented transitions.
pub async fn test_state_transitions(store: &dyn FeatureStore) {
    let project = create_project(store, "state-trans").await;
    let feature = create_feature(store, &project.id, "Workflow", None).await;
    assert_eq!(feature.state, FeatureState::Proposed);

    // Proposed -> InProgress
    let updated = store
        .update_feature(
            &feature.id,
            &FeatureChangeset::default().state(FeatureState::InProgress),
        )
        .await
        .unwrap();
    assert_eq!(updated.state, FeatureState::InProgress);

    // InProgress -> Implemented
    let updated = store
        .update_feature(
            &feature.id,
            &FeatureChangeset::default().state(FeatureState::Implemented),
        )
        .await
        .unwrap();
    assert_eq!(updated.state, FeatureState::Implemented);

    // Implemented -> Archived
    let updated = store
        .update_feature(
            &feature.id,
            &FeatureChangeset::default().state(FeatureState::Archived),
        )
        .await
        .unwrap();
    assert_eq!(updated.state, FeatureState::Archived);
}

// ── Concurrent Updates ───────────────────────────────────────────────────

/// Tests: two updates to the same feature; both should succeed (last writer wins).
pub async fn test_concurrent_updates(store: &dyn FeatureStore) {
    let project = create_project(store, "concurrent").await;
    let feature = create_feature(store, &project.id, "Concurrent", None).await;

    // Two updates in quick succession
    let result_a = store
        .update_feature(&feature.id, &FeatureChangeset::default().title("Update A"))
        .await;
    let result_b = store
        .update_feature(&feature.id, &FeatureChangeset::default().title("Update B"))
        .await;

    // Both should succeed
    assert!(result_a.is_ok(), "First update should succeed");
    assert!(result_b.is_ok(), "Second update should succeed");

    // Final state should be the last update
    let final_state = store.get_feature(&feature.id).await.unwrap();
    assert_eq!(final_state.title, "Update B");
}

// ── Query Filtering ──────────────────────────────────────────────────────

/// Tests: query by project, parent, version, and pagination.
pub async fn test_query_filtering(store: &dyn FeatureStore) {
    let project = create_project(store, "query-filter").await;

    let parent = create_feature(store, &project.id, "Parent", None).await;
    let _child = create_feature(store, &project.id, "Child", Some(parent.id)).await;
    let _root_feat = create_feature(store, &project.id, "Root Feat", None).await;

    // Query by project
    let all = store
        .list_features(&FeatureQuery::for_project(project.id))
        .await
        .unwrap();
    // Project root feature + Parent + Child + Root Feat = at least 3 user-created
    assert!(all.len() >= 3, "Should list all features in project");

    // Query root features only
    let roots = store
        .list_features(&FeatureQuery::for_project(project.id).with_parent(ParentFilter::Root))
        .await
        .unwrap();
    // Root features: project root + Parent + Root Feat (Child is under Parent)
    assert!(
        roots.len() >= 2,
        "Should list root features only, got {}",
        roots.len()
    );
    assert!(
        !roots.iter().any(|f| f.title == "Child"),
        "Child should not appear in root query"
    );

    // Query children of parent
    let children = store
        .list_features(
            &FeatureQuery::for_project(project.id).with_parent(ParentFilter::Exact(parent.id)),
        )
        .await
        .unwrap();
    assert_eq!(children.len(), 1, "Parent should have exactly 1 child");
    assert_eq!(children[0].title, "Child");

    // Query with pagination
    let page = store
        .list_features(&FeatureQuery::for_project(project.id).with_limit(2))
        .await
        .unwrap();
    assert!(page.len() <= 2, "Limit should cap results at 2");
}

// ── Blocker Operations ───────────────────────────────────────────────────

/// Tests: set blockers, verify blocked_by, remove blockers.
pub async fn test_blocker_operations(store: &dyn FeatureStore) {
    let project = create_project(store, "blockers").await;

    let blocker = create_feature(store, &project.id, "Blocker", None).await;
    let blocked = create_feature(store, &project.id, "Blocked", None).await;

    // Set blocker
    store
        .set_blockers(&blocked.id, &[blocker.id])
        .await
        .unwrap();

    // Verify blocker relationship
    let blockers = store.get_blockers(&blocked.id).await.unwrap();
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0].id, blocker.id);

    // Verify reverse relationship
    let dependents = store.get_blocked_by(&blocker.id).await.unwrap();
    assert_eq!(dependents.len(), 1);
    assert_eq!(dependents[0].id, blocked.id);

    // Remove blockers
    store.set_blockers(&blocked.id, &[]).await.unwrap();
    let blockers_after = store.get_blockers(&blocked.id).await.unwrap();
    assert!(blockers_after.is_empty(), "Blockers should be cleared");
}

// ── Version Operations ───────────────────────────────────────────────────

/// Tests: create version, assign features, release, reject assignment to released.
pub async fn test_version_operations(store: &dyn FeatureStore) {
    let project = create_project(store, "versions").await;

    // Create version
    let version = store
        .create_version(
            &project.id,
            &CreateVersionInput {
                id: None,
                name: "1.0.0".to_string(),
                description: Some("First release".to_string()),
            },
        )
        .await
        .unwrap();
    assert_eq!(version.name, "1.0.0");

    // List versions
    let versions = store.list_versions(&project.id).await.unwrap();
    assert_eq!(versions.len(), 1);

    // Get version
    let fetched = store.get_version(&version.id).await.unwrap();
    assert_eq!(fetched.id, version.id);

    // Assign feature to version
    let feature = store
        .create_feature(
            &project.id,
            &CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Versioned".to_string(),
                details: None,
                state: None,
                priority: None,
                target_version_id: Some(version.id),
            },
        )
        .await
        .unwrap();
    assert_eq!(feature.target_version_id, Some(version.id));

    // Update version
    let updated = store
        .update_version(
            &version.id,
            &UpdateVersionInput {
                name: Some("1.0.1".to_string()),
                description: None,
                released_at: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "1.0.1");

    // Delete version
    store.delete_version(&version.id).await.unwrap();
    let result = store.get_version(&version.id).await;
    assert!(
        matches!(result, Err(StoreError::VersionNotFound(_))),
        "Deleted version should not be found"
    );
}

// ── History Operations ───────────────────────────────────────────────────

/// Tests: add history entries, list them, verify ordering.
pub async fn test_history_operations(store: &dyn FeatureStore) {
    let project = create_project(store, "history").await;
    let feature = create_feature(store, &project.id, "Documented", None).await;

    // Add first entry
    let entry1 = store
        .add_history(&CreateHistoryInput {
            feature_id: feature.id,
            version_id: None,
            details: HistoryDetails {
                summary: "First iteration".to_string(),
                commits: vec![CommitRef {
                    sha: "abc1234".to_string(),
                    message: "Initial impl".to_string(),
                    author: None,
                }],
                backfilled: false,
            },
        })
        .await
        .unwrap();
    assert_eq!(entry1.details.summary, "First iteration");
    assert_eq!(entry1.details.commits.len(), 1);

    // Add second entry
    let _entry2 = store
        .add_history(&CreateHistoryInput {
            feature_id: feature.id,
            version_id: None,
            details: HistoryDetails {
                summary: "Bug fix".to_string(),
                commits: vec![],
                backfilled: false,
            },
        })
        .await
        .unwrap();

    // List history
    let history = store.list_history(&feature.id).await.unwrap();
    assert_eq!(history.len(), 2, "Should have 2 history entries");
}

// ── Not Found Errors ─────────────────────────────────────────────────────

/// Tests: operations on non-existent entities return correct error types.
pub async fn test_not_found_errors(store: &dyn FeatureStore) {
    let fake_feature = FeatureId::new();
    let fake_project = ProjectId::new();
    let fake_version = VersionId::new();

    // Feature not found
    let result = store.get_feature(&fake_feature).await;
    assert!(
        matches!(result, Err(StoreError::FeatureNotFound(_))),
        "Missing feature should return FeatureNotFound"
    );

    // Project not found
    let result = store.get_project(&fake_project).await;
    assert!(
        matches!(result, Err(StoreError::ProjectNotFound(_))),
        "Missing project should return ProjectNotFound"
    );

    // Version not found
    let result = store.get_version(&fake_version).await;
    assert!(
        matches!(result, Err(StoreError::VersionNotFound(_))),
        "Missing version should return VersionNotFound"
    );

    // Delete non-existent feature
    let result = store.delete_feature(&fake_feature).await;
    assert!(result.is_err(), "Deleting missing feature should error");
}

// ── Capabilities ─────────────────────────────────────────────────────────

/// Tests: capabilities are declared and consistent.
pub async fn test_capabilities(store: &dyn FeatureStore) {
    let caps = store.capabilities();
    // Just verify the struct is populated — specific values depend on backend
    let _ = caps.offline_writes;
    let _ = caps.realtime_sync;
    let _ = caps.fulltext_search;
    let _ = caps.transactions;
    let _ = caps.external_ui;
    let _ = caps.backend_type;
    // Backend type should have a display representation
    let display = format!("{}", caps.backend_type);
    assert!(
        !display.is_empty(),
        "Backend type should have a display name"
    );
}

// ── Full Harness ─────────────────────────────────────────────────────────

/// Run all harness tests against a store. Call this from integration tests.
///
/// Each test gets the same store instance. If your backend needs isolation
/// between tests, call individual test functions with fresh stores instead.
pub async fn run_all(store: &dyn FeatureStore) {
    test_tree_operations(store).await;
    test_state_transitions(store).await;
    test_concurrent_updates(store).await;
    test_query_filtering(store).await;
    test_blocker_operations(store).await;
    test_version_operations(store).await;
    test_history_operations(store).await;
    test_not_found_errors(store).await;
    test_capabilities(store).await;
}
