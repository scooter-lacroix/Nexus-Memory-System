# Spec: Rust CLI Application

**Track ID:** rust-cli-app_20250216
**Type:** Feature
**Status:** New

---

## Overview

Implement the CLI application in Rust using clap framework. Full parity with Python CLI including all commands (init, serve, store, search, stats, hooks), configuration management, and shell completion.

**Python Mapping:** `nexus/cli.py`

---

## Functional Requirements

### FR1: CLI Structure

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nexus")]
#[command(about = "Nexus Memory System - Cross-agent memory management", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the Nexus database
    Init { reset: bool },

    /// Start the Nexus server
    Serve {
        #[arg(long, default_value = "web")]
        transport: String,
    },

    /// Store a new memory
    Store {
        content: String,
        #[arg(long, default_value = "general")]
        category: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        labels: Option<Vec<String>>,
    },

    /// Search memories
    Search {
        query: String,
        #[arg(long)]
        agent: String,
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Show statistics
    Stats {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        verbose: bool,
    },

    /// Manage hooks
    Hooks {
        #[command(subcommand)]
        action: HooksCommands,
    },
}
```

### FR2: Commands Implementation

#### init

```bash
nexus init                    # Initialize database
nexus init --reset            # Reset and reinitialize
```

#### serve

```bash
nexus serve --transport web   # Web dashboard (port 8768)
nexus serve --transport stdio # MCP stdio transport
nexus serve --transport http  # MCP HTTP transport
```

#### store

```bash
nexus store "User prefers dark mode" \
  --agent claude-code \
  --category preferences \
  --labels ui theme
```

#### search

```bash
nexus search "UI preferences" \
  --agent claude-code \
  --limit 5
```

#### stats

```bash
nexus stats --agent claude-code --verbose
```

#### hooks

```bash
nexus hooks install --all              # Install all hooks
nexus hooks install claude-code        # Install specific
nexus hooks status --verbose           # Check status
```

### FR3: Configuration Management

```toml
# ~/.config/nexus/config.toml
[database]
path = "~/.nexus/nexus.db"

[server]
host = "127.0.0.1"
port = 8000
web_port = 8768

[embeddings]
enabled = true
model = "all-MiniLM-L6-v2"

[sync]
policy = "manual"  # manual, auto, aggressive
```

### FR4: Shell Completion

Generate completion scripts for:
- bash
- zsh
- fish
- elvish

```bash
nexus completion bash > /etc/bash_completion.d/nexus
nexus completion zsh > ~/.zfunc/_nexus
nexus completion fish > ~/.config/fish/completions/nexus.fish
```

---

## Non-Functional Requirements

### NFR1: Performance

| Metric | Target |
|--------|--------|
| CLI startup time | <100ms |
| Command execution | <500ms (local operations) |

### NFR2: User Experience

- Clear error messages
- Progress bars for long operations
- Colored output (via owo-colors or similar)
- Help text for all commands

### NFR3: Code Quality

- 95%+ test coverage
- Proper error handling
- Clean exit codes

---

## Acceptance Criteria

### AC1: All Commands Implemented

```bash
# Test each command
nexus init
nexus serve --help
nexus store "test" --agent test-agent
nexus search "test" --agent test-agent
nexus stats --agent test-agent
nexus hooks status
```

### AC2: Configuration Works

```bash
# Custom config file
nexus --config /path/to/config.toml init

# Environment variables
NEXUS_DATABASE_PATH=/tmp/test.db nexus init
```

### AC3: Shell Completion Works

```bash
# Test completion
source <(nexus completion bash)
nexus <TAB>  # Shows all commands
nexus hooks <TAB>  # Shows hooks subcommands
```

### AC4: Exit Codes

```bash
nexus store "test" --agent test
echo $?  # 0 for success

nexus store "" --agent test
echo $?  # 1 for error
```

---

## Dependencies

### External Crates

```toml
[dependencies]
clap = { version = "4.5", features = ["derive"] }
tokio = { version = "1.40", features = ["full"] }
anyhow = "1.0"
thiserror = "1.0"
owo-colors = "4.0"       # Colored output
indicatif = "0.17"       # Progress bars
dirs = "5.0"             # Config directory
toml = "0.8"
serde = { version = "1.0", features = ["derive"] }
```

### Local Dependencies

- `nexus-core` - Core types, config
- `nexus-storage` - Database operations
- `nexus-server` - NexusManager

---

## Error Messages

| Error | Message |
|-------|---------|
| Database not initialized | `Error: Nexus database not found. Run 'nexus init' first.` |
| Invalid category | `Error: Invalid category 'xyz'. Valid: general, facts, preferences, context, specifications, session` |
| Agent not found | `Error: Agent namespace 'xyz' not found. Supported: claude-code, pi-mono, oh-my-pi, ...` |
| Hook install failed | `Error: Failed to install hooks: {reason}` |

---

## Out of Scope

- Interactive TUI (future)
- Daemon mode (future)
- Remote server mode (future)

---

## References

- Python implementation: `nexus/cli.py`
- clap docs: https://docs.rs/clap/
- CLAUDE.md: Rust Port Guide

---

**Version:** 1.0
**Created:** 2025-02-16
