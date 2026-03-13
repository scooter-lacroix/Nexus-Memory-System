# Changelog

All notable changes to Nexus Memory System will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This changelog covers the public Rust workspace and its release history.

---

## [Unreleased]

### Added

- Public community-health documentation for GitHub repository launch
- Support, security, and code-of-conduct guidance
- Development workflow documentation for Rust-first contributors
- Issue and pull request templates

### Changed

- README, installation, and contributing guidance updated for a public-facing repository

### Fixed

- Rust CLI `init` now initializes the real storage layer
- Rust CLI `store` now persists live memories instead of returning stub output
- Rust CLI `stats` now reads actual database counts
- Rust CLI `hooks` now uses the hook factory rather than placeholder responses
- Rust CLI short-flag collisions that caused Clap runtime panics

---

## [1.1.0] - 2025-12-23

### Major Release: Native Hooks and Hybrid Memory Types

This release represents a significant architectural shift from MCP-based integration to **native hooks** for automated memory extraction, along with a comprehensive **hybrid memory type system**.

### Added - Native Hooks System
- **Native Agent Hooks** - Automated memory extraction without MCP protocol
  - Claude Code Skills integration (Oct 2025)
  - Gemini Function Calling + CLI Extensions
  - Qwen Hooks SubAgent support
  - Generic CLI hooks (atexit, signals) for Amp, Droid, OpenCode, Codex
- **Four-Layer Extraction System** - 95-100% reliability
  - Layer 1: Native agent hooks (primary)
  - Layer 2: Session monitor (secondary)
  - Layer 3: Inactivity detector (tertiary)
  - Layer 4: Persistent buffer for crash recovery (safety net)
- **Hooks Manager Service** - Centralized hooks orchestration
- **Session Detection** - Multi-layer session detection with fallbacks
- **Persistent Buffer** - Crash recovery with incremental buffering
- **CLI Hooks Commands** - `nexus hooks install|uninstall|status|start|stop|extract`

### Added - Hybrid Memory Type System
- **Nexus Core Categories** (6 types)
  - `general`, `facts`, `preferences`, `context`, `specifications`, `session`
- **Memory Lane Cognitive Types** (9 types)
  - `semantic`, `episodic`, `procedural`, `working`, `explicit`
  - `implicit`, `flashbulb`, `metamemory`, `collective`
- **Memory Lane Priority Types** (10 types)
  - High: `correction`, `decision`, `commitment`
  - Medium: `insight`, `learning`, `confidence`
  - Lower: `pattern_seed`, `cross_agent`, `workflow_note`, `gap`
- **Agent-Specific Categories** (7 types)
  - `claude-code`, `gemini`, `qwen`, `amp`, `droid`, `opencode`, `codex`

### Added - Orchestrator
- **Session Lifecycle Management** - Complete session tracking
- **Event Bus** - Async event processing and routing
- **Cross-Agent Sync** - Memory sharing between agent namespaces
- **Memory Consistency** - Validation and enforcement
- **Workflow Coordination** - Component orchestration

### Added - High-Performance Embeddings
- **sqlite-vec Integration** - Native SQLite vector search
- **sentence-transformers** - all-MiniLM-L6-v2 model (384 dimensions)
- **Semantic Search API** - Fast similarity search (~1000 docs/sec)
- **Embedding Service** - Configurable model selection and caching
- **GPU Support** - Optional CUDA acceleration

### Added - Web Dashboard
- **FastAPI Application** - REST API with OpenAPI/Swagger docs
- **WebSocket Events** - Real-time event streaming
- **CORS Middleware** - Cross-origin support
- **Health Checks** - `/health` endpoint
- **API Routes**
  - `/api/v1/memories` - CRUD operations
  - `/api/v1/memories/search` - Semantic search
  - `/api/v1/stats` - Statistics
  - `/api/v1/hooks/status` - Hooks status
  - `/ws/events` - WebSocket events

### Changed
- **Architecture** - 5 core components (down from 9)
  - Storage Manager, Processing Engine, Agent Hooks Manager, Orchestrator, Web Dashboard
- **Database Models** - Added hybrid memory type fields
  - `memory_lane_type` column for Memory Lane types
  - Enhanced metadata for cognitive attributes
- **Configuration** - New hooks and embeddings options
- **Documentation** - Complete rewrite focusing on native hooks
- **Project positioning** - Documentation reflected the internal deployment model in use at that time

### Fixed
- **Session Category** - Added `session` to MemoryCategory enum
- **Hooks Installation** - Fixed hook installation for all agent types
- **Buffer Recovery** - Improved crash recovery from persistent buffer
- **Embedding Storage** - Fixed sqlite-vec blob storage
- **CLI Output** - Fixed Rich markup formatting

### Deprecated
- **MCP Transport** - No longer the primary integration method
- **Old architecture references** - Updated to reflect 5-component design

### Removed
- **Old MCP-only documentation** - Replaced with hooks-focused docs

---

## [1.0.0] - 2024-12-01

### Added
- Initial release of Nexus Memory System
- Multi-agent memory management platform
- MCP (Model Context Protocol) server implementation
- Cross-agent memory sharing capabilities
- Task specification storage and reuse
- SQLite database backend with PostgreSQL support
- HTTP and STDIO transport protocols
- Web UI for memory management
- Agent namespace isolation
- Memory categorization and labeling
- Intelligent search and retrieval
- RESTful API endpoints
- WebSocket support for real-time updates
- Comprehensive CLI tool
- Docker containerization
- Systemd service configuration
- Complete documentation suite
- Testing framework with unit and integration tests

### Features
- Support for 8+ agent types: Claude Code, Gemini, Qwen, AMP, Droid, OpenCode, Codex, General
- Persistent memory storage with SQLite (default) or PostgreSQL
- Semantic search capabilities with optional embeddings
- Task specification system for reusing work
- Real-time memory updates via WebSocket
- Comprehensive web dashboard
- CLI tools for management and integration
- Docker deployment support
- Performance monitoring and analytics
- Security features with API key authentication

### Documentation
- Complete API documentation
- Agent integration guides
- Deployment instructions
- Troubleshooting guide
- Configuration reference
- Architecture documentation

### Development
- Cross-platform local development support
- Type hints throughout codebase
- Comprehensive test suite
- CI/CD ready configuration
- Code quality tools integration
- Development setup scripts

---

## [0.9.0] - 2024-11-15

### Added
- Beta release of memori-mcp-server
- Basic MCP server implementation
- Simple memory storage and retrieval
- Agent namespace support
- CLI interface

---

## [0.1.0] - 2024-11-01

### Added
- Initial project concept
- Basic architecture design
- Prototype implementation

---

**Last Updated:** 2025-12-23
