# Nexus Memory System Documentation

Welcome to the Nexus Memory System documentation. Nexus is a comprehensive, cross-agent memory management platform that enables intelligent memory storage, retrieval, and sharing across multiple AI agents.

## Table of Contents

- [Quick Start](#quick-start)
- [Features](#features)
- [Architecture](#architecture)
- [Installation](#installation)
- [Configuration](#configuration)
- [Agent Integration](#agent-integration)
- [API Documentation](#api-documentation)
- [Web Interface](#web-interface)
- [Deployment](#deployment)
- [Troubleshooting](#troubleshooting)

## Quick Start

### 1. Installation

```bash
# Clone the repository
git clone https://github.com/scooter-lacroix/nexus-memory-system.git
cd nexus-memory-system

# Install dependencies
pip install -e .

# Or using uv (recommended)
uv install
```

### 2. Initialize Database

```bash
nexus init
```

### 3. Start the Server

```bash
# HTTP mode (recommended for development)
nexus serve --transport http

# STDIO mode (for MCP integration)
nexus serve --transport stdio
```

### 4. Access Web Dashboard

Open http://localhost:8768 in your browser to access the web dashboard.

## Features

### Core Features

- **Multi-Agent Support**: Compatible with Claude Code, Gemini, Qwen, AMP, Droid, OpenCode, Codex, and more
- **Intelligent Search**: Advanced semantic search and memory matching
- **Specification Reuse**: Task specification storage and retrieval for efficiency
- **Cross-Agent Memory**: Knowledge sharing between different agent types
- **Persistent Storage**: SQLite-based storage with optional PostgreSQL support
- **Real-time Updates**: WebSocket support for live memory updates

### Advanced Features

- **Semantic Embeddings**: Optional semantic search using sentence transformers
- **Memory Relations**: Relationship mapping between memories
- **Performance Analytics**: System metrics and performance monitoring
- **Categorization**: Automatic memory categorization and labeling
- **Access Control**: Optional API key authentication

## Architecture

### System Components

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   AI Agents     │    │   MCP Server    │    │   Web Dashboard │
│                 │    │                 │    │                 │
│ • Claude Code   │◄──►│ • HTTP Transport │◄──►│ • Memory Mgmt   │
│ • Gemini        │    │ • STDIO Transport│    │ • Analytics     │
│ • Qwen          │    │ • Tool Registry │    │ • Graph View    │
│ • Droid         │    │                 │    │                 │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                    ┌─────────────────┐
                    │  Nexus Core     │
                    │                 │
                    │ • Memory Mgr    │
                    │ • Spec Mgr      │
                    │ • Search Engine │
                    │ • DB Layer      │
                    └─────────────────┘
                                 │
                    ┌─────────────────┐
                    │   Storage       │
                    │                 │
                    │ • SQLite        │
                    │ • PostgreSQL    │
                    │ • Redis Cache   │
                    └─────────────────┘
```

### Data Flow

1. **Agent Request**: AI agent sends memory operation request
2. **MCP Processing**: FastMCP server processes the request
3. **Business Logic**: Nexus Manager handles business logic
4. **Database Operations**: Database managers handle persistence
5. **Response**: Results returned to agent

## Installation

### System Requirements

- Python 3.9 or higher
- 2GB RAM minimum (4GB recommended)
- 1GB disk space minimum

### Installation Methods

#### Method 1: pip install (Recommended)

```bash
pip install nexus-memory-system
```

#### Method 2: From Source

```bash
git clone https://github.com/scooter-lacroix/nexus-memory-system.git
cd nexus-memory-system
pip install -e .
```

#### Method 3: Using uv

```bash
uv install nexus-memory-system
```

#### Method 4: Docker

```bash
docker pull scooter-lacroix/nexus-memory-system:latest
docker run -d -p 8767:8767 -p 8768:8768 scooter-lacroix/nexus-memory-system
```

### Development Installation

```bash
git clone https://github.com/scooter-lacroix/nexus-memory-system.git
cd nexus-memory-system
pip install -e ".[dev]"
```

## Configuration

### Environment Variables

Nexus can be configured through environment variables or a `.env` file:

```bash
# Database Configuration
NEXUS_DATABASE_PATH=/path/to/nexus.db
NEXUS_DATABASE_URL=postgresql://user:pass@host:port/dbname

# Server Configuration
NEXUS_HOST=0.0.0.0
NEXUS_PORT=8767
NEXUS_WEB_PORT=8768

# Memory Configuration
NEXUS_CONSCIOUS_INGEST=true
NEXUS_AUTO_INGEST=true
NEXUS_MEMORY_SEARCH_LIMIT=10

# OpenAI Configuration (optional)
OPENAI_API_KEY=your_openai_api_key
```

### Configuration File

Copy `.env.example` to `.env` and modify as needed:

```bash
cp .env.example .env
```

### Agent Configuration

See the [Agent Integration Documentation](agents/) for detailed setup instructions for each agent type.

## Agent Integration

### Supported Agents

- [Claude Code](agents/claude-code.md) - Advanced coding and development assistant
- [Gemini](agents/gemini.md) - Google's multimodal AI assistant
- [Qwen](agents/qwen.md) - Alibaba's large language model
- [AMP](agents/amp.md) - ETL/ELT data pipeline specialist
- [Droid](agents/droid.md) - Universal task automation agent
- [OpenCode](agents/opencode.md) - High-concurrency API specialist
- [Codex](agents/codex.md) - Code review and modularity expert

### Integration Methods

#### 1. MCP Protocol (Recommended)

Configure your agent to use Nexus as an MCP server:

```bash
export CLAUDE_MCP_SERVERS='{"nexus": {"command": "nexus", "args": ["serve", "--transport", "stdio"]}}'
```

#### 2. HTTP API

Make HTTP requests to the Nexus server:

```bash
curl -X POST http://localhost:8767/mcp/call \
  -H "Content-Type: application/json" \
  -d '{"tool": "store_agent_memory", "arguments": {...}}'
```

#### 3. Agent Scripts

Use the provided agent integration scripts:

```bash
python agents/scripts/claude_code_integration.py
```

## API Documentation

### MCP API

The Model Context Protocol (MCP) provides standardized access to Nexus features:

- [MCP API Reference](api/mcp-api.md)
- [Tool Documentation](api/tools.md)
- [Schema Reference](api/schema.md)

### REST API

HTTP API for direct integration:

- [REST API Documentation](api/rest-api.md)
- [Authentication](api/authentication.md)
- [Rate Limiting](api/rate-limiting.md)

### WebSocket API

Real-time updates and notifications:

- [WebSocket API Guide](api/websocket-api.md)
- [Event Types](api/events.md)
- [Client Examples](api/client-examples.md)

## Web Interface

### Dashboard

Access the web dashboard at `http://localhost:8768`:

- [Dashboard Guide](web-ui/dashboard.md)
- [Memory Management](web-ui/memory-management.md)
- [Knowledge Graph](web-ui/knowledge-graph.md)
- [Analytics](web-ui/analytics.md)

### Features

- **Memory Browser**: Browse and search memories
- **Knowledge Graph**: Visualize memory relationships
- **Analytics**: System performance and usage statistics
- **Configuration**: Web-based configuration management
- **Agent Management**: Monitor agent activity

## Deployment

### Production Deployment

- [Production Setup Guide](deployment/production.md)
- [Docker Deployment](deployment/docker.md)
- [Kubernetes Deployment](deployment/kubernetes.md)
- [Systemd Service](deployment/systemd.md)

### Cloud Deployment

- [AWS Deployment](deployment/aws.md)
- [Google Cloud Deployment](deployment/gcp.md)
- [Azure Deployment](deployment/azure.md)

### Monitoring

- [Health Checks](deployment/health-checks.md)
- [Metrics Collection](deployment/metrics.md)
- [Log Management](deployment/logging.md)
- [Alerting](deployment/alerting.md)

## Troubleshooting

### Common Issues

- [Installation Problems](troubleshooting.md#installation)
- [Database Issues](troubleshooting.md#database)
- [Performance Issues](troubleshooting.md#performance)
- [Agent Integration](troubleshooting.md#agent-integration)

### Debug Mode

Enable debug mode for detailed logging:

```bash
nexus serve --transport http --debug
```

### Getting Help

- [GitHub Issues](https://github.com/scooter-lacroix/nexus-memory-system/issues)
- [Discussions](https://github.com/scooter-lacroix/nexus-memory-system/discussions)
- [Documentation](https://github.com/scooter-lacroix/nexus-memory-system/docs)

## Contributing

We welcome contributions! See the [Contributing Guide](../CONTRIBUTING.md) for details.

### Development Setup

```bash
git clone https://github.com/scooter-lacroix/nexus-memory-system.git
cd nexus-memory-system
make dev-setup
```

### Running Tests

```bash
make test
```

### Code Style

```bash
make format
make lint
```

## License

Nexus Memory System is licensed under the MIT License. See the [LICENSE](../LICENSE) file for details.

## Changelog

See the [CHANGELOG.md](../CHANGELOG.md) for version history and updates.

---

**Nexus Memory System** - Connecting agents through intelligent memory management.