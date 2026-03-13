//! Integration tests that run the FeatureStore contract harness against SqliteStore.
//!
//! This validates that the SQLite implementation satisfies all FeatureStore contract
//! requirements. Future backends (Turso, GitHub) will have their own test files that
//! run the same harness functions.

use std::sync::Arc;

use manifest_core::db::Database;
use manifest_core::store::harness;
use manifest_core::store::SqliteStore;

async fn setup() -> SqliteStore {
    let db = Database::open_memory().await.expect("open memory db");
    db.migrate().await.expect("migrate");
    SqliteStore::new(Arc::new(db))
}

#[tokio::test]
async fn sqlite_tree_operations() {
    let store = setup().await;
    harness::test_tree_operations(&store).await;
}

#[tokio::test]
async fn sqlite_state_transitions() {
    let store = setup().await;
    harness::test_state_transitions(&store).await;
}

#[tokio::test]
async fn sqlite_concurrent_updates() {
    let store = setup().await;
    harness::test_concurrent_updates(&store).await;
}

#[tokio::test]
async fn sqlite_query_filtering() {
    let store = setup().await;
    harness::test_query_filtering(&store).await;
}

#[tokio::test]
async fn sqlite_blocker_operations() {
    let store = setup().await;
    harness::test_blocker_operations(&store).await;
}

#[tokio::test]
async fn sqlite_version_operations() {
    let store = setup().await;
    harness::test_version_operations(&store).await;
}

#[tokio::test]
async fn sqlite_history_operations() {
    let store = setup().await;
    harness::test_history_operations(&store).await;
}

#[tokio::test]
async fn sqlite_not_found_errors() {
    let store = setup().await;
    harness::test_not_found_errors(&store).await;
}

#[tokio::test]
async fn sqlite_capabilities() {
    let store = setup().await;
    harness::test_capabilities(&store).await;
}

#[tokio::test]
async fn sqlite_full_harness() {
    let store = setup().await;
    harness::run_all(&store).await;
}
