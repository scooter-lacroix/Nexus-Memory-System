# Installation Guide

This repository is a Rust-first Nexus Memory System workspace with a legacy Python implementation retained for compatibility and migration support.

For most users, the recommended path is:

1. Build the Rust CLI.
2. Install the shared `nexus` launcher with `scripts/install.sh`.
3. Initialize a local database.
4. Verify the install with `nexus stats`.

## Prerequisites

### Required for the Rust-first path

- Rust stable toolchain
- Cargo
- SQLite runtime support on your system

### Optional for legacy or migration workflows

- Python 3.11+
- `pip` or `uv`
- `venv`

## Recommended Installation

Clone the repository and build the CLI:

```bash
git clone https://github.com/scooter-lacroix/Nexus-Memory-System.git
cd Nexus-Memory-System
cargo build --release -p nexus-cli
```

Install the shared user-level launcher:

```bash
./scripts/install.sh --binary ./target/release/nexus
```

Initialize storage:

```bash
nexus init
```

Verify the install:

```bash
nexus stats
nexus store --content "installation smoke test" --agent codex --category session
nexus search --query "installation smoke test"
```

## What the Installer Does

`scripts/install.sh` is the primary setup path for local use. It:

- installs `nexus-bin` and the `nexus` wrapper into `~/.local/bin` by default
- writes shared environment files under `~/.config/nexus-memory-system/`
- configures a shared `NEXUS_DATABASE_PATH`
- creates helper launchers such as `nexus-with` and `<tool>-nexus` when supported CLIs are present

## Installer Options

```bash
./scripts/install.sh --help
```

Common examples:

```bash
# Install a freshly built local binary
./scripts/install.sh --binary ./target/release/nexus

# Use a custom database location
./scripts/install.sh --binary ./target/release/nexus --db-path "$HOME/.nexus/nexus.db"

# Skip shell profile edits
./scripts/install.sh --binary ./target/release/nexus --skip-profile
```

## Configuration

The Rust CLI reads configuration from environment and optional config files.

Common paths created by the installer:

- binary wrapper: `~/.local/bin/nexus`
- config dir: `~/.config/nexus-memory-system/`
- env file: `~/.config/nexus-memory-system/nexus.env`
- data dir: `~/.local/share/nexus-memory-system/`
- database: `~/.local/share/nexus-memory-system/nexus.db`

Common environment variables:

```bash
export NEXUS_DATABASE_PATH="$HOME/.local/share/nexus-memory-system/nexus.db"
export NEXUS_SYNC_POLICY="auto"
export NEXUS_AUTO_INGEST="true"
export NEXUS_EMBEDDINGS_ENABLED="true"
```

## Running Without Installing

You can also run the CLI directly from the build output:

```bash
cargo build --release -p nexus-cli
./target/release/nexus init
./target/release/nexus stats
```

This is useful for CI, local testing, or when you do not want to modify shell profiles.

## Legacy Python Path

The Python package remains in this repository for compatibility and migration-related workflows. It should not be treated as the primary install path for new deployments.

Create a virtual environment and install editable dependencies:

```bash
python -m venv .venv
source .venv/bin/activate
pip install -e .[dev,test]
```

Use this path only when working on:

- legacy compatibility
- migration tooling
- old Python-oriented tests
- documentation that explicitly targets the Python implementation

## Migration From Older Python Deployments

If you already have an older Python-backed Nexus installation, use the Rust CLI migration workflow:

```bash
nexus migrate discover
nexus migrate status
nexus migrate run
nexus migrate validate
```

See [MIGRATION.md](MIGRATION.md) for details.

## Validation Checklist

After installation, the following should work:

```bash
nexus --help
nexus init --help
nexus store --help
nexus hooks --help
nexus stats
```

For contributors, the recommended validation set is:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

## Troubleshooting

### `nexus` command not found

- Ensure `~/.local/bin` is on your `PATH`
- Re-open your shell after running the installer
- Or run the binary directly from `./target/release/nexus`

### Database path confusion

- Check `~/.config/nexus-memory-system/nexus.env`
- Confirm `NEXUS_DATABASE_PATH` points where you expect
- Re-run `nexus stats` after sourcing the environment

### Hooks do not appear active

- Run `nexus hooks status`
- Confirm your target CLI is installed and discoverable in `PATH`
- Review [HOOKS.md](HOOKS.md) for integration details

### Python tooling errors

- Make sure you are using a virtual environment
- Reinstall editable dependencies
- Limit Python work to the legacy compatibility path unless the task requires it

## Related Docs

- [README.md](README.md)
- [DEVELOPMENT.md](DEVELOPMENT.md)
- [HOOKS.md](HOOKS.md)
- [MIGRATION.md](MIGRATION.md)
- [SUPPORT.md](SUPPORT.md)
