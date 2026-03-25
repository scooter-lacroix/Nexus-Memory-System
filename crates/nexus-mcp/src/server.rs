//! MCP Server implementation with stdio and HTTP transports
//!
//! This module provides the main MCP server implementation for the Nexus Memory System.

use crate::protocol::*;
use crate::resources::ResourceHandler;
use crate::tools::ToolHandler;
use crate::McpConfig;
use nexus_storage::StorageManager;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Server state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
}

/// Transport type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportType {
    #[default]
    Stdio,
    Http,
}

/// MCP Server for Nexus Memory System
pub struct McpServer {
    /// Server configuration
    config: McpConfig,

    /// Server state
    state: Arc<RwLock<ServerState>>,

    /// Storage manager
    storage: Option<StorageManager>,

    /// Request ID counter
    request_counter: AtomicU64,

    /// Whether the server is initialized
    initialized: Arc<AtomicBool>,

    /// Shutdown signal
    shutdown: Arc<AtomicBool>,

    /// Client info from initialize
    client_info: Arc<RwLock<Option<InitializeParams>>>,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(config: McpConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            storage: None,
            request_counter: AtomicU64::new(0),
            initialized: Arc::new(AtomicBool::new(false)),
            shutdown: Arc::new(AtomicBool::new(false)),
            client_info: Arc::new(RwLock::new(None)),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(McpConfig::default())
    }

    /// Initialize the server with storage
    pub async fn initialize(&mut self) -> crate::Result<()> {
        if self.initialized.load(Ordering::SeqCst) {
            return Err(nexus_core::NexusError::AlreadyInitialized);
        }

        *self.state.write().await = ServerState::Starting;

        // Initialize storage
        let db_path = std::env::var("NEXUS_DATABASE_PATH").unwrap_or_else(|_| {
            let home = dirs::data_local_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("nexus");
            format!("sqlite:{}/nexus.db?mode=rwc", home.to_string_lossy())
        });
        // Ensure plain filesystem paths are prefixed with sqlite: URL scheme
        let db_url = if db_path.starts_with("sqlite:") {
            db_path
        } else {
            format!("sqlite:{}?mode=rwc", db_path)
        };

        tracing::info!("Connecting to database: {}", db_url);

        let mut storage = StorageManager::from_url(&db_url)
            .await
            .map_err(|e| nexus_core::NexusError::Database(e.to_string()))?;

        storage
            .initialize()
            .await
            .map_err(|e| nexus_core::NexusError::Database(e.to_string()))?;

        self.storage = Some(storage);

        *self.state.write().await = ServerState::Running;
        self.initialized.store(true, Ordering::SeqCst);

        tracing::info!("MCP server initialized successfully");
        Ok(())
    }

    /// Start the server with the configured transport
    pub async fn start(&mut self) -> crate::Result<()> {
        if !self.initialized.load(Ordering::SeqCst) {
            self.initialize().await?;
        }

        let transport = match self.config.transport.as_str() {
            "stdio" => TransportType::Stdio,
            "http" => TransportType::Http,
            _ => {
                return Err(nexus_core::NexusError::InvalidConfig(format!(
                    "Unknown transport: {}",
                    self.config.transport
                )))
            }
        };

        match transport {
            TransportType::Stdio => self.start_stdio().await,
            TransportType::Http => self.start_http().await,
        }
    }

    /// Stop the server
    pub async fn stop(&mut self) -> crate::Result<()> {
        tracing::info!("Stopping MCP server...");
        self.shutdown.store(true, Ordering::SeqCst);
        *self.state.write().await = ServerState::Stopping;
        *self.state.write().await = ServerState::Stopped;
        self.initialized.store(false, Ordering::SeqCst);
        tracing::info!("MCP server stopped");
        Ok(())
    }

    /// Get server state
    pub async fn state(&self) -> ServerState {
        *self.state.read().await
    }

    /// Check if server is running
    pub async fn is_running(&self) -> bool {
        matches!(*self.state.read().await, ServerState::Running)
    }

    /// Get server configuration
    pub fn config(&self) -> &McpConfig {
        &self.config
    }

    /// Generate the next internal request sequence number.
    fn next_request_sequence(&self) -> u64 {
        self.request_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Start stdio transport
    async fn start_stdio(&mut self) -> crate::Result<()> {
        tracing::info!("Starting MCP server with stdio transport");

        let stdin = io::stdin();
        let stdout = io::stdout();

        // Lock stdout for writing
        let mut stdout_lock = stdout.lock();

        // Get pool from storage
        let pool = self
            .storage
            .as_ref()
            .ok_or(nexus_core::NexusError::NotInitialized)?
            .pool()
            .clone();

        let tool_handler = ToolHandler::new(pool.clone());
        let resource_handler = ResourceHandler::new(pool);

        // Read lines from stdin
        for line in stdin.lock().lines() {
            // Check for shutdown
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }

            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to read from stdin: {}", e);
                    continue;
                }
            };

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            let request_seq = self.next_request_sequence();
            tracing::debug!(
                request_seq,
                payload = %line,
                "Received MCP request payload"
            );

            // Parse and handle the request
            let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
                Ok(request) => {
                    self.handle_request(request, &tool_handler, &resource_handler)
                        .await
                }
                Err(e) => {
                    tracing::error!("Failed to parse request: {}", e);
                    JsonRpcMessage::Error(JsonRpcErrorResponse {
                        jsonrpc: JSONRPC_VERSION.to_string(),
                        error: JsonRpcError::parse_error(e.to_string()),
                        id: None,
                    })
                }
            };

            // Serialize and write response
            let response_json = serde_json::to_string(&response)?;
            writeln!(stdout_lock, "{}", response_json)?;
            stdout_lock.flush()?;
        }

        Ok(())
    }

    /// Start HTTP transport
    async fn start_http(&mut self) -> crate::Result<()> {
        tracing::info!(
            "Starting MCP server with HTTP transport on port {}",
            self.config.port
        );

        // HTTP transport is more complex and would typically use axum or hyper
        // For now, we'll return an error suggesting stdio transport
        Err(nexus_core::NexusError::InvalidConfig(
            "HTTP transport not yet implemented. Use stdio transport for now.".to_string(),
        ))
    }

    /// Handle an incoming JSON-RPC request
    async fn handle_request(
        &self,
        request: JsonRpcRequest,
        tool_handler: &ToolHandler,
        resource_handler: &ResourceHandler,
    ) -> JsonRpcMessage {
        let total_requests = self.request_counter.load(Ordering::Relaxed);
        tracing::debug!(
            method = %request.method,
            total_requests,
            "Handling MCP request"
        );

        // Handle notifications (no response needed)
        if request.is_notification() {
            tracing::debug!("Received notification: {}", request.method);
            return JsonRpcMessage::Response(JsonRpcResponse::new(
                RequestId::Number(0),
                serde_json::json!(null),
            ));
        }

        let id = request.id.clone().unwrap_or_default();

        let result = match request.method.as_str() {
            // Initialize
            "initialize" => self.handle_initialize(request.params).await,

            // Tools
            "tools/list" => self.handle_tools_list().await,
            "tools/call" => self.handle_tools_call(request.params, tool_handler).await,

            // Resources
            "resources/list" => self.handle_resources_list(resource_handler).await,
            "resources/read" => {
                self.handle_resources_read(request.params, resource_handler)
                    .await
            }

            // Prompts
            "prompts/list" => self.handle_prompts_list().await,
            "prompts/get" => self.handle_prompts_get(request.params).await,

            // Ping
            "ping" => Ok(serde_json::json!({})),

            // Unknown method
            _ => {
                tracing::warn!("Unknown method: {}", request.method);
                Err(JsonRpcError::method_not_found(&request.method))
            }
        };

        match result {
            Ok(value) => JsonRpcMessage::Response(JsonRpcResponse::new(id, value)),
            Err(error) => JsonRpcMessage::Error(JsonRpcErrorResponse {
                jsonrpc: JSONRPC_VERSION.to_string(),
                error,
                id: Some(id),
            }),
        }
    }

    /// Handle initialize request
    async fn handle_initialize(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let params: InitializeParams = params
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| JsonRpcError::invalid_params("Missing initialize params"))?;

        tracing::info!(
            "Client connecting: {} v{} (protocol: {})",
            params.client_info.name,
            params.client_info.version,
            params.protocol_version
        );

        // Store client info
        *self.client_info.write().await = Some(params);

        // Return server capabilities
        let result = InitializeResult::default();
        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle tools/list request
    async fn handle_tools_list(&self) -> Result<serde_json::Value, JsonRpcError> {
        let tools = crate::tools::get_tools();
        let result = ListToolsResult {
            tools,
            next_cursor: None,
        };
        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle tools/call request
    async fn handle_tools_call(
        &self,
        params: Option<serde_json::Value>,
        tool_handler: &ToolHandler,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let params: CallToolParams = params
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| JsonRpcError::invalid_params("Missing tool call params"))?;

        tracing::info!("Calling tool: {}", params.name);

        let result = tool_handler.handle(&params.name, &params.arguments).await;

        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle resources/list request
    async fn handle_resources_list(
        &self,
        handler: &ResourceHandler,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let result = handler.list_resources();
        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle resources/read request
    async fn handle_resources_read(
        &self,
        params: Option<serde_json::Value>,
        handler: &ResourceHandler,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let params: ReadResourceParams = params
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| JsonRpcError::invalid_params("Missing resource read params"))?;

        tracing::info!("Reading resource: {}", params.uri);

        let result = handler.read_resource(&params.uri).await;
        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle prompts/list request
    async fn handle_prompts_list(&self) -> Result<serde_json::Value, JsonRpcError> {
        let prompts = vec![
            Prompt {
                name: "store_memory".to_string(),
                description: Some("Store a new memory in the system".to_string()),
                arguments: Some(vec![
                    PromptArgument {
                        name: "content".to_string(),
                        description: Some("The memory content to store".to_string()),
                        required: Some(true),
                    },
                    PromptArgument {
                        name: "agent_type".to_string(),
                        description: Some("Agent type (e.g., claude-code, general)".to_string()),
                        required: Some(false),
                    },
                ]),
            },
            Prompt {
                name: "search_memories".to_string(),
                description: Some("Search for memories".to_string()),
                arguments: Some(vec![
                    PromptArgument {
                        name: "query".to_string(),
                        description: Some("Search query".to_string()),
                        required: Some(true),
                    },
                    PromptArgument {
                        name: "agent_type".to_string(),
                        description: Some("Agent type to search".to_string()),
                        required: Some(false),
                    },
                ]),
            },
        ];

        let result = ListPromptsResult {
            prompts,
            next_cursor: None,
        };

        Ok(serde_json::to_value(result).unwrap_or_default())
    }

    /// Handle prompts/get request
    async fn handle_prompts_get(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, JsonRpcError> {
        let params: GetPromptParams = params
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| JsonRpcError::invalid_params("Missing prompt get params"))?;

        let result = match params.name.as_str() {
            "store_memory" => {
                let content = params
                    .arguments
                    .get("content")
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let agent_type = params
                    .arguments
                    .get("agent_type")
                    .map(|s| s.as_str())
                    .unwrap_or("general");

                GetPromptResult {
                    description: Some("Store a new memory".to_string()),
                    messages: vec![
                        PromptMessage::user(format!(
                            "Please store the following content in the memory system for agent '{}':\n\n{}",
                            agent_type, content
                        )),
                    ],
                }
            }
            "search_memories" => {
                let query = params
                    .arguments
                    .get("query")
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let agent_type = params
                    .arguments
                    .get("agent_type")
                    .map(|s| s.as_str())
                    .unwrap_or("general");

                GetPromptResult {
                    description: Some("Search memories".to_string()),
                    messages: vec![PromptMessage::user(format!(
                        "Search for memories matching '{}' for agent '{}'",
                        query, agent_type
                    ))],
                }
            }
            _ => {
                return Err(JsonRpcError::invalid_params(format!(
                    "Unknown prompt: {}",
                    params.name
                )));
            }
        };

        Ok(serde_json::to_value(result).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_new() {
        let config = McpConfig::default();
        let server = McpServer::new(config);

        assert!(matches!(server.state().await, ServerState::Stopped));
    }

    #[tokio::test]
    async fn test_server_with_defaults() {
        let server = McpServer::with_defaults();
        assert!(matches!(server.state().await, ServerState::Stopped));
    }

    #[test]
    fn test_server_state_default() {
        assert!(matches!(ServerState::default(), ServerState::Stopped));
    }

    #[test]
    fn test_transport_type_default() {
        assert!(matches!(TransportType::default(), TransportType::Stdio));
    }

    #[tokio::test]
    async fn test_server_config() {
        let server = McpServer::with_defaults();
        let config = server.config();
        assert_eq!(config.transport, "stdio");
    }
}
