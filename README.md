# Nexus Memory System

Nexus Memory System is a Rust-first memory and cognition platform for AI coding agents. It gives tools like Claude Code, Codex, Gemini CLI, Qwen, Amp, OpenCode, Droid, and Hermes a shared SQLite-backed memory runtime with automatic lifecycle capture, semantic recall, bounded dreaming, and practical observability.

## Brief Description

Nexus gives multiple coding agents one consistent memory layer. It is designed for:

- shared memory storage across agent namespaces
- structured categories such as `general`, `facts`, `preferences`, `context`, `specifications`, and `session`
- representation-first recall built from explicit observations, session digests, derived insights, and contradictions
- vector-first semantic search with bounded text fallback
- agent hook installation and status management
- search, stats, and migration-oriented operational workflows
- HTTP, WebSocket, and MCP access patterns on top of the same storage layer

## Highlights

- Current Rust workspace layout separates core types, storage, vectors, embeddings, orchestration, hooks, web, MCP, and CLI concerns
- User-level installer that upgrades the local `nexus` binary in place and refreshes wrappers, hooks, and env files
- SQLite-based persistence with repository-style access through `nexus-storage`
- Representation-first cognition with explicit derivation, digest ladders, bounded dreaming, and lineage-aware recall
- Vector-first semantic retrieval with bounded text fallback when embeddings are enabled
- Native hook management for Claude Code, pi-mono, oh-my-pi, and pi-skills (native lifecycle)
- Process monitoring for Gemini and Qwen (monitor-only, no native hooks)
- CLI wrapper support for Codex, OpenCode, Amp, Droid, and Hermes (wrapper lifecycle)
- Web dashboard and API routes under the `nexus-web` crate, including cognition observability
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

### Build a cognition-aware working set

```bash
nexus represent --agent claude-code --query "provider rollout timeline" --introspect
```

### Inspect a session digest or run a dream cycle

```bash
nexus digest --agent claude-code --session-key <session-key>
nexus dream --agent claude-code
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

### See what the cognition engine is doing

```bash
nexus recall --agent claude-code --query "provider rollout timeline"
nexus digest --agent claude-code --session-key <session-key>
nexus dream --agent claude-code
```

## Usage Examples

### Install hooks for all supported agents

```bash
nexus hooks install --agent all
nexus hooks status --verbose
```

The `hooks status` command shows each agent's support tier (`native-lifecycle`, `wrapper-lifecycle`, or `monitor-only`) alongside its lifecycle capabilities. Agents at `monitor-only` tier rely on process detection and have no native hook installation.

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

The shared domain model lives in `nexus-core`. Storage and repositories live in `nexus-storage`. Higher-level surfaces such as the CLI, hooks, MCP server, and web dashboard build on that foundation. The optional cognition runtime (`nexus-llm` + `nexus-agent`) adds derivation, digest ladders, dreaming, representation-first recall, and query synthesis. The product is documented as one system even though the current implementation is organized as multiple crates.

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
- [docs/guide/cognition-rollout.md](docs/guide/cognition-rollout.md)
- [docs/guide/cognition-excellence-release-note.md](docs/guide/cognition-excellence-release-note.md)

## Why Nexus

Nexus is built for people who want a serious local memory system for agent workflows without dragging in a heavyweight external stack. The design goal is simple: keep the storage model understandable, keep the runtime bounded, and still make the agent feel like it has a useful subconscious instead of a noisy event log.

## LLM Provider Evaluation

The `nexus eval` command tests an LLM provider against the memory system's core operations: structured extraction, consolidation, and query synthesis. Each aspect is scored out of 200 (total 600).

| Provider / Model | Extraction | Consolidation | Query | Total | Rating |
|---|---|---|---|---|---|
| OpenRouter / `arcee-ai/trinity-large-preview:free` | 185 | 185 | 185 | **555 / 600** | GOOD |
| Z.ai / `glm-4.5` | 180 | 170 | 170 | **520 / 600** | GOOD |
| Gemini / `gemini-3.1-flash-lite-preview` | 100 | 80 | 55 | **520 / 600** | GOOD |
| Groq / `moonshotai/kimi-k2-instruct-0905` | 100 | 80 | 55 | **520 / 600** | GOOD |
| Groq / `llama-3.3-70b-versatile` | 160 | 155 | 150 | **465 / 600** | ACCEPTABLE |

### Recommendations

**1st choice: OpenRouter / `arcee-ai/trinity-large-preview:free`** — Highest overall score across all aspects. Free to use with strong extraction and consolidation. Best value for production use.

**2nd choice: Z.ai / `glm-4.5`** — Best structured JSON quality with the most consistent extraction scores. Recommended when a dedicated provider is preferred over a routing layer.

## Validation

Recommended validation before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p nexus-memory-agent --bench cognition
```

## License

This project is licensed under the [MIT License](LICENSE).
