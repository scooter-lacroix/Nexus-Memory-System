//! Integration tests for nexus-mcp
//!
//! These tests verify the MCP server functionality end-to-end.

use nexus_memory_mcp::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpConfig, RequestId};
use serde_json::json;

/// Helper to create a basic initialize request
fn create_init_request() -> JsonRpcRequest {
    JsonRpcRequest::new("initialize")
        .with_params(json!({
            "protocol_version": "2024-11-05",
            "capabilities": {},
            "client_info": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }))
        .with_id(RequestId::from(1))
}

#[test]
fn test_protocol_types_serialization() {
    // Test request serialization
    let request = create_init_request();
    let json = serde_json::to_string(&request).unwrap();

    assert!(json.contains("\"method\":\"initialize\""));
    assert!(json.contains("\"jsonrpc\":\"2.0\""));

    // Test deserialization
    let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.method, "initialize");
}

#[test]
fn test_response_serialization() {
    let response = JsonRpcResponse::new(RequestId::from(1), json!({"status": "ok"}));

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"result\""));
    assert!(json.contains("\"status\":\"ok\""));
}

#[test]
fn test_error_serialization() {
    let error = JsonRpcError::method_not_found("unknown_method");

    assert_eq!(error.code, -32601);
    assert!(error.message.contains("unknown_method"));
}

#[test]
fn test_config_defaults() {
    let config = McpConfig::default();

    assert_eq!(config.name, "nexus-memory");
    assert_eq!(config.transport, "stdio");
    assert_eq!(config.port, 8768);
}

#[test]
fn test_config_builder() {
    let config = McpConfig::new()
        .with_transport("http")
        .with_port(9000)
        .with_bind_address("0.0.0.0")
        .with_max_connections(500);

    assert_eq!(config.transport, "http");
    assert_eq!(config.port, 9000);
    assert_eq!(config.bind_address, "0.0.0.0");
    assert_eq!(config.max_connections, 500);
}

#[test]
fn test_request_id_variants() {
    let num_id = RequestId::from(42);
    assert_eq!(num_id, RequestId::Number(42));

    let str_id = RequestId::from("abc".to_string());
    assert_eq!(str_id, RequestId::String("abc".to_string()));
}

#[test]
fn test_notification_detection() {
    let request_with_id = JsonRpcRequest::new("test").with_id(RequestId::from(1));
    assert!(!request_with_id.is_notification());

    let notification = JsonRpcRequest::new("test");
    assert!(notification.is_notification());
}

#[test]
fn test_tools_list_schema() {
    // Verify the tools list returns proper schema format
    let tools = nexus_memory_mcp::get_tools();

    // All tools should have valid JSON schemas
    for tool in &tools {
        assert!(!tool.name.is_empty());
        assert!(!tool.description.is_empty());
        assert!(tool.input_schema.is_object());

        let schema = &tool.input_schema;
        assert!(schema.get("type").is_some());
    }
}

#[test]
fn test_resources_list() {
    let resources = nexus_memory_mcp::get_resources();

    assert!(!resources.is_empty());

    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(uris.contains(&"memory://"));
    assert!(uris.contains(&"agent://"));
}

#[test]
fn test_server_config() {
    let config = McpConfig::stdio();
    assert_eq!(config.transport, "stdio");

    let config = McpConfig::http(8080);
    assert_eq!(config.transport, "http");
    assert_eq!(config.port, 8080);
}

// Note: Integration tests that require database access should use
// tempfile-based databases and are better suited for a separate
// integration test module with proper setup/teardown.

#[cfg(test)]
mod protocol_conformance {
    use super::*;

    #[test]
    fn test_jsonrpc_version() {
        assert_eq!(nexus_memory_mcp::JSONRPC_VERSION, "2.0");
    }

    #[test]
    fn test_error_codes() {
        // JSON-RPC standard error codes
        assert_eq!(JsonRpcError::parse_error("").code, -32700);
        assert_eq!(JsonRpcError::invalid_request("").code, -32600);
        assert_eq!(JsonRpcError::method_not_found("").code, -32601);
        assert_eq!(JsonRpcError::invalid_params("").code, -32602);
        assert_eq!(JsonRpcError::internal_error("").code, -32603);
    }

    #[test]
    fn test_message_roundtrip() {
        let request = JsonRpcRequest::new("tools/call")
            .with_params(json!({
                "name": "store_memory",
                "arguments": {
                    "content": "test content",
                    "agent_type": "test"
                }
            }))
            .with_id(RequestId::from(123));

        let json = serde_json::to_string(&request).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.method, "tools/call");
        assert_eq!(parsed.id, Some(RequestId::from(123)));
    }
}
