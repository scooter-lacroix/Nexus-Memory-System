# Spec: Rust Orchestrator Core

**Track ID:** rust-orchestrator-core_20250216
**Type:** Feature
**Status:** New

---

## Overview

Implement the orchestrator core in Rust including session lifecycle management, event bus using tokio::sync::broadcast, cross-agent synchronization, and context enhancement.

**Python Mapping:** `nexus/orchestrator/`

---

## Functional Requirements

### FR1: Session Management

```rust
pub struct Session {
    id: Uuid,
    agent_type: String,
    started_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    state: SessionState,
}

pub enum SessionState {
    Active,
    Idle,
    Completed,
}
```

### FR2: Event Bus

```rust
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

pub enum Event {
    MemoryStored(Memory),
    SessionStarted(SessionId),
    SessionEnded(SessionId),
    CrossAgentSyncRequest(SyncData),
}
```

### FR3: Session Tracker

- Track active sessions per agent type
- Detect idle sessions (timeout based)
- Manage session lifecycle
- Session statistics

### FR4: Cross-Agent Synchronization

- Share memories between agent namespaces
- Configurable sync policies (manual, auto, aggressive)
- Conflict resolution
- Sync event propagation

### FR5: Context Enhancement

- Retrieve relevant context for queries
- Vector search integration
- Memory ranking and scoring
- Context window management

---

## Non-Functional Requirements

### NFR1: Performance

| Metric | Target |
|--------|--------|
| Event propagation | <1ms |
| Concurrent sessions | 10,000+ |
| Session creation | <100μs |
| Context retrieval | <10ms |

### NFR2: Reliability

- No lost events (best-effort delivery)
- Graceful shutdown with session cleanup
- Recovery from crashes using persistent buffer

### NFR3: Code Quality

- 95%+ test coverage
- Lock-free where possible
- No deadlock scenarios

---

## Acceptance Criteria

### AC1: Session Lifecycle

```rust
let orchestrator = Orchestrator::new(Arc::new(RwLock::new(storage)));
let session = orchestrator.create_session("claude-code").await?;
assert_eq!(session.state, SessionState::Active);
```

### AC2: Event Bus Functional

```rust
let bus = EventBus::new(1000);
let mut rx = bus.subscribe();
bus.publish(Event::SessionStarted(session_id)).await;
let event = rx.recv().await?;
assert!(matches!(event, Event::SessionStarted(_)));
```

### AC3: Cross-Agent Sync

```rust
orchestrator.sync_memory(&memory, &["claude-code", "pi-mono"]).await?;
let claude_mem = orchestrator.get_memory(&memory.id, "claude-code").await?;
let pi_mem = orchestrator.get_memory(&memory.id, "pi-mono").await?;
assert_eq!(claude_mem.content, pi_mem.content);
```

### AC4: Context Enhancement

```rust
let context = orchestrator.enhance_context("user preferences", "claude-code", 5).await?;
assert!(context.memories.len() <= 5);
assert!(context.memories.iter().all(|m| m.relevance_score > 0.7));
```

---

## Dependencies

### External Crates

```toml
[dependencies]
tokio = { version = "1.40", features = ["sync", "rt-multi-thread"] }
uuid = { version = "1.10", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
```

### Local Dependencies

- `nexus-core` - Core types, traits
- `nexus-storage` - Memory, Session repositories
- `nexus-vectors` - Vector search for context enhancement
- `nexus-embeddings` - Query embedding generation

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      ORCHESTRATOR CORE                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │   Session    │  │   Event      │  │    Sync      │       │
│  │   Manager    │  │    Bus       │  │  Coordinator │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│          │                 │                  │             │
│          └─────────────────┼──────────────────┘             │
│                            ▼                                │
│                   ┌──────────────┐                          │
│                   │   Context    │                          │
│                   │  Enhancer    │                          │
│                   └──────────────┘                          │
└─────────────────────────────────────────────────────────────┘
```

---

## Out of Scope

- Multi-instance orchestration (future)
- Distributed coordination (future)
- Advanced conflict resolution policies (future)

---

## References

- Python implementation: `nexus/orchestrator/`
- CLAUDE.md: Architecture Translation section

---

**Version:** 1.0
**Created:** 2025-02-16
