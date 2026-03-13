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
```

The shared domain model lives in `nexus-core`. Storage and repositories live in `nexus-storage`. Higher-level surfaces such as the CLI, hooks, MCP server, and web dashboard build on that foundation. The product is documented as one system even though the current implementation is organized as multiple crates.

## Repository Layout

```text
.
├── crates/
│   ├── nexus-cli/
│   ├── nexus-core/
│   ├── nexus-embeddings/
│   ├── nexus-hooks/
│   ├── nexus-lephase/
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

## Validation

Recommended validation before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

## License

This project is licensed under the [MIT License](LICENSE).
