//! MCP tool response content tests.
//!
//! These tests verify what agents actually see when they call MCP tools.
//! They use an in-process TestServer → ManifestClient → tool functions pipeline
//! to assert on CallToolResult content blocks — summaries, YAML data, warnings,
//! conditional guidance, and completion contracts.

use axum_test::TestServer;
use manifest::api::create_router;
use manifest::db::Database;
use manifest::mcp::tools::{features, versions};
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
    async fn blocks_when_spec_is_sparse() {
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

        // Should block — no testable criteria
        assert_eq!(result.is_error, Some(true));
        assert!(has_text_containing(&result, "testable"));
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
    async fn blocks_when_spec_has_no_testable_criteria() {
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

        // Should block — no testable criteria
        assert_eq!(result.is_error, Some(true));
        assert!(has_text_containing(&result, "testable"));
    }

    #[tokio::test]
    async fn force_bypasses_spec_gate() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();
        // Spec with no testable criteria
        let spec = "Document the authentication subsystem architecture and decisions.";
        let feature = create_feature(&server, pid, "Auth Docs", Some(spec)).await;
        let fid: Uuid = feature.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: fid.to_string(),
                agent_type: "claude".to_string(),
                force: true,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        // Should succeed with force=true
        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "Auth Docs"));
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
                backfill: false,
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
                backfill: false,
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
                backfill: false,
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
                backfill: false,
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
                backfill: false,
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
                backfill: false,
            },
        )
        .await
        .unwrap();

        // Should succeed but include warning about stale spec
        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "spec not updated"));
    }

    #[tokio::test]
    async fn backfill_skips_proof_and_spec_requirements() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create feature with NO details (spec) — normally this would block completion
        let feature = create_feature(&server, pid, "Existing Auth", None).await;
        let fid: Uuid = feature.id.into();

        // Complete with backfill=true — should succeed without proof or spec
        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Pre-existing authentication system".to_string(),
                commits: vec![],
                backfill: true,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(has_text_containing(&result, "Backfilled"));
        assert!(has_text_containing(&result, "backfilled"));
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
                depth: None,
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
                backfill: false,
            },
        )
        .await
        .unwrap();

        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: fid.to_string(),
                include_history: true,
                depth: None,
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
                depth: None,
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
                search_mode: None,
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
                        state: None,
                        children: vec![],
                    },
                    ProposedFeature {
                        title: "Dashboard".to_string(),
                        details: None,
                        priority: 1,
                        state: None,
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
                    state: None,
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

    #[tokio::test]
    async fn creates_features_with_implemented_state() {
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
                        title: "Existing Feature".to_string(),
                        details: Some("Already built".to_string()),
                        priority: 0,
                        state: Some("implemented".to_string()),
                        children: vec![],
                    },
                    ProposedFeature {
                        title: "New Feature".to_string(),
                        details: None,
                        priority: 1,
                        state: None,
                        children: vec![],
                    },
                ],
                confirm: true,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains("Created 2 features"));
        assert!(texts[0].contains("1 already implemented"));

        // Verify the features were created with correct states
        let tree_result = features::render_feature_tree(
            &client,
            manifest::mcp::types::RenderFeatureTreeRequest {
                project_id: pid,
                max_depth: 0,
            },
        )
        .await
        .unwrap();

        let tree_text = text_contents(&tree_result);
        let tree = &tree_text[0];
        // Implemented feature should show implemented marker
        assert!(tree.contains("Existing Feature"));
        assert!(tree.contains("New Feature"));
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

// ============================================================
// Orient Tool
// ============================================================

mod orient_tool {
    use super::*;
    use manifest::mcp::tools::orient;
    use manifest::mcp::types::OrientRequest;

    #[tokio::test]
    async fn returns_project_context_and_feature_tree() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create some features
        create_feature(&server, pid, "Authentication", Some("Auth module")).await;
        create_feature(&server, pid, "Dashboard", Some("Dashboard module")).await;

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: Some(pid),
                directory_path: None,
                max_depth: 2,
                include_history: true,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));

        // Single call returns bundled context
        let texts = text_contents(&result);
        assert!(!texts.is_empty());
        let output = texts[0];

        // Project name
        assert!(
            output.contains("Test Project"),
            "should contain project name"
        );
        // Feature tree
        assert!(
            output.contains("Feature Tree"),
            "should contain feature tree section"
        );
        assert!(
            output.contains("Authentication"),
            "tree should contain features"
        );
    }

    #[tokio::test]
    async fn includes_work_queue_with_proposed_features() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        create_feature(&server, pid, "Proposed One", Some("spec 1")).await;
        create_feature(&server, pid, "Proposed Two", Some("spec 2")).await;

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: Some(pid),
                directory_path: None,
                max_depth: 2,
                include_history: true,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        let output = texts[0];
        assert!(
            output.contains("Work Queue"),
            "should include work queue section"
        );
        assert!(
            output.contains("Proposed One"),
            "work queue should list proposed features"
        );
    }

    #[tokio::test]
    async fn includes_active_sessions_for_claimed_features() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let feature = create_feature(&server, pid, "Claimed Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Claim the feature via start_feature
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

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: Some(pid),
                directory_path: None,
                max_depth: 2,
                include_history: true,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        let output = texts[0];
        assert!(
            output.contains("Active Sessions"),
            "should include active sessions section"
        );
        assert!(
            output.contains("Claimed Feature"),
            "should show the claimed feature"
        );
        assert!(output.contains("claude"), "should show the agent type");
    }

    #[tokio::test]
    async fn includes_recent_history() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let feature = create_feature(&server, pid, "Done Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Complete the feature (backfill mode to skip proof)
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

        features::complete_feature(
            &client,
            manifest::mcp::types::CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Implemented the thing".to_string(),
                commits: vec![],
                backfill: true,
            },
        )
        .await
        .unwrap();

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: Some(pid),
                directory_path: None,
                max_depth: 2,
                include_history: true,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        let output = texts[0];
        assert!(
            output.contains("Recent Completions"),
            "should include recent history"
        );
        assert!(
            output.contains("Done Feature"),
            "should show completed feature"
        );
    }

    #[tokio::test]
    async fn omits_history_when_disabled() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: Some(pid),
                directory_path: None,
                max_depth: 2,
                include_history: false,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        let output = texts[0];
        assert!(
            !output.contains("Recent Completions"),
            "should not include history when disabled"
        );
    }

    #[tokio::test]
    async fn respects_tree_depth_limit() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let parent = create_feature(&server, pid, "Parent", Some("parent details")).await;
        let parent_id: Uuid = parent.id.into();
        create_child_feature(&server, pid, parent_id, "Child", Some("child details")).await;

        // Depth 1 should truncate children
        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: Some(pid),
                directory_path: None,
                max_depth: 1,
                include_history: false,
            },
        )
        .await
        .unwrap();

        let texts = text_contents(&result);
        let output = texts[0];
        assert!(
            output.contains("(...)"),
            "depth limit should truncate with (...)"
        );
    }

    #[tokio::test]
    async fn requires_project_id_or_directory_path() {
        let (_server, client) = setup().await;

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: None,
                directory_path: None,
                max_depth: 2,
                include_history: true,
            },
        )
        .await;

        assert!(
            result.is_err(),
            "should error without project_id or directory_path"
        );
    }

    #[tokio::test]
    async fn auto_detects_project_from_directory() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Add a directory to the project
        server
            .post(&format!("/api/v1/projects/{}/directories", pid))
            .json(&manifest::models::AddDirectoryInput {
                path: "/tmp/test-orient-dir".to_string(),
                git_remote: None,
                is_primary: true,
                instructions: None,
            })
            .await;

        let result = orient::orient(
            &client,
            OrientRequest {
                project_id: None,
                directory_path: Some("/tmp/test-orient-dir".to_string()),
                max_depth: 2,
                include_history: true,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        assert!(texts[0].contains("Test Project"));
    }
}

// ============================================================
// Feature Set Context Guidance (MANIF-162)
// ============================================================

mod feature_set_context_guidance {
    use super::*;
    use manifest::mcp::types::{CreateFeatureRequest, StartFeatureRequest};

    #[tokio::test]
    async fn start_feature_nudges_when_parent_has_empty_details() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create parent feature set with NO details
        let parent = create_feature(&server, pid, "Authentication", None).await;
        let parent_id: Uuid = parent.id.into();

        // Create child with a good spec
        let child =
            create_child_feature(&server, pid, parent_id, "Email Login", Some(GOOD_SPEC)).await;
        let child_id: Uuid = child.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: child_id.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            has_text_containing(&result, "has no shared context"),
            "Expected nudge about empty parent details"
        );
    }

    #[tokio::test]
    async fn start_feature_no_nudge_when_parent_has_details() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create parent feature set WITH details
        let parent = create_feature(
            &server,
            pid,
            "Authentication",
            Some("JWT tokens with 15-min expiry. Refresh via HTTP-only cookie."),
        )
        .await;
        let parent_id: Uuid = parent.id.into();

        let child =
            create_child_feature(&server, pid, parent_id, "Email Login", Some(GOOD_SPEC)).await;
        let child_id: Uuid = child.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: child_id.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            !has_text_containing(&result, "has no shared context"),
            "Should NOT nudge when parent has details"
        );
    }

    #[tokio::test]
    async fn create_feature_under_parent_shows_feature_set_guidance() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create a parent feature first
        let parent = create_feature(&server, pid, "Authentication", None).await;
        let parent_id: Uuid = parent.id.into();

        // Create a child under it — the parent now becomes a feature set
        let result = features::create_feature(
            &client,
            CreateFeatureRequest {
                project_id: pid,
                parent_id: Some(parent_id),
                title: "Email Login".to_string(),
                details: Some(GOOD_SPEC.to_string()),
                state: "proposed".to_string(),
                priority: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        // Check for feature set guidance about adding shared context
        assert!(
            has_text_containing(&result, "shared context"),
            "Expected feature set guidance when creating child under parent"
        );
    }
}

// ============================================================
// Decision Capture in Completions (MANIF-84)
// ============================================================

mod decision_capture {
    use super::*;
    use manifest::mcp::types::{CommitRefInput, CompleteFeatureRequest, StartFeatureRequest};

    /// Helper: start + prove a feature so it's ready for completion
    async fn start_and_prove(client: &ManifestClient, feature_id: Uuid) {
        features::start_feature(
            client,
            StartFeatureRequest {
                feature_id: feature_id.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        features::prove_feature(
            client,
            manifest::mcp::types::ProveFeatureRequest {
                feature_id: feature_id.to_string(),
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
    }

    #[tokio::test]
    async fn suggests_parent_update_when_summary_contains_decisions() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create parent feature set
        let parent = create_feature(&server, pid, "Authentication", None).await;
        let parent_id: Uuid = parent.id.into();

        // Create child
        let child =
            create_child_feature(&server, pid, parent_id, "Email Login", Some(GOOD_SPEC)).await;
        let child_id: Uuid = child.id.into();

        start_and_prove(&client, child_id).await;

        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: child_id.to_string(),
                summary: "Implemented email login. Discovered that bcrypt rounds must be >=12 for compliance. Decided to use Redis for session storage instead of SQLite.".to_string(),
                commits: vec![CommitRefInput {
                    sha: "abc1234".to_string(),
                    message: "Add email login".to_string(),
                    author: None,
                }],
                backfill: false,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            has_text_containing(&result, "Consider updating parent"),
            "Expected parent context propagation suggestion"
        );
    }

    #[tokio::test]
    async fn no_propagation_when_summary_has_no_decisions() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let parent = create_feature(&server, pid, "Authentication", None).await;
        let parent_id: Uuid = parent.id.into();

        let child =
            create_child_feature(&server, pid, parent_id, "Email Login", Some(GOOD_SPEC)).await;
        let child_id: Uuid = child.id.into();

        start_and_prove(&client, child_id).await;

        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: child_id.to_string(),
                summary: "Implemented email login with standard password hashing".to_string(),
                commits: vec![],
                backfill: false,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            !has_text_containing(&result, "Consider updating parent"),
            "Should NOT suggest propagation for plain summaries"
        );
    }

    #[tokio::test]
    async fn no_propagation_for_root_features() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Root-level feature (no parent feature set)
        let feature = create_feature(&server, pid, "Email Login", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        start_and_prove(&client, fid).await;

        let result = features::complete_feature(
            &client,
            CompleteFeatureRequest {
                feature_id: fid.to_string(),
                summary: "Implemented login. Discovered that bcrypt rounds must be >=12."
                    .to_string(),
                commits: vec![],
                backfill: false,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            !has_text_containing(&result, "Consider updating parent"),
            "Should NOT suggest propagation for root features"
        );
    }
}

// ============================================================
// Context-Budgeted Delivery (MANIF-127)
// ============================================================

mod context_budgeted_delivery {
    use super::*;
    use manifest::mcp::tools::format;
    use manifest::mcp::types::{BreadcrumbItemInfo, GetFeatureRequest, StartFeatureRequest};

    // --- Unit tests for budget_breadcrumb ---

    #[test]
    fn per_level_budget_truncates_long_details() {
        let long_text = "x".repeat(3000);
        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Root".into(),
                details: Some(long_text),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Current".into(),
                details: Some("short".into()),
            },
        ];

        let result = format::budget_breadcrumb(&breadcrumb, 2000, 8000);
        // Root details should be truncated to ~2000 chars
        let root_len = result[0].details.as_ref().unwrap().len();
        assert!(
            root_len <= 2003, // 2000 + "..."
            "Root details should be truncated to ~2000 chars, got {}",
            root_len
        );
        // Current feature details untouched
        assert_eq!(result[1].details.as_deref(), Some("short"));
    }

    #[test]
    fn total_budget_truncates_distant_ancestors_first() {
        // 3 ancestors each with 3000 chars = 9000 total, over 8000 budget
        let text_a = "Para one.\n\n".to_string() + &"a".repeat(2989);
        let text_b = "b".repeat(3000);
        let text_c = "c".repeat(3000);

        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Root".into(),
                details: Some(text_a),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Middle".into(),
                details: Some(text_b),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Parent".into(),
                details: Some(text_c.clone()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Current".into(),
                details: None,
            },
        ];

        // Per-level 3000 (no per-level truncation), total 8000
        let result = format::budget_breadcrumb(&breadcrumb, 3000, 8000);

        // Root (most distant) should be truncated first
        let root_len = result[0].details.as_ref().map_or(0, |d| d.len());
        assert!(
            root_len < 3000,
            "Root should be truncated (got {} chars)",
            root_len
        );
        // Parent (nearest) should keep full details
        assert_eq!(
            result[2].details.as_ref().map(|d| d.len()),
            Some(3000),
            "Parent (nearest ancestor) should keep full details"
        );
    }

    #[test]
    fn empty_breadcrumb_returns_empty() {
        let result = format::budget_breadcrumb(&[], 2000, 8000);
        assert!(result.is_empty());
    }

    #[test]
    fn within_budget_details_preserved() {
        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Root".into(),
                details: Some("Root context with decisions".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Parent".into(),
                details: Some("Parent arch decisions\n\nMore detail here".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Current".into(),
                details: Some("Feature spec".into()),
            },
        ];

        let result = format::budget_breadcrumb(&breadcrumb, 2000, 8000);
        // All within budget — everything preserved
        assert_eq!(
            result[0].details.as_deref(),
            Some("Root context with decisions")
        );
        assert_eq!(
            result[1].details.as_deref(),
            Some("Parent arch decisions\n\nMore detail here")
        );
        assert_eq!(result[2].details.as_deref(), Some("Feature spec"));
    }

    // --- Integration tests for depth parameter ---

    #[tokio::test]
    async fn get_feature_shallow_strips_breadcrumb_and_siblings() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let parent = create_feature(
            &server,
            pid,
            "Auth System",
            Some("JWT auth with refresh tokens"),
        )
        .await;
        let parent_id: Uuid = parent.id.into();

        let child = create_child_feature(&server, pid, parent_id, "Login", Some(GOOD_SPEC)).await;
        let _sibling =
            create_child_feature(&server, pid, parent_id, "Logout", Some("Logout feature")).await;
        let child_id: Uuid = child.id.into();

        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: child_id.to_string(),
                include_history: false,
                depth: Some("shallow".to_string()),
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        // Should have the feature spec
        assert!(has_text_containing(&result, "title: Login"));
        // Breadcrumb should NOT have parent details
        assert!(
            !has_text_containing(&result, "JWT auth with refresh tokens"),
            "Shallow mode should strip breadcrumb details"
        );
        // Should NOT include siblings
        assert!(
            !has_text_containing(&result, "Logout"),
            "Shallow mode should strip siblings"
        );
    }

    #[tokio::test]
    async fn get_feature_deep_includes_history_automatically() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let feature = create_feature(&server, pid, "Deep Feature", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Start, prove, complete to create history
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
                summary: "Built deep feature".to_string(),
                commits: vec![],
                backfill: false,
            },
        )
        .await
        .unwrap();

        // Now get with depth=deep (NOT setting include_history)
        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: fid.to_string(),
                include_history: false,
                depth: Some("deep".to_string()),
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        // Deep mode should include history automatically
        assert!(
            has_text_containing(&result, "Built deep feature"),
            "Deep mode should include history without explicit include_history"
        );
    }

    #[tokio::test]
    async fn start_feature_delivers_ancestor_details_in_breadcrumb() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let parent = create_feature(
            &server,
            pid,
            "Payments",
            Some("All amounts in cents. Use Stripe API v2."),
        )
        .await;
        let parent_id: Uuid = parent.id.into();

        let child =
            create_child_feature(&server, pid, parent_id, "Checkout Flow", Some(GOOD_SPEC)).await;
        let child_id: Uuid = child.id.into();

        let result = features::start_feature(
            &client,
            StartFeatureRequest {
                feature_id: child_id.to_string(),
                agent_type: "claude".to_string(),
                force: false,
                claim_metadata: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        // Breadcrumb should include parent's context details
        assert!(
            has_text_containing(&result, "All amounts in cents"),
            "start_feature breadcrumb should include parent feature set details"
        );
    }

    #[tokio::test]
    async fn get_feature_standard_includes_breadcrumb_details() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let parent = create_feature(
            &server,
            pid,
            "Auth Module",
            Some("Uses JWT with 15min expiry. CORS localhost:5173 in dev."),
        )
        .await;
        let parent_id: Uuid = parent.id.into();

        let child = create_child_feature(
            &server,
            pid,
            parent_id,
            "OAuth Login",
            Some("Google OAuth integration"),
        )
        .await;
        let child_id: Uuid = child.id.into();

        // Standard depth (default) should include breadcrumb details
        let result = features::get_feature(
            &client,
            GetFeatureRequest {
                feature_id: child_id.to_string(),
                include_history: false,
                depth: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            has_text_containing(&result, "Uses JWT with 15min expiry"),
            "Standard depth should include ancestor details in breadcrumb"
        );
    }
}

// ============================================================
// Cross-Branch Context Search (MANIF-163)
// ============================================================

mod cross_branch_context_search {
    use super::*;
    use manifest::mcp::types::FindFeaturesRequest;

    #[tokio::test]
    async fn fts_search_finds_features_by_details_content() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        // Create features with specific details
        create_feature(
            &server,
            pid,
            "Auth Login",
            Some("Uses Redis for session state. JWT tokens with 15min expiry."),
        )
        .await;
        create_feature(
            &server,
            pid,
            "Rate Limiting",
            Some("API rate limits per IP address. No Redis needed here."),
        )
        .await;
        create_feature(&server, pid, "Profile Page", Some("Displays user avatar")).await;

        // FTS search for "Redis" should find both features mentioning Redis
        let result = features::find_features(
            &client,
            FindFeaturesRequest {
                project_id: Some(pid),
                version_id: None,
                state: None,
                query: Some("Redis".to_string()),
                search_mode: Some("full".to_string()),
                limit: None,
                offset: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        let content = texts.join(" ");
        assert!(
            content.contains("Auth Login"),
            "FTS should find feature with Redis in details"
        );
        assert!(
            content.contains("Rate Limiting"),
            "FTS should find second feature with Redis in details"
        );
        assert!(
            !content.contains("Profile Page"),
            "FTS should not find feature without Redis"
        );
    }

    #[tokio::test]
    async fn default_search_mode_uses_like_search() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        create_feature(
            &server,
            pid,
            "Widget Builder",
            Some("Creates complex dashboard widgets"),
        )
        .await;

        // Default (no search_mode) should still work via LIKE
        let result = features::find_features(
            &client,
            FindFeaturesRequest {
                project_id: Some(pid),
                version_id: None,
                state: None,
                query: Some("Widget".to_string()),
                search_mode: None,
                limit: None,
                offset: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        assert!(
            has_text_containing(&result, "Widget Builder"),
            "Default search should find by title via LIKE"
        );
    }

    #[tokio::test]
    async fn fts_search_with_state_filter() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let feature = create_feature(&server, pid, "Implemented Auth", Some(GOOD_SPEC)).await;
        let fid: Uuid = feature.id.into();

        // Start, prove, complete to make it implemented
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
                summary: "Done".to_string(),
                commits: vec![],
                backfill: false,
            },
        )
        .await
        .unwrap();

        create_feature(
            &server,
            pid,
            "Proposed Auth Improvement",
            Some("Better auth with session tokens"),
        )
        .await;

        // FTS search with state=proposed should only find proposed feature
        let result = features::find_features(
            &client,
            FindFeaturesRequest {
                project_id: Some(pid),
                version_id: None,
                state: Some("proposed".to_string()),
                query: Some("auth".to_string()),
                search_mode: Some("full".to_string()),
                limit: None,
                offset: None,
            },
        )
        .await
        .unwrap();

        assert!(result.is_error.is_none() || result.is_error == Some(false));
        let texts = text_contents(&result);
        let content = texts.join(" ");
        assert!(
            content.contains("Proposed Auth Improvement"),
            "Should find proposed feature"
        );
        assert!(
            !content.contains("Implemented Auth"),
            "Should not find implemented feature when filtering by proposed"
        );
    }

    #[tokio::test]
    async fn fts_index_syncs_on_update() {
        let (server, client) = setup().await;
        let project = create_project(&server).await;
        let pid: Uuid = project.id.into();

        let feature = create_feature(
            &server,
            pid,
            "Empty Feature",
            Some("Original content without keywords"),
        )
        .await;
        let fid: Uuid = feature.id.into();

        // Before update: search for "Kubernetes" should return empty
        let result = features::find_features(
            &client,
            FindFeaturesRequest {
                project_id: Some(pid),
                version_id: None,
                state: None,
                query: Some("Kubernetes".to_string()),
                search_mode: Some("full".to_string()),
                limit: None,
                offset: None,
            },
        )
        .await
        .unwrap();
        assert!(
            !has_text_containing(&result, "Empty Feature"),
            "Should not find feature before details mention Kubernetes"
        );

        // Update details to mention Kubernetes
        features::update_feature(
            &client,
            manifest::mcp::types::UpdateFeatureRequest {
                feature_id: fid.to_string(),
                title: None,
                details: Some("Deploy to Kubernetes cluster with Helm charts".to_string()),
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

        // After update: search should now find it
        let result = features::find_features(
            &client,
            FindFeaturesRequest {
                project_id: Some(pid),
                version_id: None,
                state: None,
                query: Some("Kubernetes".to_string()),
                search_mode: Some("full".to_string()),
                limit: None,
                offset: None,
            },
        )
        .await
        .unwrap();
        assert!(
            has_text_containing(&result, "Empty Feature"),
            "FTS index should sync on update — feature should now be found"
        );
    }
}
