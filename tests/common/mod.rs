use axum_test::TestServer;
use manifest::api::create_router;
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
        id: None,
        slug: None,
        name: "Test Project".to_string(),
        description: None,
        instructions: None,
        key_prefix: None,
        skip_default_versions: false,
    })
    .await
    .expect("Failed to create project")
}

/// Create an in-memory TestServer for API-level tests.
pub async fn setup_api() -> TestServer {
    let db = Database::open_memory()
        .await
        .expect("Failed to create database");
    db.migrate().await.expect("Failed to migrate");
    let app = create_router(db);
    TestServer::new(app).expect("Failed to create test server")
}

/// Create a test project via the API and return it.
pub async fn create_test_project_api(server: &TestServer) -> Project {
    server
        .post("/api/v1/projects")
        .json(&CreateProjectInput {
            id: None,
            slug: None,
            name: "Test Project".to_string(),
            description: None,
            instructions: None,
            key_prefix: None,
            skip_default_versions: false,
        })
        .await
        .json::<Project>()
}

/// Create a minimal feature via the API.
pub async fn create_feature(server: &TestServer, project: &Project, title: &str) -> Feature {
    server
        .post(&format!("/api/v1/projects/{}/features", project.id))
        .json(&CreateFeatureInput {
            id: None,
            parent_id: None,
            title: title.to_string(),
            state: None,
            details: None,
            priority: None,
            target_version_id: None,
        })
        .await
        .json::<Feature>()
}

/// Create a feature with details (spec) via the API.
pub async fn create_feature_with_details(
    server: &TestServer,
    project: &Project,
    title: &str,
    details: &str,
) -> Feature {
    server
        .post(&format!("/api/v1/projects/{}/features", project.id))
        .json(&CreateFeatureInput {
            id: None,
            parent_id: None,
            title: title.to_string(),
            state: None,
            details: Some(details.to_string()),
            priority: None,
            target_version_id: None,
        })
        .await
        .json::<Feature>()
}

/// Create a child feature under the given parent.
pub async fn create_child_feature(
    server: &TestServer,
    project: &Project,
    parent_id: FeatureId,
    title: &str,
) -> Feature {
    server
        .post(&format!("/api/v1/projects/{}/features", project.id))
        .json(&CreateFeatureInput {
            id: None,
            parent_id: Some(parent_id),
            title: title.to_string(),
            state: None,
            details: None,
            priority: None,
            target_version_id: None,
        })
        .await
        .json::<Feature>()
}
