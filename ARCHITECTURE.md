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
2. The surface uses `nexus-storage` repositories and related retrieval helpers.
3. `nexus-vectors` and `nexus-embeddings` may participate in semantic lookup flows.
4. Results are returned through the active interface.

### Hook flow

1. `nexus hooks install` writes tool-specific hook assets.
2. Supported agent runtimes trigger those hooks during their lifecycle.
3. Hook code extracts relevant session context.
4. Nexus persists the extracted memory through the shared storage layer.

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
