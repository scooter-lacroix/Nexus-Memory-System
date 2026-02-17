# CLI Reference

> **Complete Command-Line Interface Reference**

**Version:** 1.1.0

---

## Table of Contents

- [Overview](#overview)
- [Global Options](#global-options)
- [Commands](#commands)
- [Examples](#examples)

---

## Overview

The Nexus CLI (`nexus`) provides command-line access to all Nexus functionality.

### Basic Usage

```bash
nexus [OPTIONS] COMMAND [ARGS]...
```

### Help

```bash
# General help
nexus --help

# Command help
nexus COMMAND --help
```

---

## Global Options

| Option | Short | Description |
|--------|-------|-------------|
| `--verbose` | `-v` | Enable verbose logging |
| `--config` | `-c` | Configuration file path |
| `--version` | | Show version and exit |

---

## Commands

### `nexus init`

Initialize the Nexus database.

```bash
nexus init [OPTIONS]
```

**Options:**

| Option | Description |
|--------|-------------|
| `--reset` | Reset database before initialization (deletes data) |

**Examples:**

```bash
# Initialize database
nexus init

# Reset and reinitialize
nexus init --reset
```

---

### `nexus serve`

Start the Nexus server.

```bash
nexus serve [OPTIONS]
```

**Options:**

| Option | Description |
|--------|-------------|
| `--transport` | Transport protocol: `stdio`, `http`, or `web` (default: `stdio`) |
| `--host` | Host address |
| `--port` | Port number |
| `--web-port` | Web dashboard port |
| `--debug` | Enable debug mode |

**Examples:**

```bash
# Start web dashboard
nexus serve --transport web

# Start with custom port
nexus serve --transport web --web-port 8080

# Start HTTP transport
nexus serve --transport http --host 0.0.0.0 --port 8767

# Start in debug mode
nexus serve --transport web --debug
```

---

### `nexus status`

Show system status and statistics.

```bash
nexus status
```

**Output:**

```
[blue]Nexus System Status[/blue]

[green]Database: Connected[/green]
  Location: /home/user/.nexus-memory-system/nexus.db
  Tables:
    memories: 1,234 records
    memory_relationships: 456 records
    task_specifications: 78 records
    sessions: 23 records

[blue]Configuration:[/blue]
  Host: 0.0.0.0
  Port: 8767
  Web Port: 8000
  Conscious Ingest: True
  Auto Ingest: True
  Embeddings Enabled: True

[blue]Supported Agents (8):[/blue]
┌──────────────┬──────────────────────────────────────┐
│ Agent Type   │ Description                          │
├──────────────┼──────────────────────────────────────┤
│ amp          │ AMP pipeline specific                 │
│ claude       │ Claude Code specific                  │
│ claude-code  │ Claude Code specific                  │
│ codex        │ Codex review specific                 │
│ droid        │ Droid automation specific             │
│ gemini       │ Gemini specific                       │
│ opencode     │ OpenCode API specific                 │
│ qwen         │ Qwen specific                         │
└──────────────┴──────────────────────────────────────┘
```

---

### `nexus search`

Search memories.

```bash
nexus search [OPTIONS] QUERY
```

**Arguments:**

| Argument | Description |
|----------|-------------|
| `QUERY` | Search query string |

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--agent` | `-a` | Agent type to search (default: `general`) |
| `--limit` | `-l` | Maximum results (default: `5`) |
| `--category` | | Filter by category |

**Examples:**

```bash
# Basic search
nexus search "UI preferences"

# Search specific agent
nexus search "database schema" --agent claude-code

# Search with limit
nexus search "api endpoints" --limit 10

# Search by category
nexus search "user settings" --category preferences
```

---

### `nexus store`

Store a memory.

```bash
nexus store [OPTIONS] CONTENT
```

**Arguments:**

| Argument | Description |
|----------|-------------|
| `CONTENT` | Memory content |

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--agent` | `-a` | Agent type (default: `general`) |
| `--category` | | Memory category (default: `general`) |
| `--labels` | | Comma-separated labels |

**Examples:**

```bash
# Basic memory
nexus store "User prefers dark mode"

# With category
nexus store "Database uses PostgreSQL" --category facts

# With labels
nexus store "Payment API requires auth" --labels api,security --agent claude-code
```

---

### `nexus stats`

Show memory statistics.

```bash
nexus stats [OPTIONS]
```

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--agent` | `-a` | Agent type (default: all agents) |

**Examples:**

```bash
# All agents
nexus stats

# Specific agent
nexus stats --agent claude-code
```

**Output:**

```
[blue]Memory Statistics[/blue]

[green]Total Memories: 1,234[/green]

[blue]Memories by Category:[/blue]
┌─────────────────┬─────────┐
│ Category        │ Count   │
├─────────────────┼─────────┤
│ context         │ 350     │
│ general         │ 289     │
│ preferences     │ 234     │
│ facts           │ 198     │
│ session         │ 123     │
│ specifications  │ 40      │
└─────────────────┴─────────┘

[blue]Agent: claude-code[/blue]
```

---

### `nexus config`

Configuration management commands.

```bash
nexus config COMMAND [ARGS]...
```

#### `nexus config show`

Show current configuration.

```bash
nexus config show
```

#### `nexus config set`

Set configuration value.

```bash
nexus config set KEY VALUE
```

**Examples:**

```bash
# Show configuration
nexus config show

# Set value
nexus config set auto_ingest true
nexus config set web_port 8080
```

---

### `nexus hooks`

Agent hooks management commands.

```bash
nexus hooks COMMAND [ARGS]...
```

#### `nexus hooks install`

Install agent hooks for automated memory extraction.

```bash
nexus hooks install [OPTIONS] [AGENT]
```

**Arguments:**

| Argument | Description |
|----------|-------------|
| `AGENT` | Agent type (optional) |

**Options:**

| Option | Description |
|--------|-------------|
| `--all` | Install hooks for all supported agents |
| `--no-monitor` | Install without starting monitoring |

**Examples:**

```bash
# Install all hooks
nexus hooks install --all

# Install specific agent
nexus hooks install claude-code

# Install without monitoring
nexus hooks install gemini --no-monitor
```

#### `nexus hooks uninstall`

Uninstall hooks for an agent.

```bash
nexus hooks uninstall AGENT
```

**Examples:**

```bash
nexus hooks uninstall claude-code
```

#### `nexus hooks status`

Show hooks installation and monitoring status.

```bash
nexus hooks status [OPTIONS]
```

**Options:**

| Option | Short | Description |
|--------|-------|-------------|
| `--verbose` | `-v` | Show detailed status including statistics |

**Examples:**

```bash
# Basic status
nexus hooks status

# Detailed status
nexus hooks status --verbose
```

**Output:**

```
[blue]Agent Hooks Status[/blue]

[green]Monitoring: Active[/green]
[green]Auto Extraction: Enabled[/green]

┌──────────────┬──────────┬────────────┬─────────────┬─────────┬─────────────────┐
│ Agent        │ Status   │ Hook Type  │ Extractions │ Last    │                 │
├──────────────┼──────────┼────────────┼─────────────┼─────────┼─────────────────┤
│ claude-code  │ Installed│ Skills     │ 25          │ 10:30   │                 │
│ gemini       │ Installed│ Function   │ 10          │ 09:15   │                 │
│ qwen         │ Installed│ SubAgent   │ 5           │ 08:00   │                 │
└──────────────┴──────────┴────────────┴─────────────┴─────────┴─────────────────┘
```

#### `nexus hooks start`

Start hooks monitoring.

```bash
nexus hooks start
```

#### `nexus hooks stop`

Stop hooks monitoring.

```bash
nexus hooks stop
```

#### `nexus hooks extract`

Manually trigger memory extraction.

```bash
nexus hooks extract [OPTIONS] [AGENT]
```

**Arguments:**

| Argument | Description |
|----------|-------------|
| `AGENT` | Agent type (optional) |

**Options:**

| Option | Description |
|--------|-------------|
| `--all` | Extract from all active agents |

**Examples:**

```bash
# Extract from specific agent
nexus hooks extract claude-code

# Extract from all active agents
nexus hooks extract --all
```

---

## Examples

### Complete Workflow

```bash
# 1. Initialize database
nexus init

# 2. Install hooks
nexus hooks install --all

# 3. Store some memories
nexus store "User prefers dark mode" --agent claude-code --category preferences
nexus store "Project uses PostgreSQL" --category facts
nexus store "Working on checkout optimization" --agent claude-code --category context

# 4. Search memories
nexus search "UI preferences" --agent claude-code

# 5. View statistics
nexus stats --agent claude-code

# 6. Check hooks status
nexus hooks status --verbose

# 7. Start web dashboard
nexus serve --transport web
```

### Quick Store and Search

```bash
# Store
nexus store "API rate limit is 1000 req/min" --category facts --labels api,limits

# Search
nexus search "rate limits"
```

### Hooks Management

```bash
# Install hooks
nexus hooks install --all

# Check status
nexus hooks status

# Manual extraction
nexus hooks extract claude-code

# Stop monitoring
nexus hooks stop

# Start monitoring
nexus hooks start
```

---

## Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid usage |
| 3 | Database error |
| 4 | Network error |

---

## Related Documentation

- [Getting Started Guide](../guide/getting-started.md) - Tutorial
- [REST API Reference](rest-api.md) - HTTP API
- [INSTALLATION.md](../../INSTALLATION.md) - Installation guide

---

**Last Updated:** 2025-12-23
