# Plan: Rust Core Foundation - Vector Database with Graph Tree and LePhase Integration

**Track ID:** rust-core-foundation_20250216
**Status:** In Progress

---

## Phase 1: Project Scaffolding and Workspace Setup [checkpoint: phase1-done]

### Task 1.1: Initialize Cargo Workspace
- [x] Sub-task: Create `Cargo.toml` workspace configuration
- [x] Sub-task: Create crate directories (`nexus-core`, `nexus-storage`, `nexus-vectors`, `nexus-lephase`, `nexus-mcp`, `nexus-cli`)
- [x] Sub-task: Configure workspace dependencies and shared features
- [~] Sub-task: Set up `.cargo/config.toml` for workspace optimization
- [x] Sub-task: Create basic `lib.rs` and `main.rs` files for each crate

### Task 1.2: Development Tooling Configuration
- [~] Sub-task: Configure rustfmt with `rustfmt.toml`
- [~] Sub-task: Configure clippy with `clippy.toml`
- [ ] Sub-task: Set up `just` or `Makefile` for common commands
- [ ] Sub-task: Configure CI/CD workflow files (`.github/workflows/`)
- [ ] Sub-task: Set up pre-commit hooks (rustfmt, clippy, tests)

### Task 1.3: Core Type Definitions
- [x] Sub-task: Define memory types in `nexus-core/src/types.rs`
- [x] Sub-task: Define error types in `nexus-core/src/error.rs`
- [x] Sub-task: Define traits in `nexus-core/src/traits.rs`
- [~] Sub-task: Add tests for core type conversions
- [~] Sub-task: Add tests for error handling

### Task 1.4: Task: Maestro - Phase Verification 'Project Scaffolding and Workspace Setup'
- [x] Sub-task: Verify workspace builds with `cargo build --workspace`
- [x] Sub-task: Verify all tests pass with `cargo test --workspace`
- [ ] Sub-task: Verify code coverage meets 95%+ requirement
- [ ] Sub-task: Run `codex-reviewer` for Tzar of Excellence review

---

## Phase 2: Core Storage Manager (SQLx)

### Task 2.1: Database Schema Design
- [ ] Sub-task: Design schema compatible with Python implementation
- [ ] Sub-task: Create migration scripts in `nexus-storage/migrations/`
- [ ] Sub-task: Define SQLite schema for tables: `memories`, `agent_namespaces`, `task_specifications`, `memory_relations`
- [ ] Sub-task: Add tests for migration scripts
- [ ] Sub-task: Add tests for schema compatibility

### Task 2.2: Database Connection Pool
- [ ] Sub-task: Implement connection pool in `nexus-storage/src/pool.rs`
- [ ] Sub-task: Add configuration for pool settings
- [ ] Sub-task: Implement graceful shutdown handling
- [ ] Sub-task: Add tests for connection pool lifecycle
- [ ] Sub-task: Add tests for pool exhaustion handling

### Task 2.3: Memory Repository
- [ ] Sub-task: Implement `MemoryRepository` trait in `nexus-storage/src/memory.rs`
- [ ] Sub-task: Implement CRUD operations for memories
- [ ] Sub-task: Implement query operations with filters
- [ ] Sub-task: Add tests for CRUD operations
- [ ] Sub-task: Add tests for query operations
- [ ] Sub-task: Add tests for hybrid memory type system

### Task 2.4: Namespace and Specification Repositories
- [ ] Sub-task: Implement `AgentNamespaceRepository` in `nexus-storage/src/namespace.rs`
- [ ] Sub-task: Implement `TaskSpecificationRepository` in `nexus-storage/src/specification.rs`
- [ ] Sub-task: Add tests for namespace operations
- [ ] Sub-task: Add tests for specification operations

### Task 2.5: Task: Maestro - Phase Verification 'Core Storage Manager (SQLx)'
- [ ] Sub-task: Verify all storage operations work with SQLite
- [ ] Sub-task: Verify all tests pass with 95%+ coverage
- [ ] Sub-task: Run `codex-reviewer` for Tzar of Excellence review

---

## Phase 3: Vector Database with Graph Tree Structure

### Task 3.1: sqlite-vec Integration
- [ ] Sub-task: Add sqlite-vec dependency to `nexus-vectors/Cargo.toml`
- [ ] Sub-task: Create vector table schema in migrations
- [ ] Sub-task: Implement vector storage operations
- [ ] Sub-task: Add tests for vector storage
- [ ] Sub-task: Add tests for 384-dimensional embedding compatibility

### Task 3.2: Graph Tree Structure Implementation
- [ ] Sub-task: Design graph tree data structure for hierarchical memory organization
- [ ] Sub-task: Implement tree node management in `nexus-vectors/src/tree.rs`
- [ ] Sub-task: Implement tree traversal algorithms
- [ ] Sub-task: Implement resource management strategies
- [ ] Sub-task: Add tests for tree structure
- [ ] Sub-task: Add tests for traversal algorithms
- [ ] Sub-task: Add tests for resource management

### Task 3.3: Semantic Search Implementation
- [ ] Sub-task: Implement vector similarity search using cosine similarity
- [ ] Sub-task: Implement graph-aware search (tree-boosted relevance)
- [ ] Sub-task: Optimize search for <10ms latency target
- [ ] Sub-task: Add benchmarks for search performance
- [ ] Sub-task: Add tests for search accuracy
- [ ] Sub-task: Add tests for search performance (<10ms requirement)

### Task 3.4: Task: Maestro - Phase Verification 'Vector Database with Graph Tree Structure'
- [ ] Sub-task: Verify vector search latency <10ms
- [ ] Sub-task: Verify graph tree structure improves resource management
- [ ] Sub-task: Verify all tests pass with 95%+ coverage
- [ ] Sub-task: Run `codex-reviewer` for Tzar of Excellence review

---

## Phase 4: LePhase Integration

### Task 4.1: LePhase Library Bindings
- [ ] Sub-task: Add LePhase as local dependency in `nexus-lephase/Cargo.toml`
- [ ] Sub-task: Create wrapper module for LePhase API
- [ ] Sub-task: Implement FFI bindings if needed
- [ ] Sub-task: Add tests for LePhase bindings
- [ ] Sub-task: Add tests for compression/decompression

### Task 4.2: Memory Compression Pipeline
- [ ] Sub-task: Implement compression during memory storage
- [ ] Sub-task: Implement decompression during memory retrieval
- [ ] Sub-task: Implement token-efficient presentation formatting
- [ ] Sub-task: Add tests for compression pipeline
- [ ] Sub-task: Add tests for compression ratio (>50% target)
- [ ] Sub-task: Add tests for decompression correctness

### Task 4.3: Integration with Storage Manager
- [ ] Sub-task: Integrate LePhase compression into memory storage operations
- [ ] Sub-task: Integrate LePhase decompression into memory retrieval operations
- [ ] Sub-task: Add configuration for compression settings
- [ ] Sub-task: Add tests for end-to-end storage with compression
- [ ] Sub-task: Add tests for end-to-end retrieval with decompression

### Task 4.4: Task: Maestro - Phase Verification 'LePhase Integration'
- [ ] Sub-task: Verify compression achieves >50% ratio
- [ ] Sub-task: Verify compressed memories can be retrieved correctly
- [ ] Sub-task: Verify all tests pass with 95%+ coverage
- [ ] Sub-task: Run `codex-reviewer` for Tzar of Excellence review

---

## Phase 5: Integration and Testing

### Task 5.1: Cross-Crate Integration
- [ ] Sub-task: Integrate storage manager with vector database
- [ ] Sub-task: Integrate LePhase with storage operations
- [ ] Sub-task: Implement end-to-end memory pipeline
- [ ] Sub-task: Add tests for full pipeline integration
- [ ] Sub-task: Add tests for Python compatibility

### Task 5.2: Migration Scripts from Python
- [ ] Sub-task: Create database migration tool from Python SQLite
- [ ] Sub-task: Implement embedding format conversion
- [ ] Sub-task: Add tests for migration accuracy
- [ ] Sub-task: Add tests for migration completeness

### Task 5.3: Performance Benchmarking
- [ ] Sub-task: Create benchmark suite using `criterion`
- [ ] Sub-task: Benchmark vector search latency
- [ ] Sub-task: Benchmark memory storage throughput
- [ ] Sub-task: Benchmark LePhase compression performance
- [ ] Sub-task: Compare against Python baseline
- [ ] Sub-task: Add tests for all performance targets

### Task 5.4: Documentation
- [ ] Sub-task: Write crate-level documentation
- [ ] Sub-task: Write API documentation with examples
- [ ] Sub-task: Create migration guide from Python
- [ ] Sub-task: Create architecture diagrams
- [ ] Sub-task: Verify documentation builds with `cargo doc --open`

### Task 5.5: Task: Maestro - Phase Verification and Checkpoint 'Integration and Testing'
- [ ] Sub-task: Verify all acceptance criteria met
- [ ] Sub-task: Verify all tests pass with 95%+ coverage
- [ ] Sub-task: Verify all performance targets met (<10ms search, 10k+ concurrent)
- [ ] Sub-task: Run `codex-reviewer` for Tzar of Excellence review
- [ ] Sub-task: Create final checkpoint commit

---

## Phase 6: Final Review and Handoff

### Task 6.1: Final Quality Checks
- [ ] Sub-task: Verify all clippy lints pass
- [ ] Sub-task: Verify rustfmt compliance
- [ ] Sub-task: Verify 95%+ code coverage
- [ ] Sub-task: Verify all unsafe blocks documented
- [ ] Sub-task: Verify no TODO/FIXME left in code

### Task 6.2: Documentation Completion
- [ ] Sub-task: Verify all public APIs have rustdoc
- [ ] Sub-task: Verify README for each crate
- [ ] Sub-task: Verify examples work correctly
- [ ] Sub-task: Verify migration guide is complete

### Task 6.3: Task: Maestro - Final Phase Verification and User Approval
- [ ] Sub-task: Complete all quality checks
- [ ] Sub-task: Present track completion summary to user
- [ ] Sub-task: Await user final approval
- [ ] Sub-task: Create final checkpoint commit after approval
- [ ] Sub-task: Mark track as complete in `tracks.md`

---

## Task Summary

| Phase | Tasks | Focus |
|-------|-------|-------|
| 1 | Project Scaffolding | Workspace setup, tooling, core types |
| 2 | Storage Manager | SQLx, SQLite, repositories |
| 3 | Vector Database | sqlite-vec, graph tree, semantic search |
| 4 | LePhase Integration | Compression, token efficiency |
| 5 | Integration Testing | Cross-crate, migration, benchmarks |
| 6 | Final Review | Quality checks, documentation, handoff |

**Total Tasks:** 6 phases, 23 main tasks, 100+ sub-tasks

---

## Notes

- All implementation MUST follow TDD: Write tests first, then implement
- All code MUST achieve 95%+ test coverage
- All phases (except final) run in autonomous mode
- Final phase requires explicit user approval
- Use `codex-reviewer` via `/codex-reviewer` skill for Tzar reviews

---

**Created:** 2025-02-16
**Track:** rust-core-foundation_20250216
