# Spec: Rust Core Foundation - Vector Database with Graph Tree and LePhase Integration

**Track ID:** rust-core-foundation_20250216
**Type:** Feature
**Status:** New

---

## Overview

Establish the Rust implementation foundation for Nexus Memory System with MANDATORY components:
1. Cargo workspace structure for multi-crate architecture
2. Vector database with graph tree structure for efficient resource management
3. LePhase integration for token-efficient memory processing
4. Core storage manager using SQLx

This track focuses on the foundational Rust infrastructure that will support the full Python-to-Rust migration while meeting all MANDATORY requirements defined in the product documentation.

---

## Functional Requirements

### FR1: Cargo Workspace Structure

- Create a multi-crate Cargo workspace with the following crates:
  - `nexus-core` - Core types, traits, and business logic
  - `nexus-storage` - Database operations (SQLx)
  - `nexus-vectors` - Vector database with graph tree structure
  - `nexus-lephase` - LePhase integration wrapper
  - `nexus-mcp` - MCP server implementation (rmcp)
  - `nexus-cli` - Command-line interface

### FR2: Vector Database with Graph Tree Structure

- Implement vector storage using sqlite-vec (Rust bindings)
- Implement graph tree structure for:
  - Hierarchical memory organization
  - Efficient resource management
  - High-accuracy semantic search
- Support 384-dimensional embeddings (all-MiniLM-L6-v2 compatibility)
- Target: <10ms search latency

### FR3: LePhase Integration

- Create bindings to LePhase library at `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/`
- Implement token-efficient memory compression during storage
- Implement optimized retrieval/presentation to models
- Ensure compatibility with Python embedding format

### FR4: Core Storage Manager

- Implement async database operations using SQLx
- Support SQLite with planned PostgreSQL migration path
- Implement migrations from Python database schema
- Support the hybrid memory type system (Nexus + Memory Lane)

---

## Non-Functional Requirements

### NFR1: Performance

| Metric | Target |
|--------|--------|
| Vector Search Latency | <10ms |
| Concurrent Agents | 10,000+ |
| Memory Overhead | ~50MB base |
| Cold Start Time | <500ms |

### NFR2: Code Quality

- 95%+ test coverage (MANDATORY per workflow)
- All unsafe blocks must be documented and justified
- Clippy lints must pass (allow: pedantic is acceptable)
- rustfmt compliance

### NFR3: Compatibility

- Binary format compatibility with Python embeddings
- Database schema compatibility for migration
- API compatibility with Python MCP server

---

## Acceptance Criteria

### AC1: Workspace Builds Successfully

```bash
cargo build --workspace --all-features
# Result: All crates compile without errors
```

### AC2: Vector Database Operations

```rust
// Test vector storage and retrieval
let db = VectorDatabase::new(":memory:").await?;
db.store_vector("id1", vec![0.1; 384], "general").await?;
let results = db.search(&vec![0.1; 384], 10, 0.7).await?;
assert!(results.len() >= 1);
assert!(results[0].latency < std::time::Duration::from_millis(10));
```

### AC3: LePhase Compression

```rust
let original = "Long memory content that needs token optimization...";
let compressed = lephase.compress(original)?;
let ratio = compressed.len() as f64 / original.len() as f64;
assert!(ratio < 0.5, "Should achieve 50%+ compression");
```

### AC4: Database Schema Compatibility

- Migration script from Python database exists
- All existing memory types are supported
- Cross-agent namespace structure is preserved

### AC5: Test Coverage

```bash
cargo test --workspace --all-features
cargo llvm-cov --workspace --html --open
# Result: 95%+ coverage across all crates
```

---

## Out of Scope

This track does NOT include:
- Agent hooks implementation (deferred to separate track)
- MCP server endpoints (deferred to separate track)
- Web dashboard (Python version remains in use)
- Embedding model inference (uses Python service initially)
- Cross-agent synchronization logic (deferred)

---

## Dependencies

### External Crates

```toml
[workspace.dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "uuid"] }
tokio = { version = "1.40", features = ["full"] }
sqlite-vec = "0.1"
uuid = { version = "1.10", features = ["v4", "serde"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
```

### Local Dependencies

- LePhase library at `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/`

---

## References

- Product Guide: `maestro/product.md`
- Tech Stack: `maestro/tech-stack.md`
- LePhase: `/run/media/scooter/W.D SSD/code_index_update/LeIndexer/crates/lephase/`
- Python Implementation: `nexus/` directory
- Workflow: `maestro/workflow.md`

---

**Version:** 1.0
**Created:** 2025-02-16
