//! MCP Resource implementations for Nexus Memory System
//!
//! This module provides resource implementations:
//! - memory:// namespace for individual memories
//! - agent:// namespace for agent information

use crate::protocol::{ListResourcesResult, ReadResourceResult, Resource, ResourceContents, ResourceTemplate};
use nexus_core::Memory;
use nexus_storage::{MemoryRepository, NamespaceRepository};
use sqlx::SqlitePool;

/// Get all available resources
pub fn get_resources() -> Vec<Resource> {
    vec![
        Resource::new("memory://", "Memory Namespace")
            .with_description("Access to all stored memories")
            .with_mime_type("application/json"),
        Resource::new("agent://", "Agent Namespace")
            .with_description("Access to agent namespace information")
            .with_mime_type("application/json"),
    ]
}

/// Get resource templates for parameterized resources
pub fn get_resource_templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate {
            uri_template: "memory://{memory_id}".to_string(),
            name: "Memory by ID".to_string(),
            description: Some("Access a specific memory by its ID".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        ResourceTemplate {
            uri_template: "agent://{agent_type}/memories".to_string(),
            name: "Agent Memories".to_string(),
            description: Some("Access all memories for a specific agent type".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        ResourceTemplate {
            uri_template: "agent://{agent_type}/stats".to_string(),
            name: "Agent Statistics".to_string(),
            description: Some("Get statistics for a specific agent type".to_string()),
            mime_type: Some("application/json".to_string()),
        },
    ]
}

/// Resource handler with access to repositories
pub struct ResourceHandler {
    pool: SqlitePool,
}

impl ResourceHandler {
    /// Create a new resource handler
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// List all available resources
    pub fn list_resources(&self) -> ListResourcesResult {
        ListResourcesResult {
            resources: get_resources(),
            next_cursor: None,
        }
    }

    /// Read a specific resource by URI
    pub async fn read_resource(&self, uri: &str) -> ReadResourceResult {
        tracing::debug!("Reading resource: {}", uri);

        // Parse URI and route to appropriate handler
        if uri.starts_with("memory://") {
            self.handle_memory_resource(uri).await
        } else if uri.starts_with("agent://") {
            self.handle_agent_resource(uri).await
        } else {
            ReadResourceResult {
                contents: vec![ResourceContents::text(
                    uri.to_string(),
                    format!("Unknown resource URI: {}", uri),
                )],
            }
        }
    }

    /// Handle memory:// namespace resources
    async fn handle_memory_resource(&self, uri: &str) -> ReadResourceResult {
        let path = uri.strip_prefix("memory://").unwrap_or("");

        if path.is_empty() || path == "/" {
            // List all memories (summary)
            self.list_all_memories().await
        } else {
            // Try to parse as memory ID
            let memory_id_str = path.trim_start_matches('/');
            if let Ok(memory_id) = memory_id_str.parse::<i64>() {
                self.get_memory_by_id(memory_id).await
            } else {
                ReadResourceResult {
                    contents: vec![ResourceContents::text(
                        uri.to_string(),
                        format!("Invalid memory ID: {}", memory_id_str),
                    )],
                }
            }
        }
    }

    /// Handle agent:// namespace resources
    async fn handle_agent_resource(&self, uri: &str) -> ReadResourceResult {
        let path = uri.strip_prefix("agent://").unwrap_or("");

        if path.is_empty() || path == "/" {
            // List all agents
            self.list_all_agents().await
        } else {
            // Parse agent_type and sub-resource
            let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

            if parts.is_empty() {
                self.list_all_agents().await
            } else {
                let agent_type = parts[0];
                let sub_resource = parts.get(1).map(|s| *s);

                match sub_resource {
                    Some("memories") => self.get_agent_memories(agent_type).await,
                    Some("stats") => self.get_agent_stats(agent_type).await,
                    None => self.get_agent_info(agent_type).await,
                    _ => ReadResourceResult {
                        contents: vec![ResourceContents::text(
                            uri.to_string(),
                            format!("Unknown sub-resource: {}", parts[1]),
                        )],
                    }
                }
            }
        }
    }

    /// List all memories (summary)
    async fn list_all_memories(&self) -> ReadResourceResult {
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let mem_repo = MemoryRepository::new(self.pool.clone());

        match ns_repo.list_all().await {
            Ok(namespaces) => {
                let mut all_memories = Vec::new();
                let mut total_count = 0;

                for ns in namespaces {
                    if let Ok(count) = mem_repo.count_by_namespace(ns.id).await {
                        total_count += count;
                        all_memories.push(serde_json::json!({
                            "agent_type": ns.agent_type,
                            "count": count
                        }));
                    }
                }

                let value = serde_json::json!({
                    "total_memories": total_count,
                    "namespaces": all_memories
                });

                ReadResourceResult {
                    contents: vec![ResourceContents::json("memory://", &value)],
                }
            }
            Err(e) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    "memory://".to_string(),
                    format!("Failed to list memories: {}", e),
                )],
            }
        }
    }

    /// Get a specific memory by ID
    async fn get_memory_by_id(&self, memory_id: i64) -> ReadResourceResult {
        let mem_repo = MemoryRepository::new(self.pool.clone());

        match mem_repo.get_by_id(memory_id).await {
            Ok(Some(memory)) => {
                // Update access count
                let _ = mem_repo.touch(memory_id).await;

                let value = memory_to_json(&memory);
                ReadResourceResult {
                    contents: vec![ResourceContents::json(
                        format!("memory://{}", memory_id),
                        &value,
                    )],
                }
            }
            Ok(None) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("memory://{}", memory_id),
                    format!("Memory {} not found", memory_id),
                )],
            },
            Err(e) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("memory://{}", memory_id),
                    format!("Failed to get memory: {}", e),
                )],
            }
        }
    }

    /// List all agents
    async fn list_all_agents(&self) -> ReadResourceResult {
        let ns_repo = NamespaceRepository::new(self.pool.clone());

        match ns_repo.list_all().await {
            Ok(namespaces) => {
                let agents: Vec<_> = namespaces
                    .iter()
                    .map(|ns| serde_json::json!({
                        "name": ns.name,
                        "agent_type": ns.agent_type,
                        "description": ns.description,
                        "created_at": ns.created_at.to_rfc3339()
                    }))
                    .collect();

                let value = serde_json::json!({
                    "agents": agents,
                    "total": agents.len()
                });

                ReadResourceResult {
                    contents: vec![ResourceContents::json("agent://", &value)],
                }
            }
            Err(e) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    "agent://".to_string(),
                    format!("Failed to list agents: {}", e),
                )],
            }
        }
    }

    /// Get agent info
    async fn get_agent_info(&self, agent_type: &str) -> ReadResourceResult {
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let mem_repo = MemoryRepository::new(self.pool.clone());

        match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => {
                let memory_count = mem_repo.count_by_namespace(ns.id).await.unwrap_or(0);

                let value = serde_json::json!({
                    "name": ns.name,
                    "agent_type": ns.agent_type,
                    "description": ns.description,
                    "memory_count": memory_count,
                    "created_at": ns.created_at.to_rfc3339(),
                    "updated_at": ns.updated_at.map(|t| t.to_rfc3339())
                });

                ReadResourceResult {
                    contents: vec![ResourceContents::json(
                        format!("agent://{}", agent_type),
                        &value,
                    )],
                }
            }
            Ok(None) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("agent://{}", agent_type),
                    format!("Agent '{}' not found", agent_type),
                )],
            },
            Err(e) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("agent://{}", agent_type),
                    format!("Failed to get agent info: {}", e),
                )],
            }
        }
    }

    /// Get agent memories
    async fn get_agent_memories(&self, agent_type: &str) -> ReadResourceResult {
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let mem_repo = MemoryRepository::new(self.pool.clone());

        match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => {
                match mem_repo.search_by_namespace(ns.id, 100, 0).await {
                    Ok(memories) => {
                        let memory_list: Vec<_> = memories
                            .iter()
                            .map(|m| memory_to_json(m))
                            .collect();

                        let value = serde_json::json!({
                            "agent_type": agent_type,
                            "memories": memory_list,
                            "total": memory_list.len()
                        });

                        ReadResourceResult {
                            contents: vec![ResourceContents::json(
                                format!("agent://{}/memories", agent_type),
                                &value,
                            )],
                        }
                    }
                    Err(e) => ReadResourceResult {
                        contents: vec![ResourceContents::text(
                            format!("agent://{}/memories", agent_type),
                            format!("Failed to get agent memories: {}", e),
                        )],
                    }
                }
            }
            Ok(None) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("agent://{}/memories", agent_type),
                    format!("Agent '{}' not found", agent_type),
                )],
            },
            Err(e) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("agent://{}/memories", agent_type),
                    format!("Failed to get agent: {}", e),
                )],
            }
        }
    }

    /// Get agent stats
    async fn get_agent_stats(&self, agent_type: &str) -> ReadResourceResult {
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let mem_repo = MemoryRepository::new(self.pool.clone());

        match ns_repo.get_by_name(agent_type).await {
            Ok(Some(ns)) => {
                let total_memories = mem_repo.count_by_namespace(ns.id).await.unwrap_or(0);

                let value = serde_json::json!({
                    "agent_type": agent_type,
                    "namespace_id": ns.id,
                    "total_memories": total_memories,
                    "namespace_created": ns.created_at.to_rfc3339()
                });

                ReadResourceResult {
                    contents: vec![ResourceContents::json(
                        format!("agent://{}/stats", agent_type),
                        &value,
                    )],
                }
            }
            Ok(None) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("agent://{}/stats", agent_type),
                    format!("Agent '{}' not found", agent_type),
                )],
            },
            Err(e) => ReadResourceResult {
                contents: vec![ResourceContents::text(
                    format!("agent://{}/stats", agent_type),
                    format!("Failed to get agent stats: {}", e),
                )],
            }
        }
    }
}

/// Convert Memory to JSON value for API responses
fn memory_to_json(memory: &Memory) -> serde_json::Value {
    serde_json::json!({
        "id": memory.id,
        "namespace_id": memory.namespace_id,
        "content": memory.content,
        "category": memory.category.to_string(),
        "memory_lane_type": memory.memory_lane_type.as_ref().map(|t| t.to_string()),
        "labels": memory.labels,
        "metadata": memory.metadata,
        "created_at": memory.created_at.to_rfc3339(),
        "updated_at": memory.updated_at.map(|t| t.to_rfc3339()),
        "is_active": memory.is_active,
        "is_archived": memory.is_archived,
        "access_count": memory.access_count
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_resources() {
        let resources = get_resources();
        assert!(!resources.is_empty());

        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"memory://"));
        assert!(uris.contains(&"agent://"));
    }

    #[test]
    fn test_get_resource_templates() {
        let templates = get_resource_templates();
        assert!(!templates.is_empty());

        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Memory by ID"));
        assert!(names.contains(&"Agent Memories"));
        assert!(names.contains(&"Agent Statistics"));
    }

    #[test]
    fn test_resource_template_uris() {
        let templates = get_resource_templates();

        let memory_template = templates.iter().find(|t| t.name == "Memory by ID").unwrap();
        assert_eq!(memory_template.uri_template, "memory://{memory_id}");

        let agent_memories = templates.iter().find(|t| t.name == "Agent Memories").unwrap();
        assert_eq!(agent_memories.uri_template, "agent://{agent_type}/memories");
    }
}
