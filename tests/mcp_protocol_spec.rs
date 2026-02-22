//! MCP protocol integration tests.
//!
//! These tests spawn the actual `manifest mcp` process and communicate via
//! JSON-RPC over stdio, testing the complete MCP protocol flow.
//!
//! The rmcp library uses line-delimited JSON (each message is one line):
//! ```
//! {"jsonrpc":"2.0","id":1,"method":"initialize",...}\n
//! {"jsonrpc":"2.0","id":1,"result":{...}}\n
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// JSON-RPC 2.0 request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

/// MCP test client that spawns and communicates with the server
struct McpTestClient {
    child: Child,
    request_id: u64,
    reader: BufReader<std::process::ChildStdout>,
    /// Home directory used by this test instance (for writing context files, etc.)
    home_dir: std::path::PathBuf,
}

impl McpTestClient {
    /// Spawn a new MCP server process with an isolated test database
    fn spawn() -> Self {
        // Create temp directory for test database
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let home_dir = temp_dir.path().to_path_buf();

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_manifest"));
        cmd.arg("mcp")
            .env("XDG_DATA_HOME", temp_dir.path())
            .env("HOME", temp_dir.path()) // For macOS directories crate
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().expect("Failed to spawn mfst mcp");

        let stdout = child.stdout.take().expect("Failed to get stdout");
        let reader = BufReader::new(stdout);

        // Keep temp_dir alive by leaking it (tests are short-lived anyway)
        std::mem::forget(temp_dir);

        Self {
            child,
            request_id: 0,
            reader,
            home_dir,
        }
    }

    /// Get the path where the active context file should be written
    fn context_file_path(&self) -> std::path::PathBuf {
        self.home_dir.join(".manifest").join("active_context.json")
    }

    /// Send a message as line-delimited JSON
    fn send_message(&mut self, content: &str) {
        let stdin = self.child.stdin.as_mut().expect("Failed to get stdin");
        writeln!(stdin, "{}", content).expect("Failed to write message");
        stdin.flush().expect("Failed to flush stdin");
    }

    /// Read a message as line-delimited JSON
    fn read_message(&mut self) -> String {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("Failed to read line");
        line.trim().to_string()
    }

    /// Send a JSON-RPC request and get the response
    fn request(&mut self, method: &str, params: Option<Value>) -> JsonRpcResponse {
        self.request_id += 1;
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: self.request_id,
            method: method.to_string(),
            params,
        };

        let request_json = serde_json::to_string(&request).expect("Failed to serialize request");
        self.send_message(&request_json);

        let response_json = self.read_message();
        serde_json::from_str(&response_json).expect("Failed to parse response")
    }

    /// Send initialize request and initialized notification (required first messages)
    fn initialize(&mut self) -> JsonRpcResponse {
        let response = self.request(
            "initialize",
            Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
        );

        // Send initialized notification (required by MCP protocol)
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.send_message(&notification.to_string());

        response
    }

    /// List available tools
    fn list_tools(&mut self) -> JsonRpcResponse {
        self.request("tools/list", None)
    }

    /// Call a tool with parameters
    fn call_tool(&mut self, name: &str, arguments: Value) -> JsonRpcResponse {
        self.request(
            "tools/call",
            Some(json!({
                "name": name,
                "arguments": arguments
            })),
        )
    }
}

impl Drop for McpTestClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ============================================================
// Protocol Tests
// ============================================================

mod protocol {
    use super::*;

    #[test]
    fn initialize_returns_server_info() {
        let mut client = McpTestClient::spawn();
        let response = client.initialize();

        assert!(response.error.is_none(), "Expected success, got error");
        let result = response.result.expect("Expected result");

        // Check server info
        assert!(result.get("serverInfo").is_some());
        assert!(result.get("capabilities").is_some());
    }

    #[test]
    fn tools_list_returns_all_tools() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        let response = client.list_tools();
        assert!(response.error.is_none(), "Expected success, got error");

        let result = response.result.expect("Expected result");
        let tools = result.get("tools").expect("Expected tools array");
        let tools_array = tools.as_array().expect("Tools should be array");

        // We have 23 tools (added verify_feature + record_verification)
        assert_eq!(
            tools_array.len(),
            23,
            "Expected 23 tools, got {}",
            tools_array.len()
        );

        // Verify tool names
        let tool_names: Vec<&str> = tools_array
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();

        // Discovery tools
        assert!(tool_names.contains(&"list_projects"));
        assert!(tool_names.contains(&"get_project_instructions"));
        assert!(tool_names.contains(&"get_active_feature"));
        assert!(tool_names.contains(&"find_features"));
        assert!(tool_names.contains(&"get_feature"));
        assert!(tool_names.contains(&"render_feature_tree"));
        assert!(tool_names.contains(&"get_project_history"));
        // Setup tools
        assert!(tool_names.contains(&"init_project"));
        assert!(tool_names.contains(&"add_project_directory"));
        assert!(tool_names.contains(&"generate_feature_tree"));
        assert!(tool_names.contains(&"plan"));
        assert!(tool_names.contains(&"create_feature"));
        // Work tools
        assert!(tool_names.contains(&"start_feature"));
        assert!(tool_names.contains(&"complete_feature"));
        assert!(tool_names.contains(&"delete_feature"));
        assert!(tool_names.contains(&"get_next_feature"));
        // Version tools
        assert!(tool_names.contains(&"list_versions"));
        assert!(tool_names.contains(&"create_version"));
        assert!(tool_names.contains(&"set_feature_version"));
        assert!(tool_names.contains(&"release_version"));
    }

    #[test]
    fn tools_have_descriptions_and_schemas() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        let response = client.list_tools();
        let result = response.result.expect("Expected result");
        let tools = result
            .get("tools")
            .expect("Expected tools")
            .as_array()
            .expect("Tools should be array");

        for tool in tools {
            let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("?");
            assert!(
                tool.get("description").is_some(),
                "Tool {} missing description",
                name
            );
            assert!(
                tool.get("inputSchema").is_some(),
                "Tool {} missing inputSchema",
                name
            );
        }
    }
}

// ============================================================
// Tool Call Tests
// ============================================================

mod tool_calls {
    use super::*;

    // NOTE: These tests require the HTTP server to be running at localhost:17010.
    // The MCP server makes HTTP calls to the API, so tests fail without the server.
    // Mark as #[ignore] for CI - run locally with `cargo test -- --ignored` after starting server.

    #[test]
    #[ignore = "Requires HTTP server at localhost:17010"]
    fn init_project_succeeds() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Use a temp directory for init_project
        let temp_dir = std::env::temp_dir().join("manifest-test-project");
        std::fs::create_dir_all(&temp_dir).ok();

        let response = client.call_tool(
            "init_project",
            json!({
                "directory_path": temp_dir.to_string_lossy()
            }),
        );

        assert!(response.error.is_none(), "Expected success, got error");
        let result = response.result.expect("Expected result");

        // MCP tool results have content array
        let content = result.get("content").expect("Expected content");
        let text = content
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .expect("Expected text content");

        let response: Value = serde_json::from_str(text).expect("Expected JSON in text");
        let project = response.get("project").expect("Expected project");
        assert!(project.get("id").is_some());
        assert!(project.get("name").is_some());
    }

    #[test]
    #[ignore = "Requires HTTP server at localhost:17010"]
    fn create_feature_and_find_features() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Create project first via init_project
        let temp_dir = std::env::temp_dir().join("manifest-feature-test");
        std::fs::create_dir_all(&temp_dir).ok();
        let project_response = client.call_tool(
            "init_project",
            json!({ "directory_path": temp_dir.to_string_lossy() }),
        );
        let project_text = extract_text_content(&project_response);
        let response: Value = serde_json::from_str(&project_text).unwrap();
        let project_id = response["project"]["id"].as_str().unwrap();

        // Create feature
        let feature_response = client.call_tool(
            "create_feature",
            json!({
                "project_id": project_id,
                "title": "User Authentication",
                "details": "As a user, I want to log in",
                "state": "in_progress"
            }),
        );
        assert!(feature_response.error.is_none());

        let feature_text = extract_text_content(&feature_response);
        let feature: Value = serde_json::from_str(&feature_text).unwrap();
        assert_eq!(
            feature.get("title").and_then(|t| t.as_str()),
            Some("User Authentication")
        );

        // Find features
        let list_response = client.call_tool("find_features", json!({ "project_id": project_id }));
        assert!(list_response.error.is_none());

        let list_text = extract_text_content(&list_response);
        let list: Value = serde_json::from_str(&list_text).unwrap();
        let features = list.get("features").and_then(|f| f.as_array()).unwrap();
        assert_eq!(features.len(), 1);
    }

    #[test]
    #[ignore = "Requires HTTP server; also uses session tools that no longer exist"]
    fn full_session_workflow() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Setup: create project and feature via init_project
        let temp_dir = std::env::temp_dir().join("manifest-workflow-test");
        std::fs::create_dir_all(&temp_dir).ok();
        let project_text = extract_text_content(&client.call_tool(
            "init_project",
            json!({ "directory_path": temp_dir.to_string_lossy() }),
        ));
        let response: Value = serde_json::from_str(&project_text).unwrap();
        let project_id = response["project"]["id"].as_str().unwrap();

        let feature_text = extract_text_content(&client.call_tool(
            "create_feature",
            json!({
                "project_id": project_id,
                "title": "Test Feature",
                "state": "in_progress"
            }),
        ));
        let feature: Value = serde_json::from_str(&feature_text).unwrap();
        let feature_id = feature["id"].as_str().unwrap();

        // 1. Create session
        let session_text = extract_text_content(&client.call_tool(
            "create_session",
            json!({
                "feature_id": feature_id,
                "goal": "Implement the feature"
            }),
        ));
        let session: Value = serde_json::from_str(&session_text).unwrap();
        let session_id = session["id"].as_str().unwrap();
        assert_eq!(session["status"].as_str(), Some("active"));

        // 2. Create task
        let task_text = extract_text_content(&client.call_tool(
            "create_task",
            json!({
                "session_id": session_id,
                "title": "Write tests",
                "scope": "Add unit tests for feature",
                "agent_type": "claude"
            }),
        ));
        let task: Value = serde_json::from_str(&task_text).unwrap();
        let task_id = task["id"].as_str().unwrap();
        assert_eq!(task["status"].as_str(), Some("pending"));

        // 3. Get task context
        let context_text = extract_text_content(
            &client.call_tool("get_task_context", json!({ "task_id": task_id })),
        );
        let context: Value = serde_json::from_str(&context_text).unwrap();
        assert_eq!(context["task"]["title"].as_str(), Some("Write tests"));
        assert_eq!(context["feature"]["title"].as_str(), Some("Test Feature"));

        // 4. Start task
        let start_response = client.call_tool("start_task", json!({ "task_id": task_id }));
        assert!(start_response.error.is_none());

        // 5. Complete task
        let complete_response = client.call_tool("complete_task", json!({ "task_id": task_id }));
        assert!(complete_response.error.is_none());

        // 6. Complete session
        let complete_session_text = extract_text_content(&client.call_tool(
            "complete_session",
            json!({
                "session_id": session_id,
                "summary": "Implemented tests",
                "files_changed": ["tests/feature_spec.rs"],
                "mark_implemented": true
            }),
        ));
        let complete_session: Value = serde_json::from_str(&complete_session_text).unwrap();
        assert_eq!(
            complete_session["feature_state"].as_str(),
            Some("implemented")
        );

        // 7. Verify feature state changed
        let get_feature_text = extract_text_content(
            &client.call_tool("get_feature", json!({ "feature_id": feature_id })),
        );
        let updated_feature: Value = serde_json::from_str(&get_feature_text).unwrap();
        assert_eq!(updated_feature["state"].as_str(), Some("implemented"));
    }

    #[test]
    #[ignore = "Requires HTTP server at localhost:17010"]
    fn add_directory_and_get_project_context() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Create project via init_project with first directory
        let temp_dir1 = std::env::temp_dir().join("manifest-dir-test-1");
        let temp_dir2 = std::env::temp_dir().join("manifest-dir-test-2");
        std::fs::create_dir_all(&temp_dir1).ok();
        std::fs::create_dir_all(&temp_dir2).ok();

        let project_text = extract_text_content(&client.call_tool(
            "init_project",
            json!({ "directory_path": temp_dir1.to_string_lossy() }),
        ));
        let response: Value = serde_json::from_str(&project_text).unwrap();
        let project_id = response["project"]["id"].as_str().unwrap();

        // Add second directory
        let dir_response = client.call_tool(
            "add_project_directory",
            json!({
                "project_id": project_id,
                "path": temp_dir2.to_string_lossy(),
                "is_primary": false,
                "instructions": "cargo test"
            }),
        );
        assert!(dir_response.error.is_none());

        // Get project context for second directory
        let context_text = extract_text_content(&client.call_tool(
            "get_project_context",
            json!({ "directory_path": temp_dir2.to_string_lossy() }),
        ));
        let context: Value = serde_json::from_str(&context_text).unwrap();
        assert!(context["project"]["id"].as_str().is_some());
        assert_eq!(
            context["directory"]["path"].as_str(),
            Some(temp_dir2.to_string_lossy().as_ref())
        );
    }

    /// Helper to extract text content from MCP tool response
    pub fn extract_text_content(response: &JsonRpcResponse) -> String {
        response
            .result
            .as_ref()
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .expect("Expected text content in response")
            .to_string()
    }
}

// ============================================================
// Active Feature Context Tests
// ============================================================

mod active_feature {
    use super::*;
    use std::fs;

    // Note: get_active_feature is an IDE-only tool
    // FIXME: Tests write to home_dir/.manifest/ but tool reads from current_dir()/.manifest/
    // Need to fix path handling to make these tests work correctly.

    #[test]
    #[ignore = "Path mismatch: tests write to home_dir, tool reads from current_dir"]
    fn returns_null_when_no_context_file() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        let response = client.call_tool("get_active_feature", json!({}));
        assert!(response.error.is_none(), "Expected success, got error");

        let text = tool_calls::extract_text_content(&response);
        let result: Value = serde_json::from_str(&text).expect("Expected JSON");

        assert!(
            result.get("active_feature").unwrap().is_null(),
            "Expected active_feature to be null"
        );
        assert!(result.get("message").is_some(), "Expected message field");
    }

    #[test]
    #[ignore = "Path mismatch: tests write to home_dir, tool reads from current_dir"]
    fn returns_feature_when_context_file_exists() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Write context file to the test's home directory
        let context_path = client.context_file_path();
        fs::create_dir_all(context_path.parent().unwrap()).unwrap();
        let context = json!({
            "feature_id": "550e8400-e29b-41d4-a716-446655440000",
            "title": "Test Feature",
            "updated_at": "2024-01-15T10:30:00Z"
        });
        fs::write(
            &context_path,
            serde_json::to_string_pretty(&context).unwrap(),
        )
        .unwrap();

        let response = client.call_tool("get_active_feature", json!({}));
        assert!(response.error.is_none(), "Expected success, got error");

        let text = tool_calls::extract_text_content(&response);
        let result: Value = serde_json::from_str(&text).expect("Expected JSON");

        let active = result
            .get("active_feature")
            .expect("Expected active_feature");
        assert!(!active.is_null(), "Expected active_feature to not be null");
        assert_eq!(
            active.get("feature_id").and_then(|f| f.as_str()),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert_eq!(
            active.get("title").and_then(|t| t.as_str()),
            Some("Test Feature")
        );
    }

    #[test]
    #[ignore = "Path mismatch: tests write to home_dir, tool reads from current_dir"]
    fn returns_error_for_invalid_json_in_context_file() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Write invalid JSON to context file
        let context_path = client.context_file_path();
        fs::create_dir_all(context_path.parent().unwrap()).unwrap();
        fs::write(&context_path, "not valid json").unwrap();

        let response = client.call_tool("get_active_feature", json!({}));

        // Should return an error (either in error field or isError in result)
        let is_error = response.error.is_some()
            || response
                .result
                .as_ref()
                .and_then(|r| r.get("isError"))
                .and_then(|e| e.as_bool())
                .unwrap_or(false);

        assert!(is_error, "Expected error for invalid JSON");
    }

    #[test]
    #[ignore = "Path mismatch: tests write to home_dir, tool reads from current_dir"]
    fn preserves_all_context_fields() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // Write context with extra fields
        let context_path = client.context_file_path();
        fs::create_dir_all(context_path.parent().unwrap()).unwrap();
        let context = json!({
            "feature_id": "123e4567-e89b-12d3-a456-426614174000",
            "title": "Feature with Details",
            "updated_at": "2024-01-15T12:00:00Z",
            "extra_field": "should be preserved"
        });
        fs::write(
            &context_path,
            serde_json::to_string_pretty(&context).unwrap(),
        )
        .unwrap();

        let response = client.call_tool("get_active_feature", json!({}));
        let text = tool_calls::extract_text_content(&response);
        let result: Value = serde_json::from_str(&text).expect("Expected JSON");

        let active = result.get("active_feature").unwrap();
        assert_eq!(
            active.get("extra_field").and_then(|f| f.as_str()),
            Some("should be preserved")
        );
    }
}

// ============================================================
// Error Handling Tests
// ============================================================

mod errors {
    use super::*;

    #[test]
    fn invalid_tool_name_returns_error() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        let response = client.call_tool("nonexistent_tool", json!({}));

        assert!(response.error.is_some(), "Expected error for invalid tool");
    }

    #[test]
    fn invalid_uuid_returns_error() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        let response = client.call_tool("get_feature", json!({ "feature_id": "not-a-uuid" }));

        assert!(
            response.error.is_some() || {
                // Some implementations return error in result
                response
                    .result
                    .as_ref()
                    .and_then(|r| r.get("isError"))
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false)
            }
        );
    }

    #[test]
    fn missing_required_param_returns_error() {
        let mut client = McpTestClient::spawn();
        client.initialize();

        // init_project requires 'directory_path'
        let response = client.call_tool("init_project", json!({}));

        assert!(
            response.error.is_some() || {
                response
                    .result
                    .as_ref()
                    .and_then(|r| r.get("isError"))
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false)
            }
        );
    }
}
