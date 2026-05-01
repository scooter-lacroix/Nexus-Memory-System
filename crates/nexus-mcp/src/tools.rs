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
//! - build_working_representation: Build a bounded cognitive context for a query
//! - search_perspective_memories: Search memories filtered by perspective

use crate::protocol::{CallToolResult, Tool};
use chrono::{DateTime, Utc};
use nexus_agent::{DeriveService, RecallToolService, ReflectService, RepresentationService};
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::{
    AgentNamespace, Memory, MemoryCategory, MemoryLaneType, PerspectiveKey,
    WorkingRepresentationRequest,
};
use nexus_storage::repository::StoreMemoryParams;
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
        get_session_digest_tool(),
        run_reflective_cycle_tool(),
        explain_memory_lineage_tool(),
        build_working_representation_tool(),
        search_perspective_memories_tool(),
        derive_observations_tool(),
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

fn get_session_digest_tool() -> Tool {
    Tool::new(
        "get_session_digest",
        "Get the latest short and/or long digest for a session",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "agent_type": {
                "type": "string",
                "description": "Agent namespace to inspect",
                "default": "general"
            },
            "session_key": {
                "type": "string",
                "description": "Session key to inspect"
            },
            "digest_kind": {
                "type": "string",
                "enum": ["short", "long", "both"],
                "description": "Which digest to return",
                "default": "both"
            }
        },
        "required": ["session_key"]
    }))
}

fn run_reflective_cycle_tool() -> Tool {
    Tool::new(
        "run_reflective_cycle",
        "Run a deterministic dream/reflection cycle for an agent namespace",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "agent_type": {
                "type": "string",
                "description": "Agent namespace to reflect over",
                "default": "general"
            },
            "session_key": {
                "type": "string",
                "description": "Optional session key to scope the cycle"
            }
        },
        "required": []
    }))
}

fn explain_memory_lineage_tool() -> Tool {
    Tool::new(
        "explain_memory_lineage",
        "Show evidence lineage for a memory and the linked memories involved",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "memory_id": {
                "type": "integer",
                "description": "Memory ID whose lineage should be explained"
            }
        },
        "required": ["memory_id"]
    }))
}

/// Build working representation tool definition
fn build_working_representation_tool() -> Tool {
    Tool::new(
        "build_working_representation",
        "Build a bounded cognitive context (working representation) for a query, returning digests, recent memories, semantic matches, derived insights, and contradictions",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Query text to guide semantic search within the representation"
            },
            "agent_type": {
                "type": "string",
                "description": "Agent namespace to build the representation for",
                "default": "general"
            },
            "observer": {
                "type": "string",
                "description": "Perspective observer (who is forming the memory)"
            },
            "subject": {
                "type": "string",
                "description": "Perspective subject (who or what the memory is about)"
            },
            "session_key": {
                "type": ["string", "null"],
                "description": "Optional session key to scope the perspective"
            },
            "max_items": {
                "type": "integer",
                "description": "Maximum total items across all buckets",
                "default": 24,
                "minimum": 4,
                "maximum": 100
            },
            "include_raw": {
                "type": "boolean",
                "description": "Include raw operational activity memories in the representation",
                "default": false
            },
            "include_digests": {
                "type": "boolean",
                "description": "Include session digests in the representation",
                "default": true
            },
            "include_derived": {
                "type": "boolean",
                "description": "Include derived insights in the representation",
                "default": true
            },
            "include_contradictions": {
                "type": "boolean",
                "description": "Include contradiction records in the representation",
                "default": true
            }
        },
        "required": ["agent_type"]
    }))
}

/// Search perspective memories tool definition
fn search_perspective_memories_tool() -> Tool {
    Tool::new(
        "search_perspective_memories",
        "Search memories filtered by perspective (observer, subject, session_key), returning matches with cognitive metadata",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "agent_type": {
                "type": "string",
                "description": "Agent namespace to search within",
                "default": "general"
            },
            "observer": {
                "type": "string",
                "description": "Perspective observer to filter by"
            },
            "subject": {
                "type": "string",
                "description": "Perspective subject to filter by"
            },
            "session_key": {
                "type": ["string", "null"],
                "description": "Optional session key to scope the search"
            },
            "cognitive_level": {
                "type": "string",
                "description": "Filter by cognitive level (raw, explicit, derived, summary_short, summary_long, contradiction)",
                "enum": ["raw", "explicit", "derived", "summary_short", "summary_long", "contradiction"]
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of results",
                "default": 20,
                "minimum": 1,
                "maximum": 100
            }
        },
        "required": ["observer", "subject"]
    }))
}

/// Derive observations tool definition
fn derive_observations_tool() -> Tool {
    Tool::new(
        "derive_observations",
        "Convert a raw session memory into explicit observations using LLM analysis",
    )
    .with_schema(serde_json::json!({
        "type": "object",
        "properties": {
            "memory_id": {
                "type": "integer",
                "description": "ID of the raw memory to derive observations from"
            },
            "agent_type": {
                "type": "string",
                "description": "Agent namespace (defaults to the memory's namespace)",
                "default": "general"
            },
            "observer": {
                "type": "string",
                "description": "Observer for derived memories (defaults to agent_type)"
            },
            "subject": {
                "type": "string",
                "description": "Subject for derived memories (defaults to agent_type)"
            },
            "session_key": {
                "type": ["string", "null"],
                "description": "Session key to associate with derived memories"
            }
        },
        "required": ["memory_id"]
    }))
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
            "get_session_digest" => self.handle_get_session_digest(args).await,
            "run_reflective_cycle" => self.handle_run_reflective_cycle(args).await,
            "explain_memory_lineage" => self.handle_explain_memory_lineage(args).await,
            "build_working_representation" => self.handle_build_working_representation(args).await,
            "search_perspective_memories" => self.handle_search_perspective_memories(args).await,
            "derive_observations" => self.handle_derive_observations(args).await,
            "initialize_nexus_system" => self.handle_initialize_system(args).await,
            "tool_help" => self.handle_tool_help(args).await,
            "tool_schema" => self.handle_tool_schema(args).await,
            _ => CallToolResult::error(format!("Unknown tool: {}", name)),
        }
    }

    async fn handle_tool_help(&self, args: &serde_json::Map<String, JsonValue>) -> CallToolResult {
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
        let category = MemoryCategory::parse(category_str).unwrap_or(MemoryCategory::General);

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
            .store(StoreMemoryParams {
                namespace_id: namespace.id,
                content: &content,
                category: &category,
                memory_lane_type: None as Option<&MemoryLaneType>,
                labels: &labels,
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
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
                let results: Vec<_> = memories.iter().map(memory_to_json).collect();

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
                let results: Vec<_> = memories.iter().map(memory_to_json).collect();

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
                let results: Vec<_> = namespaces.iter().map(namespace_to_json).collect();

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

    async fn handle_get_session_digest(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let session_key = match args.get("session_key").and_then(|v| v.as_str()) {
            Some(value) if !value.is_empty() => value,
            _ => return CallToolResult::error("session_key is required"),
        };
        let digest_kind = args
            .get("digest_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("both");

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => return CallToolResult::error("Namespace not found"),
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };
        let mem_repo = MemoryRepository::new(self.pool.clone());
        let service = RecallToolService::new();
        let (short_opt, long_opt) = match service
            .get_session_digest(&mem_repo, namespace.id, session_key)
            .await
        {
            Ok((short, long)) => (short, long),
            Err(e) => return CallToolResult::error(format!("Failed to get session digest: {}", e)),
        };
        let short = if matches!(digest_kind, "short" | "both") {
            short_opt.map(|m| memory_to_json(&m))
        } else {
            None
        };
        let long = if matches!(digest_kind, "long" | "both") {
            long_opt.map(|m| memory_to_json(&m))
        } else {
            None
        };

        CallToolResult::json(serde_json::json!({
            "success": true,
            "agent_type": agent_type,
            "session_key": session_key,
            "digest_kind": digest_kind,
            "short": short,
            "long": long,
        }))
    }

    async fn handle_run_reflective_cycle(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();
        let session_key = args
            .get("session_key")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(&agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => return CallToolResult::error("Namespace not found"),
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };
        let mem_repo = MemoryRepository::new(self.pool.clone());
        let agent_config = AgentConfig {
            namespace: agent_type.clone(),
            ..AgentConfig::default()
        };
        let service = ReflectService::new(agent_config, CognitionConfig::default(), None);

        let result = match session_key {
            Some(session_key) => {
                let perspective = PerspectiveKey {
                    observer: agent_type.clone(),
                    subject: agent_type.clone(),
                    session_key: Some(session_key),
                };
                service
                    .reflect_perspective_cycle(namespace.id, &perspective, &mem_repo)
                    .await
            }
            None => service.reflect_cycle(namespace.id, &mem_repo).await,
        };

        match result {
            Ok(result) => CallToolResult::json(serde_json::json!({
                "success": true,
                "memories_scanned": result.memories_scanned,
                "pairs_compared": result.pairs_compared,
                "reinforcements": result.reinforcements,
                "contradictions_created": result.contradictions_created,
                "contradiction_ids": result.contradiction_ids,
            })),
            Err(e) => CallToolResult::error(format!("Reflection cycle failed: {}", e)),
        }
    }

    async fn handle_explain_memory_lineage(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let memory_id = match args.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return CallToolResult::error("memory_id is required"),
        };

        let mem_repo = MemoryRepository::new(self.pool.clone());
        let service = RecallToolService::new();
        let lineage = match service.get_memory_lineage(&mem_repo, memory_id).await {
            Ok(l) => l,
            Err(e) => return CallToolResult::error(format!("Failed to load lineage: {}", e)),
        };

        let mut entries = Vec::new();
        for entry in lineage {
            let derived = mem_repo
                .get_by_id(entry.derived_memory_id)
                .await
                .ok()
                .flatten()
                .map(|memory| memory_to_json(&memory));
            let source = mem_repo
                .get_by_id(entry.source_memory_id)
                .await
                .ok()
                .flatten()
                .map(|memory| memory_to_json(&memory));

            entries.push(serde_json::json!({
                "derived_memory_id": entry.derived_memory_id,
                "source_memory_id": entry.source_memory_id,
                "evidence_role": entry.evidence_role,
                "derived_memory": derived,
                "source_memory": source,
            }));
        }

        CallToolResult::json(serde_json::json!({
            "success": true,
            "memory_id": memory_id,
            "lineage": entries,
        }))
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

    /// Build a working representation for a query.
    async fn handle_build_working_representation(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let query = args.get("query").and_then(|v| v.as_str()).map(String::from);
        let observer = args.get("observer").and_then(|v| v.as_str());
        let subject = args.get("subject").and_then(|v| v.as_str());
        let session_key = args
            .get("session_key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let max_items = args.get("max_items").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
        let include_raw = args
            .get("include_raw")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_digests = args
            .get("include_digests")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_derived = args
            .get("include_derived")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_contradictions = args
            .get("include_contradictions")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => return CallToolResult::error("Namespace not found"),
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };
        let mem_repo = MemoryRepository::new(self.pool.clone());

        let perspective = if let (Some(obs), Some(sub)) = (observer, subject) {
            Some(PerspectiveKey::new(
                obs.to_string(),
                sub.to_string(),
                session_key.map(String::from),
            ))
        } else {
            None
        };

        let request = WorkingRepresentationRequest {
            namespace_id: namespace.id,
            perspective,
            query,
            max_items,
            include_raw,
            include_recent: true,
            include_semantic: true,
            include_derived,
            include_digests,
            include_contradictions,
            ..WorkingRepresentationRequest::default()
        };

        let service = RepresentationService::new();
        let representation = match service.build(&request, &mem_repo).await {
            Ok(r) => r,
            Err(e) => {
                return CallToolResult::error(format!(
                    "Failed to build working representation: {}",
                    e
                ))
            }
        };

        CallToolResult::json(serde_json::json!({
            "success": true,
            "agent_type": agent_type,
            "representation": {
                "digests": memories_to_json(&representation.digests),
                "recent": memories_to_json(&representation.recent),
                "semantic": memories_to_json(&representation.semantic),
                "derived": memories_to_json(&representation.derived),
                "contradictions": memories_to_json(&representation.contradictions),
            },
            "bucket_counts": {
                "digests": representation.digests.len(),
                "recent": representation.recent.len(),
                "semantic": representation.semantic.len(),
                "derived": representation.derived.len(),
                "contradictions": representation.contradictions.len(),
            }
        }))
    }

    /// Search memories filtered by perspective.
    async fn handle_search_perspective_memories(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let observer = match args.get("observer").and_then(|v| v.as_str()) {
            Some(v) if !v.is_empty() => v,
            _ => return CallToolResult::error("observer is required"),
        };
        let subject = match args.get("subject").and_then(|v| v.as_str()) {
            Some(v) if !v.is_empty() => v,
            _ => return CallToolResult::error("subject is required"),
        };
        let session_key = args
            .get("session_key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let cognitive_level = args.get("cognitive_level").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as i64;

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => return CallToolResult::error("Namespace not found"),
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };
        let mem_repo = MemoryRepository::new(self.pool.clone());
        let perspective = PerspectiveKey::new(
            observer.to_string(),
            subject.to_string(),
            session_key.map(String::from),
        );

        let service = RecallToolService::new();
        let memories = match cognitive_level {
            Some(level_str) => {
                let level = match nexus_core::CognitiveLevel::parse(level_str) {
                    Some(l) => l,
                    None => {
                        return CallToolResult::error(format!(
                            "Invalid cognitive level: {}",
                            level_str
                        ))
                    }
                };
                match service
                    .search_memory(&mem_repo, namespace.id, &perspective, Some(level), limit)
                    .await
                {
                    Ok(mems) => mems,
                    Err(e) => {
                        return CallToolResult::error(format!(
                            "Failed to search perspective memories: {}",
                            e
                        ))
                    }
                }
            }
            None => match service
                .search_memory(&mem_repo, namespace.id, &perspective, None, limit)
                .await
            {
                Ok(mems) => mems,
                Err(e) => {
                    return CallToolResult::error(format!(
                        "Failed to search perspective memories: {}",
                        e
                    ))
                }
            },
        };

        CallToolResult::json(serde_json::json!({
            "success": true,
            "agent_type": agent_type,
            "perspective": {
                "observer": observer,
                "subject": subject,
                "session_key": session_key,
            },
            "count": memories.len(),
            "memories": memories_to_json(&memories),
        }))
    }

    async fn handle_derive_observations(
        &self,
        args: &serde_json::Map<String, JsonValue>,
    ) -> CallToolResult {
        let memory_id = match args.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return CallToolResult::error("memory_id is required"),
        };

        let agent_type = args
            .get("agent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("general");

        let observer = args.get("observer").and_then(|v| v.as_str());
        let subject = args.get("subject").and_then(|v| v.as_str());
        let session_key = args
            .get("session_key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => ns,
            Ok(None) => return CallToolResult::error("Namespace not found"),
            Err(e) => return CallToolResult::error(format!("Failed to get namespace: {}", e)),
        };
        let mem_repo = MemoryRepository::new(self.pool.clone());

        let memory = match mem_repo.get_by_id(memory_id).await {
            Ok(Some(m)) => m,
            Ok(None) => return CallToolResult::error(format!("Memory {} not found", memory_id)),
            Err(e) => return CallToolResult::error(format!("Failed to get memory: {}", e)),
        };

        // Verify the memory belongs to the requested namespace and is a derivable raw session memory
        if memory.namespace_id != namespace.id {
            return CallToolResult::error(format!(
                "Memory {} belongs to namespace {} but requested namespace is {}",
                memory_id, memory.namespace_id, namespace.id
            ));
        }

        // Check if this is a raw session memory that can be derived
        let is_raw_session = memory.category == MemoryCategory::Session
            && memory.labels.iter().any(|l| l == "raw-activity")
            && memory
                .metadata
                .get("cognitive")
                .and_then(|c| c.get("level"))
                .and_then(|l| l.as_str())
                .map(|l| l == "raw")
                .unwrap_or(false);

        if !is_raw_session {
            return CallToolResult::error(format!(
                "Memory {} is not a raw session memory; only raw session memories can be derived",
                memory_id
            ));
        }

        let agent_config = AgentConfig {
            namespace: agent_type.to_string(),
            ..AgentConfig::default()
        };

        // Create LLM client
        let llm_client = match nexus_llm::factory::create_client_auto() {
            Ok(client) => client,
            Err(e) => {
                return CallToolResult::error(format!(
                    "Failed to create LLM client for derivation: {}. Ensure NEXUS_LLM_* environment variables are set.",
                    e
                ))
            }
        };

        let service = DeriveService::new(agent_config, llm_client, None);

        let perspective = if let (Some(obs), Some(sub)) = (observer, subject) {
            Some(PerspectiveKey::new(
                obs.to_string(),
                sub.to_string(),
                session_key.map(String::from),
            ))
        } else {
            // Default to agent_type for both observer and subject
            Some(PerspectiveKey::new(
                agent_type.to_string(),
                agent_type.to_string(),
                session_key.map(String::from),
            ))
        };

        let derived_ids = match service
            .derive_memory_with_perspective(&memory, perspective.as_ref(), &mem_repo)
            .await
        {
            Ok(ids) => ids,
            Err(e) => return CallToolResult::error(format!("Derivation failed: {}", e)),
        };

        CallToolResult::json(serde_json::json!({
            "success": true,
            "memory_id": memory_id,
            "agent_type": agent_type,
            "derived_count": derived_ids.len(),
            "derived_memory_ids": derived_ids,
            "message": format!("Derived {} observations from raw memory", derived_ids.len()),
        }))
    }
}

/// Convert a slice of Memory to a JSON array.
fn memories_to_json(memories: &[Memory]) -> Vec<JsonValue> {
    memories.iter().map(memory_to_json).collect()
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
        assert!(tool_names.contains(&"get_session_digest"));
        assert!(tool_names.contains(&"run_reflective_cycle"));
        assert!(tool_names.contains(&"explain_memory_lineage"));
        assert!(tool_names.contains(&"build_working_representation"));
        assert!(tool_names.contains(&"search_perspective_memories"));
        assert!(tool_names.contains(&"derive_observations"));
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
