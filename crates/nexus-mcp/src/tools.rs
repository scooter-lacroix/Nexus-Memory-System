//! MCP Tool implementations for Nexus Memory System
//!
//! This module provides tool implementations for memory operations:
//! - store_memory: Store a new memory
//! - search_memories: Semantic search for memories
//! - get_memory: Get a specific memory by ID
//! - list_memories: List memories with filters
//! - delete_memory: Delete a memory
//! - list_namespaces: List all agent namespaces
//! - get_stats: Get memory statistics

use crate::protocol::{CallToolResult, Tool};
use chrono::{DateTime, Utc};
use nexus_core::{AgentNamespace, Memory, MemoryCategory, MemoryLaneType};
use nexus_storage::{MemoryRepository, NamespaceRepository};
use serde_json::Value as JsonValue;
use sqlx::SqlitePool;

/// All available MCP tools
pub fn get_tools() -> Vec<Tool> {
    vec![
        store_memory_tool(),
        search_memories_tool(),
        get_memory_tool(),
        list_memories_tool(),
        delete_memory_tool(),
        list_namespaces_tool(),
        get_stats_tool(),
        initialize_system_tool(),
        tool_help_tool(),
        tool_schema_tool(),
    ]
}

/// Find a tool definition by name
pub fn find_tool(name: &str) -> Option<Tool> {
    get_tools().into_iter().find(|tool| tool.name == name)
}

/// Store memory tool definition
fn store_memory_tool() -> Tool {
    Tool::new(
        "store_memory",
        "Store a new memory in the Nexus system",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "content": {
                "type": "string",
                "description": "The memory content to store"
            },
            "agent_type": {
                "type": "string",
                "description": "Agent type (e.g., claude-code, gemini, qwen, general)",
                "default": "general"
            },
            "category": {
                "type": "string",
                "enum": ["general", "facts", "preferences", "context", "specifications", "session"],
                "description": "Memory category",
                "default": "general"
            },
            "labels": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional labels for categorization",
                "default": []
            },
            "metadata": {
                "type": "object",
                "description": "Optional additional metadata",
                "default": {}
            }
        },
        "required": ["content"]
    }))
}

/// Search memories tool definition
fn search_memories_tool() -> Tool {
    Tool::new("search_memories", "Search memories by semantic similarity").with_schema(
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find relevant memories"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Filter by agent type (optional)",
                    "default": "general"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results",
                    "default": 10,
                    "minimum": 1,
                    "maximum": 100
                },
                "threshold": {
                    "type": "number",
                    "description": "Minimum similarity threshold (0.0-1.0)",
                    "default": 0.7,
                    "minimum": 0.0,
                    "maximum": 1.0
                }
            },
            "required": ["query"]
        }),
    )
}

/// Get memory tool definition
fn get_memory_tool() -> Tool {
    Tool::new("get_memory", "Get a specific memory by ID").with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "memory_id": {
                "type": "integer",
                "description": "The memory ID to retrieve"
            }
        },
        "required": ["memory_id"]
    }))
}

/// List memories tool definition
fn list_memories_tool() -> Tool {
    Tool::new(
        "list_memories",
        "List memories with optional filters",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "agent_type": {
                "type": "string",
                "description": "Filter by agent type (optional)",
                "default": "general"
            },
            "category": {
                "type": "string",
                "enum": ["general", "facts", "preferences", "context", "specifications", "session"],
                "description": "Filter by category (optional)"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results",
                "default": 50,
                "minimum": 1,
                "maximum": 1000
            },
            "offset": {
                "type": "integer",
                "description": "Number of results to skip",
                "default": 0,
                "minimum": 0
            }
        },
        "required": []
    }))
}

/// Delete memory tool definition
fn delete_memory_tool() -> Tool {
    Tool::new("delete_memory", "Delete a memory by ID").with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "memory_id": {
                "type": "integer",
                "description": "The memory ID to delete"
            }
        },
        "required": ["memory_id"]
    }))
}

/// List namespaces tool definition
fn list_namespaces_tool() -> Tool {
    Tool::new("list_namespaces", "List all agent namespaces").with_schema(serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    }))
}

/// Get stats tool definition
fn get_stats_tool() -> Tool {
    Tool::new("get_stats", "Get memory statistics for an agent namespace").with_schema(
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent_type": {
                    "type": "string",
                    "description": "Agent type to get stats for (optional, defaults to all)",
                    "default": null
                }
            },
            "required": []
        }),
    )
}

/// Initialize system tool definition
fn initialize_system_tool() -> Tool {
    Tool::new(
        "initialize_nexus_system",
        "Initialize the Nexus memory system",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {},
        "required": []
    }))
}

/// Tool help/introspection definition
fn tool_help_tool() -> Tool {
    Tool::new(
        "tool_help",
        "List available tools or explain a specific tool",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "tool_name": {
                "type": "string",
                "description": "Optional tool name to inspect"
            }
        },
        "required": []
    }))
}

/// Tool schema/introspection definition
fn tool_schema_tool() -> Tool {
    Tool::new(
        "tool_schema",
        "Return the JSON schema for one tool or all tools",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "tool_name": {
                "type": "string",
                "description": "Optional tool name to inspect"
            }
        },
        "required": []
    }))
}

/// Tool handler with access to repositories
pub struct ToolHandler {
    pool: SqlitePool,
}

impl ToolHandler {
    /// Create a new tool handler
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Handle a tool call
    pub async fn handle(
        &self,
        name: &str,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        tracing::debug!("Handling tool call: {} with args: {:?}", name, args);

        match name {
            "store_memory" => self.handle_store_memory(args).await,
            "search_memories" => self.handle_search_memories(args).await,
            "get_memory" => self.handle_get_memory(args).await,
            "list_memories" => self.handle_list_memories(args).await,
            "delete_memory" => self.handle_delete_memory(args).await,
            "list_namespaces" => self.handle_list_namespaces(args).await,
            "get_stats" => self.handle_get_stats(args).await,
            "initialize_nexus_system" => self.handle_initialize_system(args).await,
            "tool_help" => self.handle_tool_help(args).await,
            "tool_schema" => self.handle_tool_schema(args).await,
            _ => CallToolResult::error(format!("Unknown tool: {}", name)),
        }
    }

    async fn handle_tool_help(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let requested = args
            .get("tool_name")
            .or_else(|| args.get("tool"))
            .and_then(|v| v.as_str());

        if let Some(name) = requested {
            match find_tool(name) {
                Some(tool) => CallToolResult::json(serde_json::json!({
                    "success": true,
                    "tool": {
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.input_schema
                    }
                })),
                None => CallToolResult::error(format!("Unknown tool: {}", name)),
            }
        } else {
            let tools: Vec<_> = get_tools()
                .into_iter()
                .map(|tool| {
                    serde_json::json!({
                        "name": tool.name,
                        "description": tool.description
                    })
                })
                .collect();

            CallToolResult::json(serde_json::json!({
                "success": true,
                "tools": tools,
                "usage": {
                    "tool_help": {"tool_name": "optional tool name"},
                    "tool_schema": {"tool_name": "optional tool name"}
                }
            }))
        }
    }

    async fn handle_tool_schema(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let requested = args
            .get("tool_name")
            .or_else(|| args.get("tool"))
            .and_then(|v| v.as_str());

        if let Some(name) = requested {
            match find_tool(name) {
                Some(tool) => CallToolResult::json(serde_json::json!({
                    "success": true,
                    "tool": tool.name,
                    "schema": tool.input_schema
                })),
                None => CallToolResult::error(format!("Unknown tool: {}", name)),
            }
        } else {
            let schemas: serde_json::Map<String, JsonValue> = get_tools()
                .into_iter()
                .map(|tool| (tool.name, tool.input_schema))
                .collect();

            CallToolResult::json(serde_json::json!({
                "success": true,
                "schemas": schemas
            }))
        }
    }

    /// Handle store_memory tool
    async fn handle_store_memory(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        // Extract content (required)
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) if !c.is_empty() => c.to_string(),
            _ => return CallToolResult::error("Content is required and cannot be empty"),
        };

        // Extract agent_type (optional, default "general")
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        // Extract category (optional, default "general")
        let category_str = args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let category = MemoryCategory::from_str(category_str).unwrap_or(MemoryCategory::General);

        // Extract labels (optional)
        let labels: Vec<String> = args
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Extract metadata (optional)
        let metadata = args
            .get("metadata")
            .cloned()
            .unwrap_or(JsonValue::Object(serde_json::Map::new()));

        // Get or create namespace
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_or_create(agent_type, agent_type).await {
            Ok(ns) => ns,
            Err(e) => {
                return CallToolResult::error(format!("Failed to get/create namespace: {}", e))
            }
        };

        // Store the memory
        let mem_repo = MemoryRepository::new(self.pool.clone());
        match mem_repo
            .store(
                namespace.id,
                &content,
                &category,
                None as Option<&MemoryLaneType>,
                &labels,
                &metadata,
                None, // No embedding for now
                None, // No embedding model
            )
            .await
        {
            Ok(memory) => CallToolResult::json(serde_json::json!({
                "success": true,
                "memory": memory_to_json(&memory),
                "message": format!("Memory stored successfully with ID {}", memory.id)
            })),
            Err(e) => CallToolResult::error(format!("Failed to store memory: {}", e)),
        }
    }

    /// Handle search_memories tool
    async fn handle_search_memories(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        // Extract query (required)
        let query = match args.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.is_empty() => q.to_string(),
            _ => return CallToolResult::error("Query is required and cannot be empty"),
        };

        // Extract agent_type (optional, default "general")
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        // Extract limit (optional, default 10)
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        // Get namespace
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => {
                // No namespace found, return empty results
                return CallToolResult::json(serde_json::json!({
                    "success": true,
                    "query": query,
                    "results": [],
                    "total": 0,
                    "message": "No memories found for this agent type"
                }));
            }
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };

        // Search memories
        let mem_repo = MemoryRepository::new(self.pool.clone());
        match mem_repo.search_by_namespace(namespace.id, limit, 0).await {
            Ok(memories) => {
                let results: Vec<_> = memories.iter().map(|m| memory_to_json(m)).collect();

                CallToolResult::json(serde_json::json!({
                    "success": true,
                    "query": query,
                    "results": results,
                    "total": results.len(),
                    "agent_type": agent_type
                }))
            }
            Err(e) => CallToolResult::error(format!("Failed to search memories: {}", e)),
        }
    }

    /// Handle get_memory tool
    async fn handle_get_memory(&self, args: &serde_json::Map<String, JsonValue>) -> CallToolResult {
        // Extract memory_id (required)
        let memory_id = match args.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return CallToolResult::error("memory_id is required"),
        };

        let mem_repo = MemoryRepository::new(self.pool.clone());
        match mem_repo.get_by_id(memory_id).await {
            Ok(Some(memory)) => {
                // Update access count
                let _ = mem_repo.touch(memory_id).await;

                CallToolResult::json(serde_json::json!({
                    "success": true,
                    "memory": memory_to_json(&memory)
                }))
            }
            Ok(None) => CallToolResult::error(format!("Memory with ID {} not found", memory_id)),
            Err(e) => CallToolResult::error(format!("Failed to get memory: {}", e)),
        }
    }

    /// Handle list_memories tool
    async fn handle_list_memories(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        // Extract agent_type (optional)
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        // Extract limit (optional, default 50)
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        // Extract offset (optional, default 0)
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        // Get namespace
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => {
                return CallToolResult::json(serde_json::json!({
                    "success": true,
                    "memories": [],
                    "total": 0,
                    "agent_type": agent_type
                }));
            }
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };

        // List memories
        let mem_repo = MemoryRepository::new(self.pool.clone());
        match mem_repo
            .search_by_namespace(namespace.id, limit, offset)
            .await
        {
            Ok(memories) => {
                let results: Vec<_> = memories.iter().map(|m| memory_to_json(m)).collect();

                // Get total count
                let total = mem_repo.count_by_namespace(namespace.id).await.unwrap_or(0);

                CallToolResult::json(serde_json::json!({
                    "success": true,
                    "memories": results,
                    "total": total,
                    "limit": limit,
                    "offset": offset,
                    "agent_type": agent_type
                }))
            }
            Err(e) => CallToolResult::error(format!("Failed to list memories: {}", e)),
        }
    }

    /// Handle delete_memory tool
    async fn handle_delete_memory(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        // Extract memory_id (required)
        let memory_id = match args.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return CallToolResult::error("memory_id is required"),
        };

        let mem_repo = MemoryRepository::new(self.pool.clone());
        match mem_repo.delete(memory_id).await {
            Ok(true) => CallToolResult::json(serde_json::json!({
                "success": true,
                "message": format!("Memory {} deleted successfully", memory_id)
            })),
            Ok(false) => CallToolResult::error(format!("Memory with ID {} not found", memory_id)),
            Err(e) => CallToolResult::error(format!("Failed to delete memory: {}", e)),
        }
    }

    /// Handle list_namespaces tool
    async fn handle_list_namespaces(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let _ = args; // No arguments needed

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        match ns_repo.list_all().await {
            Ok(namespaces) => {
                let results: Vec<_> = namespaces.iter().map(|ns| namespace_to_json(ns)).collect();

                CallToolResult::json(serde_json::json!({
                    "success": true,
                    "namespaces": results,
                    "total": results.len()
                }))
            }
            Err(e) => CallToolResult::error(format!("Failed to list namespaces: {}", e)),
        }
    }

    /// Handle get_stats tool
    async fn handle_get_stats(&self, args: &serde_json::Map<String, JsonValue>) -> CallToolResult {
        // Extract agent_type (optional)
        let agent_type = args.get("agent_type").and_then(|v| v.as_str());

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let mem_repo = MemoryRepository::new(self.pool.clone());

        if let Some(agent_type) = agent_type {
            // Get stats for specific namespace
            match ns_repo.get_by_name(agent_type).await {
                Ok(Some(namespace)) => {
                    let total = mem_repo.count_by_namespace(namespace.id).await.unwrap_or(0);

                    CallToolResult::json(serde_json::json!({
                        "success": true,
                        "agent_type": agent_type,
                        "total_memories": total,
                        "namespace": namespace_to_json(&namespace)
                    }))
                }
                Ok(None) => CallToolResult::json(serde_json::json!({
                    "success": true,
                    "agent_type": agent_type,
                    "total_memories": 0,
                    "message": "Namespace not found"
                })),
                Err(e) => CallToolResult::error(format!("Failed to get namespace: {}", e)),
            }
        } else {
            // Get stats for all namespaces
            match ns_repo.list_all().await {
                Ok(namespaces) => {
                    let mut stats = Vec::new();
                    let mut total_memories = 0i64;

                    for ns in namespaces {
                        let count = mem_repo.count_by_namespace(ns.id).await.unwrap_or(0);
                        total_memories += count;

                        stats.push(serde_json::json!({
                            "agent_type": ns.agent_type,
                            "namespace": ns.name,
                            "total_memories": count
                        }));
                    }

                    CallToolResult::json(serde_json::json!({
                        "success": true,
                        "total_memories": total_memories,
                        "namespaces": stats,
                        "total_namespaces": stats.len()
                    }))
                }
                Err(e) => CallToolResult::error(format!("Failed to list namespaces: {}", e)),
            }
        }
    }

    /// Handle initialize_nexus_system tool
    async fn handle_initialize_system(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let _ = args; // No arguments needed

        // Run migrations
        match nexus_storage::migrations::run_migrations(&self.pool).await {
            Ok(_) => CallToolResult::json(serde_json::json!({
                "success": true,
                "message": "Nexus memory system initialized successfully",
                "version": env!("CARGO_PKG_VERSION")
            })),
            Err(e) => CallToolResult::error(format!("Failed to initialize database: {}", e)),
        }
    }
}

/// Convert Memory to JSON value for API responses
fn memory_to_json(memory: &Memory) -> JsonValue {
    serde_json::json!({
        "id": memory.id,
        "namespace_id": memory.namespace_id,
        "content": memory.content,
        "category": memory.category.to_string(),
        "memory_lane_type": memory.memory_lane_type.as_ref().map(|t| t.to_string()),
        "labels": memory.labels,
        "metadata": memory.metadata,
        "similarity_score": memory.similarity_score,
        "relevance_score": memory.relevance_score,
        "embedding_model": memory.embedding_model,
        "created_at": memory.created_at.to_rfc3339(),
        "updated_at": memory.updated_at.map(|t| t.to_rfc3339()),
        "last_accessed": memory.last_accessed.map(|t| t.to_rfc3339()),
        "is_active": memory.is_active,
        "is_archived": memory.is_archived,
        "access_count": memory.access_count
    })
}

/// Convert AgentNamespace to JSON value for API responses
fn namespace_to_json(namespace: &AgentNamespace) -> JsonValue {
    serde_json::json!({
        "id": namespace.id,
        "name": namespace.name,
        "description": namespace.description,
        "agent_type": namespace.agent_type,
        "created_at": namespace.created_at.to_rfc3339(),
        "updated_at": namespace.updated_at.map(|t: DateTime<Utc>| t.to_rfc3339())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        assert!(!tools.is_empty());

        let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"store_memory"));
        assert!(tool_names.contains(&"search_memories"));
        assert!(tool_names.contains(&"get_memory"));
        assert!(tool_names.contains(&"list_memories"));
        assert!(tool_names.contains(&"delete_memory"));
        assert!(tool_names.contains(&"list_namespaces"));
        assert!(tool_names.contains(&"get_stats"));
        assert!(tool_names.contains(&"tool_help"));
        assert!(tool_names.contains(&"tool_schema"));
    }

    #[test]
    fn test_store_memory_tool_schema() {
        let tool = store_memory_tool();
        assert_eq!(tool.name, "store_memory");
        assert!(tool.input_schema.is_object());

        let schema = &tool.input_schema;
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&JsonValue::String("content".to_string())));
    }

    #[test]
    fn test_search_memories_tool_schema() {
        let tool = search_memories_tool();
        assert_eq!(tool.name, "search_memories");

        let schema = &tool.input_schema;
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&JsonValue::String("query".to_string())));
    }
}
