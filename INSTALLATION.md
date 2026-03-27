# Installation Guide

Nexus Memory System installs from this Rust workspace as a local CLI, wrapper, and hook-enabled cognition runtime for supported AI coding agents.

## Requirements

- Rust stable toolchain
- Cargo
- a system capable of running SQLite-backed Rust binaries

## Recommended Install Path

### 1. Clone and build

```bash
git clone https://github.com/scooter-lacroix/Nexus-Memory-System.git
cd Nexus-Memory-System
cargo build --release -p nexus-memory
```

### 2. Install or upgrade the shared launcher

```bash
./scripts/install.sh --binary ./target/release/nexus
```

### 3. Initialize the database

```bash
nexus init
```

### 4. Verify the installation

```bash
nexus --help
nexus stats
```

## Installer Behavior

The installer writes a user-level setup by default and refreshes it on repeat runs:

- `~/.local/bin/nexus`
- `~/.config/nexus-memory-system/nexus.env`
- `~/.local/share/nexus-memory-system/nexus.db`

It also creates helper wrappers such as `nexus-with` and tool-specific `*-nexus` launchers when matching tools are present in `PATH`.
Those wrappers automatically issue best-effort `nexus session start` / `nexus session end` calls around the wrapped CLI so memory capture and bounded dreaming work without manually running `nexus serve`.
If `nexus` is already installed, rerunning the installer replaces the installed binary in place so the local command stays up to date.

## Useful Installer Examples

```bash
./scripts/install.sh --binary ./target/release/nexus
./scripts/install.sh --db-path "$HOME/.nexus/nexus.db"
./scripts/install.sh --skip-profile
```

## Running Without Installing

```bash
cargo build --release -p nexus-memory
./target/release/nexus init
./target/release/nexus stats
```

## Environment Variables

Common settings written by the installer:

```bash
export NEXUS_DATABASE_PATH="$HOME/.local/share/nexus-memory-system/nexus.db"
export NEXUS_SYNC_POLICY="auto"
export NEXUS_AUTO_INGEST="true"
export NEXUS_EMBEDDINGS_ENABLED="true"
```

## Troubleshooting

### `nexus` is not found

- ensure `~/.local/bin` is on `PATH`
- restart the shell after installation
- or run `./target/release/nexus` directly

### The installed `nexus` binary seems out of date

- rebuild with `cargo build --release -p nexus-memory`
- rerun `./scripts/install.sh --binary ./target/release/nexus`
- confirm with `nexus --version`

### The wrong database is being used

- inspect `~/.config/nexus-memory-system/nexus.env`
- confirm `NEXUS_DATABASE_PATH`
- rerun `nexus stats`

### Hooks do not appear installed

- run `nexus hooks status`
- confirm the target tool exists in `PATH`
- review [HOOKS.md](HOOKS.md)
- review [Cognition Rollout Guide](docs/guide/cognition-rollout.md) for lifecycle, backfill, dreaming, and benchmark guidance
