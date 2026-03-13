# Installation Guide

Nexus Memory System is installed from this Rust workspace.

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

### 2. Install the shared launcher

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

The installer writes a user-level setup by default:

- `~/.local/bin/nexus`
- `~/.local/bin/nexus-bin`
- `~/.config/nexus-memory-system/nexus.env`
- `~/.local/share/nexus-memory-system/nexus.db`

It also creates helper wrappers such as `nexus-with` and tool-specific `*-nexus` launchers when matching tools are present in `PATH`.

## Useful Installer Examples

```bash
./scripts/install.sh --binary ./target/release/nexus
./scripts/install.sh --binary ./target/release/nexus --db-path "$HOME/.nexus/nexus.db"
./scripts/install.sh --binary ./target/release/nexus --skip-profile
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

### The wrong database is being used

- inspect `~/.config/nexus-memory-system/nexus.env`
- confirm `NEXUS_DATABASE_PATH`
- rerun `nexus stats`

### Hooks do not appear installed

- run `nexus hooks status`
- confirm the target tool exists in `PATH`
- review [HOOKS.md](HOOKS.md)
