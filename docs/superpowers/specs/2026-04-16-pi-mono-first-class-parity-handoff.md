# Pi-Mono First-Class Parity — Handoff Document

> **Date:** 2026-04-16 | **Author:** Amp Thread T-019d9896-5855-72db-b079-b1d3529cab0c  
> **Companion Docs:**  
> - [Spec Bible](./2026-04-16-pi-mono-first-class-parity-spec-bible.md)  
> - [Blocking Task List](../plans/2026-04-16-pi-mono-first-class-parity-blocking-task-list.md)

---

## 1. Mission Statement

Pi-mono (`@mariozechner/pi-coding-agent`) is a TypeScript coding agent with a rich extension system that pi-mono **actually reads**. The current Nexus integration installs a `SKILL.md` file — an artifact pi-mono **completely ignores** for lifecycle events. This work replaces the broken SKILL.md integration with a proper TypeScript extension that hooks into pi's real lifecycle events, achieving full behavioral parity with Claude Code across all 5 lifecycle capabilities (session_start, session_end, checkpoint, error_hook, compact), injection system coverage, morning recall, dream cycles, and memory enrichment.

---

## 2. Current State Analysis

### 2.1 Pi-Mono Hook (Current — What's Broken)

**File:** `crates/nexus-hooks/src/agents/pi_mono.rs` (556 lines)

**What it does now:**
- Installs a `SKILL.md` file at `~/.pi/agent/skills/nexus-memory-extraction/SKILL.md`
- Reads `.pi/sessions/*.json` for session data (pull-based, file scanning)
- Reads `.pi/logs/*.log` for command execution history
- Detects `pi`, `pi-coding-agent`, `pi-mono` processes
- Reports `SupportTier::NativeLifecycle` (dishonest — it's not truly native)

**What's wrong:**
1. **Pi-mono uses TypeScript extensions, not SKILL.md files.** The installed SKILL.md is never read by pi's runtime. It sits on disk doing nothing.
2. **`session_start: false`** — Pi-mono gets no morning recall, no context injection, no `.nexus/context.md`
3. **`error_hook: false`** — Errors during sessions are never captured
4. **Pull-based file scanning** — Reads `.pi/sessions/` on demand instead of receiving push-based lifecycle events. This misses real-time events and depends on pi-mono's internal file format.
5. **No injection target** — Pi-mono is missing from `injection.rs`, so `on_session_start()` can't inject references into pi's config files
6. **`parse_skill_metadata()` method** — Parses YAML frontmatter from SKILL.md. This entire method is vestigial since pi-mono doesn't consume skill metadata.

**Key struct (current):**
```rust
pub struct PiMonoHook {
    base: BaseHook,
    config_dir: PathBuf,      // ~/.pi
    session_dir: PathBuf,     // ~/.pi/sessions
    skills_dir: PathBuf,      // ~/.pi/agent/skills  ← WRONG
    process_monitor: ProcessMonitor,
    skill_installed: bool,    // ← WRONG terminology
}
```

**Current lifecycle capabilities:**
```rust
LifecycleCapabilities {
    session_start: false,  // ← GAP
    session_end: true,
    checkpoint: true,
    error_hook: false,     // ← GAP
    compact: true,
}
```

### 2.2 Claude Code Hook (Reference — What Parity Looks Like)

**File:** `crates/nexus-hooks/src/agents/claude.rs` (802 lines)

**What it does:**
1. **Installs SKILL.md** at `~/.claude/skills/nexus-memory-extraction/SKILL.md` (correct for Claude — Claude reads skills)
2. **Installs settings.json hook** — Writes a `SessionStart` hook entry into `~/.claude/settings.json` that invokes `nexus session start --agent claude-code --mode session`
3. **Constructor triggers injection** — `new_with_install(true)` spawns `injection::on_session_start()` via `tokio::spawn`, giving Claude morning recall, context.md, and config injection on every construction
4. **All lifecycle capabilities = true**

**Key patterns to replicate:**
- Constructor triggers `on_session_start()` asynchronously
- `install_session_start_hook()` writes agent-native config (settings.json for Claude → extension file for pi-mono)
- `find_nexus_binary()` resolves the nexus binary path
- `is_hook_installed()` checks both skill and settings hook
- Reliability score = 1.0 when both artifacts installed

### 2.3 Injection System (Context Injection)

**File:** `crates/nexus-hooks/src/injection.rs` (339 lines)

**`known_agents()` — who gets injection (line 22-46):**
| Agent | Global Config | Project Config |
|-------|--------------|----------------|
| `claude-code` | `~/.claude/CLAUDE.md` | `CLAUDE.md` |
| `amp` | `~/.config/amp/AGENTS.md` | `AGENTS.md` |
| `codex` | `~/.config/codex/AGENTS.md` | `AGENTS.md` |
| `gemini` | `~/.gemini/GEMINI.md` | `GEMINI.md` |
| **pi-mono** | **NOT LISTED** | **NOT LISTED** |

**`on_session_start()` — 9-step pipeline (line 153-271):**
1. `ProjectIdentity::resolve(cwd)` — Find project root, name, git info
2. Create `.nexus/`, `.nexus/cache/`, `.nexus/sessions/` directories
3. `StorageManager::from_url()` — Connect to SQLite
4. `NamespaceRepository::get_or_create()` — Agent namespace
5. `CognitiveCache::morning_recall()` — Load recent relevant memories
6. `build_context_md()` — Generate `.nexus/context.md` from hot cache + recalls
7. `inject_reference()` — Write `<!-- NEXUS:START -->` blocks into agent config files
8. `SessionManager::start_session()` — Create session scratch file
9. `.gitignore` hardening — Ensure `.nexus/` is always ignored

**Problem:** Pi-mono is not in `known_agents()`, so step 7 does nothing for pi-mono. More critically, `on_session_start()` is **only called from Claude's constructor** — the `nexus session start` CLI command does NOT run this pipeline.

### 2.4 Pi-Mono Extension System (TypeScript Side)

**Repository:** `/mnt/WD-SSD/Prod/work_resources/pi-mono/` (synced)

**Extension system overview:**
- **Type:** TypeScript modules, loaded via `jiti` (no compilation needed)
- **Entry point:** `export default function(pi: ExtensionAPI): void`
- **Global location:** `~/.pi/agent/extensions/*.ts`
- **Project-local:** `.pi/extensions/*.ts`
- **Auto-discovery:** Files and subdirectories with `index.ts` are auto-discovered
- **Hot reload:** `/reload` command reloads extensions in-place

**Key files in pi-mono:**
| File | Purpose |
|------|---------|
| `packages/coding-agent/src/core/extensions/types.ts` | All extension types (1461 lines) |
| `packages/coding-agent/src/core/extensions/loader.ts` | Extension loading/discovery (557 lines) |
| `packages/coding-agent/src/core/extensions/runner.ts` | Extension event dispatch (915+ lines) |
| `packages/coding-agent/docs/extensions.md` | Extension documentation (1886+ lines) |

**Available lifecycle events (complete list):**

| Event | When | Can Block | Can Modify |
|-------|------|-----------|------------|
| `session_start` | Session starts/loads/reloads | No | No |
| `session_shutdown` | Ctrl+C, Ctrl+D, SIGHUP, SIGTERM | No | No |
| `session_before_switch` | Before /new or /resume | Yes (cancel) | No |
| `session_before_fork` | Before /fork | Yes (cancel) | No |
| `session_before_compact` | Before /compact or auto-compact | Yes (cancel) | Yes (custom compaction) |
| `session_compact` | After compaction | No | No |
| `session_before_tree` | Before tree navigation | Yes (cancel) | Yes |
| `session_tree` | After tree navigation | No | No |
| `resources_discover` | After session_start | No | Yes (add paths) |
| `input` | User input received | Yes (handled) | Yes (transform) |
| `before_agent_start` | Before LLM call | No | Yes (inject message, modify system prompt) |
| `agent_start` | Agent loop starts | No | No |
| `agent_end` | Agent loop ends | No | No |
| `turn_start` | Turn begins | No | No |
| `turn_end` | Turn ends | No | No |
| `context` | Before LLM call | No | Yes (modify messages) |
| `before_provider_request` | Before API request | No | Yes (replace payload) |
| `after_provider_response` | After API response headers | No | No |
| `message_start` | Message begins | No | No |
| `message_update` | Token-by-token streaming | No | No |
| `message_end` | Message complete | No | No |
| `tool_call` | Before tool executes | Yes (block) | Yes (mutate input) |
| `tool_result` | After tool executes | No | Yes (modify result) |
| `tool_execution_start` | Tool execution begins | No | No |
| `tool_execution_update` | Tool streaming output | No | No |
| `tool_execution_end` | Tool execution ends | No | No |
| `model_select` | Model changed | No | No |
| `user_bash` | User runs ! or !! command | No | Yes (custom operations) |

**ExtensionAPI methods:**
```typescript
interface ExtensionAPI {
  // Event subscription
  on(event: string, handler: (event, ctx) => Promise<result>): void
  
  // Registration
  registerTool(tool: ToolDefinition): void
  registerCommand(name, options): void
  registerShortcut(shortcut, options): void
  registerFlag(name, options): void
  registerMessageRenderer(customType, renderer): void
  registerProvider(name, config): void
  
  // Actions
  sendMessage(message, options): void
  sendUserMessage(content, options): void
  appendEntry(customType, data): void
  setSessionName(name): void
  getSessionName(): string | undefined
  exec(command, args, options): Promise<ExecResult>
  getActiveTools(): string[]
  setActiveTools(names): void
  setModel(model): Promise<boolean>
  getThinkingLevel(): ThinkingLevel
  setThinkingLevel(level): void
  
  // Shared
  events: EventBus
}
```

**ExtensionContext (ctx):**
```typescript
interface ExtensionContext {
  ui: ExtensionUIContext          // notify, confirm, select, input, custom, widgets
  hasUI: boolean                  // false in print/RPC mode
  cwd: string                    // Current working directory
  sessionManager: ReadonlySessionManager  // Session state access
  modelRegistry: ModelRegistry    // API key resolution
  model: Model<any> | undefined  // Current model
  isIdle(): boolean
  signal: AbortSignal | undefined
  abort(): void
  hasPendingMessages(): boolean
  shutdown(): void
  getContextUsage(): ContextUsage | undefined
  compact(options?): void
  getSystemPrompt(): string
}
```

---

## 3. Target Architecture

### 3.1 Component Flow

```
┌─────────────────────────────────────────────────────────────────┐
│ Layer 4: Pi-Mono Runtime                                        │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ nexus-memory.ts (TypeScript Extension)                    │   │
│  │                                                            │   │
│  │  ┌─────────────────────────────────────────────────────┐  │   │
│  │  │ Event Handlers                                       │  │   │
│  │  │                                                       │  │   │
│  │  │  session_start  ─► nexus session start               │  │   │
│  │  │  session_shutdown ─► flush queue + nexus session end  │  │   │
│  │  │  session_compact ─► nexus session event --kind compact│  │   │
│  │  │  agent_end      ─► normalize + nexus ingest-hook-event│  │   │
│  │  │  tool_result    ─► normalize + nexus ingest-hook-event│  │   │
│  │  │  message_end    ─► debounce + nexus ingest-hook-event │  │   │
│  │  │  before_agent_start ─► inject .nexus/context.md       │  │   │
│  │  └─────────────────────────────────────────────────────┘  │   │
│  │                                                            │   │
│  │  ┌─────────────────────────────────────────────────────┐  │   │
│  │  │ Transport (CLI Spawn)                                │  │   │
│  │  │                                                       │  │   │
│  │  │  spawnNexus(args, stdinJson?)                        │  │   │
│  │  │  - Fire-and-forget, detached, 5s timeout             │  │   │
│  │  │  - Queue on failure (max 100), flush on next success │  │   │
│  │  └─────────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ CLI spawn (async)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ Layer 3: Nexus Binary (Rust CLI)                                │
│                                                                  │
│  nexus session start                                             │
│    → injection::on_session_start()  ← CENTRALIZED (new)         │
│      → Morning Recall → context.md → config injection            │
│                                                                  │
│  nexus ingest-hook-event                                         │
│    → normalize_generic_payload()    ← SHARED with Claude         │
│    → derive_candidates()            ← SHARED with Claude         │
│    → enrich_candidates()            ← SHARED with Claude         │
│    → persist_enriched_memories()    ← SHARED with Claude         │
│                                                                  │
│  nexus session end                                               │
│    → trigger_callbacks()            ← SHARED with Claude         │
│    → run_nap() (dream cycle)        ← SHARED with Claude         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ Layer 0-1: Storage (SQLite + ONNX)                              │
│                                                                  │
│  memories table → agent namespace "pi-mono"                      │
│  relations table → cross-memory links                            │
│  embeddings → ONNX vector similarity                             │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 File Inventory

**Files to CREATE:**

| File | Purpose | Size Estimate |
|------|---------|--------------|
| `crates/nexus-hooks/src/extension_ts/nexus_memory_pi.ts` | TypeScript extension source, embedded via `include_str!()` | ~300 lines |

**Files to MODIFY:**

| File | Changes | Lines Affected |
|------|---------|---------------|
| `crates/nexus-hooks/src/agents/pi_mono.rs` | Rewrite: extension-based install, full lifecycle, migration | ~400 lines (most of file) |
| `crates/nexus-hooks/src/injection.rs` | Add pi-mono entry to `known_agents()` | ~6 lines |
| `crates/nexus-hooks/src/lib.rs` | Update doc comments | ~5 lines |
| `crates/nexus-cli/src/commands/session.rs` | Add injection pipeline call to session start | ~10 lines |

**Files NOT modified (shared pipeline, already correct):**

| File | Why No Changes |
|------|---------------|
| `crates/nexus-hooks/src/claude_payload.rs` | `normalize_generic_payload()` already handles pi-mono payloads |
| `crates/nexus-hooks/src/candidate.rs` | `derive_candidates()` is agent-agnostic |
| `crates/nexus-hooks/src/enrichment.rs` | `enrich_candidates()` is agent-agnostic |
| `crates/nexus-hooks/src/persistence.rs` | `persist_enriched_memories()` is agent-agnostic |
| `crates/nexus-hooks/src/base.rs` | `BaseHook`, `AgentHook` trait unchanged |
| `crates/nexus-hooks/src/types.rs` | `AgentType::PiMono` already exists with correct metadata |
| `crates/nexus-hooks/src/session.rs` | `SessionContext` already has pi-mono fields (subagent_executions) |

### 3.3 Interface Contracts

**CLI → Nexus session start:**
```bash
nexus session start \
  --agent pi-mono \
  --session-key "2026-04-16_abc123-def456" \
  --cwd /path/to/project \
  --mode session
```

**CLI → Nexus ingest-hook-event (stdin JSON):**
```json
{
  "agent": "pi-mono",
  "event_name": "tool_result",
  "session_id": "2026-04-16_abc123-def456",
  "cwd": "/path/to/project",
  "tool_name": "edit",
  "tool_input": {"path": "src/main.ts", "old_text": "...", "new_text": "..."},
  "tool_response_text": "File updated successfully",
  "assistant_message_text": null,
  "user_message_text": null
}
```

**CLI → Nexus session event:**
```bash
nexus session event \
  --agent pi-mono \
  --session-key "2026-04-16_abc123-def456" \
  --cwd /path/to/project \
  --kind compact|checkpoint|error
```

**CLI → Nexus session end:**
```bash
nexus session end \
  --agent pi-mono \
  --session-key "2026-04-16_abc123-def456" \
  --cwd /path/to/project \
  --reason session_shutdown
```

---

## 4. Key Design Decisions

### 4.1 TypeScript Extension, Not SKILL.md

**Decision:** Install a `.ts` file at `~/.pi/agent/extensions/nexus-memory.ts`

**Rationale:** Pi-mono's extension system (`packages/coding-agent/src/core/extensions/`) loads TypeScript modules from `~/.pi/agent/extensions/` via jiti. SKILL.md files are a Claude Code concept. Pi-mono does have a "skills" system (`packages/coding-agent/src/core/skills.ts`) but skills are prompt templates, not lifecycle hooks. The extension system is where lifecycle events live.

**Evidence:** `packages/coding-agent/docs/extensions.md` line 5: "Extensions are TypeScript modules that extend pi's behavior. They can subscribe to lifecycle events, register custom tools callable by the LLM, add commands, and more."

### 4.2 CLI Transport, Not HTTP

**Decision:** Spawn `nexus` CLI commands, send JSON on stdin

**Rationale:** 
- No daemon or server required
- Matches existing `nexus session start`, `nexus session end`, `nexus ingest-hook-event` commands
- Fire-and-forget is fine — the extension doesn't need to wait for memory persistence
- HTTP transport can be added later by checking `NEXUS_SERVER_URL` and swapping the transport layer

### 4.3 Push-Based Events, Not Pull-Based File Scanning

**Decision:** Extension pushes lifecycle events to Nexus; file scanning becomes fallback only

**Rationale:**
- Push-based: Events fire in real-time with full context (tool inputs, messages, session state)
- Pull-based: Depends on pi-mono's internal file format, may miss events between file writes, requires polling

### 4.4 Centralize `on_session_start()` in CLI

**Decision:** The `nexus session start` CLI command must run the full injection pipeline

**Rationale:** Currently only `ClaudeCodeHook::new_with_install()` calls `injection::on_session_start()` — directly, not via CLI. This means any agent that calls `nexus session start` via CLI gets nothing (no morning recall, no context.md). Pi-mono's extension will call `nexus session start` via CLI, so the pipeline must be there.

### 4.5 Synthetic Error Events

**Decision:** Detect errors from failed `tool_result` and abnormal `agent_end` since pi lacks a dedicated error lifecycle event

**Rationale:** Pi-mono's event system has no `error` or `on_error` event. But we can detect errors:
- `tool_result` with `isError === true` → failed tool execution
- `agent_end` with last message having `stopReason === "error"` or `errorMessage` set
- Extension callback exceptions → internal error

### 4.6 Debouncing Strategy

**Decision:** Skip `message_update` entirely, debounce `message_end` by content identity, throttle ingestion to max once per 2 seconds

**Rationale:** `message_update` fires on every token — hundreds of times per response. Ingesting all of them would create massive noise and low-signal memories. `message_end` fires once per complete message, but identical messages in rapid succession (e.g., model retrying) should be deduplicated.

---

## 5. Risk Register

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Pi-mono extension format changes | Low | High | Extension is minimal TypeScript; only uses stable `ExtensionAPI` types |
| Nexus binary not in PATH | Medium | Medium | `findNexusBinary()` checks common paths; `NEXUS_HOOK_BINARY` env override |
| CLI spawn overhead per event | Low | Low | Fire-and-forget, detached process, no blocking |
| Noisy ingestion overwhelms enrichment | Medium | Medium | Debounce, per-turn dedup, 2s throttle, skip message_update |
| Session ID mismatch across events | Low | High | Derive once from `ctx.sessionManager.getSessionFile()`, store in closure |
| Extension causes pi-mono crash | Very Low | High | All handlers wrapped in try/catch, never throw |
| `.pi/AGENTS.md` never read by pi | High | Low | Extension reads `.nexus/context.md` directly via `before_agent_start` |
| Migration removes user's custom skill | Very Low | Low | Only removes `nexus-memory-extraction/` directory, not arbitrary skills |
| Race between constructor injection and CLI injection | Low | Low | `on_session_start()` is idempotent |

---

## 6. Dependencies & Prerequisites

### 6.1 Build Dependencies
- **Rust toolchain:** 1.75+ (MSRV)
- **Crates:** `async_trait`, `tokio`, `serde`, `serde_json`, `dirs`, `uuid`, `chrono`, `tracing` (all already in workspace)
- **No new crate dependencies required** — `serde_yaml` may be *removed* if `parse_skill_metadata()` is deleted

### 6.2 Runtime Dependencies
- **Nexus binary:** Must be installed and in PATH (or `NEXUS_HOOK_BINARY` set)
- **Pi-mono:** Must be installed (for extension auto-discovery)
- **Node.js:** Required by pi-mono (extensions loaded via jiti)
- **SQLite:** Used by Nexus storage (already required)

### 6.3 Development Dependencies
- **Pi-mono source:** `/mnt/WD-SSD/Prod/work_resources/pi-mono/` (for understanding extension types)
- **Pi-mono docs:** `packages/coding-agent/docs/extensions.md` (extension API reference)

---

## 7. Verification Checklist

### 7.1 Build Verification
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

### 7.2 Unit Test Verification
```bash
cargo test -p nexus-memory-hooks -- test_pi_mono
cargo test -p nexus-memory-hooks -- test_pi_mono_injection_target_exists
cargo test -p nexus-memory-hooks -- injection
```

### 7.3 Smoke Test
```bash
# Setup
export NEXUS_DATABASE_PATH="$(mktemp -u /tmp/nexus-pi-test.XXXXXX.db)"

# Initialize
./target/release/nexus init --reset

# Test session start (should run injection pipeline)
./target/release/nexus session start --agent pi-mono --session-key test-pi --cwd /tmp

# Verify artifacts
ls /tmp/.nexus/context.md       # Should exist
ls /tmp/.nexus/sessions/test-pi.md  # Should exist

# Test session end
./target/release/nexus session end --agent pi-mono --session-key test-pi --cwd /tmp

# Verify stats
./target/release/nexus stats    # Should show pi-mono namespace

# Cleanup
rm -f "$NEXUS_DATABASE_PATH"
```

### 7.4 Extension Installation Verification
```bash
# After building nexus with the new hook:
ls ~/.pi/agent/extensions/nexus-memory.ts  # Should exist

# Verify content
head -5 ~/.pi/agent/extensions/nexus-memory.ts
# Should show: "Nexus Memory System — Pi-Mono Extension"

# Verify legacy cleanup
ls ~/.pi/agent/skills/nexus-memory-extraction/SKILL.md 2>/dev/null
# Should NOT exist (migrated)
```

### 7.5 Integration Verification (with pi-mono)
```bash
# Start pi with the extension loaded
pi  # Should auto-discover nexus-memory.ts

# Type a prompt and observe:
# 1. .nexus/context.md should be created
# 2. nexus session start should be called
# 3. After tool executions, nexus ingest-hook-event should be called
# 4. On exit, nexus session end should be called
```

---

## 8. Parity Matrix

### 8.1 Current vs Target State

| Feature | Claude Code (Current) | Pi-Mono (Current) | Pi-Mono (Target) |
|---------|----------------------|-------------------|-------------------|
| **Artifact Type** | SKILL.md ✅ | SKILL.md ❌ (wrong) | Extension .ts ✅ |
| **session_start** | ✅ settings.json hook | ❌ false | ✅ extension event |
| **session_end** | ✅ skill trigger | ✅ process fallback | ✅ extension event |
| **checkpoint** | ✅ skill trigger | ✅ claimed | ✅ debounced agent_end |
| **error_hook** | ✅ skill trigger | ❌ false | ✅ synthetic from tool/agent |
| **compact** | ✅ skill trigger | ✅ claimed | ✅ session_compact event |
| **Morning Recall** | ✅ on_session_start | ❌ not called | ✅ via CLI pipeline |
| **context.md** | ✅ generated | ❌ not generated | ✅ generated + injected |
| **Config Injection** | ✅ CLAUDE.md | ❌ no target | ✅ .pi/AGENTS.md |
| **Soul.md Link** | ✅ injected | ❌ not injected | ✅ injected |
| **Dream Cycle** | ✅ nap on end | ❌ not triggered | ✅ via session end |
| **Rescorer** | ✅ drift detection | ❌ no rescorer | ✅ initialized |
| **.gitignore** | ✅ hardened | ❌ not hardened | ✅ hardened |
| **Retry Buffer** | ✅ PersistentBuffer | ✅ PersistentBuffer | ✅ PersistentBuffer |
| **Process Detection** | ✅ claude process | ✅ pi process | ✅ pi process |
| **Support Tier** | NativeLifecycle | NativeLifecycle (dishonest) | NativeLifecycle (honest) |
| **Reliability Score** | 1.0 | 0.95-1.0 | 1.0 |
| **Context Surfacing** | Via CLAUDE.md sentinel | None | Via before_agent_start |

### 8.2 Event Coverage Comparison

| Nexus Event | Claude Code Source | Pi-Mono Source (Target) |
|-------------|-------------------|------------------------|
| session_start | settings.json SessionStart hook | extension `session_start` event |
| session_end | skill on_session_end trigger | extension `session_shutdown` event |
| checkpoint | skill on_checkpoint trigger | extension `agent_end` (debounced) |
| error | skill on_error trigger | synthetic from failed tool_result / abnormal agent_end |
| compact | skill on_completion trigger | extension `session_compact` event |
| content_ingest | Claude hook payload | extension `tool_result` / `agent_end` / `message_end` |

---

## 9. Glossary

| Term | Definition |
|------|-----------|
| **Extension** | A TypeScript module loaded by pi-mono at `~/.pi/agent/extensions/` that exports `default function(pi: ExtensionAPI)`. Subscribes to lifecycle events, registers tools/commands. This is what pi-mono actually reads for lifecycle integration. |
| **Skill (Claude)** | A markdown file with YAML frontmatter at `~/.claude/skills/` that Claude Code reads for lifecycle triggers. NOT used by pi-mono. |
| **Skill (Pi-mono)** | A prompt template at `~/.pi/agent/skills/`. NOT the same as lifecycle extensions. Pi-mono skills are static text files, not executable code. |
| **Hook** | A Rust `AgentHook` trait implementation that manages the Nexus integration for a specific agent. Handles installation, detection, and context extraction. |
| **Injection** | Writing `<!-- NEXUS:START -->` sentinel blocks into agent config files, linking to `soul.md` and `context.md`. |
| **Enrichment** | LLM-powered analysis of memory candidates. Decides if a candidate is worth storing, assigns category/labels/memory_lane_type. |
| **Persistence** | Writing enriched memories to SQLite via `persist_enriched_memories()`. Handles merge triggers and duplicate detection. |
| **Morning Recall** | On session start, querying recent relevant memories from SQLite and loading them into the cognitive cache for context injection. |
| **Cognitive Cache** | In-memory LRU cache of hot memories, persisted to `.nexus/cache/`. Updated during morning recall and throughout the session. |
| **Dream Cycle** | Post-session processing (triggered by `run_nap()`) that consolidates, compresses, and cross-references memories. |
| **Nap** | A lightweight dream cycle triggered at session end. Processes recent memories and publishes `DreamCompleted` event. |
| **Rescorer** | Real-time re-scoring of memory relevance. Detects conversation drift and re-ranks memories accordingly. Publishes `CognitiveDrift` events. |
| **Transport** | How the TypeScript extension communicates with Nexus. V1 uses CLI spawn; future V2 may use HTTP. |
| **Normalizer** | Converts agent-specific event payloads into `NormalizedHookEvent`. Pi-mono uses `normalize_generic_payload()`, same as other generic agents. |
| **Session Context** | `SessionContext` struct containing all extracted session data: messages, decisions, files, tasks, insights, errors, subagent executions, commands. |
| **Lifecycle Capabilities** | Five boolean flags (`session_start`, `session_end`, `checkpoint`, `error_hook`, `compact`) indicating what a hook supports. |
| **Support Tier** | Classification: `NativeLifecycle` (dedicated hook + lifecycle events), `WrapperLifecycle` (generic CLI wrapper), `MonitorOnly` (process detection only). |
| **Sentinel Markers** | `<!-- NEXUS:START -->` and `<!-- NEXUS:END -->` used for idempotent config injection. Content between them is replaced on each injection. |
| **Fire-and-forget** | Transport pattern where the extension spawns a nexus process and doesn't wait for its completion. Uses detached processes with unref(). |
| **Debounce** | Suppressing rapid-fire duplicate events. `message_end` is deduplicated by content identity; all ingestion is throttled to 2s intervals. |

---

## 10. Quick Reference: Files to Read Before Starting

1. **Spec Bible:** `docs/superpowers/specs/2026-04-16-pi-mono-first-class-parity-spec-bible.md` — Complete technical specification
2. **Task List:** `docs/superpowers/plans/2026-04-16-pi-mono-first-class-parity-blocking-task-list.md` — Step-by-step implementation plan
3. **Pi-mono extension docs:** `/mnt/WD-SSD/Prod/work_resources/pi-mono/packages/coding-agent/docs/extensions.md` — Extension API reference
4. **Claude hook (reference):** `crates/nexus-hooks/src/agents/claude.rs` — What parity looks like
5. **Pi-mono hook (current):** `crates/nexus-hooks/src/agents/pi_mono.rs` — What needs to change
6. **Injection pipeline:** `crates/nexus-hooks/src/injection.rs` — Session start pipeline
7. **Base hook trait:** `crates/nexus-hooks/src/base.rs` — AgentHook trait definition
8. **Extension types (pi-mono):** `/mnt/WD-SSD/Prod/work_resources/pi-mono/packages/coding-agent/src/core/extensions/types.ts` — TypeScript type definitions

---

*End of Handoff Document — Pi-Mono First-Class Parity*
