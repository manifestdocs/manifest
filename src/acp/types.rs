//! JSON-RPC 2.0 + ACP protocol types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================
// JSON-RPC 2.0 core types
// ============================================================

/// A JSON-RPC request ID — either a number or a string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    String(String),
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{}", n),
            Self::String(s) => write!(f, "{}", s),
        }
    }
}

/// An outgoing JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

/// A JSON-RPC 2.0 notification (no `id` field).
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcNotification {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// An outgoing JSON-RPC 2.0 response (for replying to agent-initiated requests).
#[derive(Debug, Serialize)]
pub struct JsonRpcResponseOut {
    pub jsonrpc: &'static str,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponseOut {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: RequestId, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// Classifies an incoming JSON message from the agent process.
///
/// JSON-RPC messages from an agent can be:
/// - A **response** to our request (has `id`, no `method`)
/// - A **notification** from the agent (has `method`, no `id`)
/// - A **request** from the agent to us (has both `id` and `method`)
#[derive(Debug, Deserialize)]
pub struct RawJsonRpcMessage {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<RequestId>,
    pub method: Option<String>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
    #[serde(default)]
    pub params: Option<Value>,
}

/// Classified variant of a raw JSON-RPC message.
pub enum ClassifiedMessage {
    /// Response to a request we sent (has `id`, no `method`).
    Response(JsonRpcResponse),
    /// Notification from the agent (has `method`, no `id`).
    Notification(JsonRpcNotification),
    /// Request from the agent to us (has both `id` and `method`).
    AgentRequest {
        id: RequestId,
        method: String,
        params: Option<Value>,
    },
}

impl RawJsonRpcMessage {
    /// Classify this message based on the presence of `id` and `method`.
    pub fn classify(self) -> ClassifiedMessage {
        match (self.id, self.method) {
            // Has both id and method -> agent-initiated request
            (Some(id), Some(method)) => ClassifiedMessage::AgentRequest {
                id,
                method,
                params: self.params,
            },
            // Has id but no method -> response to our request
            (Some(id), None) => ClassifiedMessage::Response(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: self.result,
                error: self.error,
            }),
            // Has method but no id -> notification
            (None, Some(method)) => ClassifiedMessage::Notification(JsonRpcNotification {
                jsonrpc: "2.0".to_string(),
                method,
                params: self.params,
            }),
            // Neither -> treat as malformed notification
            (None, None) => ClassifiedMessage::Notification(JsonRpcNotification {
                jsonrpc: "2.0".to_string(),
                method: String::new(),
                params: self.params,
            }),
        }
    }
}

// ============================================================
// ACP protocol types
// ============================================================

/// Client info sent during `initialize`.
#[derive(Debug, Serialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// Parameters for the `initialize` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client_info: ClientInfo,
    pub client_capabilities: Value,
}

/// Result of the `initialize` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    #[allow(dead_code)]
    pub protocol_version: Option<u32>,
    #[allow(dead_code)]
    pub agent_info: Option<Value>,
    #[allow(dead_code)]
    pub agent_capabilities: Option<Value>,
}

/// Parameters for `session/new`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionParams {
    pub cwd: String,
    pub mcp_servers: Vec<serde_json::Value>,
}

/// Result of `session/new`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionResult {
    pub session_id: String,
}

/// A content block in a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Resource {
        uri: String,
        mime_type: String,
        text: String,
    },
}

/// Parameters for `session/prompt`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptParams {
    pub session_id: String,
    pub prompt: Vec<ContentBlock>,
}

/// Result of `session/prompt`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResult {
    #[allow(dead_code)]
    pub stop_reason: Option<String>,
}

// ============================================================
// Session update types (from `session/update` notifications)
// ============================================================

/// Parsed session update from a `session/update` notification.
#[derive(Debug, Clone)]
pub enum SessionUpdate {
    /// Streaming text chunk from the agent.
    AgentMessageChunk { text: String },
    /// Agent is calling a tool.
    ToolCall { id: String, title: String },
    /// Progress update on an in-flight tool call.
    ToolCallUpdate { id: String, status: String },
    /// Agent produced a plan (logged, not streamed to SSE).
    Plan { content: String },
}

impl SessionUpdate {
    /// Try to parse a `session/update` notification params into a `SessionUpdate`.
    ///
    /// The notification params have the structure:
    /// `{"sessionId":"...","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"..."}}}`
    pub fn from_notification_params(params: &Value) -> Option<Self> {
        // Navigate to the nested update object
        let update = params.get("update")?;
        let update_type = update.get("sessionUpdate")?.as_str()?;

        match update_type {
            "agent_message_chunk" => {
                // Content is nested: {"type":"text","text":"..."}
                let content = update.get("content")?;
                let text = content.get("text")?.as_str()?.to_string();
                Some(Self::AgentMessageChunk { text })
            }
            "tool_call" => {
                let id = update
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = update
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(Self::ToolCall { id, title })
            }
            "tool_call_update" => {
                let id = update
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let status = update
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(Self::ToolCallUpdate { id, status })
            }
            "plan" => {
                let content = update
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(Self::Plan { content })
            }
            _ => None,
        }
    }
}

// ============================================================
// Permission request (agent -> client)
// ============================================================

/// Parameters for `session/requestPermission` (agent -> client).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct RequestPermissionParams {
    #[allow(dead_code)]
    pub session_id: Option<String>,
    #[allow(dead_code)]
    pub resource: Option<Value>,
    #[allow(dead_code)]
    pub action: Option<String>,
}
