# Nexus Memory System

Nexus Memory System is a Rust memory platform for AI coding agents. It provides a shared SQLite-backed memory store, a CLI for day-to-day operations, native hook installers for supported agent tools, an MCP server, and an Axum-based web surface.

## Brief Description

Nexus gives multiple coding agents one consistent memory layer. It is designed for:

- shared memory storage across agent namespaces
- structured categories such as `general`, `facts`, `preferences`, `context`, `specifications`, and `session`
- agent hook installation and status management
- search, stats, and migration-oriented operational workflows
- HTTP, WebSocket, and MCP access patterns on top of the same storage layer

## Highlights

- Current Rust workspace layout separates core types, storage, vectors, embeddings, orchestration, hooks, web, MCP, and CLI concerns
- User-level installer that creates a shared `nexus` runtime for local agent tools
- SQLite-based persistence with repository-style access through `nexus-storage`
- Native hook management for Claude Code, Gemini, Qwen, Codex, OpenCode, Amp, and Droid
- Web dashboard and API routes under the `nexus-web` crate
- Optional always-on memory agent with LLM-driven ingest, consolidation, and query (OpenAI, Anthropic, Gemini, and more)

## Quick Start

### Build and install

```bash
git clone https://github.com/scooter-lacroix/Nexus-Memory-System.git
cd Nexus-Memory-System
cargo build --release -p nexus-memory
./scripts/install.sh --binary ./target/release/nexus
```

### Initialize storage

```bash
nexus init
```

### Store a memory

```bash
nexus store \
  --content "Codex completed release validation" \
  --agent codex \
  --category session \
  --labels release,validation
```

### Search stored memories

```bash
nexus search --query "release validation" --agent codex --limit 5
```

### Inspect tool help and schemas

```bash
nexus tools help
nexus tools help store_memory
nexus tools schema store_memory
```

### Inspect system statistics

```bash
nexus stats
```

## Usage Examples

### Install hooks for all supported agents

```bash
nexus hooks install --agent all
nexus hooks status
```

### Start the HTTP server

```bash
nexus serve --transport http --port 8768
```

### Start the web server with the always-on agent

```bash
# Set your LLM provider API key
export OPENAI_API_KEY=sk-...

# Start with agent enabled
NEXUS_AGENT_ENABLED=true nexus serve --transport web --port 8768 --agent
```

### Run the MCP-compatible stdio server

```bash
nexus serve --transport stdio
```

MCP clients can also call `tool_help` and `tool_schema` if they need tool usage details or input schemas at runtime.

### Run a quick smoke test from a local build

```bash
./target/release/nexus init --reset
./target/release/nexus store --content "smoke test" --agent codex --category session
./target/release/nexus stats
```

## Architecture At A Glance

```text
Agents and Tools
    |
    +-- nexus-cli
    +-- nexus-hooks
    +-- nexus-mcp
    +-- nexus-web
            |
            v
        nexus-core
            |
            +-- nexus-storage
            +-- nexus-vectors
            +-- nexus-embeddings
            +-- nexus-orchestrator
            +-- nexus-llm
            +-- nexus-agent
```

The shared domain model lives in `nexus-core`. Storage and repositories live in `nexus-storage`. Higher-level surfaces such as the CLI, hooks, MCP server, and web dashboard build on that foundation. The optional always-on agent (`nexus-llm` + `nexus-agent`) adds LLM-driven memory enrichment, consolidation, and query synthesis. The product is documented as one system even though the current implementation is organized as multiple crates.

## Repository Layout

```text
.
├── crates/
│   ├── nexus-agent/
│   ├── nexus-cli/
│   ├── nexus-core/
│   ├── nexus-embeddings/
│   ├── nexus-hooks/
│   ├── nexus-lephase/
│   ├── nexus-llm/
│   ├── nexus-mcp/
│   ├── nexus-orchestrator/
│   ├── nexus-storage/
│   ├── nexus-vectors/
│   └── nexus-web/
├── docs/
├── scripts/
├── Cargo.toml
└── Cargo.lock
```

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [INSTALLATION.md](INSTALLATION.md)
- [DEVELOPMENT.md](DEVELOPMENT.md)
- [HOOKS.md](HOOKS.md)
- [CONTRIBUTING.md](CONTRIBUTING.md)
- [docs/index.md](docs/index.md)

## LLM Provider Evaluation

The `nexus eval` command tests an LLM provider against the memory system's core operations: structured extraction, consolidation, and query synthesis. Each aspect is scored out of 200 (total 600).

| Provider / Model | Extraction | Consolidation | Query | Total | Rating |
|---|---|---|---|---|---|
| OpenRouter / `arcee-ai/trinity-large-preview:free` | 185 | 185 | 185 | **555 / 600** | GOOD |
| Z.ai / `glm-4.5` | 180 | 170 | 170 | **520 / 600** | GOOD |
| Groq / `llama-3.3-70b-versatile` | 160 | 155 | 150 | **465 / 600** | ACCEPTABLE |
| Gemini / `gemini-2.0-flash` | 10 | 10 | 10 | **30 / 600** | POOR |

### Recommendations

**1st choice: Z.ai / `glm-4.5`** — Best overall quality for Nexus's structured JSON tasks. Consistently produces well-formed extractions and meaningful consolidation connections. Recommended for production use with the always-on agent.

**2nd choice: OpenRouter / `arcee-ai/trinity-large-preview:free`** — Highest raw score and free to use. Strong extraction and consolidation. A practical option when cost is a concern, though response latency is higher than dedicated providers.

Gemini's poor result reflects free-tier quota exhaustion (HTTP 429), not model capability. Re-evaluation with an active quota is recommended before ruling it out.

## Validation

Recommended validation before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

## License

This project is licensed under the [MIT License](LICENSE).
