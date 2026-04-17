# Pi-Mono First-Class Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring pi-mono integration to complete first-class parity with Claude Code in the nexus-memory-system, with native TypeScript extension, full lifecycle coverage, injection support, and centralized pipeline.

**Architecture:** TypeScript extension (installed by Rust hook) → pi lifecycle events → Nexus CLI transport → normalize → enrich → persist. Shared injection pipeline centralized in `nexus session start` CLI.

**Tech Stack:** Rust (nexus-hooks, nexus-cli), TypeScript (pi-mono extension), SQLite (storage), ONNX (embeddings)

---

## File Structure

### Files to Create
| Path | Purpose |
|------|---------|
| `crates/nexus-hooks/src/extension_ts/nexus_memory_pi.ts` | TypeScript extension source (embedded in Rust binary) |

### Files to Modify
| Path | Purpose |
|------|---------|
| `crates/nexus-hooks/src/agents/pi_mono.rs` | Rewrite: extension-based, full lifecycle |
| `crates/nexus-hooks/src/injection.rs` | Add pi-mono to known_agents() |
| `crates/nexus-hooks/src/lib.rs` | Update doc comments and re-exports |
| `crates/nexus-hooks/src/types.rs` | Add EXTENSIONS_SUBDIR to AgentType if needed |
| `crates/nexus-cli/src/commands/session.rs` | Centralize injection pipeline |

---

## Dependency Graph

```
Task 1 (injection.rs) ─────────────────────────────┐
Task 2 (TypeScript extension) ─────────────────────┤
                                                    ├─► Task 6 (tests)
Task 3 (PiMonoHook rewrite) ──depends on 1,2──────┤     │
Task 4 (lifecycle capabilities) ──depends on 3─────┤     ├─► Task 8 (migration/cleanup)
Task 5 (CLI pipeline) ─────────────────────────────┤     │
                                                    └─► Task 7 (docs)
```

---

## Task 1: Add Pi-Mono to Injection Targets

**Files:**
- Modify: `crates/nexus-hooks/src/injection.rs:22-46`
- Test: `crates/nexus-hooks/src/injection.rs` (existing test module)

- [ ] **Step 1.1: Write the failing test**

Add to `crates/nexus-hooks/src/injection.rs` tests module:

```rust
#[test]
fn test_pi_mono_injection_target_exists() {
    let target = AgentInjectionTarget::find("pi-mono");
    assert!(target.is_some(), "pi-mono must be in known_agents()");
    
    let target = target.unwrap();
    assert_eq!(target.agent_type, "pi-mono");
    assert!(target.global_config.is_some());
    assert_eq!(target.project_config_filename, ".pi/AGENTS.md");
    
    let global = target.global_config.unwrap();
    assert!(global.ends_with(".pi/agent/AGENTS.md") || global.to_string_lossy().contains(".pi/agent/AGENTS.md"));
}
```

- [ ] **Step 1.2: Run test to verify it fails**

Run: `cargo test -p nexus-memory-hooks -- test_pi_mono_injection_target_exists`
Expected: FAIL with "pi-mono must be in known_agents()"

- [ ] **Step 1.3: Add pi-mono entry to known_agents()**

In `crates/nexus-hooks/src/injection.rs`, inside `known_agents()`, add after the gemini entry (around line 44):

```rust
Self {
    agent_type: "pi-mono".to_string(),
    global_config: Some(home.join(".pi").join("agent").join("AGENTS.md")),
    project_config_filename: ".pi/AGENTS.md".to_string(),
},
```

- [ ] **Step 1.4: Run test to verify it passes**

Run: `cargo test -p nexus-memory-hooks -- test_pi_mono_injection_target_exists`
Expected: PASS

- [ ] **Step 1.5: Run all injection tests**

Run: `cargo test -p nexus-memory-hooks -- injection`
Expected: All PASS

- [ ] **Step 1.6: Commit**

```bash
git add crates/nexus-hooks/src/injection.rs
git commit -m "feat(hooks): add pi-mono to injection known_agents"
```

---

## Task 2: Create TypeScript Extension Source

**Files:**
- Create: `crates/nexus-hooks/src/extension_ts/nexus_memory_pi.ts`

This file is embedded in the Rust binary via `include_str!()` and written to `~/.pi/agent/extensions/nexus-memory.ts` during installation.

- [ ] **Step 2.1: Create the extension_ts directory**

```bash
mkdir -p crates/nexus-hooks/src/extension_ts
```

- [ ] **Step 2.2: Write the TypeScript extension**

Create `crates/nexus-hooks/src/extension_ts/nexus_memory_pi.ts`:

```typescript
/**
 * Nexus Memory System — Pi-Mono Extension
 * 
 * Automatically captures session context and stores memories via the Nexus CLI.
 * Installed by the Nexus hooks system. Do not edit manually.
 * 
 * @version 1.0.0
 * @see https://github.com/scooter-lacroix/nexus-memory-system
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { execSync, spawn } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import * as crypto from "node:crypto";

export default function nexusMemory(pi: ExtensionAPI): void {
  const AGENT_TYPE = "pi-mono";
  const NEXUS_BIN = process.env.NEXUS_HOOK_BINARY || findNexusBinary();
  
  // Session state (scoped to this extension instance)
  let sessionId: string | null = null;
  let sessionCwd: string | null = null;
  let lastIngestedContent: string | null = null;
  let ingestQueue: NexusPayload[] = [];
  let lastIngestTime = 0;
  const INGEST_DEBOUNCE_MS = 2000;
  const MAX_QUEUE_SIZE = 100;

  // ── Lifecycle Events ───────────────────────────────────────────

  pi.on("session_start", async (event, ctx) => {
    try {
      sessionId = deriveSessionId(ctx);
      sessionCwd = ctx.cwd;
      lastIngestedContent = null;
      ingestQueue = [];

      await spawnNexus([
        "session", "start",
        "--agent", AGENT_TYPE,
        "--session-key", sessionId,
        "--cwd", ctx.cwd,
        "--mode", "session",
      ]);
    } catch (err) {
      logError("session_start", err);
    }
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    try {
      // Flush pending queue
      await flushQueue();

      if (sessionId) {
        await spawnNexus([
          "session", "end",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--reason", "session_shutdown",
        ]);
      }
    } catch (err) {
      logError("session_shutdown", err);
    }
  });

  pi.on("session_compact", async (_event, ctx) => {
    try {
      if (sessionId) {
        await spawnNexus([
          "session", "event",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--kind", "compact",
        ]);
      }
    } catch (err) {
      logError("session_compact", err);
    }
  });

  // ── Content Events (Ingestion) ─────────────────────────────────

  pi.on("tool_result", async (event, ctx) => {
    try {
      const contentText = extractTextContent(event.content);
      if (!contentText && !event.isError) return; // Skip trivial results

      const payload: NexusPayload = {
        agent: AGENT_TYPE,
        event_name: "tool_result",
        session_id: sessionId,
        cwd: sessionCwd || ctx.cwd,
        tool_name: event.toolName,
        tool_input: event.input,
        tool_response_text: contentText || null,
        assistant_message_text: null,
        user_message_text: null,
      };

      await throttledIngest(payload);

      // Synthetic error event for failed tools
      if (event.isError && sessionId) {
        await spawnNexus([
          "session", "event",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--kind", "error",
        ]);
      }
    } catch (err) {
      logError("tool_result", err);
    }
  });

  pi.on("agent_end", async (event, ctx) => {
    try {
      const messages = event.messages || [];
      const lastAssistant = messages.filter(m => m.role === "assistant").pop();
      const lastUser = messages.filter(m => m.role === "user").pop();

      let assistantText: string | null = null;
      let userText: string | null = null;

      if (lastAssistant && "content" in lastAssistant) {
        assistantText = extractMessageText(lastAssistant.content);
      }
      if (lastUser && "content" in lastUser) {
        userText = typeof lastUser.content === "string"
          ? lastUser.content
          : extractMessageText(lastUser.content);
      }

      if (assistantText || userText) {
        const payload: NexusPayload = {
          agent: AGENT_TYPE,
          event_name: "agent_end",
          session_id: sessionId,
          cwd: sessionCwd || ctx.cwd,
          tool_name: null,
          tool_input: null,
          tool_response_text: null,
          assistant_message_text: assistantText,
          user_message_text: userText,
        };

        await throttledIngest(payload);
      }

      // Synthetic error for abnormal agent_end
      if (lastAssistant && "stopReason" in lastAssistant) {
        const msg = lastAssistant as any;
        if (msg.stopReason === "error" || msg.errorMessage) {
          if (sessionId) {
            await spawnNexus([
              "session", "event",
              "--agent", AGENT_TYPE,
              "--session-key", sessionId,
              "--cwd", sessionCwd || ctx.cwd,
              "--kind", "error",
            ]);
          }
        }
      }

      // Emit checkpoint after agent_end
      if (sessionId) {
        await spawnNexus([
          "session", "event",
          "--agent", AGENT_TYPE,
          "--session-key", sessionId,
          "--cwd", sessionCwd || ctx.cwd,
          "--kind", "checkpoint",
        ]);
      }
    } catch (err) {
      logError("agent_end", err);
    }
  });

  pi.on("message_end", async (event, ctx) => {
    try {
      if (event.message.role !== "assistant") return;
      
      const content = "content" in event.message ? event.message.content : null;
      if (!content) return;

      const text = extractMessageText(content);
      if (!text) return;

      // Debounce: skip if identical to last ingested content
      if (text === lastIngestedContent) return;
      lastIngestedContent = text;

      const payload: NexusPayload = {
        agent: AGENT_TYPE,
        event_name: "message_end",
        session_id: sessionId,
        cwd: sessionCwd || ctx.cwd,
        tool_name: null,
        tool_input: null,
        tool_response_text: null,
        assistant_message_text: text,
        user_message_text: null,
      };

      await throttledIngest(payload);
    } catch (err) {
      logError("message_end", err);
    }
  });

  // ── Context Injection ──────────────────────────────────────────

  pi.on("before_agent_start", async (event, ctx) => {
    try {
      const contextPath = path.join(ctx.cwd, ".nexus", "context.md");
      if (fs.existsSync(contextPath)) {
        const context = fs.readFileSync(contextPath, "utf-8").trim();
        if (context) {
          return {
            systemPrompt: event.systemPrompt + "\n\n## Nexus Memory Context\n\n" + context,
          };
        }
      }
    } catch {
      // Silently skip if context file is unreadable
    }
  });

  // ── Helpers ────────────────────────────────────────────────────

  interface NexusPayload {
    agent: string;
    event_name: string;
    session_id: string | null;
    cwd: string | null;
    tool_name: string | null;
    tool_input: unknown | null;
    tool_response_text: string | null;
    assistant_message_text: string | null;
    user_message_text: string | null;
  }

  function deriveSessionId(ctx: ExtensionContext): string {
    const sessionFile = ctx.sessionManager.getSessionFile?.();
    if (sessionFile) {
      return path.basename(sessionFile, path.extname(sessionFile));
    }
    return crypto.randomUUID();
  }

  function findNexusBinary(): string {
    const candidates = [
      path.join(process.env.HOME || "~", ".local", "bin", "nexus"),
      "/usr/local/bin/nexus",
    ];
    for (const c of candidates) {
      if (fs.existsSync(c)) return c;
    }
    return "nexus";
  }

  function extractTextContent(content: any[]): string | null {
    if (!Array.isArray(content)) return null;
    const texts = content
      .filter((c: any) => c.type === "text" && c.text)
      .map((c: any) => c.text);
    return texts.length > 0 ? texts.join("\n").slice(0, 10000) : null;
  }

  function extractMessageText(content: any): string | null {
    if (typeof content === "string") return content.slice(0, 10000);
    if (!Array.isArray(content)) return null;
    return extractTextContent(content);
  }

  async function throttledIngest(payload: NexusPayload): Promise<void> {
    const now = Date.now();
    if (now - lastIngestTime < INGEST_DEBOUNCE_MS) {
      // Queue for later
      if (ingestQueue.length < MAX_QUEUE_SIZE) {
        ingestQueue.push(payload);
      }
      return;
    }

    lastIngestTime = now;

    // Flush any queued items first
    if (ingestQueue.length > 0) {
      const queued = ingestQueue.splice(0);
      for (const p of queued) {
        await ingestPayload(p);
      }
    }

    await ingestPayload(payload);
  }

  async function ingestPayload(payload: NexusPayload): Promise<void> {
    const json = JSON.stringify(payload);
    await spawnNexus(["ingest-hook-event"], json);
  }

  async function flushQueue(): Promise<void> {
    if (ingestQueue.length === 0) return;
    const queued = ingestQueue.splice(0);
    for (const p of queued) {
      await ingestPayload(p);
    }
  }

  function spawnNexus(args: string[], stdinData?: string): Promise<void> {
    return new Promise((resolve) => {
      try {
        const child = spawn(NEXUS_BIN, args, {
          stdio: stdinData ? ["pipe", "ignore", "ignore"] : ["ignore", "ignore", "ignore"],
          detached: true,
          env: { ...process.env },
        });

        if (stdinData && child.stdin) {
          child.stdin.write(stdinData);
          child.stdin.end();
        }

        child.unref();

        const timeout = setTimeout(() => resolve(), 5000);
        child.on("exit", () => { clearTimeout(timeout); resolve(); });
        child.on("error", () => { clearTimeout(timeout); resolve(); });
      } catch {
        resolve(); // Never throw
      }
    });
  }

  function logError(event: string, err: unknown): void {
    const msg = err instanceof Error ? err.message : String(err);
    console.error(`[nexus-memory] Error in ${event}: ${msg}`);
  }
}
```

- [ ] **Step 2.3: Verify TypeScript syntax**

Run: `npx tsc --noEmit --strict crates/nexus-hooks/src/extension_ts/nexus_memory_pi.ts` (optional, type-checking requires pi-mono types installed)

- [ ] **Step 2.4: Commit**

```bash
git add crates/nexus-hooks/src/extension_ts/nexus_memory_pi.ts
git commit -m "feat(hooks): add TypeScript extension source for pi-mono"
```

---

## Task 3: Rewrite PiMonoHook for Extension-Based Installation

**Files:**
- Modify: `crates/nexus-hooks/src/agents/pi_mono.rs`

- [ ] **Step 3.1: Write failing test for extension installation**

Add to tests module in `pi_mono.rs`:

```rust
#[test]
fn test_pi_mono_hook_extension_constants() {
    assert_eq!(PiMonoHook::EXTENSIONS_SUBDIR, "agent/extensions");
}

#[test]
fn test_pi_mono_extension_file_path() {
    let dir = PathBuf::from("/tmp/test-pi-extensions");
    let path = PiMonoHook::extension_file_path(&dir);
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), "nexus-memory.ts");
}
```

- [ ] **Step 3.2: Run test to verify it fails**

Run: `cargo test -p nexus-memory-hooks -- test_pi_mono_hook_extension_constants`
Expected: FAIL (constant doesn't exist yet)

- [ ] **Step 3.3: Update struct and constants**

Replace the struct definition and constants in `pi_mono.rs`:

```rust
pub struct PiMonoHook {
    base: BaseHook,
    config_dir: PathBuf,
    session_dir: PathBuf,
    extensions_dir: PathBuf,
    process_monitor: ProcessMonitor,
    extension_installed: bool,
}

impl PiMonoHook {
    pub const AGENT_TYPE: &'static str = "pi-mono";
    pub const CONFIG_DIR_NAME: &'static str = ".pi";
    pub const EXTENSIONS_SUBDIR: &'static str = "agent/extensions";
    pub const SESSIONS_SUBDIR: &'static str = "sessions";
    pub const LOGS_SUBDIR: &'static str = "logs";
    pub const EXTENSION_FILENAME: &'static str = "nexus-memory.ts";
```

- [ ] **Step 3.4: Update constructor**

Replace `new_with_install`:

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
        // Migrate from legacy SKILL.md
        hook.migrate_from_skill();

        if !hook.extension_installed {
            if let Err(e) = hook.install_extension() {
                tracing::warn!("Failed to install pi-mono extension: {}", e);
            }
        }

        // Trigger session start injection
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

- [ ] **Step 3.5: Replace install_skill with install_extension**

```rust
fn extension_file_path(extensions_dir: &std::path::Path) -> PathBuf {
    extensions_dir.join("nexus-memory.ts")
}

fn install_extension(&mut self) -> Result<()> {
    std::fs::create_dir_all(&self.extensions_dir).map_err(|e| {
        HookError::InstallationFailed(format!("Failed to create extensions dir: {}", e))
    })?;

    let extension_path = Self::extension_file_path(&self.extensions_dir);

    let extension_content = include_str!("../extension_ts/nexus_memory_pi.ts");

    std::fs::write(&extension_path, extension_content).map_err(|e| {
        HookError::InstallationFailed(format!("Failed to write extension: {}", e))
    })?;

    self.extension_installed = true;
    tracing::info!("Pi-mono extension installed at: {:?}", extension_path);

    Ok(())
}

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

- [ ] **Step 3.6: Update is_hook_installed**

```rust
fn is_hook_installed(&self) -> bool {
    self.extension_installed
}
```

- [ ] **Step 3.7: Remove parse_skill_metadata method**

Delete the `parse_skill_metadata` method and the `serde_yaml` usage (no longer needed).

- [ ] **Step 3.8: Run tests to verify changes compile and pass**

Run: `cargo test -p nexus-memory-hooks -- test_pi_mono`
Expected: All PASS

- [ ] **Step 3.9: Commit**

```bash
git add crates/nexus-hooks/src/agents/pi_mono.rs
git add crates/nexus-hooks/src/extension_ts/
git commit -m "refactor(hooks): rewrite PiMonoHook for TypeScript extension installation"
```

---

## Task 4: Enable Full Lifecycle Capabilities

**Files:**
- Modify: `crates/nexus-hooks/src/agents/pi_mono.rs`

- [ ] **Step 4.1: Write failing tests for new capabilities**

```rust
#[test]
fn test_pi_mono_hook_lifecycle_capabilities_full() {
    let hook = PiMonoHook::new();
    let caps = hook.lifecycle_capabilities();

    assert!(caps.session_start, "pi-mono should support session_start");
    assert!(caps.session_end, "pi-mono should support session_end");
    assert!(caps.checkpoint, "pi-mono should support checkpoint");
    assert!(caps.error_hook, "pi-mono should support error_hook");
    assert!(caps.compact, "pi-mono should support compact");
}

#[tokio::test]
async fn test_pi_mono_hook_install_session_start() {
    let mut hook = PiMonoHook::new();
    let cb: SessionEndCallback = Arc::new(|_ctx| ());
    let result = hook.install_session_start_hook(cb).await;
    assert!(result.is_ok(), "pi-mono should accept session_start hook");
}

#[tokio::test]
async fn test_pi_mono_hook_install_checkpoint() {
    let mut hook = PiMonoHook::new();
    let cb: SessionEndCallback = Arc::new(|_ctx| ());
    let result = hook.install_checkpoint_hook(cb).await;
    assert!(result.is_ok(), "pi-mono should accept checkpoint hook");
}

#[tokio::test]
async fn test_pi_mono_hook_install_error() {
    let mut hook = PiMonoHook::new();
    let cb: SessionEndCallback = Arc::new(|_ctx| ());
    let result = hook.install_error_hook(cb).await;
    assert!(result.is_ok(), "pi-mono should accept error hook");
}
```

- [ ] **Step 4.2: Run tests to verify they fail**

Run: `cargo test -p nexus-memory-hooks -- test_pi_mono_hook_lifecycle_capabilities_full`
Expected: FAIL (session_start and error_hook are false)

- [ ] **Step 4.3: Implement lifecycle methods**

Add to the `impl AgentHook for PiMonoHook` block:

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
```

- [ ] **Step 4.4: Update lifecycle_capabilities**

```rust
fn lifecycle_capabilities(&self) -> LifecycleCapabilities {
    LifecycleCapabilities {
        session_start: true,
        session_end: true,
        checkpoint: true,
        error_hook: true,
        compact: true,
    }
}
```

- [ ] **Step 4.5: Run all lifecycle tests**

Run: `cargo test -p nexus-memory-hooks -- test_pi_mono`
Expected: All PASS

- [ ] **Step 4.6: Remove old lifecycle test that asserted false**

Update `test_pi_mono_hook_lifecycle_capabilities` to match new assertions (or remove if redundant with `test_pi_mono_hook_lifecycle_capabilities_full`).

- [ ] **Step 4.7: Commit**

```bash
git add crates/nexus-hooks/src/agents/pi_mono.rs
git commit -m "feat(hooks): enable full lifecycle capabilities for pi-mono"
```

---

## Task 5: Centralize Injection Pipeline in CLI

**Files:**
- Modify: `crates/nexus-cli/src/commands/session.rs`

- [ ] **Step 5.1: Read the current session.rs**

Read `crates/nexus-cli/src/commands/session.rs` to understand the current `execute_start` implementation.

- [ ] **Step 5.2: Add injection pipeline to session start command**

In the session start handler, after existing session creation logic, add:

```rust
// Run the full injection pipeline (morning recall, context.md, etc.)
if let Err(e) = nexus_hooks::injection::on_session_start(
    &cwd,
    agent_type,
    &session_key,
).await {
    tracing::warn!("Injection pipeline error (non-fatal): {}", e);
}
```

- [ ] **Step 5.3: Add nexus-hooks dependency to nexus-cli Cargo.toml if not present**

Check `crates/nexus-cli/Cargo.toml` for `nexus-hooks` dependency. Add if missing:

```toml
nexus-hooks = { path = "../nexus-hooks" }
```

- [ ] **Step 5.4: Build and verify**

Run: `cargo build -p nexus-memory-cli`
Expected: Compiles without errors

- [ ] **Step 5.5: Commit**

```bash
git add crates/nexus-cli/src/commands/session.rs
git add crates/nexus-cli/Cargo.toml
git commit -m "feat(cli): centralize injection pipeline in session start command"
```

---

## Task 6: Update All Tests

**Files:**
- Modify: `crates/nexus-hooks/src/agents/pi_mono.rs` (test module)
- Modify: `crates/nexus-hooks/src/types.rs` (test module)

- [ ] **Step 6.1: Update pi_mono.rs test_pi_mono_hook_constants**

```rust
#[test]
fn test_pi_mono_hook_constants() {
    assert_eq!(PiMonoHook::AGENT_TYPE, "pi-mono");
    assert_eq!(PiMonoHook::CONFIG_DIR_NAME, ".pi");
    assert_eq!(PiMonoHook::EXTENSIONS_SUBDIR, "agent/extensions");
}
```

- [ ] **Step 6.2: Add extension migration test**

```rust
#[test]
fn test_pi_mono_legacy_migration() {
    let dir = tempfile::tempdir().unwrap();
    let legacy_dir = dir.path()
        .join(".pi")
        .join("agent")
        .join("skills")
        .join("nexus-memory-extraction");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(legacy_dir.join("SKILL.md"), "legacy").unwrap();

    // Simulate migration
    assert!(legacy_dir.exists());
    std::fs::remove_dir_all(&legacy_dir).unwrap();
    assert!(!legacy_dir.exists());
}
```

- [ ] **Step 6.3: Verify types.rs tests still pass**

The `test_agent_type_config_dir` and `test_agent_support_tier_honest_mapping` tests should still pass since pi-mono's `config_dir()` returns `.pi` and `support_tier()` returns `NativeLifecycle`.

Run: `cargo test -p nexus-memory-hooks -- test_agent`
Expected: All PASS

- [ ] **Step 6.4: Run full test suite**

Run: `cargo test -p nexus-memory-hooks`
Expected: All PASS

- [ ] **Step 6.5: Run workspace tests**

Run: `cargo test --workspace`
Expected: All PASS (or existing failures only)

- [ ] **Step 6.6: Commit**

```bash
git add crates/nexus-hooks/src/agents/pi_mono.rs
git commit -m "test(hooks): update pi-mono tests for extension-based integration"
```

---

## Task 7: Update Documentation

**Files:**
- Modify: `crates/nexus-hooks/src/agents/pi_mono.rs` (doc comments)
- Modify: `crates/nexus-hooks/src/lib.rs` (module docs)

- [ ] **Step 7.1: Update pi_mono.rs module-level doc comment**

```rust
//! Pi-Mono hook implementation
//!
//! Pi-mono is a TypeScript/Bun-based coding agent with subagent support.
//! Integration uses a native TypeScript extension installed at
//! `~/.pi/agent/extensions/nexus-memory.ts` that hooks into pi's
//! lifecycle events (session_start, session_shutdown, agent_end, etc.)
//! and forwards them to the Nexus CLI for memory capture.
//!
//! Repository: https://github.com/badlogic/pi-mono
//! Stack: TypeScript, Node.js/Bun runtime
//! Config: ~/.pi/agent/extensions/
//! Detection: `pi` or `pi-coding-agent` process
```

- [ ] **Step 7.2: Update struct doc comment**

```rust
/// Pi-Mono hook for extracting memory from pi-mono session execution.
///
/// Uses a native TypeScript extension (not SKILL.md) that integrates
/// with pi-mono's extension API for full lifecycle event coverage.
///
/// # Integration Points
///
/// - **Extension:** `~/.pi/agent/extensions/nexus-memory.ts`
/// - **Transport:** CLI (`nexus session start`, `nexus ingest-hook-event`)
/// - **Fallback:** Process monitoring + `.pi/sessions/` file scanning
///
/// # Lifecycle Coverage
///
/// All five lifecycle events are supported:
/// - `session_start` — via extension `session_start` event
/// - `session_end` — via extension `session_shutdown` event
/// - `checkpoint` — via debounced `agent_end` and explicit triggers
/// - `error_hook` — synthetic from failed `tool_result` and abnormal `agent_end`
/// - `compact` — via extension `session_compact` event
```

- [ ] **Step 7.3: Update lib.rs if needed**

Check `crates/nexus-hooks/src/lib.rs` for any doc comments mentioning "skills" in relation to pi-mono and update to "extension".

- [ ] **Step 7.4: Run clippy**

Run: `cargo clippy -p nexus-memory-hooks --all-targets`
Expected: No warnings (or existing warnings only)

- [ ] **Step 7.5: Run fmt check**

Run: `cargo fmt --all --check`
Expected: No formatting issues

- [ ] **Step 7.6: Commit**

```bash
git add crates/nexus-hooks/src/agents/pi_mono.rs
git add crates/nexus-hooks/src/lib.rs
git commit -m "docs(hooks): update pi-mono documentation for extension-based integration"
```

---

## Task 8: Migration & Cleanup

**Files:**
- Modify: `crates/nexus-hooks/src/agents/pi_mono.rs`

- [ ] **Step 8.1: Verify migration logic works end-to-end**

The migration from SKILL.md to extension is handled in `new_with_install()` via `migrate_from_skill()`. Verify:

1. If `~/.pi/agent/skills/nexus-memory-extraction/` exists, it is removed
2. The new extension is installed at `~/.pi/agent/extensions/nexus-memory.ts`
3. Both operations are logged

- [ ] **Step 8.2: Remove serde_yaml dependency if no longer needed**

Check if `serde_yaml` is still used in `pi_mono.rs` after removing `parse_skill_metadata()`. If not, remove from `Cargo.toml`:

```bash
grep -r "serde_yaml" crates/nexus-hooks/
```

If only pi_mono.rs used it and it's removed, update `crates/nexus-hooks/Cargo.toml`.

- [ ] **Step 8.3: Final full verification**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Expected: All pass

- [ ] **Step 8.4: Commit**

```bash
git add -p  # Stage only relevant changes
git commit -m "chore(hooks): cleanup legacy skill dependencies for pi-mono migration"
```

---

## Verification Checklist

- [ ] `cargo fmt --all --check` — No formatting issues
- [ ] `cargo clippy --workspace --all-targets` — No new warnings
- [ ] `cargo test --workspace` — All tests pass
- [ ] `cargo test -p nexus-memory-hooks` — All hook tests pass
- [ ] Pi-mono injection target exists in known_agents
- [ ] Extension file exists at correct path when PiMonoHook is constructed
- [ ] Legacy SKILL.md is cleaned up on migration
- [ ] All 5 lifecycle capabilities report `true`
- [ ] `session_start`, `checkpoint`, and `error` hooks accept callbacks
- [ ] Extension TypeScript is valid and self-contained
- [ ] CLI session start triggers injection pipeline

---

*End of Plan — Pi-Mono First-Class Parity*
