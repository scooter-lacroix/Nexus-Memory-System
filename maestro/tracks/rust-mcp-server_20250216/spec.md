# Spec: Rust MCP Server

**Track ID:** rust-mcp-server_20250216
**Type:** Feature
**Status:** New

---

## Overview

Implement the MCP (Model Context Protocol) server in Rust using rmcp. Supports stdio and HTTP transports with full FastMCP compatibility.

**Python Mapping:** `nexus/server/mcp_server.py`

---

## Functional Requirements

### FR1: MCP Server Implementation

- Use rmcp (Rust MCP implementation)
- Support stdio transport
- Support HTTP transport
- Full FastMCP API compatibility

### FR2: Memory Tools

All memory operations exposed as MCP tools:

| Tool | Description | Parameters |
|------|-------------|------------|
| `store_memory` | Store a new memory | content, category, labels |
| `search_memories` | Search by semantic similarity | query, limit, threshold |
| `get_memory` | Get memory by ID | memory_id |
| `update_memory` | Update existing memory | memory_id, updates |
| `delete_memory` | Delete memory | memory_id |
| `list_namespaces` | List all agent namespaces | - |
| `get_stats` | Get memory statistics | agent_type |

### FR3: Resource Management

- Graceful shutdown handling
- Connection pooling
- Rate limiting per agent
- Resource cleanup on disconnect

### FR4: Server Configuration

```rust
pub struct McpServerConfig {
    pub transport: TransportType,
    pub bind_address: Option<String>,
    pub port: Option<u16>,
    pub max_connections: usize,
}

pub enum TransportType {
    Stdio,
    Http,
    Both,
}
```

---

## Non-Functional Requirements

### NFR1: Performance

| Metric | Target |
|--------|--------|
| Tool invocation latency | <50ms |
| Concurrent connections | 1000+ |
| Message throughput | 10k+ msg/sec |

### NFR2: Compatibility

- Full FastMCP protocol compatibility
- Python client can connect to Rust server
- Identical tool responses

### NFR3: Code Quality

- 95%+ test coverage
- Proper error handling and propagation
- Resource cleanup guarantees

---

## Acceptance Criteria

### AC1: Server Starts and Accepts Connections

```rust
let config = McpServerConfig::default();
let server = McpServer::new(config, Arc::new(RwLock::new(manager))).await?;
server.serve().await?;
```

### AC2: Memory Tools Functional

```rust
// Via MCP client
let result = client.call_tool("store_memory", args).await?;
assert!(result.is_ok());

let search_result = client.call_tool("search_memories", query).await?;
assert!(search_result.is_ok());
```

### AC3: FastMCP Compatibility

- Python FastMCP client can connect
- All tools discoverable
- Tool schemas match Python version

### AC4: Graceful Shutdown

- SIGTERM triggers graceful shutdown
- All connections closed properly
- No resource leaks

---

## Dependencies

### External Crates

```toml
[dependencies]
rmcp = "0.1"           # Rust MCP implementation
tokio = { version = "1.40", features = ["full"] }
serde_json = "1.0"
hyper = { version = "1.0", features = ["full"], optional = true }
```

### Local Dependencies

- `nexus-core` - Core types
- `nexus-storage` - Memory operations
- `nexus-embeddings` - Embedding generation
- `nexus-orchestrator` - Session management

---

## Tool Definitions

### store_memory

```json
{
  "name": "store_memory",
  "description": "Store a new memory in the Nexus system",
  "inputSchema": {
    "type": "object",
    "properties": {
      "content": {"type": "string"},
      "category": {"type": "string", "enum": ["general", "facts", "preferences", "context", "specifications", "session"]},
      "labels": {"type": "array", "items": {"type": "string"}},
      "agent_type": {"type": "string"}
    },
    "required": ["content", "agent_type"]
  }
}
```

### search_memories

```json
{
  "name": "search_memories",
  "description": "Search memories by semantic similarity",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {"type": "string"},
      "agent_type": {"type": "string"},
      "limit": {"type": "integer", "default": 10},
      "threshold": {"type": "number", "default": 0.7}
    },
    "required": ["query", "agent_type"]
  }
}
```

---

## Out of Scope

- WebSocket transport (future extension)
- Custom protocol extensions
- Authentication/authorization (future)

---

## References

- Python implementation: `nexus/server/mcp_server.py`
- MCP spec: https://modelcontextprotocol.io/
- rmcp: https://github.com/napi-rs/napi-rs

---

**Version:** 1.0
**Created:** 2025-02-16
