# Spec: Complete Rust Migration - Master Orchestration Track

**Track ID:** rust-migration-master_20250216
**Type:** Orchestration
**Status:** New

---

## Overview

Master orchestration track for the complete Python-to-Rust migration of Nexus Memory System. This track coordinates all sub-tracks in dependency order, ensuring systematic migration while maintaining system functionality.

**Migration Strategy:** Gradual migration with hybrid interoperability period. Python and Rust implementations will coexist during transition.

---

## Sub-Track Catalog

### Track 1: Core Foundation ✅ (CREATED)
**ID:** rust-core-foundation_20250216
**Status:** New (ready to implement)
**Dependencies:** None

**Components:**
- Cargo workspace structure
- Vector database with graph tree structure (MANDATORY)
- LePhase integration for token efficiency (MANDATORY)
- Core storage manager (SQLx)

**Deliverables:**
- Multi-crate workspace (nexus-core, nexus-storage, nexus-vectors, nexus-lephase)
- <10ms vector search latency
- >50% memory compression with LePhase
- SQLite schema compatible with Python

---

### Track 2: Embedding Service
**ID:** rust-embedding-service_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** Track 1 (Core Foundation)

**Components:**
- EmbeddingService trait and implementation
- ONNX Runtime (ort) bindings for all-MiniLM-L6-v2
- 384-dimensional vector generation
- Async batch processing

**Python Mapping:** `nexus/embeddings/service.py`

**Deliverables:**
- <5ms embedding latency (per text)
- Batch processing support
- Compatible with existing Python embeddings

---

### Track 3: Hooks System
**ID:** rust-hooks-system_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** Track 1 (Core Foundation)

**Components:**
- AgentHook trait and factory
- Session detection and monitoring
- Four-layer extraction system
- Persistent buffer for crash recovery

**MANDATORY Pi-Agent Support:**
- pi-mono hooks (TypeScript/Bun)
- oh-my-pi hooks (TypeScript/Bun + Rust N-API)
- pi-skills cross-compatible hooks

**Python Mapping:** `nexus/hooks/`

**Deliverables:**
- Native hooks for all supported agents
- 95-100% extraction reliability
- Cross-platform signal handling

---

### Track 4: Orchestrator Core
**ID:** rust-orchestrator-core_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** Track 1 (Core Foundation), Track 2 (Embedding Service)

**Components:**
- Session lifecycle management
- Event bus (tokio::sync::broadcast)
- Cross-agent synchronization
- Context enhancement

**Python Mapping:** `nexus/orchestrator/`

**Deliverables:**
- 10,000+ concurrent session support
- Sub-millisecond event propagation
- Cross-agent memory sync

---

### Track 5: MCP Server
**ID:** rust-mcp-server_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** Track 1 (Core Foundation), Track 4 (Orchestrator Core)

**Components:**
- rmcp-based MCP server
- stdio and HTTP transports
- Memory tools implementation
- Resource management

**Python Mapping:** `nexus/server/mcp_server.py`

**Deliverables:**
- Full FastMCP compatibility
- All memory tools accessible
- Graceful shutdown handling

---

### Track 6: Web Dashboard
**ID:** rust-web-dashboard_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** Track 4 (Orchestrator Core), Track 5 (MCP Server)

**Components:**
- Axum web framework
- REST API endpoints
- WebSocket real-time updates
- Static file serving

**Python Mapping:** `nexus/web/`

**Deliverables:**
- All API endpoints ported
- WebSocket streaming functional
- Port 8768 compatibility

---

### Track 7: CLI Application
**ID:** rust-cli-app_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** Track 5 (MCP Server), Track 6 (Web Dashboard)

**Components:**
- clap-based CLI
- All commands ported (init, serve, store, search, stats, hooks)
- Configuration management
- Shell completion

**Python Mapping:** `nexus/cli.py`

**Deliverables:**
- Full CLI parity with Python
- Shell completion for bash/zsh/fish
- Config file support

---

### Track 8: Migration & Integration
**ID:** rust-migration-integration_20250216 (TO BE CREATED)
**Status:** Pending
**Dependencies:** All previous tracks

**Components:**
- Database migration tool
- Deployment automation
- Documentation updates
- Performance validation

**Deliverables:**
- Zero-downtime migration path
- Performance benchmarks passing
- Complete documentation
- Rollback procedures

---

## Dependency Graph

```
┌─────────────────────────────────────────────────────────────────┐
│                    RUST MIGRATION DEPENDENCIES                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────────┐                                          │
│  │ Track 1: Core    │  ← Foundation (NO DEPENDENCIES)          │
│  │   Foundation     │                                          │
│  └────────┬─────────┘                                          │
│           │                                                    │
│     ┌─────┴─────┬────────────┐                                │
│     ▼           ▼            ▼                                │
│  ┌────────┐ ┌────────┐ ┌──────────┐                          │
│  │Track 2│ │Track 3│ │Track 4   │  ← Can run in parallel      │
│  │Embedding│ │Hooks  │ │Orchestrat│    after Track 1          │
│  └───┬────┘ └───┬────┘ └────┬─────┘                          │
│      │          │           │                                  │
│      └──────────┴───────────┼──────┐                          │
│                             ▼      ▼                          │
│                          ┌──────────┐                         │
│                          │ Track 5  │  ← Requires 4, (1,2)   │
│                          │   MCP    │                          │
│                          └────┬─────┘                         │
│                               │                               │
│                    ┌──────────┴──────────┐                    │
│                    ▼                     ▼                    │
│              ┌──────────┐         ┌──────────┐               │
│              │ Track 6  │         │ Track 7  │  ← Parallel    │
│              │   Web    │         │   CLI    │    after 5     │
│              └────┬─────┘         └────┬─────┘               │
│                   │                   │                       │
│                   └──────────┬────────┘                       │
│                              ▼                                │
│                    ┌──────────────────┐                       │
│                    │  Track 8: Final  │  ← Requires all      │
│                    │   Integration    │                       │
│                    └──────────────────┘                       │
└─────────────────────────────────────────────────────────────────┘
```

---

## MANDATORY Requirements (from product.md)

### M1: Vector Database with Graph Tree
- Hierarchical memory organization
- Efficient resource management
- High-accuracy semantic search
- **Target:** <10ms search latency

### M2: LLM-Triggered Hooks
- Native hooks in ALL supported CLI tools
- Seamless integration with agent workflows
- Automated memory extraction

### M3: LePhase Integration
- Token-efficient memory compression during storage
- Optimized retrieval/presentation to models
- **Reference:** `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/`

### M4: Pi-Agent Family Support (MANDATORY)
- **pi-mono:** TypeScript/Bun skills-based hooks
- **oh-my-pi:** TypeScript/Bun + Rust N-API hooks
- **pi-skills:** Cross-compatible skill hooks

---

## Performance Targets

| Metric | Python Baseline | Rust Target |
|--------|-----------------|-------------|
| **Embedding latency** | ~10ms | <5ms |
| **Vector search (1k docs)** | ~50ms | <10ms |
| **Memory store** | ~5ms | <1ms |
| **Concurrent connections** | ~100 | 10,000+ |
| **Memory overhead** | ~200MB | ~50MB |

---

## Orchestration Rules

### Execution Strategy
1. **Sequential by Dependency:** Sub-tracks execute in dependency order
2. **Parallel Where Possible:** Tracks 2, 3, 4 can run in parallel after Track 1
3. **Checkpoint After Each Track:** Create git tag after each sub-track completion
4. **Rollback Capability:** Each track must be independently revertible

### Handoff Protocol
1. **Upstream Verification:** Verify dependencies are satisfied before starting
2. **Integration Testing:** Run cross-track integration tests
3. **Documentation Update:** Update CLAUDE.md with Rust equivalents
4. **API Compatibility:** Maintain Python API compatibility during transition

### Quality Gates
- 95%+ test coverage (MANDATORY per workflow.md)
- All clippy lints pass
- rustfmt compliance
- Performance targets met
- Tzar of Excellence review passed

---

## Acceptance Criteria

### Master Track Complete When:
- [ ] All 8 sub-tracks are marked complete
- [ ] All performance targets met
- [ ] Python and Rust versions can coexist
- [ ] Full migration path documented
- [ ] Zero data loss during migration
- [ ] Rollback procedure tested
- [ ] CLAUDE.md updated for Rust-first development

### Migration Success Definition:
1. **Feature Parity:** All Python features available in Rust
2. **Performance Improvement:** All performance targets met
3. **Compatibility:** Existing databases migrate without loss
4. **Quality:** 95%+ test coverage, zero critical bugs
5. **Documentation:** Complete migration guide

---

## Out of Scope

This master track does NOT include:
- New features exclusive to Rust (deferred to post-migration)
- Python feature additions during migration (freeze except critical)
- Complete removal of Python code (hybrid period after migration)
- External integrations not in original Python codebase

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| **Breaking Changes** | Maintain Python API compatibility |
| **Performance Regression** | Benchmark after each track |
| **Data Loss** | Comprehensive migration testing |
| **Blocking Issues** | Parallel Python maintenance |
| **Scope Creep** | Strict adherence to spec |

---

## References

- **CLAUDE.md:** Complete Rust Port Guide
- **maestro/product.md:** Product requirements and MANDATORY items
- **maestro/tech-stack.md:** Python and Rust technology stacks
- **maestro/workflow.md:** Development workflow and quality gates
- **Track 1:** rust-core-foundation_20250216 (already created)

---

**Version:** 1.0
**Created:** 2025-02-16
**Orchestrates:** 8 sub-tracks for complete Rust migration
