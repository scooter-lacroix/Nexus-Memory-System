//! Schema and contract tests for cognition MCP tools.
//!
//! Validates that the new cognition tools have proper schemas and that
//! tool handlers produce responses conforming to the expected shapes.

use nexus_memory_mcp::{get_tools, tools::ToolHandler};
use nexus_storage::migrations;
use sqlx::sqlite::SqlitePoolOptions;

/// Helper to extract text from a CallToolResult content block.
fn extract_text(result: &nexus_memory_mcp::protocol::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            nexus_memory_mcp::protocol::ContentBlock::Text { text } => Some(text.clone()),
            nexus_memory_mcp::protocol::ContentBlock::Resource { resource } => {
                Some(resource.text.clone().unwrap_or_default())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Verify build_working_representation tool schema exists and is well-formed.
#[test]
fn test_build_working_representation_tool_schema() {
    let tools = get_tools();
    let tool = tools
        .iter()
        .find(|t| t.name == "build_working_representation")
        .expect("build_working_representation tool should exist");

    assert!(!tool.description.is_empty());

    let schema = &tool.input_schema;
    assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));

    let props = schema.get("properties").expect("properties should exist");
    assert!(props.get("query").is_some(), "query property required");
    assert!(
        props.get("agent_type").is_some(),
        "agent_type property required"
    );
    assert!(
        props.get("observer").is_some(),
        "observer property required"
    );
    assert!(props.get("subject").is_some(), "subject property required");
    assert!(
        props.get("session_key").is_some(),
        "session_key property required"
    );
    assert!(
        props.get("max_items").is_some(),
        "max_items property required"
    );
    assert!(
        props.get("include_raw").is_some(),
        "include_raw property required"
    );
    assert!(
        props.get("include_digests").is_some(),
        "include_digests property required"
    );
    assert!(
        props.get("include_derived").is_some(),
        "include_derived property required"
    );
    assert!(
        props.get("include_contradictions").is_some(),
        "include_contradictions property required"
    );

    let required = schema.get("required").expect("required should exist");
    let required_arr = required.as_array().expect("required should be array");
    assert!(required_arr.contains(&serde_json::json!("agent_type")));
}

/// Verify search_perspective_memories tool schema exists and is well-formed.
#[test]
fn test_search_perspective_memories_tool_schema() {
    let tools = get_tools();
    let tool = tools
        .iter()
        .find(|t| t.name == "search_perspective_memories")
        .expect("search_perspective_memories tool should exist");

    assert!(!tool.description.is_empty());

    let schema = &tool.input_schema;
    assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));

    let props = schema.get("properties").expect("properties should exist");
    assert!(
        props.get("agent_type").is_some(),
        "agent_type property required"
    );
    assert!(
        props.get("observer").is_some(),
        "observer property required"
    );
    assert!(props.get("subject").is_some(), "subject property required");
    assert!(
        props.get("session_key").is_some(),
        "session_key property required"
    );
    assert!(
        props.get("cognitive_level").is_some(),
        "cognitive_level property required"
    );
    assert!(props.get("limit").is_some(), "limit property required");

    let required = schema.get("required").expect("required should exist");
    let required_arr = required.as_array().expect("required should be array");
    assert!(required_arr.contains(&serde_json::json!("observer")));
    assert!(required_arr.contains(&serde_json::json!("subject")));
}

/// Verify the cognitive_level enum has the expected values.
#[test]
fn test_search_perspective_cognitive_level_enum() {
    let tools = get_tools();
    let tool = tools
        .iter()
        .find(|t| t.name == "search_perspective_memories")
        .unwrap();

    let cognitive_level = tool
        .input_schema
        .get("properties")
        .unwrap()
        .get("cognitive_level")
        .unwrap();
    let enum_values = cognitive_level.get("enum").unwrap().as_array().unwrap();

    let expected = [
        "raw",
        "explicit",
        "derived",
        "summary_short",
        "summary_long",
        "contradiction",
    ];
    for level in &expected {
        assert!(
            enum_values.contains(&serde_json::json!(level)),
            "cognitive_level enum should contain '{}'",
            level
        );
    }
}

/// Verify all tool schemas pass basic structural validation.
#[test]
fn test_all_tool_schemas_valid() {
    let tools = get_tools();

    assert!(
        tools.len() >= 14,
        "Expected at least 14 tools, got {}",
        tools.len()
    );

    for tool in &tools {
        assert!(!tool.name.is_empty(), "Tool name should not be empty");
        assert!(
            !tool.description.is_empty(),
            "Tool '{}' description should not be empty",
            tool.name
        );
        assert!(
            tool.input_schema.is_object(),
            "Tool '{}' input_schema should be an object",
            tool.name
        );
        assert!(
            tool.input_schema.get("type").is_some(),
            "Tool '{}' input_schema should have a type field",
            tool.name
        );
    }
}

/// Verify the tool handler rejects unknown tool names.
#[tokio::test]
async fn test_tool_handler_rejects_unknown_tool() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let handler = ToolHandler::new(pool);
    let result = handler
        .handle("nonexistent_tool", &serde_json::Map::new())
        .await;

    assert_eq!(result.is_error, Some(true));
    assert!(
        extract_text(&result).contains("Unknown tool"),
        "Expected 'Unknown tool' error, got: {}",
        extract_text(&result)
    );
}

/// Verify build_working_representation handler returns correct shape on empty DB.
#[tokio::test]
async fn test_build_working_representation_empty_response_shape() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    ns_repo.get_or_create("test-ns", "test-ns").await.unwrap();

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("test-ns"));

    let result = handler.handle("build_working_representation", &args).await;

    assert_eq!(result.is_error, None);
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["success"], true);
    assert!(parsed["representation"].is_object());
    assert!(parsed["representation"]["digests"].is_array());
    assert!(parsed["representation"]["recent"].is_array());
    assert!(parsed["representation"]["semantic"].is_array());
    assert!(parsed["representation"]["derived"].is_array());
    assert!(parsed["representation"]["contradictions"].is_array());
    assert!(parsed["bucket_counts"].is_object());
}

/// Verify search_perspective_memories handler returns correct shape on empty DB.
#[tokio::test]
async fn test_search_perspective_memories_empty_response_shape() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    ns_repo.get_or_create("test-ns", "test-ns").await.unwrap();

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("test-ns"));
    args.insert("observer".into(), serde_json::json!("claude-code"));
    args.insert("subject".into(), serde_json::json!("claude-code"));

    let result = handler.handle("search_perspective_memories", &args).await;

    assert_eq!(result.is_error, None);
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["success"], true);
    assert_eq!(parsed["count"], 0);
    assert!(parsed["memories"].is_array());
    assert!(parsed["perspective"]["observer"].is_string());
    assert!(parsed["perspective"]["subject"].is_string());
}

/// Verify search_perspective_memories rejects missing required args.
#[tokio::test]
async fn test_search_perspective_memories_requires_observer_subject() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let handler = ToolHandler::new(pool);

    let result = handler
        .handle("search_perspective_memories", &serde_json::Map::new())
        .await;
    assert_eq!(result.is_error, Some(true));

    let mut args = serde_json::Map::new();
    args.insert("observer".into(), serde_json::json!("test"));
    let result = handler.handle("search_perspective_memories", &args).await;
    assert_eq!(result.is_error, Some(true));
}

/// Verify build_working_representation with perspective populates buckets.
#[tokio::test]
async fn test_build_working_representation_with_perspective() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    let namespace = ns_repo
        .get_or_create("repr-test", "repr-test")
        .await
        .unwrap();

    let mem_repo = nexus_storage::MemoryRepository::new(pool.clone());
    mem_repo
        .store(nexus_storage::repository::StoreMemoryParams {
            namespace_id: namespace.id,
            content: "test explicit observation",
            category: &nexus_core::MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "sess-1",
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("repr-test"));
    args.insert("observer".into(), serde_json::json!("claude-code"));
    args.insert("subject".into(), serde_json::json!("claude-code"));
    args.insert("session_key".into(), serde_json::json!("sess-1"));

    let result = handler.handle("build_working_representation", &args).await;

    assert_eq!(result.is_error, None);
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(!parsed["representation"]["recent"]
        .as_array()
        .unwrap()
        .is_empty());
}

/// Verify search_perspective_memories returns stored perspective memories.
#[tokio::test]
async fn test_search_perspective_memories_returns_matching() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    let namespace = ns_repo
        .get_or_create("search-test", "search-test")
        .await
        .unwrap();

    let mem_repo = nexus_storage::MemoryRepository::new(pool.clone());
    mem_repo
        .store(nexus_storage::repository::StoreMemoryParams {
            namespace_id: namespace.id,
            content: "perspective test memory",
            category: &nexus_core::MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "sess-1",
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("search-test"));
    args.insert("observer".into(), serde_json::json!("claude-code"));
    args.insert("subject".into(), serde_json::json!("claude-code"));

    let result = handler.handle("search_perspective_memories", &args).await;

    assert_eq!(result.is_error, None);
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["count"], 1);
    let memories = parsed["memories"].as_array().unwrap();
    assert_eq!(memories[0]["content"], "perspective test memory");
}

/// Verify build_working_representation can opt into raw operational memories.
#[tokio::test]
async fn test_build_working_representation_include_raw_surfaces_raw_activity() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    let namespace = ns_repo
        .get_or_create("raw-repr-test", "raw-repr-test")
        .await
        .unwrap();

    let mem_repo = nexus_storage::MemoryRepository::new(pool.clone());
    mem_repo
        .store(nexus_storage::repository::StoreMemoryParams {
            namespace_id: namespace.id,
            content: "raw hook payload",
            category: &nexus_core::MemoryCategory::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "sess-1",
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("raw-repr-test"));
    args.insert("include_raw".into(), serde_json::json!(true));

    let result = handler.handle("build_working_representation", &args).await;

    assert_eq!(result.is_error, None);
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let recent = parsed["representation"]["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0]["content"], "raw hook payload");
}

/// Verify search_perspective_memories respects session_key when filtering by level.
#[tokio::test]
async fn test_search_perspective_memories_level_filter_respects_session_key() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    let namespace = ns_repo
        .get_or_create("search-level-test", "search-level-test")
        .await
        .unwrap();

    let mem_repo = nexus_storage::MemoryRepository::new(pool.clone());
    for session in ["sess-1", "sess-2"] {
        let content = format!("memory for {}", session);
        mem_repo
            .store(nexus_storage::repository::StoreMemoryParams {
                namespace_id: namespace.id,
                content: &content,
                category: &nexus_core::MemoryCategory::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "explicit",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": session,
                        "generated_by": "test"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
    }

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("search-level-test"));
    args.insert("observer".into(), serde_json::json!("claude-code"));
    args.insert("subject".into(), serde_json::json!("claude-code"));
    args.insert("session_key".into(), serde_json::json!("sess-1"));
    args.insert("cognitive_level".into(), serde_json::json!("explicit"));

    let result = handler.handle("search_perspective_memories", &args).await;

    assert_eq!(result.is_error, None);
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["count"], 1);
    let memories = parsed["memories"].as_array().unwrap();
    assert_eq!(memories[0]["content"], "memory for sess-1");
}

/// Verify search_perspective_memories rejects invalid cognitive level.
#[tokio::test]
async fn test_search_perspective_memories_invalid_level() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    migrations::run_migrations(&pool).await.unwrap();

    let ns_repo = nexus_storage::NamespaceRepository::new(pool.clone());
    ns_repo.get_or_create("test-ns", "test-ns").await.unwrap();

    let handler = ToolHandler::new(pool);
    let mut args = serde_json::Map::new();
    args.insert("agent_type".into(), serde_json::json!("test-ns"));
    args.insert("observer".into(), serde_json::json!("test"));
    args.insert("subject".into(), serde_json::json!("test"));
    args.insert("cognitive_level".into(), serde_json::json!("invalid_level"));

    let result = handler.handle("search_perspective_memories", &args).await;

    assert_eq!(result.is_error, Some(true));
    assert!(
        extract_text(&result).contains("Invalid cognitive level"),
        "Expected 'Invalid cognitive level' error, got: {}",
        extract_text(&result)
    );
}
