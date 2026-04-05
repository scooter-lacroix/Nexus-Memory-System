# Session Hook Error Diagnostic — 2026-04-05

## Problem Statement

Three session lifecycle hooks consistently produce errors in the Claude Code integration:

1. **SessionStart** — fires via `session-start-delayed.sh` (direct binary call)
2. **Stop** — fires via `event-ingest.js` shim, mapped to `nexus session event --kind compact`
3. **SessionEnd** — fires via `event-ingest.js` shim, mapped to `nexus session end`

The errors observed in `~/.local/state/nexus-memory-system/hook-errors.log` are:

```
[2026-04-04T00:15:08.061Z] claude-code/Stop failed: <nexus stderr with LLM enrichment warnings>
[2026-04-05T01:35:17.747Z] claude-code/PostToolUse failed: Error: Supervisor error: EOF while parsing a value at line 1 column 0
```

Additionally, there are 9 repeated `LLM enrichment failed` warnings in the Stop error's stderr output, all with the same root cause:

```
WARN nexus::commands::ingest_hook_event: LLM enrichment failed
  error=Invalid JSON response from LLM: Failed to parse: missing field `memories` at line 17 column 1.
  Raw: { "accepted_memories": [ { "memory_text": "...", ...
```

## Hook Routing (Current)

All hooks are registered in `~/.claude/settings.json`:

| Hook Event | Command | Timeout |
|---|---|---|
| PostToolUse | `node event-ingest.js claude-code PostToolUse` | 30000ms |
| PreCompact | `node event-ingest.js claude-code PreCompact` | 5000ms |
| Stop | `node event-ingest.js claude-code Stop` | 30000ms |
| SessionEnd | `node event-ingest.js claude-code SessionEnd` | 30000ms |
| SessionStart | `/home/scooter/.cargo/bin/nexus session start --agent claude-code --mode session` (via `session-start-delayed.sh`) | N/A (no timeout) |

The `event-ingest.js` shim routes events as follows:

| Event Name | Nexus Subcommand |
|---|---|
| SessionStart | `nexus session start --agent --session-key --cwd` |
| PreCompact / SessionCompact / Stop | `nexus session event --agent --kind compact --session-key --cwd` |
| SessionEnd | `nexus session end --agent --session-key --cwd` |
| (anything else, e.g. PostToolUse) | `nexus ingest-hook-event --agent --event --format --session-key --cwd` |

## Error Analysis

### Error 1: Stop Hook Failure (LLM Enrichment JSON Parse Error)

**What happens:**
1. Stop fires → shim calls `nexus session event --kind compact`
2. `execute_event()` in `session.rs` is called
3. `checkpoint_flush_enabled` is `true` by default in `CognitionConfig`
4. This triggers `drain_retry_buffer_for_session(..., max_artifacts=2)`
5. The retry buffer contains 2 pending artifacts from prior failed `PostToolUse` ingestions
6. `drain_retry_buffer_for_session` calls `process_normalized_event()` for each artifact
7. `process_normalized_event()` calls `EnrichmentService::enrich_candidates()`
8. The LLM (configured as `glm-4.5-air`) returns JSON with `"accepted_memories"` but **no `"memories"` field**
9. The serde deserializer expects a `memories` field → parse error → `IngestOutcome::Deferred`
10. The artifact stays in the retry buffer
11. The Stop hook's stderr includes all 9 enrichment warnings from the drain attempt
12. The shim sees `result.status !== 0` → logs to `hook-errors.log`

**Root cause chain:**
- The LLM provider (`glm-4.5-air` via whatever API endpoint is configured) returns a JSON structure that does not match the expected `EnrichedBatch` schema. The LLM returns `accepted_memories` and/or `rejected_memories` but omits the top-level `memories` field that the Rust code expects.
- This causes every enrichment attempt to fail.
- Failed artifacts accumulate in the retry buffer.
- Every Stop hook then re-attempts to drain the buffer and fails identically.

**The enrichment prompt/schema is defined in:**
`crates/nexus-hooks/src/enrichment.rs` — `EnrichmentService::enrich_candidates()`

The LLM is being asked to return structured JSON with a `memories` field, but the model `glm-4.5-air` is producing a different structure (`accepted_memories` / `rejected_memories` at the top level).

### Error 2: PostToolUse EOF Error

**What happens:**
1. PostToolUse fires → shim calls `nexus ingest-hook-event --agent claude-code --event PostToolUse --format claude-code --session-key --cwd`
2. `ingest_hook_event::execute()` reads stdin with `std::io::stdin().read_to_string()`
3. stdin is empty → `serde_json::from_str("")` → `EOF while parsing a value at line 1 column 0`
4. The `.context("Failed to parse stdin as JSON")` wraps it and the command exits non-zero

**Root cause:**
The `event-ingest.js` shim passes `rawInput` to `spawnSync` as the `input` option. However, when the hook is invoked by Claude Code's hook system, **stdin may be empty or not provided at all** for certain hook events. The shim reads stdin asynchronously via `readStdin()` but if nothing is piped, it resolves to `""`. This empty string is then passed as `input` to `spawnSync`, and `ingest_hook_event::execute` expects valid JSON.

**Note:** `session.rs` handles this correctly via `read_optional_stdin_json()` which checks for terminal, empty input, and trims before parsing. `ingest_hook_event.rs` does NOT have this guard — it unconditionally calls `serde_json::from_str`.

### Error 3: SessionStart (No explicit error in log, but potential issue)

The `session-start-delayed.sh` script is called directly by Claude Code's `SessionStart` hook (not via the shim). It runs:

```bash
/home/scooter/.cargo/bin/nexus session start --agent claude-code --mode session "$@" >/dev/null 2>&1 || true
```

This path does not appear in the error log, suggesting it succeeds. However, the path is **hardcoded** to `/home/scooter/.cargo/bin/nexus` — the installer was updated to use a placeholder, but the previously installed hook on this machine still has the old hardcoded path. After a reinstall with the fixed installer, this would resolve correctly.

## Fixes Attempted (in this track)

1. **Installer: hardcoded binary path in `session-start-delayed.sh`** — FIXED. Replaced `/home/scooter/.cargo/bin/nexus` with `NEXUS_BIN_PATH` placeholder + sed replacement at install time. Verified the installed hook now has the correct path.

2. **Installer: dead `NEXUS_AUTO_INGEST` env var** — REMOVED. It was written to env files and Claude Code settings but never read by `Config::from_env()`. Not a cause of these errors but was dead configuration.

3. **Installer: Python quoting bugs** — FIXED. `python3 -c "..."` blocks broke on embedded double quotes. Converted to `python3 << PYTHON_EOF` heredocs. Not related to session errors but was causing the Claude Code hook configuration step to fail silently.

4. **Installer: clean reinstall mode** — ADDED `--reinstall` and `--reset-db` flags. Not related to these errors but required for the user's workflow.

None of the fixes above address the actual runtime errors in the session hook path.

## Root Causes Identified (Not Yet Fixed)

### Root Cause A: LLM Enrichment Schema Mismatch

**File:** `crates/nexus-hooks/src/enrichment.rs`

The `EnrichmentService` sends a prompt to the LLM requesting structured JSON with a `memories` field. The LLM (`glm-4.5-air`) returns a different structure:

```json
{
  "accepted_memories": [...],
  "rejected_memories": [...]
}
```

But the Rust code deserializes into a struct expecting:

```json
{
  "memories": [...],
  "rejected_memories": [...]
}
```

The field name mismatch (`accepted_memories` vs `memories`) causes every enrichment attempt to fail. This cascades into:
- Retry buffer fills up with failed artifacts
- Every Stop hook retries the same failures
- SessionEnd would attempt the same drain and fail identically

**Fix options:**
1. Update the `EnrichedBatch` struct to accept both `memories` and `accepted_memories` as aliases (use `#[serde(alias = "accepted_memories")]`)
2. Update the enrichment prompt to be more explicit about the required JSON schema
3. Add a pre-parse normalization step that maps `accepted_memories` → `memories` before deserialization
4. Switch to a more reliable LLM model for enrichment (the current `glm-4.5-air` may not follow JSON schema instructions reliably)

### Root Cause B: `ingest_hook_event::execute` Crashes on Empty Stdin

**File:** `crates/nexus-cli/src/commands/ingest_hook_event.rs`, line ~46

```rust
let raw: serde_json::Value =
    serde_json::from_str(&raw_input).context("Failed to parse stdin as JSON")?;
```

This unconditionally requires valid JSON on stdin. When Claude Code fires a PostToolUse hook with no piped data, stdin is empty and this panics.

**Contrast with `session.rs`:**
```rust
fn read_optional_stdin_json() -> Option<Value> {
    if std::io::stdin().is_terminal() { return None; }
    // ... reads, trims, returns None on empty or parse failure
}
```

**Fix:** Apply the same `read_optional_stdin_json()` pattern to `ingest_hook_event::execute`, or add an empty-stdin guard before the JSON parse.

### Root Cause C: Retry Buffer Never Drains on Persistent Enrichment Failure

**File:** `crates/nexus-cli/src/commands/ingest_hook_event.rs` — `process_normalized_event()`

When enrichment fails, artifacts are written to the retry buffer. When Stop/SessionEnd calls `drain_retry_buffer_for_session`, it calls `process_normalized_event` which hits the same enrichment failure. The artifact is never removed from the buffer. The buffer grows unbounded and every lifecycle event pays the cost of re-processing the same failures.

**Fix options:**
1. Add a max-retry count per artifact (e.g., 3 attempts, then discard)
2. Add a TTL to retry artifacts (e.g., discard after 24 hours)
3. When enrichment fails consistently, skip enrichment and fall back to storing raw activity only
4. Make the drain operation non-blocking / fire-and-forget so it does not cause the calling hook to fail

### Root Cause D: Stop Event Mapped to `--kind compact` Is Semantically Wrong

**File:** `scripts/install.sh` — `event-ingest.js` shim, line ~77

```javascript
case "PreCompact":
case "SessionCompact":
case "Stop":
  args = ["session", "event", "--agent", agent, "--kind", "compact"];
```

`Stop` is a session lifecycle termination event, not a compact/checkpoint event. Mapping it to `--kind compact` means:
1. It records a `session_compact` memory instead of a `session_stop` or `session_end` memory
2. It triggers `checkpoint_flush_enabled` logic (retry buffer drain) which is expensive and error-prone

**Fix:** Map `Stop` to its own kind (e.g., `--kind stop`) or to `--kind end`. Do not trigger retry buffer drain on Stop.

## Recommended Fix Priority

1. **Root Cause B** (empty stdin crash) — simplest fix, highest impact on user-facing errors. One-line guard in `ingest_hook_event::execute`.
2. **Root Cause A** (enrichment schema mismatch) — causes the cascading retry buffer pollution. Fix the struct deserialization or the LLM prompt.
3. **Root Cause C** (retry buffer unbounded growth) — add max-retry or TTL to prevent infinite retry loops.
4. **Root Cause D** (Stop misrouted as compact) — semantic correction, prevents unnecessary retry drain on session termination.

## Files to Modify

| File | Change |
|---|---|
| `crates/nexus-cli/src/commands/ingest_hook_event.rs` | Add empty-stdin guard (Root Cause B), add max-retry to drain (Root Cause C) |
| `crates/nexus-hooks/src/enrichment.rs` | Fix `EnrichedBatch` deserialization to accept `accepted_memories` alias (Root Cause A) |
| `scripts/install.sh` (event-ingest.js shim) | Map Stop to its own kind, not compact (Root Cause D) |
| `crates/nexus-cli/src/commands/session.rs` | Optionally make `drain_retry_buffer_for_session` non-blocking or error-resilient |

## Verification Steps After Fixes

1. Clear the retry buffer: `rm -rf ~/.local/state/nexus-memory-system/retry-buffer/` (or equivalent path)
2. Run a Claude Code session and verify:
   - PostToolUse hooks no longer produce EOF errors
   - Stop hooks complete without stderr
   - SessionEnd hooks complete without stderr
   - `hook-errors.log` remains empty or near-empty
3. Check `nexus stats` for clean session lifecycle memories
4. Verify retry buffer does not grow unbounded across multiple sessions
