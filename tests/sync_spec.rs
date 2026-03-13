//! Selective sync integration tests.
//!
//! Tests run in a separate binary to avoid SQLite symbol conflicts
//! between libsql-sys and sqlx-sqlite.

use manifest_core::sync::*;
use manifest_core::turso::TursoConnection;
use tempfile::TempDir;

/// Helper: create a TursoConnection with provisioned schema.
async fn setup_remote(dir: &TempDir, name: &str) -> TursoConnection {
    let db_path = dir.path().join(format!("{}.db", name));
    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    conn.provision_schema().await.unwrap();
    conn
}

/// Helper: set up a coordinator with a remote that has a project and optional features.
async fn setup_coord_with_data(
    dir: &TempDir,
    remote_name: &str,
    remote_id: &str,
    project: &RemoteProject,
    features: &[RemoteFeature],
) -> MergeCoordinator {
    let conn = setup_remote(dir, remote_name).await;
    let coord = MergeCoordinator::new();
    coord.add_connection(remote_id, conn).await;
    coord.push_project(remote_id, project).await.unwrap();
    if !features.is_empty() {
        coord.push_features(remote_id, features).await.unwrap();
    }
    coord
}

fn make_feature(id: &str, project_id: &str, title: &str, state: &str) -> RemoteFeature {
    RemoteFeature {
        id: id.to_string(),
        project_id: project_id.to_string(),
        parent_id: None,
        title: title.to_string(),
        details: None,
        desired_details: None,
        details_summary: None,
        state: state.to_string(),
        priority: 0,
        feature_number: None,
        target_version_id: None,
        claimed_by: None,
        claimed_at: None,
        claim_metadata: None,
        verification_result: None,
        verified_at: None,
        state_updated_at: None,
        details_updated_at: None,
        parent_id_updated_at: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

fn make_project(id: &str, slug: &str, name: &str) -> RemoteProject {
    RemoteProject {
        id: id.to_string(),
        slug: slug.to_string(),
        name: name.to_string(),
        description: None,
        instructions: None,
        current_version_id: None,
        root_feature_id: None,
        default_feature_destination: "backlog".to_string(),
        test_adapter: None,
        context_budget: None,
        key_prefix: "TST".to_string(),
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

// ── Merge coordinator tests ──────────────────────────────────────────

#[tokio::test]
async fn coordinator_manages_connections() {
    let coord = MergeCoordinator::new();
    assert_eq!(coord.connection_count().await, 0);

    let tmp = TempDir::new().unwrap();
    let conn = setup_remote(&tmp, "remote1").await;
    coord.add_connection("r1", conn).await;
    assert_eq!(coord.connection_count().await, 1);
    assert!(coord.has_connection("r1").await);
    assert!(!coord.has_connection("r2").await);

    coord.remove_connection("r1").await;
    assert_eq!(coord.connection_count().await, 0);
}

#[tokio::test]
async fn push_features_to_remote() {
    let tmp = TempDir::new().unwrap();
    let project = make_project("p1", "test", "Test Project");
    let features = vec![
        make_feature("f1", "p1", "Login", "proposed"),
        make_feature("f2", "p1", "Signup", "in_progress"),
    ];
    let coord = setup_coord_with_data(&tmp, "push_test", "r1", &project, &features).await;

    // Verify they're in the remote
    let pulled = coord.pull_features("r1", "p1").await.unwrap();
    assert_eq!(pulled.len(), 2);

    let titles: Vec<&str> = pulled.iter().map(|f| f.title.as_str()).collect();
    assert!(titles.contains(&"Login"));
    assert!(titles.contains(&"Signup"));
}

#[tokio::test]
async fn push_features_rejects_unknown_remote() {
    let coord = MergeCoordinator::new();
    let features = vec![make_feature("f1", "p1", "Login", "proposed")];
    let result = coord.push_features("nonexistent", &features).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn pull_features_from_remote() {
    let tmp = TempDir::new().unwrap();
    let project = make_project("p1", "test", "Test Project");
    let f1 = make_feature("f1", "p1", "Auth", "implemented");
    let coord = setup_coord_with_data(&tmp, "pull_test", "r1", &project, &[f1]).await;

    let pulled = coord.pull_features("r1", "p1").await.unwrap();
    assert_eq!(pulled.len(), 1);
    assert_eq!(pulled[0].title, "Auth");
    assert_eq!(pulled[0].state, "implemented");
}

#[tokio::test]
async fn pull_features_filters_by_project() {
    let tmp = TempDir::new().unwrap();
    let conn = setup_remote(&tmp, "filter_test").await;
    let coord = MergeCoordinator::new();
    coord.add_connection("r1", conn).await;

    coord
        .push_project("r1", &make_project("p1", "proj1", "Project 1"))
        .await
        .unwrap();
    coord
        .push_project("r1", &make_project("p2", "proj2", "Project 2"))
        .await
        .unwrap();
    coord
        .push_features("r1", &[make_feature("f1", "p1", "Feature A", "proposed")])
        .await
        .unwrap();
    coord
        .push_features("r1", &[make_feature("f2", "p2", "Feature B", "proposed")])
        .await
        .unwrap();

    let pulled_p1 = coord.pull_features("r1", "p1").await.unwrap();
    assert_eq!(pulled_p1.len(), 1);
    assert_eq!(pulled_p1[0].id, "f1");

    let pulled_p2 = coord.pull_features("r1", "p2").await.unwrap();
    assert_eq!(pulled_p2.len(), 1);
    assert_eq!(pulled_p2[0].id, "f2");
}

// ── Field-level merge tests ──────────────────────────────────────────

#[tokio::test]
async fn merge_features_field_level_conflict_resolution() {
    // Developer A changes state to implemented at 14:30:00
    let mut local = make_feature("f1", "p1", "Login", "implemented");
    local.state_updated_at = Some("2024-01-01T14:30:00Z".to_string());
    local.details = Some("Original details".to_string());
    local.details_updated_at = Some("2024-01-01T14:00:00Z".to_string());

    // Developer B changes details at 14:30:05 but state is still proposed
    let mut remote = make_feature("f1", "p1", "Login", "proposed");
    remote.state_updated_at = Some("2024-01-01T14:00:00Z".to_string());
    remote.details = Some("Updated details from B".to_string());
    remote.details_updated_at = Some("2024-01-01T14:30:05Z".to_string());

    let (merged, conflicts) = MergeCoordinator::merge_features(&[local], &[remote]);

    assert_eq!(merged.len(), 1);
    let m = &merged[0];

    // State should come from local (newer state_updated_at)
    assert_eq!(m.state, "implemented");
    // Details should come from remote (newer details_updated_at)
    assert_eq!(m.details, Some("Updated details from B".to_string()));
    // Two fields had different values
    assert!(conflicts >= 2);
}

#[tokio::test]
async fn merge_features_remote_only_features_get_pulled() {
    let local = vec![make_feature("f1", "p1", "Login", "proposed")];
    let remote = vec![
        make_feature("f1", "p1", "Login", "proposed"),
        make_feature("f2", "p1", "Signup", "proposed"),
    ];

    let (merged, _) = MergeCoordinator::merge_features(&local, &remote);
    assert_eq!(merged.len(), 2);

    let ids: Vec<&str> = merged.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"f1"));
    assert!(ids.contains(&"f2"));
}

#[tokio::test]
async fn merge_features_local_only_features_kept() {
    let local = vec![
        make_feature("f1", "p1", "Login", "proposed"),
        make_feature("f3", "p1", "Dashboard", "proposed"),
    ];
    let remote = vec![make_feature("f1", "p1", "Login", "proposed")];

    let (merged, _) = MergeCoordinator::merge_features(&local, &remote);
    assert_eq!(merged.len(), 2);

    let ids: Vec<&str> = merged.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"f1"));
    assert!(ids.contains(&"f3"));
}

#[tokio::test]
async fn merge_features_no_timestamps_remote_wins() {
    // When no field-level timestamps, remote wins (pull bias)
    let local = vec![make_feature("f1", "p1", "Login", "proposed")];
    let mut remote_f = make_feature("f1", "p1", "Login", "in_progress");
    remote_f.updated_at = "2024-01-02T00:00:00Z".to_string();
    let remote = vec![remote_f];

    let (merged, _) = MergeCoordinator::merge_features(&local, &remote);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].state, "in_progress");
}

// ── Offline queue tests ──────────────────────────────────────────────

#[tokio::test]
async fn offline_queue_enqueue_and_dequeue() {
    let queue = OfflineQueue::new();
    assert_eq!(queue.pending_count().await, 0);

    let id = queue
        .enqueue("p1", "r1", "features", "f1", WriteOperation::Upsert, "{}")
        .await;
    assert_eq!(queue.pending_count().await, 1);

    let pending = queue.pending_for_remote("r1").await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].row_id, "f1");

    queue.dequeue(id).await;
    assert_eq!(queue.pending_count().await, 0);
}

#[tokio::test]
async fn offline_queue_filters_by_remote() {
    let queue = OfflineQueue::new();

    queue
        .enqueue("p1", "r1", "features", "f1", WriteOperation::Upsert, "{}")
        .await;
    queue
        .enqueue("p1", "r2", "features", "f2", WriteOperation::Upsert, "{}")
        .await;
    queue
        .enqueue("p2", "r1", "features", "f3", WriteOperation::Upsert, "{}")
        .await;

    assert_eq!(queue.pending_count().await, 3);
    assert_eq!(queue.pending_for_remote("r1").await.len(), 2);
    assert_eq!(queue.pending_for_remote("r2").await.len(), 1);
    assert_eq!(queue.pending_for_remote("r3").await.len(), 0);
}

#[tokio::test]
async fn offline_queue_flush_pushes_to_remote() {
    let tmp = TempDir::new().unwrap();
    let project = make_project("p1", "test", "Test");
    let coord = setup_coord_with_data(&tmp, "flush_test", "r1", &project, &[]).await;

    let queue = OfflineQueue::new();

    // Queue a feature write
    let feature = make_feature("f1", "p1", "Queued Feature", "proposed");
    let payload = serde_json::to_string(&feature).unwrap();
    queue
        .enqueue(
            "p1",
            "r1",
            "features",
            "f1",
            WriteOperation::Upsert,
            &payload,
        )
        .await;

    // Flush
    let flushed = queue.flush_remote("r1", &coord).await.unwrap();
    assert_eq!(flushed, 1);
    assert_eq!(queue.pending_count().await, 0);

    // Verify feature is on remote
    let pulled = coord.pull_features("r1", "p1").await.unwrap();
    assert_eq!(pulled.len(), 1);
    assert_eq!(pulled[0].title, "Queued Feature");
}

// ── Push versions test ──────────────────────────────────────────────

#[tokio::test]
async fn push_and_pull_versions() {
    let tmp = TempDir::new().unwrap();
    let project = make_project("p1", "test", "Test");
    let coord = setup_coord_with_data(&tmp, "versions_test", "r1", &project, &[]).await;

    let versions = vec![RemoteVersion {
        id: "v1".to_string(),
        project_id: "p1".to_string(),
        name: "0.1.0".to_string(),
        description: Some("Initial release".to_string()),
        released_at: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    }];

    let pushed = coord.push_versions("r1", &versions).await.unwrap();
    assert_eq!(pushed, 1);
}

// ── Push project test ──────────────────────────────────────────────

#[tokio::test]
async fn push_project_to_remote() {
    let tmp = TempDir::new().unwrap();
    let project = make_project("p1", "my-app", "My App");
    let f = make_feature("f1", "p1", "Test", "proposed");
    let coord = setup_coord_with_data(&tmp, "project_test", "r1", &project, &[f]).await;

    let pulled = coord.pull_features("r1", "p1").await.unwrap();
    assert_eq!(pulled.len(), 1);
}

// ── Write operation serialization ──────────────────────────────────

#[tokio::test]
async fn write_operation_roundtrip() {
    assert_eq!(
        WriteOperation::from_str(WriteOperation::Upsert.as_str()),
        Some(WriteOperation::Upsert)
    );
    assert_eq!(
        WriteOperation::from_str(WriteOperation::Delete.as_str()),
        Some(WriteOperation::Delete)
    );
    assert_eq!(WriteOperation::from_str("unknown"), None);
}
