# Spec: Rust Web Dashboard

**Track ID:** rust-web-dashboard_20250216
**Type:** Feature
**Status:** New

---

## Overview

Implement the web dashboard in Rust using Axum framework. Includes REST API endpoints, WebSocket real-time updates, and static file serving. Port 8768 compatibility.

**Python Mapping:** `nexus/web/`

---

## Functional Requirements

### FR1: Axum Web Application

```rust
pub struct WebDashboard {
    router: Router,
    manager: Arc<RwLock<NexusManager>>,
}

impl WebDashboard {
    pub fn new(manager: Arc<RwLock<NexusManager>>) -> Router;
    pub async fn serve(self, addr: SocketAddr) -> Result<(), Error>;
}
```

### FR2: REST API Endpoints

#### Memories

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/memories` | List memories (with filters) |
| POST | `/api/memories` | Store new memory |
| GET | `/api/memories/:id` | Get memory by ID |
| PUT | `/api/memories/:id` | Update memory |
| DELETE | `/api/memories/:id` | Delete memory |
| POST | `/api/memories/search` | Semantic search |

#### Namespaces

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/namespaces` | List all namespaces |
| GET | `/api/namespaces/:id` | Get namespace details |
| POST | `/api/namespaces` | Create namespace |

#### Statistics

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/stats/:agent` | Get agent statistics |
| GET | `/api/stats` | Get global statistics |

### FR3: WebSocket Real-Time Updates

```rust
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(manager): State<Arc<RwLock<NexusManager>>>,
) -> Response {
    // Subscribe to event bus and push updates
}
```

Updates include:
- New memories stored
- Memory updates/deletions
- Session events
- Statistics changes

### FR4: Static File Serving

- Serve dashboard UI from `/`
- Support for SPA routing
- Asset caching headers

### FR5: CORS and Security

- CORS configuration for cross-origin requests
- Request validation
- Rate limiting per IP

---

## Non-Functional Requirements

### NFR1: Performance

| Metric | Target |
|--------|--------|
| API response latency | <50ms (p95) |
| WebSocket message latency | <10ms |
| Concurrent connections | 1000+ |

### NFR2: Compatibility

- Port 8768 (same as Python version)
- API response format identical
- WebSocket message format compatible

### NFR3: Code Quality

- 95%+ test coverage
- Proper error responses
- Request logging

---

## Acceptance Criteria

### AC1: Server Starts

```rust
let dashboard = WebDashboard::new(manager);
let addr = SocketAddr::from(([0, 0, 0, 0], 8768));
dashboard.serve(addr).await?;
```

### AC2: API Endpoints Functional

```bash
# Store memory
curl -X POST http://localhost:8768/api/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "test", "agent_type": "claude-code"}'

# Search memories
curl -X POST http://localhost:8768/api/memories/search \
  -H "Content-Type: application/json" \
  -d '{"query": "test", "agent_type": "claude-code"}'
```

### AC3: WebSocket Connected

```javascript
const ws = new WebSocket('ws://localhost:8768/api/ws');
ws.onmessage = (event) => {
    const update = JSON.parse(event.data);
    console.log('Received update:', update);
};
```

### AC4: Static Files Served

```bash
curl http://localhost:8768/
# Returns index.html

curl http://localhost:8768/assets/style.css
# Returns CSS file
```

---

## Dependencies

### External Crates

```toml
[dependencies]
axum = "0.7"
tokio = { version = "1.40", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.5", features = ["cors", "fs"] }
serde_json = "1.0"
async-trait = "0.1"
```

### Local Dependencies

- `nexus-core` - Core types
- `nexus-storage` - Database operations
- `nexus-orchestrator` - Event subscription

---

## API Response Format

### Memory Response

```json
{
  "id": 123,
  "content": "User prefers dark mode",
  "category": "preferences",
  "memory_lane_type": null,
  "labels": ["ui", "theme"],
  "metadata": {},
  "similarity_score": 0.95,
  "relevance_score": 0.92,
  "created_at": "2025-02-16T22:00:00Z",
  "updated_at": "2025-02-16T22:00:00Z",
  "last_accessed": "2025-02-16T22:00:00Z",
  "is_active": true,
  "is_archived": false,
  "access_count": 5
}
```

### WebSocket Message Format

```json
{
  "type": "memory_stored",
  "data": {
    "memory": { ... },
    "agent_type": "claude-code"
  },
  "timestamp": "2025-02-16T22:00:00Z"
}
```

---

## Out of Scope

- Dashboard UI implementation (use existing Python UI files)
- Authentication system (future)
- Advanced analytics (future)

---

## References

- Python implementation: `nexus/web/`
- Axum docs: https://docs.rs/axum/
- CLAUDE.md: Rust Port Guide

---

**Version:** 1.0
**Created:** 2025-02-16
