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
    /// Kept alive so the temp directory isn't deleted while the process runs.
    _temp_dir: tempfile::TempDir,
}

impl McpTestClient {
    /// Spawn a new MCP server process with an isolated test database
    fn spawn() -> Self {
        // Create temp directory for test database
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");

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

        Self {
            child,
            request_id: 0,
            reader,
            _temp_dir: temp_dir,
        }
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

        // Verify we have a reasonable number of tools (exact count changes as features are added)
        assert!(
            tools_array.len() >= 25,
            "Expected at least 25 tools, got {}",
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
        assert!(tool_names.contains(&"sync"));
        // Setup tools
        assert!(tool_names.contains(&"init_project"));
        assert!(tool_names.contains(&"add_project_directory"));
        assert!(tool_names.contains(&"generate_feature_tree"));
        assert!(tool_names.contains(&"plan"));
        assert!(tool_names.contains(&"create_feature"));
        // Work tools
        assert!(tool_names.contains(&"start_feature"));
        assert!(tool_names.contains(&"complete_feature"));
        assert!(tool_names.contains(&"prove_feature"));
        assert!(tool_names.contains(&"get_feature_proof"));
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

// Tool call and active feature tests removed — they required a live HTTP server
// at localhost:17010 and could never pass in CI. The MCP tool functionality is
// tested via mcp_tools_spec.rs which uses an in-process TestServer.

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
