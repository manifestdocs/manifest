//! Turso embedded replica connection tests.
//!
//! These tests validate the Turso embedded replica connection layer.

use std::time::Duration;

use manifest_core::turso::{TursoConfig, TursoConnection};
use tempfile::TempDir;

#[tokio::test]
async fn open_local_creates_database() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    let c = conn.connect().unwrap();

    c.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)", ())
        .await
        .unwrap();
    c.execute("INSERT INTO test (value) VALUES ('hello')", ())
        .await
        .unwrap();

    let mut rows = c.query("SELECT value FROM test", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let value: String = row.get(0).unwrap();
    assert_eq!(value, "hello");
}

#[tokio::test]
async fn provision_schema_creates_tables() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("schema.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    conn.provision_schema().await.unwrap();

    let c = conn.connect().unwrap();

    for table in [
        "projects",
        "features",
        "versions",
        "feature_history",
        "feature_blockers",
    ] {
        let mut rows = c
            .query(
                &format!(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='{}'",
                    table
                ),
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap();
        assert!(
            row.is_some(),
            "table '{}' should exist after provisioning",
            table
        );
    }
}

#[tokio::test]
async fn provision_schema_is_idempotent() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("idem.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    conn.provision_schema().await.unwrap();
    conn.provision_schema().await.unwrap();
}

#[tokio::test]
async fn sync_is_noop_for_local() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("local.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    conn.sync().await.unwrap();
}

#[tokio::test]
async fn ping_returns_none_for_local() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("ping.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    let latency = conn.ping().await.unwrap();
    assert!(latency.is_none(), "local DB ping should return None");
}

#[tokio::test]
async fn info_returns_stats() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("info.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    conn.provision_schema().await.unwrap();

    let info = conn.info().await.unwrap();
    assert!(info.table_count >= 5, "should have at least 5 tables");
    assert_eq!(info.project_count, 0);
    assert_eq!(info.feature_count, 0);
    assert!(info.url.is_none());
}

#[tokio::test]
async fn read_write_through_provisioned_schema() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("rw.db");

    let conn = TursoConnection::open_local(&db_path).await.unwrap();
    conn.provision_schema().await.unwrap();

    let c = conn.connect().unwrap();

    c.execute(
        "INSERT INTO projects (id, slug, name, key_prefix, created_at, updated_at) VALUES ('p1', 'test', 'Test', 'TST', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
        (),
    )
    .await
    .unwrap();

    c.execute(
        "INSERT INTO features (id, project_id, title, created_at, updated_at) VALUES ('f1', 'p1', 'Login', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
        (),
    )
    .await
    .unwrap();

    let mut rows = c
        .query("SELECT title FROM features WHERE id = 'f1'", ())
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let title: String = row.get(0).unwrap();
    assert_eq!(title, "Login");

    let info = conn.info().await.unwrap();
    assert_eq!(info.project_count, 1);
    assert_eq!(info.feature_count, 1);
}

#[tokio::test]
async fn config_from_remote_uses_correct_path() {
    let config = TursoConfig::from_remote("work", "libsql://work.turso.io", "token123");
    assert!(config.replica_path.ends_with("work.db"));
    assert_eq!(config.url, "libsql://work.turso.io");
    assert_eq!(config.auth_token, "token123");
    assert_eq!(config.sync_interval, Duration::from_secs(5));
    assert!(config.read_your_writes);
}
