use axum::http::StatusCode;
use axum_test::TestServer;
use manifest::api::create_router;
use manifest::db::Database;
use manifest::models::*;

async fn setup() -> TestServer {
    let db = Database::open_memory()
        .await
        .expect("Failed to create database");
    db.migrate().await.expect("Failed to migrate");
    let app = create_router(db);
    TestServer::new(app).expect("Failed to create test server")
}

async fn create_test_project(server: &TestServer) -> Project {
    server
        .post("/api/v1/projects")
        .json(&CreateProjectInput {
            slug: None,
            name: "Test Project".to_string(),
            description: None,
            instructions: None,
        })
        .await
        .json::<Project>()
}

mod feature_roots {
    use super::*;

    #[tokio::test]
    async fn returns_empty_list_when_no_features_exist() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let response = server
            .get(&format!("/api/v1/projects/{}/features/roots", project.id))
            .await;

        response.assert_status_ok();
        let features: Vec<Feature> = response.json();
        assert!(features.is_empty());
    }

    #[tokio::test]
    async fn returns_only_root_features() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        // Create root feature
        let root = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Root".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        // Create child feature
        server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(root.id),
                title: "Child".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await;

        let response = server
            .get(&format!("/api/v1/projects/{}/features/roots", project.id))
            .await;

        response.assert_status_ok();
        let features: Vec<Feature> = response.json();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].title, "Root");
        // With the root feature model, "root" features are children of the project's
        // root_feature, so they have parent_id = root_feature_id (not None)
        assert_eq!(features[0].parent_id, project.root_feature_id);
    }
}

mod feature_children {
    use super::*;

    #[tokio::test]
    async fn returns_empty_list_when_feature_has_no_children() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let feature = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Leaf".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        let response = server
            .get(&format!("/api/v1/features/{}/children", feature.id))
            .await;

        response.assert_status_ok();
        let children: Vec<Feature> = response.json();
        assert!(children.is_empty());
    }

    #[tokio::test]
    async fn returns_direct_children_ordered_by_title() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let parent = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Parent".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Zebra".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await;

        server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Alpha".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await;

        let response = server
            .get(&format!("/api/v1/features/{}/children", parent.id))
            .await;

        response.assert_status_ok();
        let children: Vec<Feature> = response.json();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].title, "Alpha");
        assert_eq!(children[1].title, "Zebra");
    }

    #[tokio::test]
    async fn does_not_return_grandchildren() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let root = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Root".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        let child = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(root.id),
                title: "Child".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(child.id),
                title: "Grandchild".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await;

        let response = server
            .get(&format!("/api/v1/features/{}/children", root.id))
            .await;

        response.assert_status_ok();
        let children: Vec<Feature> = response.json();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "Child");
    }
}

mod feature_hierarchy_create {
    use super::*;

    #[tokio::test]
    async fn creates_child_feature_with_parent_id() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let parent = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Authentication".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        let response = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Login".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await;

        response.assert_status(StatusCode::CREATED);
        let child: Feature = response.json();
        assert_eq!(child.parent_id, Some(parent.id));
        assert_eq!(child.title, "Login");
    }

    #[tokio::test]
    async fn creates_deeply_nested_features() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let level0 = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Authentication".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        let level1 = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(level0.id),
                title: "OAuth".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        let level2 = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(level1.id),
                title: "Google".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        assert_eq!(level2.parent_id, Some(level1.id));

        // Verify via GET
        let response = server.get(&format!("/api/v1/features/{}", level2.id)).await;
        let fetched: Feature = response.json();
        assert_eq!(fetched.parent_id, Some(level1.id));
    }
}

mod feature_cascade_delete {
    use super::*;

    #[tokio::test]
    async fn deletes_children_when_parent_is_deleted() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let parent = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "Parent".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        let child = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: Some(parent.id),
                title: "Child".to_string(),
                state: None,

                details: None,
                priority: None,
                target_version_id: None,
            })
            .await
            .json::<Feature>();

        // Delete parent
        server
            .delete(&format!("/api/v1/features/{}", parent.id))
            .await
            .assert_status(StatusCode::NO_CONTENT);

        // Child should be gone
        server
            .get(&format!("/api/v1/features/{}", child.id))
            .await
            .assert_status_not_found();
    }
}

mod feature_history {
    use super::*;

    #[tokio::test]
    async fn returns_empty_list_when_no_history() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let feature = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "New Feature".to_string(),

                details: None,
                priority: None,
                target_version_id: None,
                state: None,
            })
            .await
            .json::<Feature>();

        let response = server
            .get(&format!("/api/v1/features/{}/history", feature.id))
            .await;

        response.assert_status_ok();
        let history: Vec<FeatureHistory> = response.json();
        assert!(history.is_empty());
    }
}

// ============================================================
// Security - API Key Authentication
// ============================================================

mod security_auth {
    use super::*;
    use manifest::api::{create_router_with_config, SecurityConfig};

    async fn setup_with_auth(api_key: &str) -> TestServer {
        let db = Database::open_memory()
            .await
            .expect("Failed to create database");
        db.migrate().await.expect("Failed to migrate");
        let config = SecurityConfig::with_api_key(api_key);
        let app = create_router_with_config(db, config);
        TestServer::new(app).expect("Failed to create test server")
    }

    #[tokio::test]
    async fn health_endpoint_is_accessible_without_auth() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server.get("/api/v1/health").await;

        response.assert_status_ok();
    }

    #[tokio::test]
    async fn protected_endpoint_requires_auth() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server.get("/api/v1/projects").await;

        response.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_endpoint_accepts_valid_bearer_token() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server
            .get("/api/v1/projects")
            .add_header("Authorization", "Bearer test-secret-key")
            .await;

        response.assert_status_ok();
    }

    #[tokio::test]
    async fn protected_endpoint_rejects_invalid_bearer_token() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server
            .get("/api/v1/projects")
            .add_header("Authorization", "Bearer wrong-key")
            .await;

        response.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_endpoint_rejects_malformed_auth_header() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server
            .get("/api/v1/projects")
            .add_header("Authorization", "Basic dXNlcjpwYXNz")
            .await;

        response.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_endpoint_requires_auth() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server
            .post("/api/v1/projects")
            .json(&CreateProjectInput {
                slug: None,
                name: "Test".to_string(),
                description: None,
                instructions: None,
            })
            .await;

        response.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_endpoint_works_with_valid_auth() {
        let server = setup_with_auth("test-secret-key").await;

        let response = server
            .post("/api/v1/projects")
            .add_header("Authorization", "Bearer test-secret-key")
            .json(&CreateProjectInput {
                slug: None,
                name: "Test".to_string(),
                description: None,
                instructions: None,
            })
            .await;

        response.assert_status(StatusCode::CREATED);
    }
}
