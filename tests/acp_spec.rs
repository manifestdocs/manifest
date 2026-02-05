//! ACP (Agent Client Protocol) integration tests.
//!
//! These tests spawn Claude Code via ACP and verify the chat flow works correctly.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// JSON-RPC 2.0 request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC 2.0 response (can be result or error)
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

/// JSON-RPC 2.0 notification (no id)
#[derive(Debug, Deserialize)]
struct JsonRpcNotification {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// Raw message that could be response or notification
#[derive(Debug, Deserialize)]
struct RawMessage {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    method: Option<String>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

/// ACP test client that spawns and communicates with Claude Code
struct AcpTestClient {
    child: Child,
    request_id: u64,
    reader: BufReader<std::process::ChildStdout>,
}

impl AcpTestClient {
    /// Spawn Claude Code in ACP mode
    fn spawn() -> Result<Self, String> {
        let mut cmd = Command::new("claude-code-acp");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn claude --acp: {}", e))?;

        let stdout = child.stdout.take().expect("Failed to get stdout");
        let reader = BufReader::new(stdout);

        Ok(Self {
            child,
            request_id: 0,
            reader,
        })
    }

    /// Send a JSON-RPC request and wait for response
    fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value, String> {
        self.request_id += 1;
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: self.request_id,
            method: method.to_string(),
            params,
        };

        let stdin = self.child.stdin.as_mut().expect("Failed to get stdin");
        let json = serde_json::to_string(&req).unwrap();
        eprintln!(">>> {}", json);
        writeln!(stdin, "{}", json).map_err(|e| format!("Failed to write: {}", e))?;
        stdin
            .flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;

        // Read response (skip notifications)
        self.read_response(self.request_id)
    }

    /// Read until we get a response with the expected ID
    fn read_response(&mut self, expected_id: u64) -> Result<Value, String> {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => return Err("EOF".to_string()),
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    eprintln!("<<< {}", line);

                    let msg: RawMessage = serde_json::from_str(line)
                        .map_err(|e| format!("Parse error: {} in '{}'", e, line))?;

                    // Check if it's a response (has id) or notification (has method, no id)
                    if let Some(id) = msg.id {
                        if id == expected_id {
                            if let Some(err) = msg.error {
                                return Err(format!(
                                    "JSON-RPC error {}: {} {:?}",
                                    err.code, err.message, err.data
                                ));
                            }
                            return Ok(msg.result.unwrap_or(Value::Null));
                        }
                    } else if let Some(method) = &msg.method {
                        eprintln!("    [notification: {}]", method);
                        // Continue reading for the actual response
                    }
                }
                Err(e) => return Err(format!("Read error: {}", e)),
            }
        }
    }

    /// Read notifications until timeout, collecting session/update events
    fn read_notifications(&mut self, timeout: Duration) -> Vec<JsonRpcNotification> {
        let mut notifications = Vec::new();
        let start = std::time::Instant::now();

        // Set non-blocking... actually this is tricky with BufReader
        // For now, just read what's available
        while start.elapsed() < timeout {
            let mut line = String::new();
            // This will block, but for testing we'll just collect a few
            match self.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    eprintln!("<<< {}", line);

                    if let Ok(notif) = serde_json::from_str::<JsonRpcNotification>(line) {
                        notifications.push(notif);
                    }
                }
                Err(_) => break,
            }
        }

        notifications
    }
}

impl Drop for AcpTestClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn test_acp_initialize() {
    let mut client = match AcpTestClient::spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Skipping test: {}", e);
            return;
        }
    };

    // Step 1: Initialize
    let result = client
        .request(
            "initialize",
            Some(json!({
                "protocolVersion": 1,
                "clientInfo": {
                    "name": "manifest-test",
                    "version": "0.1.0"
                }
            })),
        )
        .expect("Initialize failed");

    eprintln!("Initialize result: {:?}", result);
    assert!(
        result.get("protocolVersion").is_some(),
        "Should have protocolVersion"
    );
}

#[test]
fn test_acp_session_new() {
    let mut client = match AcpTestClient::spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Skipping test: {}", e);
            return;
        }
    };

    // Step 1: Initialize
    client
        .request(
            "initialize",
            Some(json!({
                "protocolVersion": 1,
                "clientInfo": {
                    "name": "manifest-test",
                    "version": "0.1.0"
                }
            })),
        )
        .expect("Initialize failed");

    // Step 2: Create session
    let result = client
        .request(
            "session/new",
            Some(json!({
                "cwd": "/tmp",
                "mcpServers": []
            })),
        )
        .expect("session/new failed");

    eprintln!("Session result: {:?}", result);
    assert!(result.get("sessionId").is_some(), "Should have sessionId");
}

#[test]
fn test_acp_prompt() {
    let mut client = match AcpTestClient::spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Skipping test: {}", e);
            return;
        }
    };

    // Step 1: Initialize
    client
        .request(
            "initialize",
            Some(json!({
                "protocolVersion": 1,
                "clientInfo": {
                    "name": "manifest-test",
                    "version": "0.1.0"
                }
            })),
        )
        .expect("Initialize failed");

    // Step 2: Create session
    let session_result = client
        .request(
            "session/new",
            Some(json!({
                "cwd": "/tmp",
                "mcpServers": []
            })),
        )
        .expect("session/new failed");

    let session_id = session_result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .expect("No sessionId");

    eprintln!("Session ID: {}", session_id);

    // Step 3: Send prompt
    let prompt_result = client.request(
        "session/prompt",
        Some(json!({
            "sessionId": session_id,
            "prompt": [{
                "type": "text",
                "text": "Say 'hello test' and nothing else."
            }]
        })),
    );

    match prompt_result {
        Ok(result) => {
            eprintln!("Prompt result: {:?}", result);
        }
        Err(e) => {
            eprintln!("Prompt error: {}", e);
            panic!("Prompt failed: {}", e);
        }
    }
}
