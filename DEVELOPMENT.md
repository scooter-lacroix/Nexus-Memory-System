# Development Guide

This repository is developed as a Rust workspace.

## Workspace Layout

- `crates/nexus-core`: shared domain types and configuration
- `crates/nexus-storage`: persistence and repository layer
- `crates/nexus-vectors`: vector lookup support
- `crates/nexus-embeddings`: embedding integration
- `crates/nexus-orchestrator`: higher-level coordination and context flow
- `crates/nexus-hooks`: agent hook installation and extraction support
- `crates/nexus-mcp`: MCP surface
- `crates/nexus-web`: Axum-based web/API surface
- `crates/nexus-cli`: command-line entrypoint

## Prerequisites

- Rust stable
- Cargo
- SQLite support on the local machine

## Common Commands

### Build

```bash
cargo build --workspace
```

### Format

```bash
cargo fmt --all
```

### Lint

```bash
cargo clippy --workspace --all-targets
```

### Test

```bash
cargo test --workspace
```

### Release build

```bash
cargo build --release -p nexus-memory
```

## Local Smoke Test

```bash
export NEXUS_DATABASE_PATH="$(mktemp -u /tmp/nexus-dev.XXXXXX.db)"
./target/release/nexus init --reset
./target/release/nexus store --content "development smoke test" --agent codex --category session
./target/release/nexus search --query "development smoke test"
./target/release/nexus stats
```

## Release Readiness

Before cutting a release:

- ensure `cargo test --workspace` passes
- verify the README examples still work
- confirm the installer matches the current CLI behavior
- update [CHANGELOG.md](CHANGELOG.md)

## Working Tree Hygiene

Keep commits focused and avoid bundling unrelated local tooling state into release work.
