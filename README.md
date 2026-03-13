# Nexus Memory System

Nexus Memory System is a Rust-first memory platform for AI coding agents and related tooling. It provides shared SQLite-backed memory storage, CLI workflows, migration tooling, and hook-oriented integrations for multi-agent environments.

The current repository contains:
- a primary Rust workspace under `crates/`
- a legacy Python implementation under `nexus/`
- installation, migration, and operational docs for both paths

## Highlights

- Rust CLI for `init`, `store`, `stats`, `search`, `hooks`, and migration flows
- SQLite-based storage with structured namespaces and memory categories
- Hook framework for agent integrations such as Claude Code, Gemini, Qwen, Codex, and OpenCode
- Shared-install workflow for one local Nexus runtime across multiple CLIs
- Legacy Python implementation retained for compatibility and migration support

## Project Status

- Rust workspace: primary implementation
- Python package: legacy/compatibility path
- Repository maturity: active development
- License: MIT

## Quick Start

### 1. Clone and build

```bash
git clone https://github.com/scooter-lacroix/Nexus-Memory-System.git
cd Nexus-Memory-System
cargo build --release -p nexus-cli
```

### 2. Install the shared CLI

```bash
./scripts/install.sh --binary ./target/release/nexus
```

### 3. Initialize storage

```bash
nexus init
```

### 4. Store and inspect a memory

```bash
nexus store --content "Codex completed onboarding" --agent codex --category session
nexus stats
```

## Common Commands

```bash
# Show current statistics
nexus stats

# Store a memory
nexus store --content "User prefers concise output" --agent claude-code --category preferences

# Install or inspect hooks
nexus hooks status --verbose
nexus hooks install --agent all

# Migrate from an older Python-backed database
nexus migrate discover
nexus migrate run
nexus migrate validate
```

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
├── nexus/                  # Legacy Python implementation
├── docs/
├── tests/
├── scripts/
├── Cargo.toml
└── pyproject.toml
```

## Documentation

- [INSTALLATION.md](INSTALLATION.md): installation and environment setup
- [DEVELOPMENT.md](DEVELOPMENT.md): local development workflow
- [CONTRIBUTING.md](CONTRIBUTING.md): contribution process and standards
- [SECURITY.md](SECURITY.md): vulnerability reporting and security expectations
- [SUPPORT.md](SUPPORT.md): where to get help and how to ask good questions
- [ARCHITECTURE.md](ARCHITECTURE.md): system architecture overview
- [HOOKS.md](HOOKS.md): hook and integration model
- [MIGRATION.md](MIGRATION.md): migration guidance from older deployments
- [CHANGELOG.md](CHANGELOG.md): release history

Additional docs live under [`docs/`](docs/), including API, deployment, and getting-started guides.

## Supported Integrations

The repository includes hook or monitoring support for:

- Claude Code
- Gemini
- Qwen
- Codex
- OpenCode
- Amp
- Droid
- Generic CLI workflows

Integration readiness varies by tool and installation path. See [HOOKS.md](HOOKS.md) and [INSTALLATION.md](INSTALLATION.md) for operational details.

## Development

Recommended local validation for the Rust workspace:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

If you touch the legacy Python tree, also run the relevant Python checks described in [DEVELOPMENT.md](DEVELOPMENT.md).

## Contributing

Contributions are welcome. Please read:

- [CONTRIBUTING.md](CONTRIBUTING.md)
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- [SECURITY.md](SECURITY.md)

## License

This project is licensed under the [MIT License](LICENSE).
