# Product Guide: Nexus Memory System

## Overview

Nexus Memory System is a **cross-agent memory management platform** for AI agents. It provides automated memory extraction, semantic search with vector embeddings, and cross-agent knowledge sharing.

**Current Status:** Private/Internal Use (Python implementation with Rust port imminent)

---

## Target Users

| User Type | Description | Use Case |
|-----------|-------------|----------|
| **AI Researchers** | Building memory-enabled agents | Research context persistence and agent evolution |
| **Production Teams** | Deploying multiple AI agents | Cross-agent knowledge sharing in production |
| **Individual Developers** | Using Claude Code, Cursor, etc. | Personal AI agent memory management |
| **Agent Framework Developers** | Building custom agent integrations | Platform for agent memory interoperability |

---

## Core Goals

### 1. Cross-Session Memory
Enable AI agents to remember context across sessions, maintaining continuity of conversations and learnings over time.

### 2. Cross-Agent Knowledge Sharing
Share learned information between different AI agents (Claude Code, Gemini, Qwen, pi-mono, oh-my-pi, etc.) through a unified memory namespace system.

### 3. Automated Memory Extraction
Four-layer automated extraction system with 95-100% reliability:
- **Layer 1:** Native Agent Hooks (100% success)
- **Layer 2:** Session Monitor (95% success)
- **Layer 3:** Inactivity Detector (90% success)
- **Layer 4:** Persistent Buffer (99% crash recovery)

### 4. Automated Memory Pruning
Intelligent memory management to prevent clutter while preserving important context.

### 5. Token-Efficient Memory Processing
Model memory analysis using LePhase-style compression for token-efficient storage and retrieval.

---

## Key Features

### Semantic Vector Search
- **Technology:** sqlite-vec with 384-dimensional embeddings
- **Model:** all-MiniLM-L6-v2 (sentence-transformers)
- **Performance:** Sub-10ms search latency (Rust port target)
- **Capability:** Semantic similarity search across all memories

### Multi-Agent Hooks System
Native hooks support for all major AI agents:
| Agent | Hook Type | Status |
|-------|-----------|--------|
| Claude Code | Skills (Oct 2025) | Fully Supported |
| **pi-mono** | Skills (TypeScript/Bun) | MANDATORY |
| **oh-my-pi** | Skills (TypeScript/Bun + Rust N-API) | MANDATORY |
| **pi-skills** | Skills (Cross-compatible) | MANDATORY |
| Gemini | Function Calling | Fully Supported |
| Qwen | Hooks SubAgent | Fully Supported |
| Amp, Droid, OpenCode, Codex | CLI atexit/signals | Fully Supported |

### Web Dashboard & API
- **REST API:** Complete CRUD for memories, specifications, stats
- **WebSocket:** Real-time updates for live monitoring
- **Dashboard:** Browser-based memory visualization and management
- **Port:** 8768 (default web dashboard)

### Hybrid Memory Type System
- **Nexus Categories:** general, facts, preferences, context, specifications, session
- **Memory Lane Types:** semantic, episodic, procedural, working, explicit, implicit, flashbulb, metamemory, collective
- **Priority Types:** correction, decision, commitment (high) | insight, learning, confidence (medium) | pattern_seed, cross_agent, workflow_note, gap (low)

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Memory Capture Reliability** | 95-100% | Four-layer extraction success rate |
| **Search Performance** | <10ms (Rust) | Semantic search latency |
| **Scalability** | 10,000+ concurrent | Concurrent agent connections |
| **Agent Evolution** | Persistent identity | Agent memories persist without context clutter |
| **Token Efficiency** | LePhase-optimized | Token-efficient storage/retrieval |

---

## Technical Context

### Current Stack (Python)
- Python 3.9+, SQLAlchemy 2.0, FastAPI
- sqlite-vec, sentence-transformers, FastMCP
- Asyncio, aiosqlite, loguru

### Target Stack (Rust)
- Rust 1.75+, SQLx/SeaORM, Axum
- sqlite-vec, candle-transformers (or ort)
- rmcp (Rust MCP implementation)

### Architecture
5-layer component system:
1. **Storage Manager** - Database operations, CRUD, transactions
2. **Processing Engine** - Embeddings, NLP, categorization, vector search
3. **Agent Hooks Manager** - Native hooks, session detection, automated extraction
4. **Orchestrator** - Session lifecycle, event routing, cross-agent sync
5. **Web Dashboard** - HTTP API, WebSocket, UI, visualization

---

## Rust Port Requirements (MANDATORY)

### Vector Database with Graph Tree Structure
- Efficient resource management
- High accuracy semantic search
- Performance-optimized storage backend

### LLM-Triggered Hooks
- Native hooks in ALL supported CLI tools
- Seamless integration with agent workflows
- Automated memory extraction triggered by LLM analysis

### LePhase Integration
- Token-efficient memory compression during storage
- Optimized retrieval/presentation to models
- Reference implementation: `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/`
