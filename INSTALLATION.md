# Installation Guide

> **Complete Installation Instructions for Nexus Memory System**

**Version:** 1.1.0

---

## Table of Contents

- [System Requirements](#system-requirements)
- [Installation Methods](#installation-methods)
- [Database Setup](#database-setup)
- [Dependencies](#dependencies)
- [Verification](#verification)
- [Troubleshooting](#troubleshooting)

---

## System Requirements

### Minimum Requirements

- **Python:** 3.9 or higher
- **RAM:** 2GB minimum (4GB recommended)
- **Disk:** 500MB for installation + storage for memories
- **OS:** Linux, macOS, or Windows

### Recommended for Production

- **Python:** 3.11 or 3.12
- **RAM:** 4GB+
- **Disk:** SSD with sufficient space
- **Database:** PostgreSQL (optional, for production)

### Optional Dependencies

- **CUDA:** For GPU-accelerated embeddings
- **PostgreSQL:** For production database backend
- **Redis:** For caching layer (optional)

---

## Installation Methods

### Method 1: Using uv (Recommended)

`uv` is a fast Python package installer and resolver.

```bash
# Install uv if not already installed
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install Nexus with embeddings support
uv pip install nexus-memory-system[embeddings]

# Or install from local source
uv pip install -e .[embeddings]
```

### Method 2: Using pip

```bash
# Install from PyPI
pip install nexus-memory-system[embeddings]

# Or install from local source
pip install -e .[embeddings]
```

### Method 3: From Source

```bash
# Clone repository (internal access only)
git clone https://github.com/scooter-lacroix/nexus-memory-system.git
cd nexus-memory-system

# Create virtual environment
python -m venv venv
source venv/bin/activate  # Linux/macOS
# or venv\Scripts\activate  # Windows

# Install with dependencies
pip install -e .[embeddings,dev]
```

### Installation Extras

```bash
# Core installation
pip install nexus-memory-system

# With embeddings support
pip install nexus-memory-system[embeddings]

# With development tools
pip install nexus-memory-system[dev]

# With PostgreSQL support
pip install nexus-memory-system[postgres]

# Everything
pip install nexus-memory-system[embeddings,dev,postgres]
```

---

## Database Setup

### SQLite (Default)

SQLite is the default database and requires no additional setup.

```bash
# Initialize database
nexus init

# Reset database (deletes existing data)
nexus init --reset
```

Database location: `~/.nexus-memory-system/nexus.db`

### PostgreSQL (Optional)

For production deployments, PostgreSQL is recommended.

#### Install PostgreSQL

```bash
# Ubuntu/Debian
sudo apt-get install postgresql postgresql-contrib

# macOS
brew install postgresql

# Windows
# Download from https://www.postgresql.org/download/windows/
```

#### Create Database

```bash
# Switch to postgres user
sudo -u postgres psql

# Create database and user
CREATE DATABASE nexus_memory;
CREATE USER nexus_user WITH PASSWORD 'your_secure_password';
GRANT ALL PRIVILEGES ON DATABASE nexus_memory TO nexus_user;
\q
```

#### Configure Nexus

Set environment variable:

```bash
export NEXUS_DATABASE_URL="postgresql://nexus_user:your_secure_password@localhost/nexus_memory"
```

Or add to `.env` file:

```bash
echo "NEXUS_DATABASE_URL=postgresql://nexus_user:your_secure_password@localhost/nexus_memory" >> .env
```

---

## Dependencies

### Core Dependencies

Core dependencies are installed automatically:

```
fastmcp>=0.11.0
fastapi>=0.115.0
uvicorn[standard]>=0.32.0
pydantic>=2.10.0
pydantic-settings>=2.6.0
loguru>=0.7.2
sqlalchemy>=2.0.0
alembic>=1.14.0
aiofiles>=24.1.0
python-multipart>=0.0.17
websockets>=14.0
python-dotenv>=1.0.0
click>=8.1.0
rich>=13.9.0
httpx>=0.28.0
```

### Embedding Dependencies

For semantic search capabilities:

```bash
# Install embeddings extra
pip install nexus-memory-system[embeddings]
```

This includes:
```
sentence-transformers>=3.3.0
torch>=2.5.0
transformers>=4.47.0
numpy>=1.24.0
scikit-learn>=1.5.0
```

### Vector Search

```bash
# sqlite-vec for vector search
pip install sqlite-vec>=0.1.1
```

### Development Dependencies

For development and testing:

```bash
# Install dev extra
pip install nexus-memory-system[dev]
```

This includes:
```
pytest>=8.3.0
pytest-asyncio>=0.24.0
pytest-cov>=6.0.0
black>=24.10.0
ruff>=0.8.0
mypy>=1.14.0
pre-commit>=4.0.0
```

---

## Configuration

### Environment Variables

Create a `.env` file in your home directory or project root:

```bash
# Database
NEXUS_DATABASE_PATH=~/.nexus-memory-system/nexus.db
# NEXUS_DATABASE_URL=postgresql://user:pass@host/dbname

# Server
NEXUS_HOST=0.0.0.0
NEXUS_PORT=8767
NEXUS_WEB_PORT=8000

# Memory
NEXUS_CONSCIOUS_INGEST=true
NEXUS_AUTO_INGEST=true
NEXUS_MEMORY_SEARCH_LIMIT=10

# Hooks
NEXUS_NATIVE_HOOKS=true
NEXUS_BUFFER_ENABLED=true
NEXUS_MONITOR_INTERVAL=5
NEXUS_INACTIVITY_THRESHOLD=300

# Embeddings
NEXUS_EMBEDDINGS_ENABLED=true
NEXUS_EMBEDDING_MODEL=all-MiniLM-L6-v2
NEXUS_EMBEDDING_DEVICE=cpu

# Cross-Agent Sync
NEXUS_SYNC_POLICY=manual
NEXUS_AUTO_SHARE_LABELS=cross-agent,shared
```

### Configuration File

You can also use a YAML configuration file:

```yaml
# ~/.nexus-config.yml
database:
  path: ~/.nexus-memory-system/nexus.db
  # url: postgresql://user:pass@host/dbname

server:
  host: 0.0.0.0
  port: 8767
  web_port: 8000

memory:
  conscious_ingest: true
  auto_ingest: true
  search_limit: 10

hooks:
  native_hooks: true
  buffer_enabled: true
  monitor_interval: 5
  inactivity_threshold: 300

embeddings:
  enabled: true
  model: all-MiniLM-L6-v2
  device: cpu
```

---

## Agent Hooks Installation

### Install All Hooks

```bash
nexus hooks install --all
```

### Install Specific Agent Hook

```bash
# Claude Code
nexus hooks install claude-code

# Gemini
nexus hooks install gemini

# Qwen
nexus hooks install qwen

# CLI Agents
nexus hooks install amp
nexus hooks install droid
nexus hooks install opencode
nexus hooks install codex
```

### Verify Installation

```bash
nexus hooks status --verbose
```

---

## Verification

### Check Installation

```bash
# Check version
nexus --version

# Show system status
nexus status
```

### Test Database

```bash
# Initialize database
nexus init

# Check database info
nexus status
```

### Test Memory Storage

```bash
# Store a test memory
nexus store "Test memory" --agent general --category general

# Search for it
nexus search "test" --agent general
```

### Start Web Dashboard

```bash
# Start web server
nexus serve --transport web

# Visit http://localhost:8000
# API docs at http://localhost:8000/api/docs
```

### Run Tests (Development)

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=nexus --cov-report=html

# Run specific test
pytest tests/test_embeddings.py
```

---

## Troubleshooting

### Common Issues

#### Issue: Module not found

```bash
# Ensure installation was successful
pip show nexus-memory-system

# Reinstall if needed
pip install --force-reinstall nexus-memory-system[embeddings]
```

#### Issue: Database initialization fails

```bash
# Reset database
nexus init --reset

# Check permissions
ls -la ~/.nexus-memory-system/

# Manual database creation
sqlite3 ~/.nexus-memory-system/nexus.db "VACUUM;"
```

#### Issue: Embedding model download fails

```bash
# Check internet connection
ping huggingface.co

# Set cache directory
export TRANSFORMERS_CACHE=~/.cache/huggingface
export HF_HOME=~/.cache/huggingface

# Pre-download model
python -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('all-MiniLM-L6-v2')"
```

#### Issue: Hooks installation fails

```bash
# Check supported agents
nexus hooks install --help

# Check agent directories exist
ls -la ~/.claude/skills/
ls -la ~/.gemini/extensions/

# Install without monitoring
nexus hooks install claude-code --no-monitor
```

#### Issue: Port already in use

```bash
# Check what's using the port
lsof -i :8000
# or
netstat -tuln | grep 8000

# Use different port
nexus serve --transport web --web-port 8001
```

---

## Getting Help

- **Documentation:** See [docs/](docs/)
- **Troubleshooting:** [docs/troubleshooting.md](docs/troubleshooting.md)
- **Issues:** https://github.com/scooter-lacroix/nexus-memory-system/issues

---

## Next Steps

After installation:

1. **[Getting Started Guide](docs/guide/getting-started.md)** - Step-by-step tutorial
2. **[Hooks Documentation](HOOKS.md)** - Install and configure agent hooks
3. **[API Reference](docs/api/rest-api.md)** - REST API documentation
4. **[CLI Reference](docs/api/cli-reference.md)** - CLI command reference

---

**Last Updated:** 2025-12-23
