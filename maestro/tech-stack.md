# Technology Stack: Nexus Memory System

This document defines the technology stack for Nexus Memory System across both the current Python implementation and the planned Rust port.

---

## Current Implementation (Python)

### Core Runtime

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Language** | Python | 3.9+ | Core runtime |
| **Async Runtime** | asyncio | stdlib | Asynchronous operations |
| **Database Driver** | aiosqlite | latest | Async SQLite adapter |

### Web & API

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Web Framework** | FastAPI | latest | REST API & WebSocket |
| **MCP Framework** | FastMCP | latest | Model Context Protocol server |
| **CLI** | Click | latest | Command-line interface |

### Data & Storage

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **ORM** | SQLAlchemy | 2.0+ | Database abstraction |
| **Vector Search** | sqlite-vec | latest | Semantic similarity search |
| **Embeddings** | sentence-transformers | latest | Vector embeddings (all-MiniLM-L6-v2, 384-dim) |

### Development Tools

| Tool | Purpose |
|------|---------|
| **uv** | Package management (recommended) |
| **pytest** | Test runner |
| **pytest-cov** | Coverage reporting |
| **pytest-asyncio** | Async test support |
| **ruff** | Fast linting |
| **black** | Code formatting |
| **mypy** | Type checking |
| **loguru** | Structured logging |

---

## Target Implementation (Rust Port)

### Core Runtime

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Language** | Rust | 1.75+ | Core runtime |
| **Async Runtime** | tokio | latest | Async operations |

### Web & API

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Web Framework** | Axum | latest | REST API & WebSocket |
| **MCP Framework** | rmcp | latest | Rust MCP implementation |

### Data & Storage

| Component | Technology | Version | Purpose |
|-----------|-----------|---------|---------|
| **Database** | SQLx | latest | Compile-time checked SQL |
| **ORM (Optional)** | SeaORM | latest | ORM abstraction |
| **Vector DB** | sqlite-vec (Rust) | latest | High-performance vector search |
| **Embeddings** | candle-transformers / ort | latest | On-device inference |

### Token Efficiency (MANDATORY)

| Component | Integration Path | Purpose |
|-----------|------------------|---------|
| **LePhase** | `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/` | Token-efficient memory compression |

### Performance Targets

| Metric | Python Baseline | Rust Target |
|--------|-----------------|-------------|
| **Search Latency** | ~50-100ms | <10ms |
| **Concurrent Agents** | ~1,000 | 10,000+ |
| **Memory Overhead** | ~200MB base | ~50MB base |

---

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│                      5-LAYER ARCHITECTURE                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │   Storage    │  │  Processing  │  │  Agent Hooks │       │
│  │   Manager    │  │    Engine    │  │   Manager    │       │
│  │              │  │              │  │              │       │
│  │ SQLAlchemy   │  │ sentence-    │  │ Native Hooks │       │
│  │ / SQLx       │  │ transformers │  │ / Skills     │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│          │                 │                  │             │
│          └─────────────────┼──────────────────┘             │
│                            ▼                                │
│                   ┌──────────────┐                          │
│                   │ Orchestrator │                          │
│                   │              │                          │
│                   │ Event Bus    │                          │
│                   │ Session Mgmt │                          │
│                   └──────────────┘                          │
│                            │                                │
│                            ▼                                │
│                   ┌──────────────┐                          │
│                   │   Web        │                          │
│                   │  Dashboard   │                          │
│                   │              │                          │
│                   │ FastAPI/     │                          │
│                   │ Axum         │                          │
│                   └──────────────┘                          │
└─────────────────────────────────────────────────────────────┘
```

---

## Python vs. Rust: Key Differences

| Aspect | Python | Rust |
|--------|--------|------|
| **Embedding Inference** | sentence-transformers (Python) | candle-transformers / ort |
| **Vector Search** | sqlite-vec (Python bindings) | sqlite-vec (native Rust) |
| **Memory Safety** | GC-managed | Compile-time guaranteed |
| **Concurrency** | asyncio | tokio (true parallelism) |
| **Distribution** | Wheel + source | Binary + cargo |

---

## Dependency Management

### Python (uv)

```bash
# Install with uv
uv pip install -e ".[dev,embeddings]"

# Development dependencies
uv pip install -e ".[dev]"
```

### Rust (cargo)

```bash
# Build release
cargo build --release

# Run tests
cargo test --all-features

# Install locally
cargo install --path .
```

---

## Supported Agent Integrations

| Agent | Hook Type | Python Status | Rust Status |
|-------|-----------|---------------|-------------|
| **Claude Code** | Skills (Oct 2025) | ✅ Supported | ✅ Planned |
| **Gemini** | Function Calling | ✅ Supported | ✅ Planned |
| **Qwen** | Hooks SubAgent | ✅ Supported | ✅ Planned |
| **pi-mono** | Skills (TypeScript/Bun) | ✅ Supported | ✅ MANDATORY |
| **oh-my-pi** | Skills (TS/Bun + Rust N-API) | ✅ Supported | ✅ MANDATORY |
| **pi-skills** | Skills (Cross-compatible) | ✅ Supported | ✅ MANDATORY |

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEXUS_DATABASE_PATH` | `~/.nexus/nexus.db` | SQLite database location |
| `NEXUS_HOST` | `127.0.0.1` | Server bind address |
| `NEXUS_PORT` | `8000` | Server port |
| `NEXUS_WEB_PORT` | `8768` | Web dashboard port |
| `NEXUS_EMBEDDINGS_ENABLED` | `true` | Enable embedding generation |
| `NEXUS_EMBEDDING_MODEL` | `all-MiniLM-L6-v2` | Embedding model name |
| `NEXUS_SYNC_POLICY` | `manual` | Cross-agent sync policy |

---

## MANDATORY Requirements for Rust Port

1. **Vector Database with Graph Tree Structure**
   - Efficient resource management
   - High-accuracy semantic search
   - Performance-optimized storage

2. **LLM-Triggered Hooks**
   - Native hooks in ALL supported CLI tools
   - Seamless integration with agent workflows
   - Automated memory extraction

3. **LePhase Integration**
   - Token-efficient memory compression during storage
   - Optimized retrieval/presentation to models
   - Reference: `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/`

---

**Version:** 1.0
**Last Updated:** 2025-02-16
