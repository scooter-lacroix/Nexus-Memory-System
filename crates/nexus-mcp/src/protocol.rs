//! MCP protocol types following the Model Context Protocol specification
//!
//! This module implements the JSON-RPC 2.0 based MCP protocol types for
//! communication between MCP clients and servers.

use serde::{Deserialize, Serialize};

// =============================================================================
// JSON-RPC Base Types
// =============================================================================

/// JSON-RPC 2.0 version
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC Request ID type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
}

impl Default for RequestId {
    fn default() -> Self {
        Self::Number(0)
    }
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

// =============================================================================
// MCP Server Information
// =============================================================================

/// Server implementation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
}

impl Implementation {
    /// Create new implementation info
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl Default for Implementation {
    fn default() -> Self {
        Self {
            name: "nexus-memory".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Server capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    /// Experimental capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Map<String, serde_json::Value>>,
    /// Tool capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    /// Resource capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    /// Prompt capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
}

/// Tool capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    /// Whether the server supports list_changed notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Resource capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapability {
    /// Whether the server supports subscribe/unsubscribe
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Whether the server supports list_changed notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Prompt capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsCapability {
    /// Whether the server supports list_changed notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

// =============================================================================
// MCP Initialize Types
// =============================================================================

/// Initialize request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// Protocol version the client supports
    pub protocol_version: String,
    /// Client capabilities
    pub capabilities: ClientCapabilities,
    /// Client implementation info
    pub client_info: Implementation,
}

/// Client capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    /// Experimental capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<serde_json::Map<String, serde_json::Value>>,
    /// Root capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
    /// Sampling capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Roots capability for listing roots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsCapability {
    /// Whether the client supports list_changed notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Initialize result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the server uses
    pub protocol_version: String,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
    /// Server implementation info
    pub server_info: Implementation,
    /// Instructions for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl Default for InitializeResult {
    fn default() -> Self {
        Self {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability { list_changed: Some(false) }),
                resources: Some(ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(false),
                }),
                prompts: Some(PromptsCapability { list_changed: Some(false) }),
                experimental: None,
            },
            server_info: Implementation::default(),
            instructions: Some(
                "Nexus Memory System MCP Server - Store and search memories across agent namespaces. Use tool_help or tool_schema if you need tool usage details or input schemas."
                    .to_string(),
            ),
        }
    }
}

// =============================================================================
// MCP Tool Types
// =============================================================================

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Input schema (JSON Schema)
    pub input_schema: serde_json::Value,
}

impl Tool {
    /// Create a new tool definition
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    /// Set input schema
    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = schema;
        self
    }
}

/// List tools result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// List of tools
    pub tools: Vec<Tool>,
    /// Next cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Tool call request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    /// Tool name
    pub name: String,
    /// Tool arguments
    #[serde(default)]
    pub arguments: serde_json::Map<String, serde_json::Value>,
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    /// Result content
    pub content: Vec<ContentBlock>,
    /// Whether the tool call resulted in an error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl CallToolResult {
    /// Create a successful result with text content
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(content)],
            is_error: None,
        }
    }

    /// Create a successful result with JSON content
    pub fn json(value: serde_json::Value) -> Self {
        Self {
            content: vec![ContentBlock::resource_json("result", value)],
            is_error: None,
        }
    }

    /// Create an error result
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(message)],
            is_error: Some(true),
        }
    }
}

/// Content block for tool results
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// Text content
    #[serde(rename = "text")]
    Text { text: String },
    /// Image content
    #[serde(rename = "image")]
    Image { data: String, mime_type: String },
    /// Resource content
    #[serde(rename = "resource")]
    Resource { resource: ResourceContents },
}

impl ContentBlock {
    /// Create a text content block
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            text: content.into(),
        }
    }

    /// Create a JSON resource content block
    pub fn resource_json(uri: impl Into<String>, value: serde_json::Value) -> Self {
        Self::Resource {
            resource: ResourceContents {
                uri: uri.into(),
                mime_type: Some("application/json".to_string()),
                text: Some(serde_json::to_string(&value).unwrap_or_default()),
                blob: None,
            },
        }
    }
}

// =============================================================================
// MCP Resource Types
// =============================================================================

/// Resource definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Resource URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Resource MIME type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl Resource {
    /// Create a new resource
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            description: None,
            mime_type: None,
        }
    }

    /// Add description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add MIME type
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

/// Resource template for parameterized resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    /// Template URI pattern
    pub uri_template: String,
    /// Template name
    pub name: String,
    /// Template description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Template MIME type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// List resources result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesResult {
    /// List of resources
    pub resources: Vec<Resource>,
    /// Next cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Read resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceParams {
    /// Resource URI
    pub uri: String,
}

/// Read resource result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// Resource contents
    pub contents: Vec<ResourceContents>,
}

/// Resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    /// Resource URI
    pub uri: String,
    /// MIME type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text content (if text)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Binary content (if binary, base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

impl ResourceContents {
    /// Create text resource contents
    pub fn text(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some("text/plain".to_string()),
            text: Some(text.into()),
            blob: None,
        }
    }

    /// Create JSON resource contents
    pub fn json(uri: impl Into<String>, value: &serde_json::Value) -> Self {
        Self {
            uri: uri.into(),
            mime_type: Some("application/json".to_string()),
            text: Some(serde_json::to_string_pretty(value).unwrap_or_default()),
            blob: None,
        }
    }
}

// =============================================================================
// MCP Prompt Types
// =============================================================================

/// Prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// Prompt name
    pub name: String,
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Prompt arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
}

/// Prompt argument definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Argument name
    pub name: String,
    /// Argument description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether argument is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// List prompts result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPromptsResult {
    /// List of prompts
    pub prompts: Vec<Prompt>,
    /// Next cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Get prompt request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptParams {
    /// Prompt name
    pub name: String,
    /// Prompt arguments (string values)
    #[serde(default)]
    pub arguments: std::collections::HashMap<String, String>,
}

/// Prompt message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    /// Message role
    pub role: String,
    /// Message content
    pub content: ContentBlock,
}

impl PromptMessage {
    /// Create a user message
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: ContentBlock::text(text),
        }
    }

    /// Create an assistant message
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: ContentBlock::text(text),
        }
    }
}

/// Get prompt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptResult {
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Prompt messages
    pub messages: Vec<PromptMessage>,
}

// =============================================================================
// JSON-RPC Messages
// =============================================================================

/// JSON-RPC Request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Request method
    pub method: String,
    /// Request parameters
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    /// Request ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
}

impl JsonRpcRequest {
    /// Create a new request
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params: None,
            id: None,
        }
    }

    /// Create a notification (no ID)
    pub fn notification(method: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params: None,
            id: None,
        }
    }

    /// Add parameters
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = Some(params);
        self
    }

    /// Add request ID
    pub fn with_id(mut self, id: RequestId) -> Self {
        self.id = Some(id);
        self
    }

    /// Check if this is a notification (no response expected)
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC Response message (success)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Response result
    pub result: serde_json::Value,
    /// Request ID
    pub id: RequestId,
}

impl JsonRpcResponse {
    /// Create a new response
    pub fn new(id: RequestId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result,
            id,
        }
    }
}

/// JSON-RPC Error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Error details
    pub error: JsonRpcError,
    /// Request ID
    pub id: Option<RequestId>,
}

/// JSON-RPC Error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Parse error (-32700)
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
            data: None,
        }
    }

    /// Invalid request (-32600)
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
            data: None,
        }
    }

    /// Method not found (-32601)
    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method.into()),
            data: None,
        }
    }

    /// Invalid params (-32602)
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    /// Internal error (-32603)
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }
}

/// JSON-RPC message (union type)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// Request message
    Request(JsonRpcRequest),
    /// Success response
    Response(JsonRpcResponse),
    /// Error response
    Error(JsonRpcErrorResponse),
}

// =============================================================================
// Legacy Compatibility Types (for backward compatibility)
// =============================================================================

/// MCP request (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    /// Request method
    pub method: String,
    /// Request parameters
    #[serde(default)]
    pub params: serde_json::Value,
    /// Request ID
    #[serde(default)]
    pub id: Option<i64>,
}

impl McpRequest {
    /// Create a new request
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            params: serde_json::Value::Null,
            id: None,
        }
    }

    /// Add parameters
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }

    /// Add request ID
    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }
}

/// MCP response (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    /// Response result (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Response error (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
    /// Request ID (echoed back)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
}

impl McpResponse {
    /// Create a successful response
    pub fn success(result: serde_json::Value) -> Self {
        Self {
            result: Some(result),
            error: None,
            id: None,
        }
    }

    /// Create an error response
    pub fn error(error: McpError) -> Self {
        Self {
            result: None,
            error: Some(error),
            id: None,
        }
    }

    /// Add request ID
    pub fn with_id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    /// Check if response is successful
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// MCP error (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl McpError {
    /// Create a new error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Add error data
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Parse error (-32700)
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(-32700, message)
    }

    /// Invalid request (-32600)
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(-32600, message)
    }

    /// Method not found (-32601)
    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::new(-32601, format!("Method not found: {}", method.into()))
    }

    /// Invalid params (-32602)
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(-32602, message)
    }

    /// Internal error (-32603)
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(-32603, message)
    }

    /// Not found (-32001)
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(-32001, message)
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "McpError(code={}, message={})", self.code, self.message)
    }
}

impl std::error::Error for McpError {}

// Legacy type aliases
pub type McpTool = Tool;
pub type McpResource = Resource;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id() {
        let id_num = RequestId::from(42);
        assert_eq!(id_num, RequestId::Number(42));

        let id_str = RequestId::from("abc".to_string());
        assert_eq!(id_str, RequestId::String("abc".to_string()));
    }

    #[test]
    fn test_implementation_default() {
        let impl_info = Implementation::default();
        assert_eq!(impl_info.name, "nexus-memory");
    }

    #[test]
    fn test_initialize_result_default() {
        let result = InitializeResult::default();
        assert_eq!(result.protocol_version, "2024-11-05");
        assert!(result.capabilities.tools.is_some());
        assert!(result.capabilities.resources.is_some());
    }

    #[test]
    fn test_tool_new() {
        let tool = Tool::new("store_memory", "Store a memory");
        assert_eq!(tool.name, "store_memory");
        assert_eq!(tool.description, "Store a memory");
    }

    #[test]
    fn test_call_tool_result_text() {
        let result = CallToolResult::text("Hello");
        assert_eq!(result.content.len(), 1);
        assert!(result.is_error.is_none());
    }

    #[test]
    fn test_call_tool_result_error() {
        let result = CallToolResult::error("Something went wrong");
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_content_block_text() {
        let block = ContentBlock::text("test content");
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "test content"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_resource_new() {
        let resource = Resource::new("memory://1", "Memory 1");
        assert_eq!(resource.uri, "memory://1");
        assert_eq!(resource.name, "Memory 1");
    }

    #[test]
    fn test_json_rpc_request_new() {
        let request = JsonRpcRequest::new("test_method");
        assert_eq!(request.method, "test_method");
        assert!(request.is_notification());
    }

    #[test]
    fn test_json_rpc_error_codes() {
        let parse = JsonRpcError::parse_error("test");
        assert_eq!(parse.code, -32700);

        let method = JsonRpcError::method_not_found("unknown");
        assert_eq!(method.code, -32601);
    }

    #[test]
    fn test_legacy_mcp_request_new() {
        let request = McpRequest::new("test_method");
        assert_eq!(request.method, "test_method");
        assert!(request.id.is_none());
    }

    #[test]
    fn test_legacy_mcp_response_success() {
        let response = McpResponse::success(serde_json::json!({"result": "ok"}));
        assert!(response.is_success());
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_legacy_mcp_response_error() {
        let response = McpResponse::error(McpError::invalid_params("Missing parameter"));
        assert!(!response.is_success());
        assert!(response.result.is_none());
        assert!(response.error.is_some());
    }

    #[test]
    fn test_legacy_mcp_error_codes() {
        let parse = McpError::parse_error("test");
        assert_eq!(parse.code, -32700);

        let not_found = McpError::not_found("test");
        assert_eq!(not_found.code, -32001);
    }

    #[test]
    fn test_prompt_message() {
        let user_msg = PromptMessage::user("Hello");
        assert_eq!(user_msg.role, "user");

        let assistant_msg = PromptMessage::assistant("Hi there");
        assert_eq!(assistant_msg.role, "assistant");
    }

    #[test]
    fn test_resource_contents() {
        let text = ResourceContents::text("memory://1", "content");
        assert_eq!(text.uri, "memory://1");
        assert_eq!(text.text, Some("content".to_string()));

        let json = ResourceContents::json("memory://2", &serde_json::json!({"key": "value"}));
        assert_eq!(json.mime_type, Some("application/json".to_string()));
    }

    #[test]
    fn test_serialization() {
        let request = JsonRpcRequest::new("initialize")
            .with_params(serde_json::json!({"protocolVersion": "2024-11-05"}))
            .with_id(RequestId::from(1));

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"initialize\""));
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }
}
