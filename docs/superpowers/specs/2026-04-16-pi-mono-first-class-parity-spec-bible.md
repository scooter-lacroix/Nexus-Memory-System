# Pi-Mono First-Class Parity — Spec Bible

> **Version:** 1.0.0 | **Date:** 2026-04-16 | **Status:** Draft  
> **Scope:** Bring pi-mono to full first-class citizen parity with Claude Code in nexus-memory-system  
> **Crate:** `nexus-hooks` (primary), `nexus-cli` (session pipeline), TypeScript extension (new artifact)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Problem Statement & Gap Analysis](#2-problem-statement--gap-analysis)
3. [Architecture Overview](#3-architecture-overview)
4. [Component Design](#4-component-design)
5. [TypeScript Extension Specification](#5-typescript-extension-specification)
6. [Rust Hook Modifications](#6-rust-hook-modifications)
7. [Injection System Updates](#7-injection-system-updates)
8. [CLI Pipeline Centralization](#8-cli-pipeline-centralization)
9. [Event Flow Diagrams](#9-event-flow-diagrams)
10. [Transport Protocol Specification](#10-transport-protocol-specification)
11. [Error Handling & Recovery](#11-error-handling--recovery)
12. [Session Management](#12-session-management)
13. [Memory Enrichment Integration](#13-memory-enrichment-integration)
14. [Configuration & Environment Variables](#14-configuration--environment-variables)
15. [Security Considerations](#15-security-considerations)
16. [Testing Strategy](#16-testing-strategy)
17. [Migration Plan](#17-migration-plan)
18. [Compatibility Matrix](#18-compatibility-matrix)
19. [Glossary](#19-glossary)

---

## 1. Executive Summary

Pi-mono (`@mariozechner/pi-coding-agent`) is a TypeScript/Bun-based coding agent with a rich extension system. Its current Nexus integration installs a `SKILL.md` file — an artifact type pi-mono doesn't actually consume. This spec defines the work required to replace that broken integration with a native TypeScript extension that hooks into pi's real lifecycle events, achieving full behavioral parity with the Claude Code integration.

**Deliverables:**
1. A TypeScript extension (`nexus-memory.ts`) installed by the Rust hook into `~/.pi/agent/extensions/`
2. Rust `PiMonoHook` rewritten to install the extension, with all 5 lifecycle capabilities enabled
3. Pi-mono added to the injection system for morning recall and context injection
4. Centralized session-start pipeline in the `nexus session start` CLI command
5. Comprehensive tests and documentation updates

**Non-goals:**
- HTTP/MCP transport (CLI-first; HTTP deferred to v2)
- Real-time streaming memory hints during `message_update`
- Changes to pi-mono's own codebase (extension is installed externally)

---

## 2. Problem Statement & Gap Analysis

### 2.1 Gap: Wrong Artifact Type

| Aspect | Claude Code | Pi-Mono (Current) | Pi-Mono (Target) |
|--------|-------------|-------------------|-------------------|
| Native integration type | SKILL.md (correct for Claude) | SKILL.md (wrong for pi) | TypeScript extension (correct) |
| Install location | `~/.claude/skills/nexus-memory-extraction/SKILL.md` | `~/.pi/agent/skills/nexus-memory-extraction/SKILL.md` | `~/.pi/agent/extensions/nexus-memory.ts` |
| Auto-discovery | Claude reads skills dir | Pi **ignores** skills dir for lifecycle | Pi auto-discovers extensions dir |

Pi-mono's extension system is at `~/.pi/agent/extensions/` (global) and `.pi/extensions/` (project-local). It loads TypeScript modules via `jiti`. The SKILL.md file installed by the current hook is never read by pi-mono for lifecycle events.

### 2.2 Gap: Incomplete Lifecycle Capabilities

| Capability | Claude Code | Pi-Mono (Current) | Pi-Mono (Target) |
|------------|-------------|-------------------|-------------------|
| `session_start` | ✅ true | ❌ false | ✅ true |
| `session_end` | ✅ true | ✅ true | ✅ true |
| `checkpoint` | ✅ true | ✅ true | ✅ true |
| `error_hook` | ✅ true | ❌ false | ✅ true |
| `compact` | ✅ true | ✅ true | ✅ true |

### 2.3 Gap: No Injection Target

`injection.rs::AgentInjectionTarget::known_agents()` contains entries for `claude-code`, `amp`, `codex`, `gemini` — but **not** `pi-mono`. This means:
- No morning recall on session start
- No `context.md` generation
- No config reference injection
- No soul.md linking

### 2.4 Gap: Pull-Based vs Push-Based

Current `PiMonoHook::extract_session_context()` reads `.pi/sessions/*.json` and `.pi/logs/*.log` as its primary data source. This is:
- **Unreliable:** Files may not exist, may be stale, or may be in an inconsistent state
- **Incomplete:** Misses real-time events that happen between file writes
- **Fragile:** Depends on pi-mono's internal file format, which may change without notice

Target: The TypeScript extension pushes events to Nexus in real-time via the pi lifecycle API.

### 2.5 Gap: No Session Start Pipeline

Claude Code's constructor triggers `injection::on_session_start()` via `tokio::spawn`. Pi-mono's constructor does not. The `nexus session start` CLI command also does not run this pipeline. This means:
- Only Claude Code gets morning recall and context injection
- Other agents calling `nexus session start` get a no-op

---

## 3. Architecture Overview

### 3.1 High-Level Data Flow

```
┌──────────────────────────────────────────────────────────────────────┐
│ Pi-Mono Runtime (TypeScript/Node.js)                                │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐     │
│  │ nexus-memory.ts (Extension)                                  │     │
│  │                                                               │     │
│  │  pi.on("session_start") ──► spawn nexus session start        │     │
│  │  pi.on("agent_end")     ──► spawn nexus ingest-hook-event    │     │
│  │  pi.on("tool_result")   ──► spawn nexus ingest-hook-event    │     │
│  │  pi.on("message_end")   ──► spawn nexus ingest-hook-event    │     │
│  │  pi.on("session_compact")──► spawn nexus session event       │     │
│  │  pi.on("session_shutdown")─► spawn nexus session end         │     │
│  └──────────────────┬──────────────────────────────────────────┘     │
│                     │ CLI spawn (async, fire-and-forget)              │
└─────────────────────┼────────────────────────────────────────────────┘
                      │
                      ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Nexus Binary (Rust)                                                  │
│                                                                      │
│  nexus session start ──► injection::on_session_start()               │
│    ├─► ProjectIdentity::resolve()                                    │
│    ├─► StorageManager::from_url()                                    │
│    ├─► CognitiveCache::morning_recall()                              │
│    ├─► build_context_md()  ──► .nexus/context.md                     │
│    ├─► inject_reference()  ──► .pi/AGENTS.md, ~/.pi/agent/AGENTS.md  │
│    ├─► SessionManager::start_session()                               │
│    ├─► SessionRescorer::new()                                        │
│    └─► .gitignore hardening                                          │
│                                                                      │
│  nexus ingest-hook-event ──► normalize_generic_payload()             │
│    ├─► derive_candidates()                                           │
│    ├─► EnrichmentService::enrich_candidates()                        │
│    └─► persist_enriched_memories()                                   │
│                                                                      │
│  nexus session end ──► trigger_callbacks()                           │
│    └─► run_nap() (dream cycle)                                       │
└──────────────────────────────────────────────────────────────────────┘
```

### 3.2 Layered Architecture

```
Layer 4: Extension (TypeScript)   ──  nexus-memory.ts
Layer 3: Transport (CLI)          ──  spawn + stdin JSON  
Layer 2: Normalizer (Rust)        ──  normalize_generic_payload()
Layer 1: Pipeline (Rust)          ──  enrich → persist → dream
Layer 0: Storage (SQLite)         ──  memories, namespaces, relations
```

### 3.3 Central Unified Design Principle

All agents share the same pipeline from Layer 2 downward. Agent-specific code exists only at Layers 3-4 (transport and lifecycle binding). This means:
- Same enrichment LLM prompts for all agents
- Same persistence logic (SQLite, merge triggers, duplicate detection)
- Same dream cycle behavior on session end
- Same morning recall and context injection on session start

---

## 4. Component Design

### 4.1 TypeScript Extension (`nexus-memory.ts`)

**Responsibilities:**
- Subscribe to pi lifecycle events
- Derive session identity from pi's session context
- Normalize event data into Nexus-compatible JSON payloads
- Spawn `nexus` CLI commands with payloads on stdin
- Handle transport failures gracefully (log, don't crash pi)
- Debounce high-frequency events to avoid noise

**Interface:**
```typescript
// Entry point signature
export default function(pi: ExtensionAPI): void

// Internal types
interface NexusTransport {
  sessionStart(cwd: string, sessionId: string): Promise<void>
  sessionEnd(cwd: string, sessionId: string): Promise<void>
  sessionEvent(kind: "compact" | "checkpoint" | "error", cwd: string, sessionId: string): Promise<void>
  ingestEvent(payload: NexusHookEventPayload): Promise<void>
}

interface NexusHookEventPayload {
  agent: "pi-mono"
  event_name: string
  session_id: string | null
  cwd: string | null
  tool_name: string | null
  tool_input: unknown | null
  tool_response_text: string | null
  assistant_message_text: string | null
  user_message_text: string | null
}
```

### 4.2 Rust `PiMonoHook` (Rewritten)

**Responsibilities:**
- Install TypeScript extension to `~/.pi/agent/extensions/nexus-memory.ts`
- Detect and clean up legacy SKILL.md installation
- Report all lifecycle capabilities as `true`
- Implement all `install_*_hook()` methods
- Trigger session start injection from constructor (like Claude)
- Provide fallback detection via process monitoring and file scanning

**Struct:**
```rust
pub struct PiMonoHook {
    base: BaseHook,
    config_dir: PathBuf,
    session_dir: PathBuf,
    extensions_dir: PathBuf,         // was: skills_dir
    process_monitor: ProcessMonitor,
    extension_installed: bool,       // was: skill_installed
}
```

### 4.3 Injection Target (New Entry)

```rust
Self {
    agent_type: "pi-mono".to_string(),
    global_config: Some(home.join(".pi").join("agent").join("AGENTS.md")),
    project_config_filename: ".pi/AGENTS.md".to_string(),
}
```

### 4.4 CLI Session Pipeline (Centralized)

The `nexus session start` CLI command must be enhanced to run the full `injection::on_session_start()` pipeline, not just create a session scratch file. This is the convergence point for all agents.

---

## 5. TypeScript Extension Specification

### 5.1 Complete Extension Source

The extension MUST be a single self-contained TypeScript file. It MUST NOT require any npm dependencies beyond what pi-mono already provides (`@mariozechner/pi-coding-agent`).

### 5.2 Event Subscriptions

| Event | Handler Behavior |
|-------|-----------------|
| `session_start` | Call `nexus session start --agent pi-mono --session-key <id> --cwd <cwd>`. Derive session ID from `ctx.sessionManager.getSessionFile()` or generate UUID. Store session ID for subsequent events. |
| `session_shutdown` | Call `nexus session end --agent pi-mono --session-key <id> --cwd <cwd>`. Flush any pending ingest queue. |
| `session_compact` | Call `nexus session event --kind compact --agent pi-mono --session-key <id>`. |
| `agent_end` | Normalize `event.messages` into a payload. Call `nexus ingest-hook-event` with JSON on stdin. If messages indicate errors (stopReason === "error"), also emit synthetic error event. |
| `tool_result` | Normalize tool name, input, output into payload. Call `nexus ingest-hook-event`. Only ingest if tool has meaningful output (skip empty/trivial results). |
| `message_end` | Normalize assistant message text. Call `nexus ingest-hook-event`. Debounce: skip if identical to previous message_end in same turn. |

### 5.3 Events NOT Subscribed

| Event | Reason for Exclusion |
|-------|---------------------|
| `message_update` | Too noisy — fires on every token. Would create churn and low-signal memories. |
| `turn_start` | No meaningful content to capture. |
| `tool_execution_start` | Premature — no result yet. |
| `tool_execution_update` | Streaming partial — too noisy. |
| `model_select` | Model changes are not memory-worthy in most cases. |
| `input` | Raw user input is captured via `message_end` and `before_agent_start`. |

### 5.4 Transport Implementation

```typescript
async function spawnNexus(args: string[], stdinPayload?: string): Promise<void> {
  const nexusBin = process.env.NEXUS_HOOK_BINARY || "nexus";
  
  return new Promise((resolve) => {
    const child = spawn(nexusBin, args, {
      stdio: stdinPayload ? ["pipe", "ignore", "ignore"] : ["ignore", "ignore", "ignore"],
      detached: true,
    });
    
    if (stdinPayload && child.stdin) {
      child.stdin.write(stdinPayload);
      child.stdin.end();
    }
    
    child.unref();
    
    // Fire-and-forget with timeout safety
    const timeout = setTimeout(() => resolve(), 5000);
    child.on("exit", () => { clearTimeout(timeout); resolve(); });
    child.on("error", () => { clearTimeout(timeout); resolve(); });
  });
}
```

### 5.5 Session ID Derivation

```typescript
function deriveSessionId(ctx: ExtensionContext): string {
  const sessionFile = ctx.sessionManager.getSessionFile();
  if (sessionFile) {
    // Use the session filename as a stable ID
    return path.basename(sessionFile, path.extname(sessionFile));
  }
  // Fallback: generate a UUID for ephemeral sessions
  return crypto.randomUUID();
}
```

### 5.6 Payload Normalization

The payload MUST match the shape consumed by `normalize_generic_payload()` in `crates/nexus-hooks/src/claude_payload.rs`:

```typescript
interface NexusHookEventPayload {
  agent: "pi-mono";
  event_name: string;         // e.g., "tool_result", "agent_end", "message_end"
  session_id: string | null;
  cwd: string | null;
  tool_name: string | null;   // For tool_result events
  tool_input: unknown | null;  // Tool call arguments
  tool_response_text: string | null;  // Tool output text
  assistant_message_text: string | null;  // Assistant's response text
  user_message_text: string | null;  // User's prompt text
}
```

### 5.7 Error Detection

Since pi-mono lacks a dedicated `error` lifecycle event, errors are detected synthetically:

1. **Failed tool_result:** `event.isError === true`
2. **Abnormal agent_end:** Last message has `stopReason === "error"` or `errorMessage` is set
3. **Extension callback exception:** Wrap all handlers in try/catch; emit error event on exception

### 5.8 Debouncing Strategy

- **Per-turn deduplication:** Track `lastIngestedContent` per turn. Skip `message_end` if content matches previous.
- **Minimum interval:** At most one `ingest-hook-event` per 2 seconds.
- **Queue batching:** For rapid-fire events (multiple tool_results in one turn), queue payloads and flush after turn_end or a 1-second debounce timer.

---

## 6. Rust Hook Modifications

### 6.1 Struct Changes (`pi_mono.rs`)

```rust
// BEFORE
pub struct PiMonoHook {
    base: BaseHook,
    config_dir: PathBuf,
    session_dir: PathBuf,
    skills_dir: PathBuf,          // ← RENAME
    process_monitor: ProcessMonitor,
    skill_installed: bool,        // ← RENAME
}

// AFTER
pub struct PiMonoHook {
    base: BaseHook,
    config_dir: PathBuf,
    session_dir: PathBuf,
    extensions_dir: PathBuf,       // ← RENAMED
    process_monitor: ProcessMonitor,
    extension_installed: bool,     // ← RENAMED
}
```

### 6.2 Constant Changes

```rust
// BEFORE
pub const SKILLS_SUBDIR: &'static str = "agent/skills";

// AFTER  
pub const EXTENSIONS_SUBDIR: &'static str = "agent/extensions";
```

### 6.3 Constructor Changes

```rust
fn new_with_install(auto_install: bool) -> Self {
    let config_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(Self::CONFIG_DIR_NAME);

    let session_dir = config_dir.join(Self::SESSIONS_SUBDIR);
    let extensions_dir = config_dir.join(Self::EXTENSIONS_SUBDIR);
    let extension_installed = Self::extension_file_path(&extensions_dir).exists();

    let mut hook = Self {
        base: BaseHook::new(Self::AGENT_TYPE),
        config_dir,
        session_dir,
        extensions_dir,
        process_monitor: ProcessMonitor::new(),
        extension_installed,
    };

    if auto_install {
        // Migrate from legacy SKILL.md if present
        hook.migrate_from_skill();
        
        if !hook.extension_installed {
            if let Err(e) = hook.install_extension() {
                tracing::warn!("Failed to install pi-mono extension: {}", e);
            }
        }

        // Trigger session start injection (like Claude)
        let session_id = uuid::Uuid::new_v4().to_string();
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = crate::injection::on_session_start(&cwd, "pi-mono", &session_id).await;
                });
            }
        }
    }

    hook
}
```

### 6.4 Extension Installation (replaces `install_skill`)

```rust
fn install_extension(&mut self) -> Result<()> {
    std::fs::create_dir_all(&self.extensions_dir).map_err(|e| {
        HookError::InstallationFailed(format!("Failed to create extensions dir: {}", e))
    })?;

    let extension_path = Self::extension_file_path(&self.extensions_dir);
    
    let extension_content = include_str!("../../../nexus-hooks-extension/nexus-memory.ts");
    // OR: embed the TypeScript content as a const &str in the Rust source

    std::fs::write(&extension_path, extension_content).map_err(|e| {
        HookError::InstallationFailed(format!("Failed to write extension: {}", e))
    })?;

    self.extension_installed = true;
    tracing::info!("Pi-mono extension installed at: {:?}", extension_path);

    Ok(())
}

fn extension_file_path(extensions_dir: &std::path::Path) -> PathBuf {
    extensions_dir.join("nexus-memory.ts")
}
```

### 6.5 Legacy Migration

```rust
fn migrate_from_skill(&mut self) {
    let legacy_skill_dir = self.config_dir
        .join("agent")
        .join("skills")
        .join("nexus-memory-extraction");
    
    if legacy_skill_dir.exists() {
        tracing::info!("Migrating pi-mono from SKILL.md to TypeScript extension");
        if let Err(e) = std::fs::remove_dir_all(&legacy_skill_dir) {
            tracing::warn!("Failed to remove legacy skill dir: {}", e);
        }
    }
}
```

### 6.6 Lifecycle Capability Methods

```rust
async fn install_session_start_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
    self.base.add_callback(callback);
    Ok(())
}

async fn install_checkpoint_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
    self.base.add_callback(callback);
    Ok(())
}

async fn install_error_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
    self.base.add_callback(callback);
    Ok(())
}

fn lifecycle_capabilities(&self) -> LifecycleCapabilities {
    LifecycleCapabilities {
        session_start: true,   // was: false
        session_end: true,
        checkpoint: true,
        error_hook: true,      // was: false
        compact: true,
    }
}
```

---

## 7. Injection System Updates

### 7.1 New Known Agent Entry

File: `crates/nexus-hooks/src/injection.rs`, function `known_agents()`

Add after the gemini entry:
```rust
Self {
    agent_type: "pi-mono".to_string(),
    global_config: Some(home.join(".pi").join("agent").join("AGENTS.md")),
    project_config_filename: ".pi/AGENTS.md".to_string(),
},
```

### 7.2 Config File Management

The injected files (`.pi/AGENTS.md` and `~/.pi/agent/AGENTS.md`) are **extension-managed**:
- Created by the injection pipeline if they don't exist (empty content before injection)
- The extension can read these files to surface Nexus context to pi's system prompt
- Pi-mono itself does not natively read these files, so the extension must bridge the gap

### 7.3 Context Surfacing Strategy

The TypeScript extension should:
1. On `session_start`, read `.nexus/context.md` if it exists
2. On `before_agent_start`, optionally inject context into the system prompt:

```typescript
pi.on("before_agent_start", async (event, ctx) => {
  const contextPath = path.join(ctx.cwd, ".nexus", "context.md");
  if (fs.existsSync(contextPath)) {
    const context = fs.readFileSync(contextPath, "utf-8");
    if (context.trim()) {
      return {
        systemPrompt: event.systemPrompt + "\n\n## Nexus Memory Context\n" + context,
      };
    }
  }
});
```

---

## 8. CLI Pipeline Centralization

### 8.1 Current Problem

`injection::on_session_start()` is a 9-step pipeline that currently runs only when:
- `ClaudeCodeHook::new_with_install(true)` is called (via tokio::spawn in constructor)

The `nexus session start` CLI command does NOT run this pipeline. This means any agent that calls `nexus session start` via CLI (like our pi-mono extension will) gets a no-op.

### 8.2 Solution

File: `crates/nexus-cli/src/commands/session.rs`

The `execute_start()` function must be enhanced to call `injection::on_session_start()`:

```rust
async fn execute_start(agent: &str, session_key: &str, cwd: &Path) -> anyhow::Result<()> {
    // Existing session start logic...
    
    // NEW: Run the full injection pipeline
    nexus_hooks::injection::on_session_start(cwd, agent, session_key).await?;
    
    Ok(())
}
```

### 8.3 Idempotency

`on_session_start()` must be idempotent:
- Context.md is always overwritten (latest recall data)
- Config injection uses sentinel markers (already idempotent)
- Session scratch file creation is already idempotent
- .gitignore hardening checks before appending

---

## 9. Event Flow Diagrams

### 9.1 Session Start Flow

```
Pi-Mono starts
  │
  ├─► Loads nexus-memory.ts extension (via jiti)
  │
  ├─► Emits session_start { reason: "startup" }
  │     │
  │     └─► Extension handler fires
  │           │
  │           ├─► Derive sessionId from ctx.sessionManager.getSessionFile()
  │           ├─► Store sessionId in extension closure
  │           └─► spawn: nexus session start --agent pi-mono --session-key <id> --cwd <cwd>
  │                 │
  │                 ├─► ProjectIdentity::resolve(cwd)
  │                 ├─► Create .nexus/ directory structure
  │                 ├─► StorageManager::from_url() → SQLite
  │                 ├─► NamespaceRepository::get_or_create("nexus", "pi-mono")
  │                 ├─► CognitiveCache::morning_recall() → recalls
  │                 ├─► build_context_md(hot_cache, recalls) → .nexus/context.md
  │                 ├─► inject_reference(.pi/AGENTS.md, soul.md, context.md)
  │                 ├─► inject_reference(~/.pi/agent/AGENTS.md, soul.md, context.md)
  │                 ├─► SessionManager::start_session(id, "pi-mono")
  │                 ├─► SessionRescorer::new(project, interval, threshold)
  │                 └─► .gitignore hardening (.nexus/)
  │
  └─► Emits resources_discover { reason: "startup" }
```

### 9.2 Tool Result Ingestion Flow

```
LLM calls tool (e.g., edit_file)
  │
  ├─► tool_execution_start { toolCallId, toolName: "edit", args }
  ├─► tool_execution_end  { toolCallId, toolName: "edit", result, isError }
  │
  └─► tool_result { toolCallId, toolName: "edit", input, content, details, isError }
        │
        └─► Extension handler fires
              │
              ├─► Skip if trivial (empty content, no error)
              ├─► Normalize payload:
              │     { agent: "pi-mono", event_name: "tool_result",
              │       tool_name: "edit", tool_input: {...},
              │       tool_response_text: "...", session_id, cwd }
              │
              ├─► If isError: also spawn nexus session event --kind error
              │
              └─► spawn: nexus ingest-hook-event (stdin: JSON payload)
                    │
                    ├─► normalize_generic_payload() → NormalizedHookEvent
                    ├─► derive_candidates() → Vec<MemoryCandidate>
                    ├─► EnrichmentService::enrich_candidates() → EnrichmentBatchResult
                    └─► persist_enriched_memories() → SQLite
```

### 9.3 Session Shutdown Flow

```
User presses Ctrl+C / Ctrl+D / SIGHUP / SIGTERM
  │
  └─► session_shutdown event fires
        │
        └─► Extension handler fires
              │
              ├─► Flush any pending ingest queue
              └─► spawn: nexus session end --agent pi-mono --session-key <id> --cwd <cwd>
                    │
                    ├─► trigger_callbacks(context)
                    │     └─► For each registered callback: callback(context)
                    │
                    └─► If nap_on_session_end:
                          ├─► StorageManager::from_url()
                          ├─► create_client_auto_with_fallback() → LLM
                          ├─► create_embedding_service() → Embedder
                          └─► run_nap(session_id, cwd, namespace_id, services, timeout)
                                ├─► Process recent memories
                                ├─► Consolidate insights
                                └─► Publish DreamCompleted event
```

---

## 10. Transport Protocol Specification

### 10.1 CLI Commands

#### `nexus session start`
```
nexus session start --agent pi-mono --session-key <uuid> --cwd /path/to/project --mode session
```

#### `nexus session end`
```
nexus session end --agent pi-mono --session-key <uuid> --cwd /path/to/project --reason session_shutdown
```

#### `nexus session event`
```
nexus session event --agent pi-mono --session-key <uuid> --cwd /path/to/project --kind compact
nexus session event --agent pi-mono --session-key <uuid> --cwd /path/to/project --kind checkpoint
nexus session event --agent pi-mono --session-key <uuid> --cwd /path/to/project --kind error
```

#### `nexus ingest-hook-event`
```
echo '{"agent":"pi-mono","event_name":"tool_result",...}' | nexus ingest-hook-event
```

### 10.2 Stdin JSON Schema

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["agent", "event_name"],
  "properties": {
    "agent": { "type": "string", "const": "pi-mono" },
    "event_name": { "type": "string", "enum": ["tool_result", "agent_end", "message_end"] },
    "session_id": { "type": ["string", "null"] },
    "cwd": { "type": ["string", "null"] },
    "tool_name": { "type": ["string", "null"] },
    "tool_input": {},
    "tool_response_text": { "type": ["string", "null"] },
    "assistant_message_text": { "type": ["string", "null"] },
    "user_message_text": { "type": ["string", "null"] }
  }
}
```

### 10.3 HTTP Transport (Future v2)

When `NEXUS_SERVER_URL` is set, the extension MAY use HTTP POST instead of CLI:
- `POST /api/session/start` with JSON body
- `POST /api/session/end` with JSON body
- `POST /api/hooks/ingest` with JSON body

This is NOT part of v1 scope.

---

## 11. Error Handling & Recovery

### 11.1 Extension Error Handling

All event handlers MUST be wrapped in try/catch:
```typescript
pi.on("tool_result", async (event, ctx) => {
  try {
    await handleToolResult(event, ctx);
  } catch (err) {
    console.error("[nexus-memory] Error handling tool_result:", err);
    // Don't throw — extension errors must not crash pi
  }
});
```

### 11.2 Transport Failure Recovery

If `nexus` CLI spawn fails:
1. Log warning to stderr
2. Queue the payload in memory (max 100 entries)
3. On next successful spawn, flush the queue
4. On session_shutdown, attempt one final flush
5. Drop queued items on process exit (they'll be in the retry buffer)

### 11.3 Nexus-Side Retry Buffer

The existing `PersistentBuffer` in `crates/nexus-hooks/src/retry_buffer.rs` handles persistence failures:
- Failed writes are stored in a buffer file
- On next startup, buffered items are retried
- This is already agent-agnostic and works for pi-mono

### 11.4 Fallback Detection

If the extension is not installed or fails to load:
- `PiMonoHook::detect_session_activity()` falls back to process monitoring and `.pi/sessions/` scanning
- `PiMonoHook::extract_session_context()` falls back to reading session JSON files
- This provides degraded but functional memory capture

---

## 12. Session Management

### 12.1 Session ID Strategy

| Source | Priority | Method |
|--------|----------|--------|
| Pi session file | 1 (highest) | `ctx.sessionManager.getSessionFile()` → extract filename stem |
| Generated UUID | 2 (fallback) | `crypto.randomUUID()` |

The session ID MUST be stable across all events in a single session. It is derived once in `session_start` and stored in the extension closure.

### 12.2 Session Lifecycle Mapping

| Pi Lifecycle | Nexus Session State |
|-------------|-------------------|
| `session_start { reason: "startup" }` | New session created |
| `session_start { reason: "resume" }` | Existing session resumed (new nexus session) |
| `session_start { reason: "new" }` | Previous session ended, new one started |
| `session_start { reason: "fork" }` | Branch from existing session |
| `session_shutdown` | Session ended (cleanup, dream cycle) |
| `session_compact` | Checkpoint within session |

### 12.3 Multi-Session Handling

When pi switches sessions (`/new`, `/resume`):
1. `session_before_switch` fires (extension can prepare)
2. `session_shutdown` fires (extension calls `nexus session end`)
3. Extension is reloaded for new session
4. `session_start` fires with new reason (extension calls `nexus session start`)

The extension must handle this gracefully — each session gets its own ID.

---

## 13. Memory Enrichment Integration

### 13.1 Shared Pipeline

Pi-mono events flow through the exact same enrichment pipeline as Claude Code events:

```
Raw Event → normalize_generic_payload() → NormalizedHookEvent
  → derive_candidates() → Vec<MemoryCandidate>
  → EnrichmentService::enrich_candidates() → EnrichmentBatchResult
  → persist_enriched_memories() → SQLite
```

### 13.2 Event-Specific Normalization

| Event | `event_name` | Key Fields |
|-------|-------------|------------|
| `tool_result` | `"tool_result"` | `tool_name`, `tool_input`, `tool_response_text` |
| `agent_end` | `"agent_end"` | `assistant_message_text` (last assistant message) |
| `message_end` | `"message_end"` | `assistant_message_text` |

### 13.3 Signal Score Expectations

The `derive_candidates()` function assigns signal scores based on content analysis:
- Tool results with file modifications: ~0.7-0.9
- Decision statements in assistant messages: ~0.8-1.0
- Simple acknowledgments: ~0.1-0.3 (likely rejected by enrichment)
- Error messages: ~0.8-0.9 (high signal for debugging context)

---

## 14. Configuration & Environment Variables

### 14.1 Nexus Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `NEXUS_DATABASE_PATH` | SQLite database location | `~/.local/share/nexus-memory-system/nexus.db` |
| `NEXUS_HOOK_BINARY` | Path to nexus binary | Auto-detect or "nexus" |
| `NEXUS_AUTO_INGEST` | Enable auto-ingestion | `true` |
| `NEXUS_EMBEDDINGS_ENABLED` | Toggle embedding pipeline | `true` |
| `NEXUS_SERVER_URL` | Remote server URL (v2) | Not set (CLI mode) |
| `NEXUS_SYNC_POLICY` | Sync behavior | `auto` |

### 14.2 Pi-Mono Environment Variables

| Variable | Purpose |
|----------|---------|
| `PI_AGENT_DIR` | Override `~/.pi/agent/` directory |

### 14.3 Extension Configuration

The extension reads Nexus config from environment variables. No pi-mono settings.json integration is required in v1.

---

## 15. Security Considerations

### 15.1 Extension Permissions

Pi-mono extensions run with full system permissions. The nexus-memory extension:
- Reads the filesystem (`.nexus/context.md`, session files)
- Spawns child processes (`nexus` CLI)
- Sends session data to the nexus binary

This is consistent with pi-mono's security model (documented in extensions.md: "Extensions run with your full system permissions").

### 15.2 Data Sensitivity

- Session content may contain API keys, passwords, or secrets visible in tool outputs
- The enrichment LLM should not be sent raw secrets
- The `derive_candidates()` function filters content before enrichment
- No data leaves the local machine unless `NEXUS_SERVER_URL` is explicitly set

### 15.3 File Permissions

- Extension file: `0644` (readable by owner and group)
- `.nexus/` directory: inherits project permissions
- Session scratch files: `0600` (owner only)

---

## 16. Testing Strategy

### 16.1 Rust Unit Tests

| Test | File | Purpose |
|------|------|---------|
| `test_pi_mono_hook_new` | `pi_mono.rs` | Basic construction |
| `test_pi_mono_hook_detect_activity` | `pi_mono.rs` | Process detection |
| `test_pi_mono_hook_constants` | `pi_mono.rs` | Constants correct |
| `test_pi_mono_hook_lifecycle_capabilities` | `pi_mono.rs` | All caps `true` |
| `test_pi_mono_hook_install_session_start` | `pi_mono.rs` | Session start hook installation |
| `test_pi_mono_hook_install_checkpoint` | `pi_mono.rs` | Checkpoint hook installation |
| `test_pi_mono_hook_install_error` | `pi_mono.rs` | Error hook installation |
| `test_pi_mono_hook_install_compact` | `pi_mono.rs` | Compact hook installation |
| `test_pi_mono_extension_installation` | `pi_mono.rs` | Extension file written correctly |
| `test_pi_mono_legacy_migration` | `pi_mono.rs` | SKILL.md cleaned up |
| `test_pi_mono_injection_target` | `injection.rs` | Pi-mono in known_agents |

### 16.2 Integration Tests

| Test | Purpose |
|------|---------|
| `test_pi_mono_session_lifecycle` | Full session start → events → end cycle |
| `test_pi_mono_injection_pipeline` | Context.md and config injection |
| `test_pi_mono_ingest_pipeline` | Event → normalize → enrich → persist |

### 16.3 Smoke Test

```bash
export NEXUS_DATABASE_PATH="$(mktemp -u /tmp/nexus-pi-test.XXXXXX.db)"
./target/release/nexus init --reset
./target/release/nexus session start --agent pi-mono --session-key test-pi-session --cwd /tmp
ls /tmp/.nexus/context.md  # Should exist
./target/release/nexus session end --agent pi-mono --session-key test-pi-session --cwd /tmp
./target/release/nexus stats  # Should show pi-mono namespace
```

---

## 17. Migration Plan

### 17.1 From SKILL.md to Extension

1. On `PiMonoHook::new()`, check for legacy SKILL.md at `~/.pi/agent/skills/nexus-memory-extraction/SKILL.md`
2. If found, remove the entire `nexus-memory-extraction/` directory
3. Install the new TypeScript extension at `~/.pi/agent/extensions/nexus-memory.ts`
4. Log: `"Migrated pi-mono Nexus integration from SKILL.md to TypeScript extension"`

### 17.2 Backward Compatibility

- Existing memories in SQLite are unaffected (agent namespace "pi-mono" is preserved)
- Session files in `.pi/sessions/` are still readable as fallback
- No user action required — migration is automatic

### 17.3 Rollback

If the extension causes issues:
- Delete `~/.pi/agent/extensions/nexus-memory.ts`
- Pi-mono will load without the extension
- Nexus falls back to process monitoring and file scanning
- Re-install with `nexus hooks install pi-mono`

---

## 18. Compatibility Matrix

### 18.1 Full Parity Comparison (Target State)

| Feature | Claude Code | Pi-Mono (Target) | Notes |
|---------|-------------|-------------------|-------|
| Integration type | SKILL.md + settings.json | TypeScript extension | Language-appropriate |
| session_start | ✅ settings.json hook | ✅ extension event | Both trigger injection pipeline |
| session_end | ✅ skill trigger | ✅ extension event | Both trigger dream cycle |
| checkpoint | ✅ skill trigger | ✅ extension event + debounced agent_end | |
| error_hook | ✅ skill trigger | ✅ synthetic from tool_result/agent_end | |
| compact | ✅ skill trigger | ✅ session_compact event | |
| Morning recall | ✅ on_session_start() | ✅ on_session_start() | Shared pipeline |
| Context injection | ✅ CLAUDE.md | ✅ .pi/AGENTS.md | Agent-specific config file |
| Memory enrichment | ✅ same pipeline | ✅ same pipeline | Identical |
| Dream cycle | ✅ on session end | ✅ on session end | Identical |
| Rescorer | ✅ drift detection | ✅ drift detection | Identical |
| Retry buffer | ✅ PersistentBuffer | ✅ PersistentBuffer | Identical |
| Process detection | ✅ claude process | ✅ pi/pi-mono process | Agent-specific names |
| Support tier | NativeLifecycle | NativeLifecycle | Both native |
| Reliability score | 1.0 | 1.0 | Both fully native |

---

## 19. Glossary

| Term | Definition |
|------|-----------|
| **Extension** | A TypeScript module loaded by pi-mono that subscribes to lifecycle events and registers tools/commands |
| **Skill** | A markdown file with frontmatter used by Claude Code (NOT by pi-mono) for lifecycle triggers |
| **Hook** | A Rust implementation of the `AgentHook` trait that manages integration with a specific agent |
| **Injection** | The process of writing Nexus context references into an agent's configuration files |
| **Enrichment** | LLM-powered analysis of memory candidates to decide storage worthiness, categorization, and labeling |
| **Persistence** | Writing enriched memories to SQLite via the storage layer |
| **Morning Recall** | On session start, loading recent relevant memories from the database to prime the context |
| **Cognitive Cache** | In-memory cache of hot memories, updated during morning recall and throughout the session |
| **Dream Cycle** | Post-session processing that consolidates, compresses, and cross-references memories |
| **Nap** | A lightweight dream cycle triggered at session end |
| **Rescorer** | Real-time re-scoring of memory relevance based on conversation drift |
| **Transport** | The mechanism by which the TypeScript extension communicates with the Nexus binary (CLI or HTTP) |
| **Normalizer** | Converts agent-specific event payloads into the common `NormalizedHookEvent` format |
| **Session Context** | The `SessionContext` struct containing all extracted data from an agent session |
| **Lifecycle Capabilities** | The set of lifecycle events a hook implementation supports (session_start, session_end, checkpoint, error_hook, compact) |
| **Support Tier** | Classification of integration depth: NativeLifecycle, WrapperLifecycle, MonitorOnly |
| **Sentinel Markers** | `<!-- NEXUS:START -->` and `<!-- NEXUS:END -->` markers used for idempotent config injection |

---

*End of Spec Bible — Pi-Mono First-Class Parity*
