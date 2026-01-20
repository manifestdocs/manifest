use anyhow::{Context, Result};
use rusqlite::Connection;

struct Migration {
    version: &'static str,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: "001",
        name: "initial",
        sql: include_str!("migrations/001_initial.sql"),
    },
    Migration {
        version: "002",
        name: "add_instructions",
        sql: include_str!("migrations/002_add_instructions.sql"),
    },
    Migration {
        version: "003",
        name: "remove_notes",
        sql: include_str!("migrations/003_remove_notes.sql"),
    },
    Migration {
        version: "004",
        name: "history_details",
        sql: include_str!("migrations/004_history_details.sql"),
    },
    Migration {
        version: "005",
        name: "feature_priority",
        sql: include_str!("migrations/005_feature_priority.sql"),
    },
    Migration {
        version: "006",
        name: "remove_story",
        sql: include_str!("migrations/006_remove_story.sql"),
    },
    Migration {
        version: "007",
        name: "desired_details",
        sql: include_str!("migrations/007_desired_details.sql"),
    },
    Migration {
        version: "008",
        name: "remove_history_legacy_columns",
        sql: include_str!("migrations/008_remove_history_legacy_columns.sql"),
    },
    Migration {
        version: "009",
        name: "versions",
        sql: include_str!("migrations/009_versions.sql"),
    },
    Migration {
        version: "010",
        name: "history_versions",
        sql: include_str!("migrations/010_history_versions.sql"),
    },
    Migration {
        version: "011",
        name: "rename_specified_to_in_progress",
        sql: include_str!("migrations/011_rename_specified_to_in_progress.sql"),
    },
    Migration {
        version: "012",
        name: "root_feature",
        sql: include_str!("migrations/012_root_feature.sql"),
    },
    Migration {
        version: "013",
        name: "auth",
        sql: include_str!("migrations/013_auth.sql"),
    },
    Migration {
        version: "014",
        name: "project_memberships",
        sql: include_str!("migrations/014_project_memberships.sql"),
    },
    Migration {
        version: "015",
        name: "audit_log",
        sql: include_str!("migrations/015_audit_log.sql"),
    },
];

pub fn run_migrations(conn: &Connection) -> Result<()> {
    // Create migrations tracking table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
    )
    .context("Failed to create schema_migrations table")?;

    // Check for existing database without version tracking (upgrade path)
    let needs_baseline = check_needs_baseline(conn)?;
    if needs_baseline {
        mark_migration_applied(conn, "001", "initial")?;
        tracing::info!("Detected existing database, marked migration 001 as applied");
    }

    // Get applied migrations
    let applied = get_applied_migrations(conn)?;

    // Run pending migrations
    for migration in MIGRATIONS {
        if !applied.contains(&migration.version.to_string()) {
            apply_migration(conn, migration)?;
        }
    }

    Ok(())
}

fn check_needs_baseline(conn: &Connection) -> Result<bool> {
    // If schema_migrations is empty but tables exist, this is an existing database
    let migration_count: i32 =
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;

    if migration_count > 0 {
        return Ok(false);
    }

    // Check if core tables exist (features table is a good indicator)
    let tables_exist: i32 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='features'",
        [],
        |row| row.get(0),
    )?;

    Ok(tables_exist > 0)
}

fn get_applied_migrations(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
    let versions = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(versions)
}

fn mark_migration_applied(conn: &Connection, version: &str, name: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)",
        (version, name, &now),
    )?;
    Ok(())
}

fn apply_migration(conn: &Connection, migration: &Migration) -> Result<()> {
    tracing::info!(
        "Applying migration {}: {}",
        migration.version,
        migration.name
    );

    // Run migration in a transaction
    conn.execute_batch(&format!("BEGIN TRANSACTION; {} COMMIT;", migration.sql))
        .with_context(|| {
            format!(
                "Failed to apply migration {}: {}",
                migration.version, migration.name
            )
        })?;

    mark_migration_applied(conn, migration.version, migration.name)?;

    tracing::info!("Migration {} applied successfully", migration.version);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_run_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify tables exist
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='features'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify all migrations were recorded
        let versions = get_applied_migrations(&conn).unwrap();
        assert_eq!(
            versions,
            vec![
                "001", "002", "003", "004", "005", "006", "007", "008", "009", "010", "011", "012",
                "013", "014", "015"
            ]
        );
    }

    #[test]
    fn test_migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap(); // Should not fail

        let versions = get_applied_migrations(&conn).unwrap();
        assert_eq!(
            versions,
            vec![
                "001", "002", "003", "004", "005", "006", "007", "008", "009", "010", "011", "012",
                "013", "014", "015"
            ]
        );
    }

    #[test]
    fn test_existing_db_gets_baseline() {
        let conn = Connection::open_in_memory().unwrap();

        // Simulate existing database created before migration tracking
        // This represents the 001_initial schema (without later migrations like priority)
        // Baseline marks 001 as applied, then migrations 002-006 will run
        conn.execute_batch("
            CREATE TABLE features (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                title TEXT NOT NULL,
                story TEXT,
                details TEXT,
                state TEXT NOT NULL DEFAULT 'proposed',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT, description TEXT, created_at TEXT, updated_at TEXT);
            CREATE TABLE project_directories (id TEXT PRIMARY KEY, project_id TEXT, path TEXT, git_remote TEXT, is_primary INTEGER, created_at TEXT);
            CREATE TABLE feature_history (id TEXT PRIMARY KEY, feature_id TEXT, session_id TEXT, summary TEXT, files_changed JSON, author TEXT, created_at TEXT);
            CREATE INDEX idx_features_project ON features(project_id);
            CREATE INDEX idx_features_parent ON features(parent_id);
        ").unwrap();

        // Run migrations - should detect existing DB and baseline, then apply remaining
        run_migrations(&conn).unwrap();

        let versions = get_applied_migrations(&conn).unwrap();
        assert_eq!(
            versions,
            vec![
                "001", "002", "003", "004", "005", "006", "007", "008", "009", "010", "011", "012",
                "013", "014", "015"
            ]
        );
    }
}
