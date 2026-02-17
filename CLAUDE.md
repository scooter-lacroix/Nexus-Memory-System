# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Nexus Memory System is a cross-agent memory management platform for AI agents. It provides automated memory extraction, semantic search with vector embeddings, and cross-agent knowledge sharing.

**Current Stack (Python):** Python 3.9+, SQLAlchemy, FastAPI, sqlite-vec, sentence-transformers, FastMCP

**Target Stack (Rust):** Rust 1.75+, SQLx/SeaORM, Axum, sqlite-vec, candle-transformers (or ort), rmcp

## Development Commands

```bash
# Python (current implementation)
pip install -e ".[dev]"                    # Dev dependencies
pip install -e ".[dev,embeddings]"         # With embedding support

# Testing
pytest                                     # Run all tests
pytest -x -v tests/unit/                   # Unit tests only
pytest -x -v tests/integration/            # Integration tests only
pytest --cov=nexus --cov-report=term       # With coverage

# Code Quality
ruff check nexus/ tests/                   # Lint
black nexus/ tests/ && ruff format nexus/ tests/  # Format
mypy nexus/                                # Type check
make quality                               # lint + type-check + test

# Database
nexus init                                 # Initialize database
nexus init --reset                         # Reset and reinitialize

# Server
nexus serve --transport web                # Web dashboard (port 8768)
nexus serve --transport stdio              # MCP stdio transport
nexus serve --transport http               # MCP HTTP transport

# Hooks Management
nexus hooks install --all                  # Install hooks for all agents
nexus hooks install claude-code            # Install for specific agent
nexus hooks status --verbose               # Check hook status

# Memory Operations
nexus store "content" --agent claude-code --category preferences
nexus search "query" --agent claude-code --limit 5
nexus stats --agent claude-code
```

## Architecture

### Core Components (5 layers)

```
nexus/
├── database/          # Storage Manager - SQLAlchemy models, managers, migrations
│   ├── models.py      # Memory, AgentNamespace, TaskSpecification, MemoryRelation
│   ├── managers.py    # DatabaseManager, MemoryManager, SpecificationManager
│   └── enums.py       # Hybrid memory type system (Nexus + Memory Lane types)
│
├── embeddings/        # Processing Engine - Vector embeddings
│   ├── service.py     # EmbeddingService (all-MiniLM-L6-v2, 384-dim vectors)
│   └── sqlite_vec.py  # Vector search operations
│
├── hooks/             # Agent Hooks - Automated extraction system
│   ├── base.py        # AgentHook abstract base class
│   ├── factory.py     # Hook factory for creating agent-specific hooks
│   ├── claude.py      # Claude Code Skills hooks
│   ├── gemini.py      # Gemini Function Calling hooks
│   ├── qwen.py        # Qwen Hooks SubAgent
│   ├── cli.py         # Generic CLI hooks (atexit/signals)
│   ├── monitor.py     # Session monitoring
│   ├── detector.py    # Multi-layer session detection
│   └── buffer.py      # Persistent buffer for crash recovery
│
├── orchestrator/      # Coordination Layer - Session lifecycle & sync
│   ├── orchestrator.py    # Main Orchestrator class
│   ├── session_tracker.py # Session state management
│   ├── event_bus.py       # Event-driven architecture
│   └── sync.py            # Cross-agent memory synchronization
│
├── services/          # Business Logic
│   └── hooks_manager.py   # Hooks orchestration
│
├── web/               # Web Dashboard - FastAPI
│   ├── app.py             # FastAPI application factory
│   ├── routes/memories.py # Memory CRUD endpoints
│   ├── routes/stats.py    # Statistics endpoints
│   └── websocket/manager.py # WebSocket real-time updates
│
├── server/            # MCP Server & Main Manager
│   ├── nexus_manager.py   # NexusManager - main business logic coordinator
│   └── mcp_server.py      # FastMCP server implementation
│
├── config/            # Configuration
│   └── settings.py        # ServerConfig with environment variables
│
└── cli.py             # Command-line interface (Click)
```

### Data Flow

1. **Memory Storage:** Agent → HooksManager → ProcessingEngine (embeddings) → StorageManager → Orchestrator (events/cross-agent sync)
2. **Memory Search:** Query → ProcessingEngine (query embedding) → StorageManager (vector search) → Orchestrator (context enhancement)

### Hybrid Memory Type System

The system uses a two-field approach:
- `category`: Required Nexus category (general, facts, preferences, context, specifications, session)
- `memory_lane_type`: Optional Memory Lane type (cognitive or priority types)

**Memory Lane Priority Types:** correction, decision, commitment (high) | insight, learning, confidence (medium) | pattern_seed, cross_agent, workflow_note, gap (low)

### Four-Layer Extraction System

1. **Native Hooks** (100% success): Claude Skills, Gemini Functions, Qwen Hooks
2. **Session Monitor** (95%): Process monitoring via psutil
3. **Inactivity Detector** (90%): 5-min timeout detection
4. **Persistent Buffer** (99%): Crash recovery from buffer

## Key Patterns

### Async Initialization Pattern
Most managers use lazy async initialization:
```python
manager = NexusManager()
await manager.initialize()  # or await manager.ensure_initialized()
```

### Sync Wrapper Pattern
For FastMCP compatibility, async methods have sync wrappers:
```python
def store_memory_sync(self, *args, **kwargs):
    return asyncio.run(self.store_memory(*args, **kwargs))
```

### Singleton Pattern
Global instances via module-level functions:
```python
_nexus_manager: Optional[NexusManager] = None
def get_memory_manager() -> NexusManager:
    global _nexus_manager
    if _nexus_manager is None:
        _nexus_manager = NexusManager()
    return _nexus_manager
```

## Database Schema

- **memories**: Core memory storage with embeddings, category, memory_lane_type, labels, metadata
- **agent_namespaces**: Per-agent isolation (one namespace per agent type)
- **task_specifications**: Reusable task specs with complexity scoring
- **memory_relations**: Relationships between memories (similar, related, parent/child)
- **system_metrics**: Monitoring and analytics

## Configuration

Environment variables (prefix `NEXUS_`):
- `NEXUS_DATABASE_PATH`: SQLite database location
- `NEXUS_HOST`, `NEXUS_PORT`: Server binding
- `NEXUS_WEB_PORT`: Web dashboard port (default 8768)
- `NEXUS_EMBEDDINGS_ENABLED`: Enable/disable embeddings
- `NEXUS_EMBEDDING_MODEL`: Model name (default all-MiniLM-L6-v2)
- `NEXUS_SYNC_POLICY`: Cross-agent sync policy (manual, auto, aggressive)

## Important Files

- `nexus/server/nexus_manager.py`: Main business logic entry point
- `nexus/database/managers.py`: All database operations
- `nexus/orchestrator/orchestrator.py`: Session and sync coordination
- `nexus/hooks/factory.py`: Agent hook creation logic
- `nexus/cli.py`: CLI command definitions

## Supported Agents

| Agent | Hook Type | Implementation |
|-------|-----------|----------------|
| Claude Code | Skills (Oct 2025) | `nexus/hooks/claude.py` |
| **pi-mono** | Skills (TypeScript/Bun) | `nexus/hooks/pi_mono.py` |
| **oh-my-pi** | Skills (TypeScript/Bun + Rust N-API) | `nexus/hooks/oh_my_pi.py` |
| **pi-skills** | Skills (Cross-compatible) | `nexus/hooks/pi_skills.py` |
| Gemini | Function Calling | `nexus/hooks/gemini.py` |
| Qwen | Hooks SubAgent | `nexus/hooks/qwen.py` |
| Amp, Droid, OpenCode, Codex | CLI atexit/signals | `nexus/hooks/cli.py` |

### Pi Agent Family (MANDATORY FULL SUPPORT)

The following agents MUST be fully supported with native hooks implementation:

#### pi-mono (badlogic/pi-mono)
- **Repository:** https://github.com/badlogic/pi-mono
- **Stack:** TypeScript, Bun runtime
- **Config Dirs:** `~/.pi/`, `.pi/`, `~/.pi/agent/`, `.pi/agent/`
- **Skills Dir:** `~/.pi/agent/skills/`, `.pi/skills/`
- **Hook Type:** Skills-based (SKILL.md format, compatible with Claude Code)
- **Key Features:** Multi-provider LLM, TUI library, agent runtime, Slack bot

#### oh-my-pi (can1357/oh-my-pi) - Fork of pi-mono
- **Repository:** https://github.com/can1357/oh-my-pi
- **Stack:** TypeScript, Bun runtime, Rust N-API native addon (~7,500 lines)
- **Config Dirs:** `~/.omp/`, `.omp/`, `~/.omp/agent/`, `.omp/agent/`
- **Skills Dir:** `~/.omp/agent/skills/`, `.omp/skills/`
- **Hook Type:** Skills-based (SKILL.md format, TTSR - Time Traveling Streamed Rules)
- **Key Features:**
  - Native Rust engine: grep, shell, text, keys, highlight, glob, task, ps, prof, clipboard
  - LSP integration with format-on-write
  - Browser automation (Puppeteer with stealth)
  - Task tool (subagent system)
  - Universal config discovery (8 AI tools)
  - MCP plugin system

#### pi-skills (badlogic/pi-skills)
- **Repository:** https://github.com/badlogic/pi-skills
- **Compatibility:** pi-mono, oh-my-pi, Claude Code, Codex CLI, Amp, Droid
- **Skills:** brave-search, browser-tools, gccli, gdcli, gmcli, transcribe, vscode, youtube-transcript
- **Format:** SKILL.md with `{baseDir}` placeholder

### Pi Agent Hook Implementation Requirements

```python
# nexus/hooks/pi_mono.py
class PiMonoHook(AgentHook):
    """
    Hook for pi-mono coding agent

    Config paths:
    - ~/.pi/agent/skills/ (user-level skills)
    - .pi/skills/ (project-level skills)
    - ~/.pi/agent/config.json (agent config)

    Session detection:
    - Process: `pi` or `pi-coding-agent`
    - Session files: ~/.pi/sessions/
    """
    AGENT_TYPE = "pi-mono"
    CONFIG_DIR = Path.home() / ".pi"
    SKILLS_DIR = CONFIG_DIR / "agent" / "skills"

# nexus/hooks/oh_my_pi.py
class OhMyPiHook(AgentHook):
    """
    Hook for oh-my-pi coding agent (pi-mono fork)

    Config paths:
    - ~/.omp/agent/skills/ (user-level skills)
    - .omp/skills/ (project-level skills)
    - ~/.omp/agent/config.json (agent config)
    - ~/.omp/logs/ (centralized logs)

    Session detection:
    - Process: `omp` or `oh-my-pi`
    - Session files: ~/.omp/sessions/

    Native features (Rust N-API):
    - grep, shell, text, keys, highlight, glob, task, ps, prof, clipboard
    """
    AGENT_TYPE = "oh-my-pi"
    CONFIG_DIR = Path.home() / ".omp"
    SKILLS_DIR = CONFIG_DIR / "agent" / "skills"

# nexus/hooks/pi_skills.py
class PiSkillsHook(AgentHook):
    """
    Hook for pi-skills compatible agents

    Supports skills from badlogic/pi-skills repository
    Compatible with: pi-mono, oh-my-pi, Claude Code, Codex CLI, Amp, Droid
    """
    AGENT_TYPE = "pi-skills"
```

### Skill Format (SKILL.md)

```markdown
---
name: skill-name
description: Short description shown to agent
triggers:  # Optional for TTSR (oh-my-pi)
  - on_session_end
  - on_checkpoint
---

# Instructions

Detailed instructions here...
Helper files available at: {baseDir}/
```

---

## Rust Port Guide

### Recommended Crate Mapping

| Python Package | Rust Crate | Notes |
|----------------|------------|-------|
| SQLAlchemy 2.0 | SQLx or SeaORM | SQLx for raw speed, SeaORM for ORM features |
| FastAPI | Axum | Async-first, tower middleware ecosystem |
| Pydantic | serde + validator | Derive macros for validation |
| sentence-transformers | candle-transformers or ort | ort = ONNX Runtime bindings |
| sqlite-vec | sqlite-vec-sys | FFI bindings, same C library |
| FastMCP | rmcp | Rust MCP implementation |
| asyncio | tokio | Async runtime |
| loguru | tracing | Structured logging |
| click | clap | CLI with derive macros |
| rich | comfy-table or termimad | Terminal formatting |
| websockets | tokio-tungstenite | WebSocket support |
| httpx | reqwest | HTTP client |
| aiosqlite | sqlx::Sqlite | Async SQLite |

### Type Mappings

```rust
// Database models (from models.py)
struct Memory {
    id: i32,
    namespace_id: i32,
    content: String,
    category: String,           // enum in Rust
    memory_lane_type: Option<MemoryLaneType>,
    labels: Vec<String>,        // JSON array
    extra_metadata: serde_json::Value,
    similarity_score: Option<f32>,
    relevance_score: Option<f32>,
    content_embedding: Option<Vec<f32>>,  // 384-dim vector
    embedding_model: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
    last_accessed: Option<DateTime<Utc>>,
    is_active: bool,
    is_archived: bool,
    access_count: i32,
}

struct AgentNamespace {
    id: i32,
    name: String,
    description: Option<String>,
    agent_type: String,
    created_at: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
}

struct TaskSpecification {
    id: i32,
    namespace_id: i32,
    spec_id: String,
    task_description: String,
    spec_content: serde_json::Value,
    complexity_score: f32,
    usage_count: i32,
    success_rate: f32,
    // ... other fields
}

struct MemoryRelation {
    id: i32,
    source_memory_id: i32,
    target_memory_id: i32,
    relation_type: String,      // enum in Rust
    strength: f32,
    extra_metadata: Option<serde_json::Value>,
}
```

### Architecture Translation

```
src/
├── db/
│   ├── mod.rs
│   ├── models.rs       # SeaORM entities or SQLx queries
│   ├── repository.rs   # Database operations (MemoryRepository, etc.)
│   └── migrations/     # SQL migrations
│
├── embedding/
│   ├── mod.rs
│   ├── service.rs      # EmbeddingService trait + implementation
│   └── sqlite_vec.rs   # Vector search operations
│
├── hooks/
│   ├── mod.rs
│   ├── base.rs         # AgentHook trait
│   ├── factory.rs      # Hook factory
│   ├── claude.rs       # Claude Code hooks
│   ├── gemini.rs       # Gemini hooks
│   ├── qwen.rs         # Qwen hooks
│   ├── cli.rs          # CLI hooks (signal handling)
│   ├── monitor.rs      # Process monitoring
│   ├── detector.rs     # Session detection
│   └── buffer.rs       # Persistent buffer
│
├── orchestrator/
│   ├── mod.rs
│   ├── orchestrator.rs # Main Orchestrator
│   ├── session.rs      # Session tracking
│   ├── event_bus.rs    # Event system (tokio::sync::broadcast)
│   └── sync.rs         # Cross-agent sync
│
├── services/
│   ├── mod.rs
│   └── hooks_manager.rs
│
├── web/
│   ├── mod.rs
│   ├── app.rs          # Axum app factory
│   ├── routes/
│   │   ├── memories.rs
│   │   └── stats.rs
│   └── websocket.rs    # WebSocket handler
│
├── server/
│   ├── mod.rs
│   ├── nexus_manager.rs # Main manager (Arc<Mutex<NexusManager>>)
│   └── mcp_server.rs    # rmcp implementation
│
├── config/
│   ├── mod.rs
│   └── settings.rs      # Config struct with clap/figment
│
├── cli.rs               # CLI with clap
└── main.rs
```

### Key Rust Patterns

```rust
// Singleton pattern (lazy_static or once_cell)
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::RwLock;

static NEXUS_MANAGER: Lazy<Arc<RwLock<NexusManager>>> = Lazy::new(|| {
    Arc::new(RwLock::new(NexusManager::new()))
});

// Async initialization
impl NexusManager {
    pub async fn initialize(&mut self) -> Result<(), Error> {
        // ...
    }

    pub async fn ensure_initialized(&self) -> Result<(), Error> {
        // Check and init if needed
    }
}

// Sync wrapper for blocking contexts
impl NexusManager {
    pub fn store_memory_blocking(&self, args: StoreArgs) -> Result<Memory, Error> {
        tokio::task::block_in_place(|| {
            Handle::current().block_on(self.store_memory(args))
        })
    }
}

// Event bus (tokio broadcast channel)
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn publish(&self, event: Event) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
}

// Embedding service trait
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn encode(&self, texts: &[str]) -> Result<Vec<Vec<f32>>, Error>;
    fn dimension(&self) -> usize { 384 }
}

// Hook trait
#[async_trait]
pub trait AgentHook: Send + Sync {
    async fn install_session_end_hook(&mut self, callback: Box<dyn Fn(String) + Send>) -> Result<(), Error>;
    async fn detect_session_activity(&self) -> bool;
    async fn extract_session_context(&self) -> Result<SessionContext, Error>;
}
```

### Embedding Considerations

- **Model:** all-MiniLM-L6-v2 (384 dimensions)
- **Options:**
  1. `candle-transformers` - Pure Rust, slower inference
  2. `ort` (ONNX Runtime) - FFI, faster, requires ONNX model export
  3. External API call (OpenAI, local inference server)

```rust
// Example with ort
pub struct OrtEmbeddingService {
    session: ort::Session,
    tokenizer: Tokenizer,
}

impl OrtEmbeddingService {
    pub async fn encode(&self, text: &str) -> Result<Vec<f32>, Error> {
        let tokens = self.tokenizer.encode(text)?;
        let outputs = self.session.run(ort::inputs![tokens]?)?;
        // Extract embedding from outputs
    }
}
```

### Performance Targets

| Metric | Python | Rust Target |
|--------|--------|-------------|
| Embedding latency | ~10ms | <5ms (with ort) |
| Vector search (1k docs) | ~50ms | <10ms |
| Memory store | ~5ms | <1ms |
| Concurrent connections | ~100 | ~10,000+ |

### Critical Files for Port Priority

1. `nexus/database/models.py` - Define Rust structs first
2. `nexus/database/managers.py` - Core CRUD operations
3. `nexus/server/nexus_manager.py` - Main business logic
4. `nexus/embeddings/service.py` - Embedding service
5. `nexus/config/settings.py` - Configuration
6. `nexus/orchestrator/orchestrator.py` - Coordination
7. `nexus/hooks/` - Hook system (lower priority, can use CLI hooks initially)

### Testing Strategy

```bash
# Rust testing
cargo test                              # All tests
cargo test --lib                        # Unit tests only
cargo test --test integration           # Integration tests
cargo bench                             # Benchmarks
cargo tarpaulin --out Html              # Coverage
```
