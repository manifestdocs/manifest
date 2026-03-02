//! MCP tool response content tests.
//!
//! These tests verify what agents actually see when they call MCP tools.
//! They use an in-process TestServer → ManifestClient → tool functions pipeline
//! to assert on CallToolResult content blocks — summaries, YAML data, warnings,
//! conditional guidance, and completion contracts.

use axum_test::TestServer;
use manifest::api::create_router;
use manifest::db::Database;
use manifest::mcp::tools::{features, memories, versions};
use manifest::mcp::ManifestClient;
use manifest::models::*;
use rmcp::model::{CallToolResult, RawContent};
use uuid::Uuid;

mod common;

// ============================================================
// Test Infrastructure
// ============================================================

async fn setup() -> (TestServer, ManifestClient) {
    let db = Database::open_memory()
        .await
        .expect("Failed to create in-memory database");
    db.migrate().await.expect("Failed to run migrations");
    let app = create_router(db);
    let server = TestServer::builder()
        .http_transport()
        .build(app)
        .expect("Failed to create test server");
    let base = server.server_address().expect("No server address");
    let url = format!("{}api/v1", base);
    let client = ManifestClient::new(url, None).expect("Failed to create ManifestClient");
    (server, client)
}

/// Extract all text content blocks from a CallToolResult.
fn text_contents(result: &CallToolResult) -> Vec<&str> {
    result
        .content
        .iter()
        .filter_map(|c| match &c.raw {
            RawContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect()
}

/// Check that a text block containing `needle` exists in the result.
fn has_text_containing(result: &CallToolResult, needle: &str) -> bool {
    text_contents(result).iter().any(|t| t.contains(needle))
}

/// Create a project via the API for test setup.
async fn create_project(server: &TestServer) -> Project {
    server
        .post("/api/v1/projects")
        .json(&CreateProjectInput {
            slug: None,
            name: "Test Project".to_string(),
            description: None,
            instructions: None,
            key_prefix: None,
        })
        .await
        .json::<Project>()
}

/// Create a feature via the API for test setup.
async fn create_feature(
    server: &TestServer,
    project_id: Uuid,
    title: &str,
    details: Option<&str>,
) -> Feature {
    server
        .post(&format!("/api/v1/projects/{}/features", project_id))
        .json(&CreateFeatureInput {
            id: None,
            parent_id: None,
            title: title.to_string(),
            details: details.map(|s| s.to_string()),
            state: None,
            priority: None,
            target_version_id: None,
        })
        .await
        .json::<Feature>()
}

/// Create a feature with a parent.
async fn create_child_feature(
    server: &TestServer,
    project_id: Uuid,
    parent_id: Uuid,
    title: &str,
    details: Option<&str>,
) -> Feature {
    server
        .post(&format!("/api/v1/projects/{}/features", project_id))
        .json(&CreateFeatureInput {
            id: None,
            parent_id: Some(parent_id.into()),
            title: title.to_string(),
            details: details.map(|s| s.to_string()),
            state: None,
            priority: None,
            target_version_id: None,
        })
        .await
        .json::<Feature>()
}

/// A spec with sufficient detail and testable criteria for standard config.
const GOOD_SPEC: &str = "\
As a user, I can log in with email and password so that I can access my account.

- [ ] Accepts valid email and password and returns a session token
- [ ] Rejects invalid credentials with a 401 error
- [ ] Rate-limits login attempts to 5 per minute per IP address";

// ============================================================
// Tier 1: Work Lifecycle — start_feature, complete_feature, prove_feature
// ============================================================

mod start_feature_tool {
    use super::*;
    use manifest::mcp::types::StartFeatureRequest;

    #[tokio::test]
    async fn returns_yaml_with_feature_data() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "User Login", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));

        let texts = text_contents(&result);
        // First block is human-readable summary
        assert!(texts[0].contains("Started 'User Login'"));
        assert!(texts[0].contains("in_progress"));

        // YAML block should contain feature fields
        assert!(has_text_containing(&result, "title: User Login"));
        assert!(has_text_containing(&result, "state: in_progress"));
    }

    #[tokio::test]
    async fn contains_completion_contract() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "User Login", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert!(has_text_containing(&result, "COMPLETION CONTRACT"));
    }

    #[tokio::test]
    async fn advisory_policy_includes_testing_guidance_with_prove_feature() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        // Default testing_policy is advisory
        let feature = create_feature(&server, pid, "User Login", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Testing guidance should mention prove_feature
        assert!(has_text_containing(&result, "prove_feature"));
        // Completion contract should also mention prove_feature (advisory policy)
        let texts = text_contents(&result);
        let contract = texts
            .iter()
            .find(|t| t.contains("COMPLETION CONTRACT"))
            .expect("completion contract missing");
        assert!(contract.contains("prove_feature"));
    }

    #[tokio::test]
    async fn tdd_policy_includes_required_before_guidance() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Set testing_policy to tdd
        server
            .put(&format!("/api/v1/projects/{}", pid))
            .json(&serde_json::json!({ "testing_policy": "tdd" }))
            .await;

        let feature = create_feature(&server, pid, "User Login", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert!(has_text_containing(&result, "REQUIRED"));
        assert!(has_text_containing(&result, "BEFORE"));
    }

    #[tokio::test]
    async fn blocks_when_feature_has_no_details() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "No Spec Feature", None).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(has_text_containing(&result, "specification required"));
    }

    #[tokio::test]
    async fn warns_when_spec_is_sparse() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Sparse Feature", Some("Handle login.")).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Should succeed but include a warning
        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "\u{26a0}"));
    }

    #[tokio::test]
    async fn warns_on_conflict_when_already_claimed() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Claimed Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // First claim succeeds
        let _ = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Second claim without force should get conflict
        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "gemini".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(has_text_containing(&result, "CONFLICT"));
        assert!(has_text_containing(&result, "claude"));
    }

    #[tokio::test]
    async fn rejects_feature_set_with_children() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let parent = create_feature(&server, pid, "Auth", Some("Auth subsystem")).await;
        let parent_id: Uuid = parent.id.into();
        let _child = create_child_feature(&server, pid, parent_id, "Login", Some(GOOD_SPEC)).await;

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: parent_id.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(has_text_containing(&result, "feature set"));
        assert!(has_text_containing(&result, "children"));
    }

    #[tokio::test]
    async fn warns_when_spec_has_no_testable_criteria() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        // Spec with prose but no checkbox acceptance criteria
        let spec = "As a user, I can log in so that I can access my account.\n\n\
                     The system should authenticate users against the database and manage \
                     sessions correctly with proper error handling throughout.";
        let feature = create_feature(&server, pid, "Login", Some(spec)).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Should succeed but warn about missing testable criteria
        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "No testable criteria"));
    }
}

mod complete_feature_tool {
    use super::*;
    use manifest::mcp::types::{CommitRefInput, CompleteFeatureRequest, StartFeatureRequest};

    #[tokio::test]
    async fn confirms_completion_with_title() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "CSV Export", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Start first
        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Prove first
        features::prove_feature(
            &client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                output: None,
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        // Complete
        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Implemented CSV export with streaming".to_string(),
                commits: vec![CommitRefInput {
                    sha: "abc1234".to_string(),
                    message: "Add CSV export".to_string(),
                    author: None,
                }],
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));

        let texts = text_contents(&result);
        // Summary should confirm completion with title
        assert!(texts[0].contains("Completed 'CSV Export'"));
        assert!(texts[0].contains("1 commit"));
    }

    #[tokio::test]
    async fn includes_verification_status() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Feature A", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Prove first
        features::prove_feature(
            &client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                output: None,
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Done".to_string(),
                commits: vec![],
            },
        )
        .await
        .unwrap();

        // Should succeed and include verification status since proof was provided
        assert!(has_text_containing(&result, "Verification: exit code 0"));
    }

    #[tokio::test]
    async fn transitions_to_implemented() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Done Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Prove first
        features::prove_feature(
            &client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                output: None,
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Built it".to_string(),
                commits: vec![],
            },
        )
        .await
        .unwrap();

        // JSON block should show implemented state
        assert!(has_text_containing(&result, "implemented"));
    }

    #[tokio::test]
    async fn blocks_completion_without_proof() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Unproven Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Start but do NOT call prove_feature
        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Attempt to complete without proof — should fail
        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Done without tests".to_string(),
                commits: vec![],
            },
        )
        .await;

        assert!(
            result.is_err(),
            "complete_feature should fail without proof"
        );
    }

    #[tokio::test]
    async fn blocks_completion_with_failing_proof() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Failing Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Record a failing proof (exit_code != 0)
        features::prove_feature(
            &client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 1,
                output: Some("test failed".to_string()),
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        // Attempt to complete with failing proof — should fail
        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Done with failing tests".to_string(),
                commits: vec![],
            },
        )
        .await;

        assert!(
            result.is_err(),
            "complete_feature should fail with failing proof"
        );
    }

    #[tokio::test]
    async fn warns_when_spec_not_updated_since_claiming() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Stale Spec", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Prove (passing) but do NOT update the feature spec
        features::prove_feature(
            &client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                output: None,
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Done".to_string(),
                commits: vec![],
            },
        )
        .await
        .unwrap();

        // Should succeed but include warning about stale spec
        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "spec not updated"));
    }
}

mod prove_feature_tool {
    use super::*;
    use manifest::mcp::types::{
        ProveFeatureRequest, StartFeatureRequest, TestResultInput, TestSuiteInput,
    };

    #[tokio::test]
    async fn confirms_proof_recorded() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Tested Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        let result = features::prove_feature(
            &client,
            ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                output: None,
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "Verification recorded"));
    }

    #[tokio::test]
    async fn renders_test_tree_with_structured_results() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Suite Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        let result = features::prove_feature(
            &client,
            ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test auth_spec".to_string(),
                exit_code: 0,
                output: None,
                test_suites: Some(vec![TestSuiteInput {
                    name: "auth_spec".to_string(),
                    file: Some("tests/auth_spec.rs".to_string()),
                    tests: vec![
                        TestResultInput {
                            name: "logs in with valid credentials".to_string(),
                            suite: None,
                            state: "passed".to_string(),
                            file: None,
                            line: None,
                            duration_ms: Some(12),
                            message: None,
                        },
                        TestResultInput {
                            name: "rejects invalid password".to_string(),
                            suite: None,
                            state: "passed".to_string(),
                            file: None,
                            line: None,
                            duration_ms: Some(5),
                            message: None,
                        },
                    ],
                }]),
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        let output = texts.join("\n");
        // Should render the test tree with suite and test names
        assert!(output.contains("auth_spec") || output.contains("tests/auth_spec.rs"));
        assert!(output.contains("logs in with valid credentials"));
        assert!(output.contains("rejects invalid password"));
    }

    #[tokio::test]
    async fn handles_failing_tests() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Failing Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        let result = features::prove_feature(
            &client,
            ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 1,
                output: Some("test failed".to_string()),
                test_suites: Some(vec![TestSuiteInput {
                    name: "auth_spec".to_string(),
                    file: None,
                    tests: vec![TestResultInput {
                        name: "login works".to_string(),
                        suite: None,
                        state: "failed".to_string(),
                        file: None,
                        line: None,
                        duration_ms: None,
                        message: Some("assertion failed: expected 200, got 401".to_string()),
                    }],
                }]),
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        // Should still succeed (recording proof even for failures is valid)
        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "Verification recorded"));
    }
}

// ============================================================
// Tier 2: Navigation — get_next_feature, get_feature, find_features, render_feature_tree
// ============================================================

mod get_next_feature_tool {
    use super::*;
    use manifest::mcp::types::GetNextFeatureRequest;

    #[tokio::test]
    async fn returns_highest_priority_proposed_feature() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Use auto-created version (project creation ensures 4 versions exist)
        let versions: Vec<serde_json::Value> = server
            .get(&format!("/api/v1/projects/{}/versions", pid))
            .await
            .json();
        let vid: Uuid = versions[0]["id"].as_str().unwrap().parse().unwrap();

        // Create features with different priorities
        let f1 = create_feature(&server, pid, "Low Priority", Some(GOOD_SPEC)).await;
        let f2 = create_feature(&server, pid, "High Priority", Some(GOOD_SPEC)).await;
        let f1id: Uuid = f1.id.into();
        let f2id: Uuid = f2.id.into();

        // Assign to version and set priorities
        server
            .put(&format!("/api/v1/features/{}", f1id))
            .json(&serde_json::json!({ "priority": 10, "target_version_id": vid }))
            .await;
        server
            .put(&format!("/api/v1/features/{}", f2id))
            .json(&serde_json::json!({ "priority": 1, "target_version_id": vid }))
            .await;

        let result = features::get_next_feature(
            &client,
            GetNextFeatureRequest {
                project_id: pid,
                version_id: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "High Priority"));
    }

    #[tokio::test]
    async fn returns_message_when_no_features_exist() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = features::get_next_feature(
            &client,
            GetNextFeatureRequest {
                project_id: pid,
                version_id: None,
            },
        )
        .await
        .unwrap();

        assert!(has_text_containing(&result, "No workable features"));
    }
}

mod get_feature_tool {
    use super::*;
    use manifest::mcp::types::GetFeatureRequest;

    #[tokio::test]
    async fn returns_yaml_with_feature_fields() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Profile Page", Some("Shows user info")).await;
        let fid: Uuid = feature.id.into();

        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: fid.to_string(),
                include_history: false,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));

        let texts = text_contents(&result);
        // First block is summary
        assert!(texts[0].contains("Profile Page"));
        assert!(texts[0].contains("proposed"));

        // YAML block should contain feature data
        assert!(has_text_containing(&result, "title: Profile Page"));
    }

    #[tokio::test]
    async fn includes_history_when_requested() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "History Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Start and complete to create history
        features::start_feature(
            &client,
            manifest::mcp::types::StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Prove First
        features::prove_feature(
            &client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: fid.to_string(),
                command: "cargo test".to_string(),
                exit_code: 0,
                output: None,
                test_suites: None,
                tests: None,
                evidence: vec![],
                commit_sha: None,
            },
        )
        .await
        .unwrap();

        features::complete_feature(
            &client,
            manifest::mcp::types::CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Built history feature".to_string(),
                commits: vec![manifest::mcp::types::CommitRefInput {
                    sha: "def5678".to_string(),
                    message: "Add history".to_string(),
                    author: None,
                }],
            },
        )
        .await
        .unwrap();

        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: fid.to_string(),
                include_history: true,
            },
        )
        .await
        .unwrap();

        // YAML should include history
        assert!(has_text_containing(&result, "Built history feature"));
    }

    #[tokio::test]
    async fn includes_breadcrumb_with_parent_context() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let parent = create_feature(
            &server,
            pid,
            "Auth System",
            Some("OAuth-based authentication using JWT tokens."),
        )
        .await;
        let parent_id: Uuid = parent.id.into();
        let child = create_child_feature(&server, pid, parent_id, "Login", Some(GOOD_SPEC)).await;
        let child_id: Uuid = child.id.into();

        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: child_id.to_string(),
                include_history: false,
            },
        )
        .await
        .unwrap();

        // Breadcrumb should include parent
        assert!(has_text_containing(&result, "Auth System"));
    }
}

mod find_features_tool {
    use super::*;
    use manifest::mcp::types::FindFeaturesRequest;

    #[tokio::test]
    async fn returns_markdown_table_of_features() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        create_feature(&server, pid, "Feature One", Some("details")).await;
        create_feature(&server, pid, "Feature Two", Some("details")).await;

        let result = features::find_features(
            &client,
            FindFeaturesRequest {
                project_id: Some(pid),
                version_id: None,
                state: None,
                query: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(!texts.is_empty());
        // Should be a markdown table with headers
        assert!(texts[0].contains("ID"));
        assert!(texts[0].contains("Title"));
        assert!(texts[0].contains("Feature One"));
        assert!(texts[0].contains("Feature Two"));
    }
}

mod render_feature_tree_tool {
    use super::*;
    use manifest::mcp::types::RenderFeatureTreeRequest;

    #[tokio::test]
    async fn returns_ascii_tree_with_state_symbols() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let parent = create_feature(&server, pid, "Auth", Some("Auth subsystem")).await;
        let parent_id: Uuid = parent.id.into();
        create_child_feature(&server, pid, parent_id, "Login", Some(GOOD_SPEC)).await;
        create_child_feature(&server, pid, parent_id, "Logout", Some("Logout flow")).await;

        let result = features::render_feature_tree(
            &client,
            RenderFeatureTreeRequest {
                project_id: pid,
                max_depth: 0,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        let tree = texts[0];
        // Should contain the tree characters and feature names
        assert!(tree.contains("Auth"));
        assert!(tree.contains("Login"));
        assert!(tree.contains("Logout"));
        // Proposed features use ◇ symbol
        assert!(tree.contains("\u{25c7}"));
    }
}

// ============================================================
// Tier 3: CRUD — create_feature, update_feature, plan
// ============================================================

mod create_feature_tool {
    use super::*;
    use manifest::mcp::types::CreateFeatureRequest;

    #[tokio::test]
    async fn confirms_creation_with_title_and_state() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = features::create_feature(
            &client,
            CreateFeatureRequest {
                project_id: pid,
                parent_id: None,
                title: "New Feature".to_string(),
                details: Some("A brand new feature".to_string()),
                state: "proposed".to_string(),
                priority: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains("Created 'New Feature'"));
        assert!(texts[0].contains("proposed"));
    }
}

mod update_feature_tool {
    use super::*;
    use manifest::mcp::types::UpdateFeatureRequest;

    #[tokio::test]
    async fn confirms_what_changed() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        let feature = create_feature(&server, pid, "Old Title", Some("details")).await;
        let fid: Uuid = feature.id.into();

        let result = features::update_feature(
            &client,
            UpdateFeatureRequest {
                feature_id: fid.to_string(),
                title: Some("New Title".to_string()),
                details: None,
                desired_details: None,
                details_summary: None,
                state: None,
                priority: None,
                parent_id: None,
                target_version_id: None,
                clear_version: false,
                blocked_by: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains("Updated 'New Title'"));
    }
}

mod plan_tool {
    use super::*;
    use manifest::mcp::types::{PlanFeaturesRequest, ProposedFeature};

    #[tokio::test]
    async fn proposal_mode_returns_preview_without_creating() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = features::plan(
            &client,
            PlanFeaturesRequest {
                project_id: pid,
                target_version_id: None,
                features: vec![
                    ProposedFeature {
                        title: "Auth".to_string(),
                        details: None,
                        priority: 0,
                        children: vec![],
                    },
                    ProposedFeature {
                        title: "Dashboard".to_string(),
                        details: None,
                        priority: 1,
                        children: vec![],
                    },
                ],
                confirm: false,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains("Proposed 2 features"));
    }

    #[tokio::test]
    async fn confirm_mode_creates_features() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = features::plan(
            &client,
            PlanFeaturesRequest {
                project_id: pid,
                target_version_id: None,
                features: vec![ProposedFeature {
                    title: "API Layer".to_string(),
                    details: Some("REST API endpoints".to_string()),
                    priority: 0,
                    children: vec![],
                }],
                confirm: true,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        assert!(texts[0].contains("Created 1 feature"));
    }
}

// ============================================================
// Tier 4: Versions & Memories
// ============================================================

mod list_versions_tool {
    use super::*;
    use manifest::mcp::types::ListVersionsRequest;

    #[tokio::test]
    async fn shows_versions_with_status() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create a version
        server
            .post(&format!("/api/v1/projects/{}/versions", pid))
            .json(&serde_json::json!({ "name": "0.1.0" }))
            .await;

        let result = versions::list_versions(&client, ListVersionsRequest { project_id: pid })
            .await
            .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "0.1.0"));
        assert!(has_text_containing(&result, "next"));
    }
}

mod create_version_tool {
    use super::*;
    use manifest::mcp::types::CreateVersionRequest;

    #[tokio::test]
    async fn confirms_version_name() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = versions::create_version(
            &client,
            CreateVersionRequest {
                project_id: pid,
                name: "1.0.0".to_string(),
                description: Some("First release".to_string()),
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains("Created version '1.0.0'"));
    }
}

mod release_version_tool {
    use super::*;
    use manifest::mcp::types::ReleaseVersionRequest;

    #[tokio::test]
    async fn confirms_release() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Use auto-created version (project creation ensures 4 versions exist)
        let versions: Vec<serde_json::Value> = server
            .get(&format!("/api/v1/projects/{}/versions", pid))
            .await
            .json();
        let vid: Uuid = versions[0]["id"].as_str().unwrap().parse().unwrap();
        let vname = versions[0]["name"].as_str().unwrap();

        let result = versions::release_version(&client, ReleaseVersionRequest { version_id: vid })
            .await
            .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains(&format!("Released '{}'", vname)));
    }
}

mod remember_tool {
    use super::*;
    use manifest::mcp::types::RememberRequest;

    #[tokio::test]
    async fn confirms_memory_storage() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = memories::remember(
            &client,
            RememberRequest {
                project_id: pid,
                content: "Use pnpm, not npm".to_string(),
                tags: vec!["tooling".to_string()],
                source_feature_id: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "pnpm"));
    }
}

mod recall_tool {
    use super::*;
    use manifest::mcp::types::{RecallRequest, RememberRequest};

    #[tokio::test]
    async fn returns_matching_memories() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Store a memory first
        memories::remember(
            &client,
            RememberRequest {
                project_id: pid,
                content: "Always use snake_case for variables".to_string(),
                tags: vec!["conventions".to_string()],
                source_feature_id: None,
            },
        )
        .await
        .unwrap();

        let result = memories::recall(
            &client,
            RecallRequest {
                project_id: pid,
                query: Some("snake_case".to_string()),
                limit: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "snake_case"));
    }
}

// NOTE: `memories::forget` is not tested here because DELETE requests hang
// against axum-test's HTTP transport (both via reqwest and axum-test's own
// server.delete()). The function is trivial (calls delete_memory, returns
// JSON `{ deleted: true, memory_id }`). DELETE is covered in api_spec.rs
// via mock transport (feature_cascade_delete).
