# Installation Guide

Nexus installs as a local CLI, runtime wrapper layer, hook integration set, and optional web/API surface for supported agent workflows.

The recommended install path keeps the binary current, refreshes wrappers and hooks, and gives you a reliable local runtime without requiring a heavyweight service stack.

## Requirements

- Rust stable toolchain
- Cargo
- a machine capable of running SQLite-backed Rust binaries

## Recommended Install Path

### 1. Clone and build

```bash
git clone https://github.com/scooter-lacroix/Nexus-Memory-System.git
cd Nexus-Memory-System
cargo build --release -p nexus-memory
```

### 2. Install or upgrade Nexus

```bash
./scripts/install.sh --binary ./target/release/nexus
```

### 3. Initialize the database

```bash
nexus init
```

### 4. Verify the install

```bash
nexus --version
nexus stats
nexus hooks status
```

## What the Installer Does

The installer writes and refreshes a user-level setup by default:

- `~/.local/bin/nexus`
- `~/.config/nexus-memory-system/nexus.env`
- `~/.local/share/nexus-memory-system/nexus.db`

It also creates helper wrappers such as `nexus-with` and tool-specific `*-nexus` launchers when matching tools are present in `PATH`.

Those wrappers can issue best-effort `nexus session start` and `nexus session end` around supported CLIs so memory capture and bounded dreaming work during normal usage without manually starting a separate server.

If `nexus` is already installed, rerunning the installer replaces the installed binary in place so the local command stays current.

## Installer Examples

```bash
./scripts/install.sh --binary ./target/release/nexus
./scripts/install.sh --db-path "$HOME/.nexus/nexus.db"
./scripts/install.sh --skip-profile
```

## Running Without Installing

You can run directly from the built binary when needed:

```bash
cargo build --release -p nexus-memory
./target/release/nexus init
./target/release/nexus stats
```

That said, the installed path is the better operator experience because it also keeps wrappers, hooks, and environment wiring aligned.

## Environment Model

The installer writes `~/.config/nexus-memory-system/nexus.env`, which acts as the shared runtime environment for the installed system.

Common settings include:

```bash
export NEXUS_DATABASE_PATH="$HOME/.local/share/nexus-memory-system/nexus.db"
export NEXUS_SYNC_POLICY="auto"
export NEXUS_AUTO_INGEST="true"
export NEXUS_AGENT_ENABLED="true"
```

### Generation and embeddings

Nexus treats generation and embeddings as separate configurable systems.

That means you can choose:

- a remote provider for generation
- the same or different provider for embeddings
- the same or different model for embeddings
- a local ONNX embedding model
- a local OpenAI-compatible runtime such as `vLLM`, `LM Studio`, or `llama.cpp`

The interactive `nexus config` flow is the easiest way to set this up.

## First-Run Suggestions

After installation:

```bash
nexus config
nexus hooks install --agent all
nexus hooks status --verbose
```

Then try:

```bash
nexus store --content "Nexus is installed and ready" --agent codex --category session
nexus recall --agent codex --query "What just happened?"
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

- run `nexus hooks status --verbose`
- confirm the target tool exists in `PATH`
- review [HOOKS.md](HOOKS.md)

### Semantic recall is not active

- run `nexus config show`
- verify the embedding backend, provider, and model
- review [Embeddings Guide](docs/guide/embeddings.md)

### I want rollout and migration guidance

- review [Cognition Rollout Guide](docs/guide/cognition-rollout.md)
