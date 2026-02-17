# Getting Started Guide

> **Step-by-Step Tutorial for Nexus Memory System**

**Version:** 1.1.0

---

## Table of Contents

- [Introduction](#introduction)
- [Installation](#installation)
- [First Time Setup](#first-time-setup)
- [Installing Hooks](#installing-hooks)
- [Storing Your First Memory](#storing-your-first-memory)
- [Searching Memories](#searching-memories)
- [Using the Web Dashboard](#using-the-web-dashboard)
- [Verifying It Works](#verifying-it-works)
- [Next Steps](#next-steps)

---

## Introduction

This guide will walk you through setting up Nexus Memory System from scratch. By the end, you will have:

1. Installed Nexus
2. Initialized the database
3. Installed agent hooks
4. Stored and searched memories
5. Accessed the web dashboard

---

## Installation

### Step 1: Install Nexus

Choose your preferred installation method:

#### Using uv (Recommended)

```bash
# Install uv if needed
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install Nexus with embeddings
uv pip install nexus-memory-system[embeddings]
```

#### Using pip

```bash
# Install Nexus with embeddings
pip install nexus-memory-system[embeddings]
```

#### From Source

```bash
# Clone repository (internal access only)
git clone https://github.com/scooter-lacroix/nexus-memory-system.git
cd nexus-memory-system

# Create virtual environment
python -m venv venv
source venv/bin/activate  # Linux/macOS
# or venv\Scripts\activate  # Windows

# Install
pip install -e .[embeddings]
```

### Step 2: Verify Installation

```bash
# Check version
nexus --version

# Show help
nexus --help
```

You should see output like:

```
Nexus Memory System v1.1.0
Cross-agent memory management platform
```

---

## First Time Setup

### Step 1: Initialize Database

```bash
nexus init
```

Expected output:

```
[blue]Initializing Nexus database...[/blue]
[green]Database initialized successfully[/green]
Database location: /home/user/.nexus-memory-system/nexus.db
Tables created: ['memories', 'memory_relationships', 'task_specifications', 'sessions']
```

### Step 2: Check System Status

```bash
nexus status
```

This shows:

- Database connection status
- Table record counts
- Configuration settings
- Supported agents

---

## Installing Hooks

Hooks enable **automated memory extraction** when your agent sessions end.

### Install All Hooks

```bash
nexus hooks install --all
```

Expected output:

```
[blue]Installing hooks for all supported agents...[/blue]
[green]  claude-code: success[/green]
[green]  claude: success[/green]
[green]  gemini: success[/green]
[green]  qwen: success[/green]
[green]  opencode: success[/green]
[green]  codex: success[/green]
[green]  amp: success[/green]
[green]  droid: success[/green]

[green]Monitoring is active[/green]
```

### Verify Hooks Installation

```bash
nexus hooks status --verbose
```

This shows:

- Which agents have hooks installed
- Hook types (Skills, Functions, CLI)
- Extraction statistics
- Monitoring status

---

## Storing Your First Memory

### Method 1: Using CLI

```bash
nexus store "User prefers dark mode in the UI" --agent claude-code --category preferences --labels ui,theme
```

Expected output:

```
[blue]Storing memory...[/blue]
[green]Memory stored successfully[/green]
Memory ID: 1
Agent: claude-code
Category: preferences
```

### Method 2: Using Python API

Create a test script `test_memory.py`:

```python
import asyncio
from nexus.server import get_memory_manager

async def main():
    # Get manager
    manager = get_memory_manager()
    await manager.initialize()

    # Store memory
    result = await manager.store_memory(
        content="User prefers dark mode in the UI",
        agent_type="claude-code",
        category="preferences",
        labels=["ui", "theme"],
        metadata={"source": "manual_test"}
    )

    print(f"Stored memory ID: {result['memory_id']}")
    print(f"Success: {result['success']}")

    # Close manager
    await manager.close()

if __name__ == "__main__":
    asyncio.run(main())
```

Run it:

```bash
python test_memory.py
```

---

## Searching Memories

### Method 1: Using CLI

```bash
nexus search "UI theme preferences" --agent claude-code --limit 5
```

Expected output:

```
[blue]Searching memories for agent 'claude-code'[/blue]
Query: UI theme preferences

[green]Found 1 memories[/green]

1. Memory ID: 1
Category: preferences
Created: 2025-12-23 10:30:00
Access Count: 1
Labels: ui, theme
Content: User prefers dark mode in the UI
--------------------------------------------------
```

### Method 2: Using Python API

```python
import asyncio
from nexus.server import get_memory_manager

async def main():
    manager = get_memory_manager()
    await manager.initialize()

    # Search memories
    results = await manager.search_memories(
        query="UI theme preferences",
        agent_type="claude-code",
        limit=5
    )

    if results["success"]:
        print(f"Found {len(results['results'])} memories")
        for memory in results["results"]:
            print(f"- {memory['content'][:50]}...")

    await manager.close()

asyncio.run(main())
```

---

## Using the Web Dashboard

### Start the Web Server

```bash
nexus serve --transport web
```

Expected output:

```
[green]Starting Nexus Web Dashboard[/green]
Host: 0.0.0.0, Port: 8000
Dashboard URL: http://localhost:8000
API Docs: http://localhost:8000/api/docs

INFO:     Started server process
INFO:     Waiting for application startup.
INFO:     Application startup complete.
INFO:     Uvicorn running on http://0.0.0.0:8000
```

### Access the Dashboard

1. **Main Dashboard:** http://localhost:8000
2. **API Documentation:** http://localhost:8000/api/docs
3. **ReDoc Documentation:** http://localhost:8000/api/redoc

### Using the API

Via curl:

```bash
# Store a memory
curl -X POST http://localhost:8000/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{
    "content": "User prefers dark mode",
    "agent_type": "claude-code",
    "category": "preferences",
    "labels": ["ui", "theme"]
  }'
```

Via Python requests:

```python
import requests

# Store memory
response = requests.post(
    "http://localhost:8000/api/v1/memories",
    json={
        "content": "User prefers dark mode",
        "agent_type": "claude-code",
        "category": "preferences",
        "labels": ["ui", "theme"]
    }
)
print(response.json())
```

---

## Verifying It Works

### Run Health Check

```bash
# Using curl
curl http://localhost:8000/health

# Expected response
{
  "status": "healthy",
  "timestamp": "2025-12-23T10:30:00.000000Z",
  "version": "1.0.0"
}
```

### View Statistics

```bash
# CLI
nexus stats --agent claude-code

# API
curl http://localhost:8000/api/v1/stats
```

### Check Hooks Status

```bash
nexus hooks status --verbose
```

### Test Automated Extraction

1. Start your agent (e.g., Claude Code)
2. Do some work
3. Exit the agent normally
4. Check if memories were extracted:

```bash
nexus search "session" --agent claude-code --limit 10
```

---

## Next Steps

### Learn More

- **[Memory Types Guide](memory-types.md)** - Understanding hybrid memory types
- **[Embeddings Guide](embeddings.md)** - Semantic search and embeddings
- **[API Reference](../api/rest-api.md)** - Complete REST API documentation
- **[CLI Reference](../api/cli-reference.md)** - All CLI commands

### Advanced Configuration

- **[ARCHITECTURE.md](../../ARCHITECTURE.md)** - System architecture
- **[INSTALLATION.md](../../INSTALLATION.md)** - Advanced installation options
- **[HOOKS.md](../../HOOKS.md)** - Hooks configuration

### Deployment

- **[Production Deployment](../deployment/production.md)** - Production setup
- **[Docker Deployment](../deployment/docker.md)** - Docker configuration

### Troubleshooting

- **[Troubleshooting](../../troubleshooting.md)** - Common issues and solutions

---

## Quick Reference

### Essential Commands

```bash
# Installation
pip install nexus-memory-system[embeddings]

# Initialization
nexus init

# Hooks
nexus hooks install --all
nexus hooks status

# Store memory
nexus store "content" --agent claude-code --category preferences

# Search memories
nexus search "query" --agent claude-code --limit 5

# Statistics
nexus stats

# Web dashboard
nexus serve --transport web
```

### Common Patterns

```python
# Store memory
await manager.store_memory(
    content="Memory content",
    agent_type="claude-code",
    category="preferences",
    labels=["ui", "theme"]
)

# Search memories
results = await manager.search_memories(
    query="UI preferences",
    agent_type="claude-code",
    limit=10
)
```

---

**Congratulations!** You now have Nexus Memory System up and running.

---

**Last Updated:** 2025-12-23
