# Nexus Memory System

> **Private Cross-Agent Memory Management Platform**

A comprehensive memory management system for AI agents featuring automated extraction, semantic search, and cross-agent knowledge sharing.

**Version:** 1.1.0
**Status:** Private/Internal Use
**Host:** GitHub (for accessibility only - not open source)

---

## About This System

Nexus Memory System is a **private, internal-use system** hosted on GitHub for accessibility. It is **not open-source** and external contributions are not accepted.

### Key Features

- **Native Hooks System** - Automated memory extraction without MCP protocol
- **Hybrid Memory Types** - Nexus categories + Memory Lane cognitive types
- **High-Performance Search** - sqlite-vec embeddings with semantic search
- **Cross-Agent Sync** - Share memories between different AI agents
- **Web Dashboard** - REST API with real-time WebSocket updates
- **Automated Session Extraction** - Four-layer extraction system with 95-100% reliability

---

## Quick Start

### Installation (Rust - Recommended)

The Rust implementation provides significant performance improvements and is the recommended way to use Nexus.

```bash
# Build from source
git clone https://github.com/scooter-lacroix/nexus-memory-system
cd nexus-memory-system
cargo build --release

# The binary will be at ./target/release/nexus
# Optionally, install it:
cargo install --path crates/nexus-cli
```

### Initialize Database

```bash
nexus init
```

### Migrating from Python

If you have an existing Python Nexus database:

```bash
# Discover existing databases
nexus migrate discover

# Run migration
nexus migrate run

# Validate migration
nexus migrate validate
```

See [MIGRATION.md](MIGRATION.md) for detailed migration instructions.

### Installation (Python - Legacy)

The Python implementation is still available for backward compatibility.

```bash
# Install with uv
uv pip install nexus-memory-system[embeddings]

# Or with pip
pip install nexus-memory-system[embeddings]
```

### Install Agent Hooks

```bash
# Install hooks for all supported agents
nexus hooks install --all

# Or install for specific agent
nexus hooks install claude-code
```

### Start Web Dashboard

```bash
nexus serve --transport web
```

Visit http://localhost:8000 for the dashboard and http://localhost:8000/api/docs for API documentation.

---

## Architecture Overview

Nexus consists of **5 core components**:

```
┌─────────────────────────────────────────────────────────────┐
│                    NEXUS MEMORY SYSTEM                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │   Storage    │  │  Processing  │  │  Agent Hooks │       │
│  │   Manager    │  │    Engine    │  │   Manager    │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│          │                 │                  │             │
│          └─────────────────┼──────────────────┘             │
│                            ▼                                │
│                   ┌──────────────┐                          │
│                   │ Orchestrator │                          │
│                   └──────────────┘                          │
│                            │                                │
│                            ▼                                │
│                   ┌──────────────┐                          │
│                   │   Web        │                          │
│                   │  Dashboard   │                          │
│                   └──────────────┘                          │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

| Component | Description |
|-----------|-------------|
| **Storage Manager** | Database operations, CRUD, transactions |
| **Processing Engine** | Embeddings, NLP, categorization, vector search |
| **Agent Hooks Manager** | Native hooks, session detection, automated extraction |
| **Orchestrator** | Session lifecycle, event routing, cross-agent sync |
| **Web Dashboard** | HTTP API, WebSocket, UI, visualization |

---

## Directory Structure

```
nexus-memory-system/
├── crates/                    # Rust implementation (primary)
│   ├── nexus-core/           # Core types, traits, config
│   ├── nexus-storage/        # Database operations (SQLx)
│   ├── nexus-vectors/        # Vector search (sqlite-vec)
│   ├── nexus-embeddings/     # Embedding service (ORT)
│   ├── nexus-orchestrator/   # Session lifecycle, sync
│   ├── nexus-hooks/          # Native hooks system
│   ├── nexus-mcp/            # MCP server
│   ├── nexus-web/            # Web dashboard (Axum)
│   └── nexus-cli/            # Command-line interface
├── nexus/                    # Python implementation (legacy)
│   ├── database/             # Database models, managers
│   ├── embeddings/           # Embedding service
│   ├── hooks/                # Native hooks system
│   ├── orchestrator/         # Session lifecycle & sync
│   ├── services/             # Business logic
│   ├── web/                  # FastAPI web dashboard
│   └── cli.py                # Command-line interface
├── docs/                     # Documentation
├── tests/                    # Test suite
├── Cargo.toml                # Rust workspace config
├── pyproject.toml            # Python project config
└── README.md
```

---

## Supported Agents

| Agent | Hook Type | Status |
|-------|-----------|--------|
| **Claude Code** | Skills (Oct 2025) | Fully Supported |
| **Gemini** | Function Calling + CLI Extensions | Fully Supported |
| **Qwen** | Hooks SubAgent | Fully Supported |
| **Amp** | CLI atexit/signals | Fully Supported |
| **Droid** | CLI atexit/signals | Fully Supported |
| **OpenCode** | CLI atexit/signals | Fully Supported |
| **Codex** | CLI atexit/signals | Fully Supported |

---

## Usage Examples

### CLI Commands

```bash
# Store a memory
nexus store "User prefers dark mode" --agent claude-code --category preferences

# Search memories
nexus search "UI preferences" --agent claude-code --limit 5

# View statistics
nexus stats --agent claude-code

# Check hooks status
nexus hooks status --verbose
```

### Python API

```python
from nexus.server import get_memory_manager

# Initialize manager
manager = get_memory_manager()
await manager.initialize()

# Store memory
result = await manager.store_memory(
    content="User prefers dark mode in the UI",
    agent_type="claude-code",
    category="preferences",
    labels=["ui", "theme"],
    metadata={"source": "conversation"}
)

# Search memories
results = await manager.search_memories(
    query="UI theme preferences",
    agent_type="claude-code",
    limit=5
)
```

---

## Documentation

- **[MIGRATION.md](MIGRATION.md)** - Python to Rust migration guide
- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Complete architecture documentation
- **[INSTALLATION.md](INSTALLATION.md)** - Detailed installation guide
- **[HOOKS.md](HOOKS.md)** - Native hooks documentation
- **[docs/guide/getting-started.md](docs/guide/getting-started.md)** - Step-by-step tutorial
- **[docs/guide/memory-types.md](docs/guide/memory-types.md)** - Hybrid memory type guide
- **[docs/api/rest-api.md](docs/api/rest-api.md)** - REST API reference
- **[docs/api/cli-reference.md](docs/api/cli-reference.md)** - CLI command reference

---

## Memory Types

Nexus uses a **hybrid memory type system** combining:

### Nexus Categories (Core)

- `general` - General purpose memories
- `facts` - Factual information
- `preferences` - User preferences and settings
- `context` - Situational context
- `specifications` - Task specifications
- `session` - Session-based memories

### Memory Lane Cognitive Types (Additive)

- `semantic` - General knowledge
- `episodic` - Event-based experiences
- `procedural` - How-to processes
- `working` - Temporary active memory
- `explicit` - Conscious declarative facts
- `implicit` - Unconscious patterns
- `flashbulb` - High-importance events
- `metamemory` - Knowledge about memory
- `collective` - Cross-agent shared knowledge

### Memory Lane Priority Types

- `correction` - User corrected agent behavior (high priority)
- `decision` - Explicit choice with reasoning (high priority)
- `commitment` - User preference/commitment (high priority)
- `insight` - Non-obvious discovery (medium priority)
- `learning` - New knowledge gained (medium priority)
- `confidence` - Strong confidence (medium priority)
- `pattern_seed` - Repeated behavior (lower priority)
- `cross_agent` - Info relevant to other agents (lower priority)
- `workflow_note` - Process observation (lower priority)
- `gap` - Missing capability (lower priority)

---

## Automated Extraction System

Nexus uses a **four-layer automated extraction system**:

```
LAYER 1: Native Agent Hooks (PRIMARY)
  └─ Claude Code Skills, Gemini Functions, Qwen Hooks
  └─ Success Rate: 100% (when hooks work)

LAYER 2: Session Monitor (SECONDARY)
  └─ Process monitoring, state detection
  └─ Success Rate: 95%

LAYER 3: Inactivity Detector (TERTIARY)
  └─ Timeout detection (5 min default)
  └─ Success Rate: 90%

LAYER 4: Persistent Buffer (SAFETY NET)
  └─ Crash recovery from buffer
  └─ Success Rate: 99%
```

**Overall Reliability:** 95-100% memory capture, even when user forgets.

---

## Performance (Rust vs Python)

The Rust implementation provides significant performance improvements:

| Operation | Python | Rust | Improvement |
|-----------|--------|------|-------------|
| Embedding | ~10ms | <5ms | 2x faster |
| Vector Search (1k docs) | ~50ms | <10ms | 5x faster |
| Memory Store | ~5ms | <1ms | 5x faster |
| Concurrent Connections | ~100 | 10,000+ | 100x more |

Run benchmarks to verify:

```bash
cargo bench --workspace
```

---

## Development

**This is a private project.** External contributions are not accepted.

For internal contributors:
- See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines
- Use the issue tracker for bugs and feature requests
- Follow the existing code style and patterns

---

## License

MIT License - Internal Use Only

---

## Links

- **Repository:** https://github.com/scooter-lacroix/nexus-memory-system
- **Documentation:** https://github.com/scooter-lacroix/nexus-memory-system/tree/main/docs
- **Issues:** https://github.com/scooter-lacroix/nexus-memory-system/issues

---

**Note:** This system is for internal use only. GitHub hosting is for accessibility convenience.
