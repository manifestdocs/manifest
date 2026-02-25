use manifest::db::Database;
use manifest::models::*;

pub async fn setup() -> Database {
    let db = Database::open_memory()
        .await
        .expect("Failed to create in-memory database");
    db.migrate().await.expect("Failed to run migrations");
    db
}

pub async fn create_test_project(db: &Database) -> Project {
    db.create_project(CreateProjectInput {
        slug: None,
        name: "Test Project".to_string(),
        description: None,
        instructions: None,
        key_prefix: None,
    })
    .await
    .expect("Failed to create project")
}
