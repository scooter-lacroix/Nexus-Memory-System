# Always-On Memory Agent Integration - Blocking Task List

**Status**: Ready for Implementation
**Total Tasks**: 67 across 7 tracks
**Last Updated**: 2026-03-24

---

## MCP Usage Directives

### Required MCP Servers

| Server | Purpose | Required Tools |
|--------|---------|----------------|
| **LeIndex** | Code exploration, search, and editing | `leindex_read_file`, `leindex_edit_apply`, `leindex_project_map`, `leindex_text_search`, `leindex_grep_symbols` |
| **Sequential Thinking** | Task planning and complex problem decomposition | `sequentialthinking` |

### LeIndex Tool Usage Rules

**MANDATORY**: Use LeIndex tools instead of standard file operations:

| Instead Of | Use LeIndex |
|------------|-------------|
| `read_file` | `leindex_read_file` |
| `search_file_content` / `grep` | `leindex_text_search` |
| `glob` / `list_directory` | `leindex_project_map` |
| `replace` / `write_file` | `leindex_edit_apply` |
| Finding symbols | `leindex_grep_symbols` |

**Example Commands**:
```bash
# Read file with context
leindex tools run leindex_read_file -p /mnt/WD-SSD/Moved_Docs/nexus-memory-system --args '{"file_path": "/path/to/file.rs"}'

# Edit file
leindex tools run leindex_edit_apply -p /mnt/WD-SSD/Moved_Docs/nexus-memory-system --args '{"file_path": "/path/to/file.rs", "old_text": "...", "new_text": "..."}'

# Search text
leindex tools run leindex_text_search -p /mnt/WD-SSD/Moved_Docs/nexus-memory-system --args '{"pattern": "search_term"}'

# Get project structure
leindex tools run leindex_project_map -p /mnt/WD-SSD/Moved_Docs/nexus-memory-system --args '{"scope": "/path/to/crate"}'
```

### Sequential Thinking Usage

**MANDATORY**: Use `sequentialthinking` for:
- Pre-implementation planning before each track
- Complex refactoring decisions
- Debugging compilation errors
- Integration planning between components

**Usage Pattern**:
1. Call `sequentialthinking` to plan approach for the track
2. Execute planned edits using LeIndex tools
3. Verify results with `cargo check` / `cargo build`
4. Update task status in this document

---

## Dependency Graph

```
Track 1: Core Config & Contracts (NO DEPENDENCIES - START HERE)
    ├── Track 2: nexus-llm Crate
    └── Track 3: Storage Extensions
            └── Track 4: nexus-agent Crate
                    └── Track 5: Serve/Web Integration
                            └── Track 6: Tests & Verification
```

---

## Track 1: Core Config & Contracts (6 tasks)
**Crate**: nexus-core  
**Status**: ✅ COMPLETE  
**Blocking**: None

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T1-1 | Add LlmConfig struct to nexus-core/src/config.rs | ✅ Complete | High |
| T1-2 | Add AgentConfig struct to nexus-core/src/config.rs | ✅ Complete | High |
| T1-3 | Add llm and agent fields to main Config struct | ✅ Complete | High |
| T1-4 | Add env var loading for LLM/Agent config in Config::from_env() | ✅ Complete | High |
| T1-5 | Add Llm and Agent error variants to nexus-core/src/error.rs | ✅ Complete | High |
| T1-6 | Verify cargo check -p nexus-memory-core | ✅ Complete | High |

**Key Deliverables**:
- `LlmConfig` with provider, model, api_key_env, base_url, timeout_secs, max_tokens, temperature
- `AgentConfig` with enabled, namespace, inbox_dir, scan_interval_secs, consolidation_interval_mins, consolidation_batch_size, query_context_limit
- `NexusError::Llm(String)` and `NexusError::Agent(String)` variants

---

## Track 2: nexus-llm Crate (11 tasks)
**Crate**: crates/nexus-llm (NEW)  
**Status**: ✅ COMPLETE  
**Depends On**: Track 1  
**Blocking**: None

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T2-1 | Create crates/nexus-llm/Cargo.toml | ✅ Complete | High |
| T2-2 | Create src/types.rs with ChatMessage, GenerateParams, GenerateResponse, TokenUsage | ✅ Complete | High |
| T2-3 | Create src/error.rs with LlmError enum | ✅ Complete | High |
| T2-4 | Create src/provider.rs with Provider enum for 8 providers | ✅ Complete | High |
| T2-5 | Create src/client.rs with LlmClient trait | ✅ Complete | High |
| T2-6 | Create src/openai.rs with OpenAiCompatibleClient | ✅ Complete | High |
| T2-7 | Create src/anthropic.rs with AnthropicCompatibleClient | ✅ Complete | High |
| T2-8 | Create src/factory.rs with create_client() | ✅ Complete | High |
| T2-9 | Create src/lib.rs with module exports | ✅ Complete | High |
| T2-10 | Register nexus-llm in workspace Cargo.toml | ✅ Complete | High |
| T2-11 | Verify cargo check -p nexus-memory-llm | ✅ Complete | High |

**Key Deliverables**:
- Multi-provider LLM abstraction supporting: OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, Mistral
- Two protocol implementations: OpenAI-compatible (6 providers) and Anthropic-compatible (2 providers)
- Async trait-based client with JSON generation support

---

## Track 3: Storage Extensions (9 tasks)
**Crate**: nexus-storage  
**Status**: ✅ COMPLETE  
**Depends On**: Track 1  
**Blocking**: None

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T3-1 | Add ProcessedFileRow to nexus-storage/src/models.rs | ✅ Complete | High |
| T3-2 | Add create_processed_files_table() to migrations.rs | ✅ Complete | High |
| T3-3 | Add ProcessedFileRepository to repository.rs | ✅ Complete | High |
| T3-4 | Add MemoryRelationRepository to repository.rs | ✅ Complete | High |
| T3-5 | Add get_unconsolidated() to MemoryRepository | ✅ Complete | High |
| T3-6 | Add mark_consolidated() to MemoryRepository | ✅ Complete | High |
| T3-7 | Add search_by_text() to MemoryRepository | ✅ Complete | High |
| T3-8 | Update nexus-storage/src/lib.rs exports | ✅ Complete | High |
| T3-9 | Verify cargo check -p nexus-memory-storage | ✅ Complete | High |

**Key Deliverables**:
- `processed_files` table for inbox file deduplication
- `ProcessedFileRepository` with is_processed, mark_processed, mark_failed, clear_namespace
- `MemoryRelationRepository` with store, get_related
- Helper queries for unconsolidated memories and text search

---

## Track 4: nexus-agent Crate (11 tasks)
**Crate**: crates/nexus-agent (NEW)
**Status**: ✅ COMPLETE
**Depends On**: Tracks 1, 2, 3
**Blocking**: Track 5

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T4-1 | Create crates/nexus-agent/Cargo.toml | ✅ Complete | High |
| T4-2 | Create src/types.rs with IngestExtraction, ConsolidationResult, QueryAnswer, AgentStatus | ✅ Complete | High |
| T4-3 | Create src/prompts.rs with LLM prompt templates | ✅ Complete | High |
| T4-4 | Create src/ingest.rs with IngestService | ✅ Complete | High |
| T4-5 | Create src/consolidate.rs with ConsolidateService | ✅ Complete | High |
| T4-6 | Create src/query.rs with QueryService | ✅ Complete | High |
| T4-7 | Create src/inbox.rs with InboxScanner | ✅ Complete | High |
| T4-8 | Create src/supervisor.rs with AgentSupervisor | ✅ Complete | High |
| T4-9 | Create src/lib.rs with module exports | ✅ Complete | High |
| T4-10 | Register nexus-agent in workspace Cargo.toml | ✅ Complete | High |
| T4-11 | Verify cargo check -p nexus-memory-agent | ✅ Complete | High |

**Key Deliverables**:
- **IngestService**: Raw text → LLM extraction → enriched memory storage
- **ConsolidateService**: Periodic pattern finding across unconsolidated memories
- **QueryService**: LLM-synthesized answers with memory citations
- **InboxScanner**: File watcher for automatic ingestion (polling-based)
- **AgentSupervisor**: Manages background loops for inbox scanning and consolidation

---

## Track 5: Serve/Web Integration (8 tasks)
**Crates**: nexus-web, nexus-cli
**Status**: ✅ COMPLETE
**Depends On**: Tracks 1-4
**Blocking**: Track 6

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T5-1 | Add agent models to nexus-web/src/models.rs | ✅ Complete | High |
| T5-2 | Create nexus-web/src/api/agent.rs with 4 endpoint handlers | ✅ Complete | High |
| T5-3 | Add agent routes to nexus-web/src/lib.rs router | ✅ Complete | High |
| T5-4 | Add agent_supervisor field to nexus-web/src/state.rs AppState | ✅ Complete | High |
| T5-5 | Add --agent flag to nexus-cli/src/main.rs Serve command | ✅ Complete | High |
| T5-6 | Add nexus-agent and nexus-llm deps to nexus-web/Cargo.toml | ✅ Complete | High |
| T5-7 | Add nexus-agent dep to nexus-cli/Cargo.toml | ✅ Complete | High |
| T5-8 | Verify cargo check -p nexus-memory-web | ✅ Complete | High |

**Key Deliverables**:
- 4 new API endpoints:
  - `POST /api/agent/ingest` - Ingest text with LLM enrichment
  - `POST /api/agent/query` - Query memory with LLM synthesis
  - `POST /api/agent/consolidate` - Trigger manual consolidation
  - `GET /api/agent/status` - Get agent health/stats
- `--agent` CLI flag for `nexus serve`
- AppState integration with AgentSupervisor

---

## Track 6: Documentation & Verification (6 tasks)
**Status**: ✅ COMPLETE
**Depends On**: All implementation tracks

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T6-1 | Update .env.example with agent configuration variables | ✅ Complete | Medium |
| T6-2 | Update ARCHITECTURE.md with agent section | ✅ Complete | Medium |
| T6-3 | Update README.md with agent quick start | ✅ Complete | Medium |
| T6-4 | Update CHANGELOG.md with new features | ✅ Complete | Medium |
| T6-5 | Final verification: cargo build --workspace | ✅ Complete | High |
| T6-6 | Final verification: cargo test --workspace | ✅ Complete | High |

### Additional Work Completed (Tracks 1-6)

These items were discovered and resolved during Track 6 verification. They are not tracked as individual tasks but represent real work that must be accounted for:

**Doctest/Benchmark Crate Name Fixes (v1.1.2 rename incomplete)**:
- `nexus-mcp/tests/integration_test.rs` — `nexus_mcp` → `nexus_memory_mcp` (4 references)
- `nexus-embeddings/src/lib.rs` doctest — `nexus_embeddings` → `nexus_memory_embeddings`
- `nexus-embeddings/benches/embedding.rs` — `nexus_embeddings` → `nexus_memory_embeddings`
- `nexus-hooks/src/lib.rs` doctest — `nexus_hooks` → `nexus_memory_hooks`
- `nexus-hooks/src/factory.rs` doctest — `nexus_hooks` → `nexus_memory_hooks`
- `nexus-hooks/src/buffer.rs` doctest — `nexus_hooks` → `nexus_memory_hooks`
- `nexus-hooks/src/agents/pi_mono.rs` doctest — `nexus_hooks` → `nexus_memory_hooks`
- `nexus-hooks/src/extractor.rs` doctest — `nexus_hooks` → `nexus_memory_hooks`
- `nexus-hooks/src/base.rs` doctest — `nexus_hooks` → `nexus_memory_hooks`
- `nexus-vectors/src/lib.rs` doctest — `nexus_vectors` → `nexus_memory_vectors`
- `nexus-web/src/lib.rs` doctest — `nexus_web` → `nexus_memory_web`
- `nexus-mcp/src/lib.rs` doctest — `nexus_mcp` → `nexus_memory_mcp`
- `nexus-orchestrator/src/lib.rs` doctest — `nexus_orchestrator` → `nexus_memory_orchestrator`

**Orchestrator Benchmark Rewrite**:
- `crates/nexus-orchestrator/benches/orchestrator_bench.rs` — Rewrote entirely to match current API (removed stale `SessionConfig`, `initialize()`, `record_activity()` calls; added `tokio::runtime::block_on` for async methods)

**Clippy Warning Resolution (47 warnings → 0)**:
- `nexus-core` (6): derive Default on Config/MemoryCategory, rename `from_str` → `parse` on 4 types
- `nexus-storage` (3): extract `StoreMemoryParams` struct, simplify closures, use `is_none()`
- `nexus-vectors` (8): remove needless borrows, inline defaults, convert loop to `while let`, simplify closures
- `nexus-mcp` (8): derive Default, simplify closures, remove unnecessary closures
- `nexus-orchestrator` (2): derive Default on EventPriority/SyncPolicy
- `nexus-lephase` (6): derive Default, rename `from_str` → `parse`, inline defaults
- `nexus-llm` (1): rename `from_str` → `parse`
- `nexus-agent` (1): simplify redundant closure
- `nexus-hooks` (3): remove needless borrows, `#[allow]` on large enum variant, rename `from_str` → `parse`
- `nexus-embeddings` (1): replace `vec!` with array literals, fix `FRAC_1_SQRT_2` type

**Final Verification State**:
- `cargo fmt --all --check`: PASS
- `cargo clippy --workspace --all-targets`: 0 errors, 0 warnings
- `cargo test --workspace`: 0 failures (all tests + doctests + integration tests pass)

---

## Implementation Notes

### Environment Variables

```bash
# LLM Configuration
NEXUS_LLM_PROVIDER=openai
NEXUS_LLM_MODEL=gpt-4o-mini
NEXUS_LLM_API_KEY_ENV=OPENAI_API_KEY
NEXUS_LLM_BASE_URL=

# Agent Configuration
NEXUS_AGENT_ENABLED=false
NEXUS_AGENT_NAMESPACE=nexus-agent
NEXUS_AGENT_INBOX_DIR=./inbox
NEXUS_AGENT_CONSOLIDATION_INTERVAL=30

# Provider API Keys
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
GEMINI_API_KEY=AI...
OPENROUTER_API_KEY=sk-or-...
GROQ_API_KEY=gsk_...
ZAI_API_KEY=...
MINIMAX_API_KEY=...
MISTRAL_API_KEY=...
```

### Supported LLM Providers

| Provider | Protocol | Base URL | Auth Header |
|----------|----------|----------|-------------|
| OpenAI | OpenAI | https://api.openai.com/v1 | Authorization: Bearer |
| OpenRouter | OpenAI | https://openrouter.ai/api/v1 | Authorization: Bearer |
| Groq | OpenAI | https://api.groq.com/openai/v1 | Authorization: Bearer |
| Mistral | OpenAI | https://api.mistral.ai/v1 | Authorization: Bearer |
| Minimax | OpenAI | https://api.minimax.io/v1 | Authorization: Bearer |
| Gemini | OpenAI | https://generativelanguage.googleapis.com/v1beta/openai/ | Authorization: Bearer |
| Anthropic | Anthropic | https://api.anthropic.com | x-api-key + anthropic-version |
| Z.ai | Anthropic | https://api.z.ai/api/anthropic | x-api-key + anthropic-version |

### Database Schema Changes

**New Table: processed_files**
```sql
CREATE TABLE processed_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    content_hash TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    memory_id INTEGER,
    last_error TEXT,
    processed_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME,
    FOREIGN KEY (namespace_id) REFERENCES agent_namespaces(id),
    FOREIGN KEY (memory_id) REFERENCES memories(id),
    UNIQUE(namespace_id, path)
);
```

---

## Track 7: Claude Hook Remediation (13 tasks)
**Crates**: nexus-cli, nexus-hooks, nexus-core
**Status**: ✅ COMPLETE
**Depends On**: Tracks 1, 2 (LLM client + config)
**Source Plan**: `docs/plans/claude-hook-memory-remediation-plan.md`

| ID | Task | Status | Priority |
|----|------|--------|----------|
| T7-1 | Add `ingest-hook-event` CLI command to nexus-cli | ✅ Complete | High |
| T7-2 | Create `crates/nexus-cli/src/commands/ingest_hook_event.rs` | ✅ Complete | High |
| T7-3 | Add `NormalizedHookEvent` + Claude payload normalization in nexus-hooks | ✅ Complete | High |
| T7-4 | Add message flattening helpers for `message.content` arrays | ✅ Complete | High |
| T7-5 | Add high-signal candidate derivation with duplicate suppression | ✅ Complete | High |
| T7-6 | Add LLM enrichment client + strict JSON enrichment prompt/parser | ✅ Complete | High |
| T7-7 | Add persistence adapter with rich metadata (source, evidence, llm_comment) | ✅ Complete | High |
| T7-8 | Extend CLI `store` command to accept metadata and memory_lane_type | ✅ Complete | Medium |
| T7-9 | Add retry buffering for failed enrichment (~/.local/state/nexus-memory-system/pending-enrichment/) | ✅ Complete | Medium |
| T7-10 | Add normalization, candidate, and enrichment unit tests | ✅ Complete | Medium |
| T7-11 | Add end-to-end ingest tests with Claude payload fixtures | ✅ Complete | Medium |
| T7-12 | Replace external JS hook with thin passthrough shim | ✅ Complete | High |
| T7-13 | Validate live Claude ingestion and inspect stored rows | ✅ Complete | High |

**Key Deliverables**:
- `nexus ingest-hook-event` CLI command for structured hook ingestion
- `NormalizedHookEvent` model with Claude-specific normalization
- High-signal candidate derivation with duplicate suppression
- LLM enrichment with required comment generation on every stored memory
- Rich metadata schema: `source`, `evidence`, `ingestion`, `llm_comment`
- Retry buffering for enrichment failures (fail-closed)
- External JS hook converted to thin transport shim

**Minimum Viable Acceptance Criteria**:
1. Real Claude `PostToolUse` payload populates `tool_name`, `tool_input`, message/response text
2. No more timestamp-only Claude hook pings stored as primary memories
3. Every stored memory assigned exactly one category (general/facts/preferences/context/specifications/session)
4. Every stored memory includes `metadata.llm_comment.text`
5. Metadata contains enough evidence to explain why the memory exists
6. Low-signal duplicate events are skipped or buffered, not promoted into junk memories
7. External Claude JS hook becomes a transport shim, not the home of memory intelligence

---

## Progress Tracker

- [x] Track 1: Core Config & Contracts (6/6) ✅
- [x] Track 2: nexus-llm Crate (11/11) ✅
- [x] Track 3: Storage Extensions (9/9) ✅
- [x] Track 4: nexus-agent Crate (11/11) ✅
- [x] Track 5: Serve/Web Integration (8/8) ✅
- [x] Track 6: Documentation & Verification (6/6) ✅
- [x] Track 7: Claude Hook Remediation (13/13) ✅

**Overall Progress**: 67/67 tasks (100%) ✅
**Last Updated**: 2026-03-24

### Track 7 Implementation Details

**New files created:**
- `crates/nexus-hooks/src/claude_payload.rs` — NormalizedHookEvent, normalize_claude_payload(), flatten_message_content(), get_string()
- `crates/nexus-hooks/src/candidate.rs` — MemoryCandidate, derive_candidates(), signal scoring, duplicate suppression via SHA-256 fingerprinting
- `crates/nexus-hooks/src/enrichment.rs` — EnrichmentService, EnrichedMemory, EnrichmentBatchResult, strict JSON enrichment prompt
- `crates/nexus-hooks/src/retry_buffer.rs` — RetryBuffer, RetryArtifact, file-based buffering to ~/.local/state/nexus-memory-system/pending-enrichment/
- `crates/nexus-hooks/src/persistence.rs` — persist_enriched_memories(), PersistResult, rich metadata (source/evidence/ingestion/llm_comment)
- `crates/nexus-cli/src/commands/ingest_hook_event.rs` — Full ingestion pipeline: stdin → normalize → candidates → LLM enrich → persist

**Modified files:**
- `crates/nexus-hooks/src/lib.rs` — Added 5 new module declarations + re-exports
- `crates/nexus-hooks/Cargo.toml` — Added nexus-llm, nexus-storage, sha2, hex, sqlx (dev-dep)
- `crates/nexus-cli/src/main.rs` — Added IngestHookEvent command + Store extended with metadata_json/memory_lane_type
- `crates/nexus-cli/src/commands/mod.rs` — Added ingest_hook_event module
- `crates/nexus-cli/src/commands/store.rs` — Extended execute() with metadata_json + memory_lane_type params

**Verification state:**
- `cargo clippy --workspace --all-targets`: 0 errors, 0 warnings
- `cargo test --workspace`: 0 failures (289 tests + doctests all pass)
