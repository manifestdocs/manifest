mod common;

use axum::http::StatusCode;
use axum_test::TestServer;
use common::{create_child_feature, create_feature, create_feature_with_details, setup_api};
use manifest::api::{create_router_with_config, SecurityConfig};
use manifest::db::Database;
use manifest::models::*;

async fn setup() -> TestServer {
    setup_api().await
}

async fn create_test_project(server: &TestServer) -> Project {
    common::create_test_project_api(server).await
}

// ============================================================
// Project Settings
// ============================================================

mod project_settings {
    use super::*;

    mod test_adapter {
        use super::*;

        #[tokio::test]
        async fn defaults_to_null_in_project_response() {
            let server = setup().await;
            let project = create_test_project(&server).await;

            assert!(project.test_adapter.is_none());
        }

        #[tokio::test]
        async fn can_be_set_via_update() {
            let server = setup().await;
            let project = create_test_project(&server).await;

            let response = server
                .put(&format!("/api/v1/projects/{}", project.id))
                .json(&serde_json::json!({
                    "test_adapter": "cargo-test"
                }))
                .await;

            response.assert_status_ok();
            let updated: Project = response.json();
            assert_eq!(updated.test_adapter.as_deref(), Some("cargo-test"));
        }

        #[tokio::test]
        async fn persists_through_get() {
            let server = setup().await;
            let project = create_test_project(&server).await;

            // Set via PUT
            server
                .put(&format!("/api/v1/projects/{}", project.id))
                .json(&serde_json::json!({
                    "test_adapter": "pytest"
                }))
                .await;

            // Verify via GET
            let response = server
                .get(&format!("/api/v1/projects/{}", project.id))
                .await;

            response.assert_status_ok();
            let fetched: serde_json::Value = response.json();
            // GET /projects/{id} returns ProjectWithDirectories (project fields are flattened)
            assert_eq!(fetched["test_adapter"].as_str(), Some("pytest"));
        }
    }
}

// ============================================================
// Features
// ============================================================

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

        let root = create_feature(&server, &project, "Root").await;
        create_child_feature(&server, &project, root.id, "Child").await;

        let response = server
            .get(&format!("/api/v1/projects/{}/features/roots", project.id))
            .await;

        response.assert_status_ok();
        let features: Vec<Feature> = response.json();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].title, "Root");
        assert_eq!(features[0].parent_id, project.root_feature_id);
    }
}

mod feature_children {
    use super::*;

    #[tokio::test]
    async fn returns_empty_list_when_feature_has_no_children() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let feature = create_feature(&server, &project, "Leaf").await;

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

        let parent = create_feature(&server, &project, "Parent").await;
        create_child_feature(&server, &project, parent.id, "Zebra").await;
        create_child_feature(&server, &project, parent.id, "Alpha").await;

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

        let root = create_feature(&server, &project, "Root").await;
        let child = create_child_feature(&server, &project, root.id, "Child").await;
        create_child_feature(&server, &project, child.id, "Grandchild").await;

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

        let parent = create_feature(&server, &project, "Authentication").await;

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

        let level0 = create_feature(&server, &project, "Authentication").await;
        let level1 = create_child_feature(&server, &project, level0.id, "OAuth").await;
        let level2 = create_child_feature(&server, &project, level1.id, "Google").await;

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

        let parent = create_feature(&server, &project, "Parent").await;
        let child = create_child_feature(&server, &project, parent.id, "Child").await;

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

        let feature = create_feature(&server, &project, "New Feature").await;

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
                id: None,
                slug: None,
                name: "Test".to_string(),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: false,
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
                id: None,
                slug: None,
                name: "Test".to_string(),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: false,
            })
            .await;

        response.assert_status(StatusCode::CREATED);
    }
}

// ============================================================
// P0 - Filesystem Security
// ============================================================

mod filesystem {
    use super::*;

    #[tokio::test]
    async fn browse_rejects_path_traversal() {
        let server = setup().await;

        let response = server
            .get("/api/v1/filesystem/browse?path=/../../../etc/passwd")
            .await;

        // Should be 400 or 403, not 200
        let status = response.status_code();
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::FORBIDDEN,
            "Expected 400 or 403, got {}",
            status
        );
    }

    #[tokio::test]
    async fn browse_rejects_relative_path() {
        let server = setup().await;

        let response = server
            .get("/api/v1/filesystem/browse?path=relative/path")
            .await;

        response.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn mkdir_rejects_path_traversal() {
        let server = setup().await;

        let response = server
            .post("/api/v1/filesystem/mkdir")
            .json(&serde_json::json!({ "path": "/tmp/../etc/evil" }))
            .await;

        let status = response.status_code();
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::FORBIDDEN,
            "Expected 400 or 403 for path traversal, got {}",
            status
        );
    }

    #[tokio::test]
    async fn mkdir_rejects_relative_path() {
        let server = setup().await;

        let response = server
            .post("/api/v1/filesystem/mkdir")
            .json(&serde_json::json!({ "path": "relative/new-dir" }))
            .await;

        response.assert_status(StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn browse_returns_entries_for_valid_path() {
        let server = setup().await;

        // Browse the home directory (always allowed, always has entries)
        let home = dirs::home_dir().expect("home dir");
        let response = server
            .get(&format!(
                "/api/v1/filesystem/browse?path={}",
                home.display()
            ))
            .await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert!(body["path"].is_string());
        let entries = body["entries"].as_array().expect("entries array");
        assert!(!entries.is_empty(), "Home dir should have entries");
        // Each entry should have name and path
        assert!(entries[0]["name"].is_string());
        assert!(entries[0]["path"].is_string());
    }

    #[tokio::test]
    async fn browse_returns_404_for_nonexistent_path() {
        let server = setup().await;

        let response = server
            .get("/api/v1/filesystem/browse?path=/nonexistent_path_xyz_123")
            .await;

        response.assert_status_not_found();
    }
}

// ============================================================
// P0 - Claims
// ============================================================

mod claims {
    use super::*;

    #[tokio::test]
    async fn claim_succeeds_on_unclaimed_feature() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Claimable").await;

        let response = server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({
                "agent_type": "claude",
                "metadata": "{\"branch\": \"feature/test\"}"
            }))
            .await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["ok"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn claim_returns_409_when_already_claimed() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Contested").await;

        // First claim
        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await
            .assert_status_ok();

        // Second claim should conflict
        let response = server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "gemini" }))
            .await;

        response.assert_status(StatusCode::CONFLICT);
        let body: serde_json::Value = response.json();
        assert_eq!(body["error"].as_str(), Some("claim_conflict"));
        assert!(body["conflict"].is_object(), "Expected conflict object");
        assert_eq!(
            body["conflict"]["agent_type"].as_str(),
            Some("claude"),
            "Conflict should show original claimer"
        );
    }

    #[tokio::test]
    async fn claim_force_overrides_existing_claim() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Override Me").await;

        // First claim
        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await
            .assert_status_ok();

        // Force claim by different agent
        let response = server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "gemini", "force": true }))
            .await;

        response.assert_status_ok();

        // Verify the feature is now claimed by gemini
        let get_response = server
            .get(&format!("/api/v1/features/{}", feature.id))
            .await;
        let updated: Feature = get_response.json();
        assert_eq!(updated.claimed_by.as_deref(), Some("gemini"));
    }

    #[tokio::test]
    async fn claim_transitions_proposed_to_in_progress() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Start Me").await;
        assert_eq!(feature.state, FeatureState::Proposed);

        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await
            .assert_status_ok();

        let get_response = server
            .get(&format!("/api/v1/features/{}", feature.id))
            .await;
        let updated: Feature = get_response.json();
        assert_eq!(updated.state, FeatureState::InProgress);
    }
}

// ============================================================
// P1 - Complete Feature
// ============================================================

mod complete_feature {
    use super::*;

    #[tokio::test]
    async fn completes_with_backfill_flag() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature_with_details(
            &server,
            &project,
            "Backfill Feature",
            "As a user, I can do things.\n\n- [x] Criterion met",
        )
        .await;

        // Claim to move to in_progress
        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await;

        // Complete with backfill=true (skips proof requirement)
        let response = server
            .post(&format!("/api/v1/features/{}/complete", feature.id))
            .json(&serde_json::json!({
                "summary": "Backfilled existing feature",
                "commits": [],
                "backfill": true
            }))
            .await;

        response.assert_status(StatusCode::CREATED);
        let body: serde_json::Value = response.json();
        assert_eq!(body["feature"]["state"].as_str(), Some("implemented"));
        assert!(body["history"]["id"].is_string());
    }

    #[tokio::test]
    async fn rejects_without_proof_when_not_backfill() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature_with_details(
            &server,
            &project,
            "Needs Proof",
            "As a user, I can test.\n\n- [ ] Test passes",
        )
        .await;

        // Claim to move to in_progress
        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await;

        // Try complete without proof
        let response = server
            .post(&format!("/api/v1/features/{}/complete", feature.id))
            .json(&serde_json::json!({
                "summary": "Completed without proof",
                "commits": []
            }))
            .await;

        // Should be rejected (400 or 409)
        let status = response.status_code();
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::CONFLICT,
            "Expected rejection without proof, got {}",
            status
        );
    }

    #[tokio::test]
    async fn completes_with_passing_proof() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature_with_details(
            &server,
            &project,
            "Proved Feature",
            "As a user, I can verify.\n\n- [x] All tests pass",
        )
        .await;

        // Claim to move to in_progress
        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await;

        // Record passing proof
        server
            .post(&format!("/api/v1/features/{}/proofs", feature.id))
            .json(&serde_json::json!({
                "command": "cargo test",
                "exit_code": 0,
                "output": "test result: ok. 5 passed"
            }))
            .await
            .assert_status_ok();

        // Complete feature
        let response = server
            .post(&format!("/api/v1/features/{}/complete", feature.id))
            .json(&serde_json::json!({
                "summary": "Implemented with tests",
                "commits": [{ "sha": "abc123", "message": "Add feature" }]
            }))
            .await;

        response.assert_status(StatusCode::CREATED);
        let body: serde_json::Value = response.json();
        assert_eq!(body["feature"]["state"].as_str(), Some("implemented"));
        // Claims should be cleared
        assert!(body["feature"]["claimed_by"].is_null());
    }

    #[tokio::test]
    async fn rejects_with_failing_proof() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature_with_details(
            &server,
            &project,
            "Failing Tests",
            "As a user, I can test.\n\n- [ ] Tests pass",
        )
        .await;

        // Claim
        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await;

        // Record failing proof
        server
            .post(&format!("/api/v1/features/{}/proofs", feature.id))
            .json(&serde_json::json!({
                "command": "cargo test",
                "exit_code": 1,
                "output": "FAILED. 2 tests failed"
            }))
            .await
            .assert_status_ok();

        // Try to complete with failing proof
        let response = server
            .post(&format!("/api/v1/features/{}/complete", feature.id))
            .json(&serde_json::json!({
                "summary": "Tried to complete",
                "commits": []
            }))
            .await;

        let status = response.status_code();
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::CONFLICT,
            "Expected rejection with failing proof, got {}",
            status
        );
    }
}

// ============================================================
// P1 - Version Release
// ============================================================

mod version_release {
    use super::*;

    #[tokio::test]
    async fn release_marks_version_as_released() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        // Get existing versions (project creates default versions)
        let versions_response = server
            .get(&format!("/api/v1/projects/{}/versions", project.id))
            .await;
        let versions: Vec<serde_json::Value> = versions_response.json();
        assert!(!versions.is_empty(), "Project should have default versions");

        let version_id = versions[0]["id"].as_str().unwrap();

        // Release the version
        let response = server
            .put(&format!("/api/v1/versions/{}", version_id))
            .json(&serde_json::json!({
                "released_at": "2026-03-13T00:00:00Z"
            }))
            .await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert!(body["released_at"].is_string());
    }

    #[tokio::test]
    async fn release_creates_history_entry() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let versions_response = server
            .get(&format!("/api/v1/projects/{}/versions", project.id))
            .await;
        let versions: Vec<serde_json::Value> = versions_response.json();
        let version_id = versions[0]["id"].as_str().unwrap();
        let version_name = versions[0]["name"].as_str().unwrap().to_string();

        // Release
        server
            .put(&format!("/api/v1/versions/{}", version_id))
            .json(&serde_json::json!({
                "released_at": "2026-03-13T00:00:00Z"
            }))
            .await;

        // Check history on root feature
        if let Some(root_id) = project.root_feature_id {
            let history_response = server
                .get(&format!("/api/v1/features/{}/history", root_id))
                .await;
            let history: Vec<serde_json::Value> = history_response.json();
            let has_release_entry = history.iter().any(|h| {
                h["details"]["summary"]
                    .as_str()
                    .is_some_and(|s| s.contains("Released") && s.contains(&version_name))
            });
            assert!(has_release_entry, "Expected release history entry");
        }
    }

    #[tokio::test]
    async fn release_ensures_minimum_unreleased_versions() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let versions_response = server
            .get(&format!("/api/v1/projects/{}/versions", project.id))
            .await;
        let versions: Vec<serde_json::Value> = versions_response.json();
        let version_id = versions[0]["id"].as_str().unwrap();

        // Release the first version
        server
            .put(&format!("/api/v1/versions/{}", version_id))
            .json(&serde_json::json!({
                "released_at": "2026-03-13T00:00:00Z"
            }))
            .await;

        // Check that unreleased versions still exist (ensure_minimum_versions)
        let updated_versions_response = server
            .get(&format!("/api/v1/projects/{}/versions", project.id))
            .await;
        let updated_versions: Vec<serde_json::Value> = updated_versions_response.json();
        let unreleased_count = updated_versions
            .iter()
            .filter(|v| v["released_at"].is_null())
            .count();
        assert!(
            unreleased_count >= 4,
            "Expected at least 4 unreleased versions after release, got {}",
            unreleased_count
        );
    }
}

// ============================================================
// P1 - Bulk Create Features
// ============================================================

mod bulk_create_features {
    use super::*;

    #[tokio::test]
    async fn creates_features_when_confirmed() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let response = server
            .post(&format!("/api/v1/projects/{}/features/bulk", project.id))
            .json(&serde_json::json!({
                "confirm": true,
                "features": [
                    { "title": "Auth", "priority": 0, "children": [
                        { "title": "Login", "priority": 0, "children": [] },
                        { "title": "Logout", "priority": 1, "children": [] }
                    ]},
                    { "title": "Dashboard", "priority": 1, "children": [] }
                ]
            }))
            .await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["created"].as_bool(), Some(true));
        let ids = body["created_feature_ids"].as_array().unwrap();
        assert_eq!(
            ids.len(),
            4,
            "Should create Auth + Login + Logout + Dashboard"
        );
    }

    #[tokio::test]
    async fn preview_mode_does_not_create_features() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        let response = server
            .post(&format!("/api/v1/projects/{}/features/bulk", project.id))
            .json(&serde_json::json!({
                "confirm": false,
                "features": [
                    { "title": "Preview Only", "priority": 0, "children": [] }
                ]
            }))
            .await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        assert_eq!(body["created"].as_bool(), Some(false));
        let ids = body["created_feature_ids"].as_array().unwrap();
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn rejects_exceeding_max_bulk_features() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        // Create 201 features (over the 200 limit)
        let features: Vec<serde_json::Value> = (0..201)
            .map(|i| {
                serde_json::json!({
                    "title": format!("Feature {}", i),
                    "priority": 0,
                    "children": []
                })
            })
            .collect();

        let response = server
            .post(&format!("/api/v1/projects/{}/features/bulk", project.id))
            .json(&serde_json::json!({
                "confirm": true,
                "features": features
            }))
            .await;

        response.assert_status(StatusCode::BAD_REQUEST);
    }
}

// ============================================================
// P1 - Feature Update
// ============================================================

mod feature_update {
    use super::*;

    #[tokio::test]
    async fn updates_feature_details() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Updateable").await;

        let response = server
            .put(&format!("/api/v1/features/{}", feature.id))
            .json(&serde_json::json!({
                "details": "Updated specification content"
            }))
            .await;

        response.assert_status_ok();
        let updated: Feature = response.json();
        assert_eq!(
            updated.details.as_deref(),
            Some("Updated specification content")
        );
    }

    #[tokio::test]
    async fn updates_feature_state() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "State Change").await;
        assert_eq!(feature.state, FeatureState::Proposed);

        let response = server
            .put(&format!("/api/v1/features/{}", feature.id))
            .json(&serde_json::json!({ "state": "in_progress" }))
            .await;

        response.assert_status_ok();
        let updated: Feature = response.json();
        assert_eq!(updated.state, FeatureState::InProgress);
    }

    #[tokio::test]
    async fn returns_404_for_nonexistent_feature() {
        let server = setup().await;

        let fake_id = uuid::Uuid::new_v4();
        let response = server
            .put(&format!("/api/v1/features/{}", fake_id))
            .json(&serde_json::json!({ "title": "Ghost" }))
            .await;

        response.assert_status_not_found();
    }
}

// ============================================================
// P2 - Error Response Format
// ============================================================

mod error_responses {
    use super::*;

    #[tokio::test]
    async fn not_found_returns_json_error_shape() {
        let server = setup().await;

        let fake_id = uuid::Uuid::new_v4();
        let response = server.get(&format!("/api/v1/features/{}", fake_id)).await;

        response.assert_status_not_found();
        let body: serde_json::Value = response.json();
        assert!(
            body["error"].is_string(),
            "404 should return {{\"error\": \"...\"}}"
        );
    }

    #[tokio::test]
    async fn validation_error_returns_400_with_json() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        // Empty title should fail validation (min length 1)
        let response = server
            .post(&format!("/api/v1/projects/{}/features", project.id))
            .json(&serde_json::json!({
                "title": ""
            }))
            .await;

        let status = response.status_code();
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "Expected 400 or 422 for validation error, got {}",
            status
        );
    }

    #[tokio::test]
    async fn claim_conflict_returns_structured_body() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Conflict Shape").await;

        server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "claude" }))
            .await;

        let response = server
            .put(&format!("/api/v1/features/{}/claim", feature.id))
            .json(&serde_json::json!({ "agent_type": "gemini" }))
            .await;

        response.assert_status(StatusCode::CONFLICT);
        let body: serde_json::Value = response.json();
        // Verify the full structured error shape
        assert_eq!(body["error"].as_str(), Some("claim_conflict"));
        assert!(body["message"].is_string());
        assert!(body["conflict"]["agent_type"].is_string());
        assert!(body["conflict"]["feature_id"].is_string());
        assert!(body["conflict"]["claimed_at"].is_string());
    }
}

// ============================================================
// P2 - Pagination & Query Parameters
// ============================================================

mod pagination {
    use super::*;

    #[tokio::test]
    async fn list_features_with_limit_and_offset() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        // Create 5 features
        for i in 0..5 {
            create_feature(&server, &project, &format!("Feature {}", i)).await;
        }

        // Request with limit=2
        let response = server
            .get(&format!("/api/v1/projects/{}/features?limit=2", project.id))
            .await;
        response.assert_status_ok();
        let features: Vec<serde_json::Value> = response.json();
        assert_eq!(features.len(), 2);

        // Request with offset — should return fewer results than without offset
        let all_response = server
            .get(&format!(
                "/api/v1/projects/{}/features?limit=100",
                project.id
            ))
            .await;
        let all_features: Vec<serde_json::Value> = all_response.json();
        let total = all_features.len();

        let response = server
            .get(&format!(
                "/api/v1/projects/{}/features?limit=100&offset=3",
                project.id
            ))
            .await;
        response.assert_status_ok();
        let features: Vec<serde_json::Value> = response.json();
        assert_eq!(
            features.len(),
            total - 3,
            "Offset should skip 3 features from total {}",
            total
        );
    }

    #[tokio::test]
    async fn search_features_by_query() {
        let server = setup().await;
        let project = create_test_project(&server).await;

        create_feature(&server, &project, "User Authentication").await;
        create_feature(&server, &project, "Dashboard Widgets").await;

        let response = server
            .get(&format!(
                "/api/v1/features/search?q=Authentication&project_id={}",
                project.id
            ))
            .await;
        response.assert_status_ok();
        let results: Vec<serde_json::Value> = response.json();
        assert!(
            !results.is_empty(),
            "Search should find 'User Authentication'"
        );
        assert!(results
            .iter()
            .any(|f| f["title"].as_str() == Some("User Authentication")));
    }

    #[tokio::test]
    async fn resolve_feature_by_display_id() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "Resolvable").await;

        // Resolve by full UUID
        let response = server
            .get(&format!("/api/v1/features/resolve?prefix={}", feature.id))
            .await;
        response.assert_status_ok();
        let resolved: Feature = response.json();
        assert_eq!(resolved.id, feature.id);
    }

    #[tokio::test]
    async fn resolve_feature_returns_404_for_no_match() {
        let server = setup().await;

        let response = server
            .get("/api/v1/features/resolve?prefix=NONEXIST-999")
            .await;

        response.assert_status_not_found();
    }
}

// ============================================================
// P2 - Feature Dependents
// ============================================================

mod feature_dependents {
    use super::*;

    #[tokio::test]
    async fn returns_empty_when_no_dependents() {
        let server = setup().await;
        let project = create_test_project(&server).await;
        let feature = create_feature(&server, &project, "No Dependents").await;

        let response = server
            .get(&format!("/api/v1/features/{}/dependents", feature.id))
            .await;

        response.assert_status_ok();
        let dependents: Vec<serde_json::Value> = response.json();
        assert!(dependents.is_empty());
    }

    #[tokio::test]
    async fn returns_404_for_nonexistent_feature() {
        let server = setup().await;

        let fake_id = uuid::Uuid::new_v4();
        let response = server
            .get(&format!("/api/v1/features/{}/dependents", fake_id))
            .await;

        response.assert_status_not_found();
    }
}
