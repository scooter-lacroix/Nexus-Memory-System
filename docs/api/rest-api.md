# REST API Reference

> **Complete REST API Documentation**

**Version:** 1.1.0
**Base URL:** `http://localhost:8000/api/v1`

---

## Table of Contents

- [Overview](#overview)
- [Authentication](#authentication)
- [Memory Endpoints](#memory-endpoints)
- [Statistics Endpoints](#statistics-endpoints)
- [Hooks Endpoints](#hooks-endpoints)
- [WebSocket Events](#websocket-events)
- [Error Responses](#error-responses)

---

## Overview

The Nexus REST API provides HTTP endpoints for memory management, search, and system monitoring.

### Base URL

```
http://localhost:8000/api/v1
```

### API Documentation

Interactive API documentation is available at:
- **Swagger UI:** http://localhost:8000/api/docs
- **ReDoc:** http://localhost:8000/api/redoc
- **OpenAPI JSON:** http://localhost:8000/api/openapi.json

---

## Authentication

**Note:** Authentication is not enabled by default for local development. For production deployment, implement authentication middleware.

### Example: Adding API Key Authentication

```python
from fastapi import Security, HTTPException
from fastapi.security import APIKeyHeader

API_KEY_HEADER = APIKeyHeader(name="X-API-Key")

async def verify_api_key(api_key: str = Security(API_KEY_HEADER)):
    if api_key != os.getenv("NEXUS_API_KEY"):
        raise HTTPException(status_code=403, detail="Invalid API key")
    return api_key

# Use in endpoints
@app.post("/memories", dependencies=[Depends(verify_api_key)])
async def create_memory(...):
    ...
```

---

## Memory Endpoints

### Create Memory

Store a new memory in the system.

**Endpoint:** `POST /memories`

**Request Body:**

```json
{
  "content": "User prefers dark mode in the UI",
  "agent_type": "claude-code",
  "category": "preferences",
  "labels": ["ui", "theme"],
  "metadata": {
    "source": "conversation"
  },
  "memory_lane_type": null
}
```

**Parameters:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `content` | string | Yes | Memory content (1-10000 chars) |
| `agent_type` | string | No | Agent type (default: "general") |
| `category` | string | No | Memory category (default: "general") |
| `labels` | array | No | Optional labels |
| `metadata` | object | No | Additional metadata |
| `memory_lane_type` | string | No | Memory Lane type |

**Response:** `201 Created`

```json
{
  "success": true,
  "memory_id": 1,
  "agent_type": "claude-code",
  "category": "preferences",
  "error": null
}
```

**Example:**

```bash
curl -X POST http://localhost:8000/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{
    "content": "User prefers dark mode",
    "agent_type": "claude-code",
    "category": "preferences",
    "labels": ["ui", "theme"]
  }'
```

```python
import requests

response = requests.post(
    "http://localhost:8000/api/v1/memories",
    json={
        "content": "User prefers dark mode",
        "agent_type": "claude-code",
        "category": "preferences",
        "labels": ["ui", "theme"]
    }
)
print(response.json())
```

---

### Get Memory

Retrieve a specific memory by ID.

**Endpoint:** `GET /memories/{id}`

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | integer | Memory ID |

**Response:** `200 OK`

```json
{
  "success": true,
  "memory": {
    "id": 1,
    "content": "User prefers dark mode in the UI",
    "category": "preferences",
    "category_description": "User preferences and settings",
    "memory_lane_type": null,
    "labels": ["ui", "theme"],
    "metadata": {"source": "conversation"},
    "similarity_score": null,
    "relevance_score": null,
    "created_at": "2025-12-23T10:30:00Z",
    "last_accessed": "2025-12-23T11:00:00Z",
    "access_count": 5
  }
}
```

**Example:**

```bash
curl http://localhost:8000/api/v1/memories/1
```

---

### Update Memory

Update an existing memory.

**Endpoint:** `PUT /memories/{id}`

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | integer | Memory ID |

**Request Body:**

```json
{
  "content": "Updated content",
  "category": "facts",
  "labels": ["updated"],
  "metadata": {"updated": true},
  "is_active": true,
  "is_archived": false
}
```

**Response:** `200 OK`

```json
{
  "success": true,
  "memory": {
    "id": 1,
    "content": "Updated content",
    "category": "facts",
    ...
  }
}
```

---

### Delete Memory

Delete a memory by ID.

**Endpoint:** `DELETE /memories/{id}`

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | integer | Memory ID |

**Response:** `200 OK`

```json
{
  "success": true,
  "message": "Memory deleted successfully"
}
```

---

### List Memories

List memories with optional filtering.

**Endpoint:** `GET /memories`

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `agent_type` | string | "all" | Filter by agent type |
| `category` | string | null | Filter by category |
| `memory_lane_type` | string | null | Filter by Memory Lane type |
| `labels` | string | null | Filter by labels (comma-separated) |
| `limit` | integer | 100 | Maximum results |
| `offset` | integer | 0 | Pagination offset |
| `sort_by` | string | "created_at" | Sort field |
| `sort_order` | string | "desc" | Sort direction (asc/desc) |

**Response:** `200 OK`

```json
{
  "success": true,
  "total": 150,
  "results": [
    {
      "id": 1,
      "content": "User prefers dark mode",
      "category": "preferences",
      "created_at": "2025-12-23T10:30:00Z",
      ...
    }
  ],
  "query": null,
  "agent_type": "claude-code",
  "filters": {
    "category": null,
    "labels": null
  }
}
```

**Example:**

```bash
curl "http://localhost:8000/api/v1/memories?agent_type=claude-code&category=preferences&limit=10"
```

---

### Semantic Search

Search memories by semantic similarity.

**Endpoint:** `POST /memories/search`

**Request Body:**

```json
{
  "query": "UI theme preferences",
  "agent_type": "claude-code",
  "k": 10,
  "threshold": 0.7,
  "category": null,
  "memory_lane_type": null
}
```

**Parameters:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | Yes | Search query (1-500 chars) |
| `agent_type` | string | No | Agent type (default: "general") |
| `k` | integer | No | Max results (1-100, default: 10) |
| `threshold` | float | No | Min similarity (0.0-1.0) |
| `category` | string | No | Filter by category |
| `memory_lane_type` | string | No | Filter by Memory Lane type |

**Response:** `200 OK`

```json
{
  "success": true,
  "results": [
    {
      "id": 1,
      "content": "User prefers dark mode in the UI",
      "similarity_score": 0.89,
      "category": "preferences",
      "created_at": "2025-12-23T10:30:00Z",
      ...
    }
  ],
  "total": 5,
  "query": "UI theme preferences",
  "agent_type": "claude-code",
  "filters": {
    "threshold": 0.7
  }
}
```

**Example:**

```bash
curl -X POST http://localhost:8000/api/v1/memories/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "UI theme preferences",
    "agent_type": "claude-code",
    "k": 10
  }'
```

---

## Statistics Endpoints

### Get Statistics

Get memory statistics and system info.

**Endpoint:** `GET /stats`

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `agent_type` | string | "all" | Filter by agent type |

**Response:** `200 OK`

```json
{
  "success": true,
  "total_memories": 1500,
  "categories": {
    "general": 500,
    "preferences": 200,
    "facts": 300,
    "context": 250,
    "specifications": 150,
    "session": 100
  },
  "system_info": {
    "version": "1.1.0",
    "database_type": "sqlite",
    "embeddings_enabled": true,
    "hooks_enabled": true
  },
  "performance_metrics": {
    "avg_search_time_ms": 8.5,
    "total_searches": 1500,
    "cache_hit_rate": 0.85
  }
}
```

**Example:**

```bash
curl http://localhost:8000/api/v1/stats?agent_type=claude-code
```

---

## Hooks Endpoints

### Get Hooks Status

Get status of installed agent hooks.

**Endpoint:** `GET /hooks/status`

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `verbose` | boolean | false | Include detailed stats |

**Response:** `200 OK`

```json
{
  "success": true,
  "hooks": [
    {
      "agent_type": "claude-code",
      "installed": true,
      "monitoring": true,
      "hook_type": "Skills",
      "last_extraction": "2025-12-23T10:30:00Z",
      "extraction_count": 25,
      "error_count": 0,
      "last_error": null
    },
    {
      "agent_type": "gemini",
      "installed": true,
      "monitoring": true,
      "hook_type": "FunctionCalling",
      "last_extraction": "2025-12-23T09:15:00Z",
      "extraction_count": 10,
      "error_count": 1,
      "last_error": "Timeout waiting for response"
    }
  ]
}
```

**Example:**

```bash
curl http://localhost:8000/api/v1/hooks/status?verbose=true
```

---

## WebSocket Events

### Connect to Events Stream

Real-time event streaming via WebSocket.

**Endpoint:** `WS /ws/events`

**Connection:**

```javascript
const ws = new WebSocket('ws://localhost:8000/ws/events');

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log('Event:', data);
};

ws.onerror = (error) => {
  console.error('WebSocket error:', error);
};
```

**Event Types:**

| Event | Description |
|-------|-------------|
| `memory_stored` | New memory created |
| `memory_updated` | Memory updated |
| `memory_deleted` | Memory deleted |
| `session_start` | Agent session started |
| `session_end` | Agent session ended |
| `extraction_complete` | Memory extraction finished |

**Event Format:**

```json
{
  "event_type": "memory_stored",
  "timestamp": "2025-12-23T10:30:00Z",
  "data": {
    "memory_id": 1,
    "agent_type": "claude-code",
    "category": "preferences"
  }
}
```

---

## Error Responses

### Error Format

All errors follow this format:

```json
{
  "success": false,
  "error": "Error message",
  "detail": "Detailed error description",
  "status_code": 400
}
```

### Common HTTP Status Codes

| Code | Description |
|------|-------------|
| 200 | Success |
| 201 | Created |
| 400 | Bad Request |
| 404 | Not Found |
| 422 | Validation Error |
| 500 | Internal Server Error |

### Example Errors

**Validation Error (422):**

```json
{
  "success": false,
  "error": "Validation error",
  "detail": {
    "content": ["Field required"],
    "agent_type": ["Invalid agent type"]
  },
  "status_code": 422
}
```

**Not Found (404):**

```json
{
  "success": false,
  "error": "Memory not found",
  "detail": "Memory with ID 999 does not exist",
  "status_code": 404
}
```

---

## Python Client Example

```python
import requests
from typing import List, Dict, Any

class NexusClient:
    """Simple Nexus REST API client"""

    def __init__(self, base_url: str = "http://localhost:8000/api/v1"):
        self.base_url = base_url

    def create_memory(
        self,
        content: str,
        agent_type: str = "general",
        category: str = "general",
        labels: List[str] = None,
        metadata: Dict[str, Any] = None
    ) -> Dict:
        """Create a new memory"""
        response = requests.post(
            f"{self.base_url}/memories",
            json={
                "content": content,
                "agent_type": agent_type,
                "category": category,
                "labels": labels or [],
                "metadata": metadata or {}
            }
        )
        return response.json()

    def search_memories(
        self,
        query: str,
        agent_type: str = "general",
        k: int = 10
    ) -> Dict:
        """Semantic search"""
        response = requests.post(
            f"{self.base_url}/memories/search",
            json={
                "query": query,
                "agent_type": agent_type,
                "k": k
            }
        )
        return response.json()

    def get_memory(self, memory_id: int) -> Dict:
        """Get memory by ID"""
        response = requests.get(f"{self.base_url}/memories/{memory_id}")
        return response.json()

    def get_stats(self, agent_type: str = "all") -> Dict:
        """Get statistics"""
        response = requests.get(
            f"{self.base_url}/stats",
            params={"agent_type": agent_type}
        )
        return response.json()

# Usage
client = NexusClient()

# Store memory
result = client.create_memory(
    content="User prefers dark mode",
    agent_type="claude-code",
    category="preferences",
    labels=["ui", "theme"]
)
print(f"Stored memory ID: {result['memory_id']}")

# Search
results = client.search_memories(
    query="UI preferences",
    agent_type="claude-code",
    k=5
)
for memory in results["results"]:
    print(f"{memory['similarity_score']:.2f}: {memory['content']}")
```

---

## Related Documentation

- [Getting Started Guide](../guide/getting-started.md) - Tutorial
- [CLI Reference](cli-reference.md) - CLI commands
- [ARCHITECTURE.md](../../ARCHITECTURE.md) - System architecture

---

**Last Updated:** 2025-12-23
