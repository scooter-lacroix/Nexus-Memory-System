//! Nexus MCP - MCP server implementation
//!
//! This crate provides MCP (Model Context Protocol) server implementation
//! for the Nexus Memory System.
//!
//! ## Features
//!
//! - Full MCP protocol support (2024-11-05)
//! - stdio transport (primary)
//! - HTTP transport (optional, planned)
//! - Memory tools (store, search, get, list, delete)
//! - Resource namespace (memory://, agent://)
//! - Prompt support
//!
//! ## Example
//!
//! ```rust,ignore
//! use nexus_mcp::{McpServer, McpConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Create server with default config (stdio transport)
//!     let mut server = McpServer::with_defaults();
//!
//!     // Initialize and start
//!     server.initialize().await.unwrap();
//!     server.start().await.unwrap();
//! }
//! ```

pub mod server;
pub mod tools;
pub mod resources;
pub mod protocol;

// Re-export server
pub use server::McpServer;

// Re-export tool functions
pub use tools::get_tools;

// Re-export resource functions
pub use resources::{get_resources, get_resource_templates};

// Re-export protocol types
pub use protocol::{
    // Modern MCP types
    RequestId, Implementation, ServerCapabilities, ClientCapabilities,
    InitializeParams, InitializeResult,
    Tool, ListToolsResult, CallToolParams, CallToolResult,
    Resource, ResourceTemplate, ResourceContents, ListResourcesResult, ReadResourceParams, ReadResourceResult,
    Prompt, PromptArgument, ListPromptsResult, GetPromptParams, GetPromptResult, PromptMessage,
    JsonRpcRequest, JsonRpcResponse, JsonRpcErrorResponse, JsonRpcError, JsonRpcMessage,
    ContentBlock,

    // Legacy compatibility types
    McpRequest, McpResponse, McpError, McpTool, McpResource,

    // Constants
    JSONRPC_VERSION,
};

use serde::{Deserialize, Serialize};

/// Result type for MCP operations
pub type Result<T> = std::result::Result<T, nexus_core::NexusError>;

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Server name
    pub name: String,

    /// Server version
    pub version: String,

    /// Transport type (stdio, http, websocket)
    pub transport: String,

    /// Port for HTTP/WebSocket transport
    pub port: u16,

    /// Bind address for HTTP transport
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// Maximum connections (for HTTP)
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_max_connections() -> usize {
    1000
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            name: "nexus-memory".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            transport: "stdio".to_string(),
            port: 8768,
            bind_address: default_bind_address(),
            max_connections: default_max_connections(),
        }
    }
}

impl McpConfig {
    /// Create new MCP config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set transport
    pub fn with_transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = transport.into();
        self
    }

    /// Set port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set bind address
    pub fn with_bind_address(mut self, address: impl Into<String>) -> Self {
        self.bind_address = address.into();
        self
    }

    /// Set max connections
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Create config for stdio transport
    pub fn stdio() -> Self {
        Self::default().with_transport("stdio")
    }

    /// Create config for HTTP transport
    pub fn http(port: u16) -> Self {
        Self::default()
            .with_transport("http")
            .with_port(port)
    }
}

/// Transport type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Transport {
    /// Standard input/output transport
    Stdio,
    /// HTTP transport
    Http,
    /// WebSocket transport (future)
    WebSocket,
}

impl Default for Transport {
    fn default() -> Self {
        Self::Stdio
    }
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Stdio => write!(f, "stdio"),
            Transport::Http => write!(f, "http"),
            Transport::WebSocket => write!(f, "websocket"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_config_default() {
        let config = McpConfig::default();
        assert_eq!(config.name, "nexus-memory");
        assert_eq!(config.transport, "stdio");
        assert_eq!(config.port, 8768);
    }

    #[test]
    fn test_mcp_config_builder() {
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
    fn test_mcp_config_stdio() {
        let config = McpConfig::stdio();
        assert_eq!(config.transport, "stdio");
    }

    #[test]
    fn test_mcp_config_http() {
        let config = McpConfig::http(8080);
        assert_eq!(config.transport, "http");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_transport_default() {
        assert!(matches!(Transport::default(), Transport::Stdio));
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(Transport::Stdio.to_string(), "stdio");
        assert_eq!(Transport::Http.to_string(), "http");
        assert_eq!(Transport::WebSocket.to_string(), "websocket");
    }
}
