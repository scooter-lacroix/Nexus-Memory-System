# Development Guide

This repository is Rust-first, with a legacy Python implementation retained for compatibility and migration support.

## Repository Model

- `crates/`: primary implementation
- `nexus/`: legacy Python path
- `scripts/`: installation and helper scripts
- `docs/`: user-facing and operational documentation
- `tests/`: Python-oriented tests and shared validation assets

## Prerequisites

Recommended local environment:

- Rust stable toolchain
- Cargo
- Python 3.11+ for legacy and migration flows
- SQLite tooling for direct database inspection

## Rust Workflow

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
cargo build --release -p nexus-cli
```

## Python Workflow

Use the Python path only when working on legacy compatibility, migration, or docs that explicitly depend on it.

```bash
python -m venv .venv
source .venv/bin/activate
pip install -e .[dev,test]
pytest
```

## Shared CLI Smoke Test

Useful end-to-end validation for the Rust CLI:

```bash
export NEXUS_DATABASE_PATH="$(mktemp -u /tmp/nexus-dev.XXXXXX.db)"
./target/release/nexus init --reset
./target/release/nexus store --content "development smoke test" --agent codex --category session
./target/release/nexus stats
```

## Docs Expectations

When you change any of the following, update docs in the same branch:

- CLI flags or behavior
- install or migration flow
- supported integrations
- environment variables
- public contribution workflow

Minimum docs touched for behavior changes are usually:

- [README.md](README.md)
- [INSTALLATION.md](INSTALLATION.md)
- [CHANGELOG.md](CHANGELOG.md)

## Working in a Dirty Tree

This repository may contain local experimentation or unrelated uncommitted changes. Keep your change set focused:

- avoid reverting unrelated work
- stage only files relevant to your task
- use narrow diffs for commits

## Suggested Review Checklist

Before opening a PR, confirm:

- code builds
- relevant tests pass
- docs match behavior
- examples use current CLI syntax
- no new stubbed or placeholder command paths remain
