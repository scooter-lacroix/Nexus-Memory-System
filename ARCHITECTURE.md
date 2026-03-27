# Nexus Memory System Architecture

This document describes the current public architecture of Nexus Memory System as implemented by the Rust workspace in `crates/`.

## System Overview

Nexus is currently organized as a set of focused Rust crates around one shared domain model and one shared storage layer. This is an implementation detail of the current workspace layout, not a requirement for the public product shape.

```text
┌─────────────────────────────────────────────────────────────────┐
│                         External Surfaces                       │
├─────────────────────────────────────────────────────────────────┤
│  nexus-cli  │  nexus-hooks  │  nexus-mcp  │  nexus-web        │
└─────────────────────────────────────────────────────────────────┘
                 \        |        |        /
                  \       |        |       /
                   └──────┴────────┴──────┘
                              |
                              v
                     ┌──────────────────┐
                     │   nexus-core     │
                     │ types + config   │
                     └────────┬─────────┘
                              |
          ┌───────────────────┼───────────────────┐
          v                   v                   v
 ┌────────────────┐  ┌────────────────┐  ┌────────────────────┐
 │ nexus-storage  │  │ nexus-vectors  │  │ nexus-embeddings   │
 │ sqlite + repos │  │ vector search  │  │ embedding pipeline │
 └────────┬───────┘  └────────┬───────┘  └──────────┬─────────┘
          \                   |                     /
           \                  |                    /
            └─────────────────┴───────────────────┘
                              |
                              v
                     ┌────────────────────┐
                     │ nexus-orchestrator │
                     │ context + sync     │
                     └────────────────────┘
```

## Workspace Crates

### `nexus-core`

Shared domain types, configuration, category definitions, identifiers, and common error handling live here. This crate is the foundation for the rest of the workspace.

### `nexus-storage`

The storage layer owns:

- SQLite connection management
- schema initialization and migrations
- repository access for namespaces and memories
- persistence of task specifications and statistics-related queries

`StorageManager` is the main entrypoint for opening and initializing the database.

### `nexus-vectors`

This crate provides vector-oriented indexing and retrieval logic on top of stored memories. It is intended to support semantic and structural lookup patterns over the shared memory corpus.

### `nexus-embeddings`

Embeddings-related responsibilities live here, including model integration and embedding generation support used by higher-level retrieval workflows.

### `nexus-orchestrator`

The orchestrator crate coordinates higher-level memory workflows such as enriched context construction, event flow, and synchronization behavior across the system.

### `nexus-hooks`

Hooks are the native integration layer for supported agent tools. This crate contains:

- agent-specific hook installers
- extraction and monitoring support
- session and signal handling
- hook factory and shared hook types

### `nexus-mcp`

This crate exposes Nexus through an MCP-compatible surface so MCP clients can read and work with the same backing store as the CLI and web stack.

### `nexus-web`

The web layer is built with Axum and exposes:

- `/api/memories`
- `/api/memories/search`
- `/api/namespaces`
- `/api/stats`
- `/api/stats/:agent`
- `/api/health`
- `/ws`

It depends on `nexus-storage`, `nexus-orchestrator`, and `nexus-vectors`.

### `nexus-cli`

The CLI is the main operational surface. Current commands include:

- `init`
- `serve`
- `store`
- `search`
- `stats`
- `hooks`
- `migrate`

### `nexus-lephase`

This crate provides LePhase integration glue used by the wider workspace.

## Data Flow

### Store flow

1. A caller invokes `nexus store`, a hook callback, or another surface.
2. The command or service resolves configuration from `nexus-core`.
3. `nexus-storage` opens the database and ensures schema availability.
4. The namespace is resolved or created.
5. The memory record is persisted with category, labels, and metadata.

### Query flow

1. A caller invokes `search`, stats endpoints, MCP tools, or web routes.
2. The surface builds a bounded `WorkingRepresentation` from digests, recent explicit memories, semantic matches, derived insights, and contradictions.
3. `nexus-vectors` and `nexus-embeddings` participate in vector-first semantic lookup with bounded text fallback.
4. `nexus-lephase` compresses larger contexts and `nexus-agent` produces lineage-aware answers or introspection output.
5. Results are returned through the active interface.

### Hook flow

1. `nexus hooks install` writes tool-specific hook assets.
2. Supported agent runtimes trigger those hooks during their lifecycle.
3. Hook code extracts relevant session context.
4. Nexus persists the extracted memory through the shared storage layer.

### Agent Support Tiers

Nexus supports multiple agent integrations with varying depths of lifecycle coverage. Each agent is classified into one of three support tiers:

| Agent | Tier | Lifecycle Events | Detection |
|-------|------|-----------------|-----------|
| Claude Code | native-lifecycle | start, end, checkpoint, error, compact | Skill file + process |
| pi-mono | native-lifecycle | end, checkpoint, compact | Skill file + process |
| oh-my-pi | native-lifecycle | end, checkpoint, error, compact | Skill file + process |
| pi-skills | native-lifecycle | end, checkpoint, compact | Skill file + process |
| Gemini | monitor-only | — | Process monitoring |
| Qwen | monitor-only | — | Process monitoring |
| Codex | wrapper-lifecycle | — | CLI wrapper (atexit) |
| Amp | wrapper-lifecycle | — | CLI wrapper (atexit) |
| OpenCode | wrapper-lifecycle | — | CLI wrapper (atexit) |
| Droid | wrapper-lifecycle | — | CLI wrapper (atexit) |
| Hermes | wrapper-lifecycle | — | CLI wrapper (atexit) |

**Tier definitions:**

- **native-lifecycle**: Dedicated hook implementation with native skill/hook file installation. Multiple lifecycle events are wired (session start/end, checkpoint, error, compact).
- **wrapper-lifecycle**: Agent uses a shared generic CLI wrapper for process detection and atexit fallback. No dedicated hook implementation exists.
- **monitor-only**: Process detection only. No native hooks, no session lifecycle events. Memory capture relies on process monitoring and inactivity detection.

## Hybrid Category System

Nexus uses a hybrid category system built around the core categories defined in `nexus-core`.

Core categories currently include:

- `general`
- `facts`
- `preferences`
- `context`
- `specifications`
- `session`

Additional metadata and labels can be attached to memories without changing the primary category model. This keeps the storage and retrieval model simple while still allowing richer downstream classification.

## Public Interfaces

### CLI

Primary path for initialization, storage, stats, search, server startup, and hook management.

### Web

HTTP and WebSocket interface for browser and service consumers.

### MCP

Protocol-oriented access for MCP-capable clients and toolchains.

### Hooks

Automatic ingestion path for supported agent runtimes.

## Design Notes

- The workspace is Rust-first for all public architecture guidance.
- `nexus-core` and `nexus-storage` are the backbone of the current implementation.
- Higher-level surfaces share the same backing store instead of maintaining separate memory silos.
- The externally visible model is one Nexus system, even if the internal crate boundaries change over time.

## Always-On Cognition Runtime

Nexus includes an optional always-on cognition runtime that provides LLM-driven memory processing and background subconscious behavior.

### Cognition Crates

- **`nexus-llm`**: Multi-provider LLM abstraction supporting OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, and Mistral.
- **`nexus-agent`**: Cognition runtime with native services for:
  - **IngestService**: Stores raw activity and lifecycle events with canonical cognitive metadata
  - **DeriveService**: Turns raw or low-signal activity into explicit observations with evidence lineage
  - **DigestService**: Maintains short and long session digests with bounded rollover
  - **ReflectService**: Runs bounded dream cycles for reinforcement, contradiction handling, and induced insights
  - **RepresentationService**: Assembles a working set from digests, semantic matches, recent explicit memories, derived insights, and contradictions
  - **QueryService**: Produces lineage-aware answers and introspection from the working representation

### Cognition Data Flow

The runtime stores all data using the shared storage layer:
- Raw activity, explicit observations, derived insights, contradictions, and digests are stored in `memories` with structured `cognitive` metadata
- Evidence links for derived outputs are stored in `memory_evidence`
- Session digest pointers and coverage windows are stored in `session_digests`
- Bounded background work is stored in `memory_jobs`
- Relations remain available in `memory_relations` where graph-style links are useful
- Processed inbox files are tracked in `processed_files`

### Cognition Endpoints

When the cognition runtime is enabled (`NEXUS_AGENT_ENABLED=true` or `nexus serve --agent`):
- `POST /api/agent/ingest` — Ingest text or hook-derived content into the cognition pipeline
- `POST /api/agent/query` — Query memory with representation-first recall and answer synthesis
- `POST /api/agent/consolidate` — Trigger a manual dream/reflection pass
- `GET /api/agent/status` — Runtime health and statistics
- `GET /api/cognition/dashboard?namespace=<name>` — Operator view of digests, recall mix, dream throughput, and adaptive state
