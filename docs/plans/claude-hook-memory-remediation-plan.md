# Claude Hook + Memory Model Remediation Plan

Date: 2026-03-24

## Goal

Fix the Claude Code hook so it extracts the real tool, message, and input fields from Claude's hook payload, and in the same implementation sequence upgrade the Nexus memory model so stored memories are:

- high-signal rather than raw hook pings
- categorized into `general`, `facts`, `preferences`, `context`, `specifications`, or `session`
- stored with rich metadata and evidence
- accompanied by an LLM-generated comment on every persisted memory

This plan is intentionally detailed enough that a smaller implementation model can turn it into a blocking task list and begin coding with minimal additional discovery.

## Confirmed Findings

### 1. The current Claude hook bug is real

The file actually responsible for the low-signal Claude hook ingestion is not in this repository. It lives at:

- `~/.config/nexus-memory-system/hooks/event-ingest.js`

The current implementation assumes payload fields like:

```js
tool: payload.tool || payload.matcher || null
message:
  payload.last_assistant_message ||
  payload.output ||
  payload.text ||
  payload.command
input:
  payload.input_messages ||
  payload["input-messages"] ||
  payload.arguments
```

That is the wrong shape for Claude Code hook input.

Local Claude hook consumers on this machine already show the actual payload shape is closer to:

```json
{
  "hook_event_name": "PostToolUse",
  "tool_name": "Bash",
  "tool_input": {
    "command": "rg -n \"foo\" ."
  },
  "tool_response": "...",
  "message": {
    "content": [
      { "type": "text", "text": "..." }
    ]
  }
}
```

Evidence from local Claude hook code:

- `~/.claude/hooks/tirith-check.py` reads `hook_event_name`, `tool_name`, and `tool_input`
- `~/.claude/hooks/reasoning-capture-hook.py` reads `tool_response`, `message.content`, and `tool_name`
- `~/.claude/plugins/tracklens/dist/index.js` reads `event.tool_input?.plan`

Conclusion:

- `summarizePayload()` is looking for the wrong keys
- `tool`, `message`, and `input` are empty because the code never checks `tool_name`, `tool_input`, `tool_response`, or structured `message.content`

### 2. The repo already supports richer categories

The core category model already exists in [`crates/nexus-core/src/types.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-core/src/types.rs):

```rust
pub enum MemoryCategory {
    General,
    Facts,
    Preferences,
    Context,
    Specifications,
    Session,
}
```

So the categorization requirement does not require inventing a new enum.

### 3. The storage layer already supports metadata and memory lane types

The repository store path in [`crates/nexus-storage/src/repository.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-storage/src/repository.rs) already accepts:

```rust
pub async fn store(
    &self,
    namespace_id: i64,
    content: &str,
    category: &Category,
    memory_lane_type: Option<&MemoryLaneType>,
    labels: &[String],
    metadata: &serde_json::Value,
    embedding: Option<&[f32]>,
    embedding_model: Option<&str>,
) -> Result<Memory>
```

So the database is not the main blocker.

### 4. The current CLI store path throws away metadata richness

The current CLI `store` command in [`crates/nexus-cli/src/commands/store.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/commands/store.rs) always stores:

```rust
&serde_json::json!({})
```

This means the hook currently persists low-value string blobs and discards the evidence needed for useful memory retrieval.

### 5. The hook/extractor model is split across two worlds

There are currently two different ingestion stories:

- repo-native Rust hook/session extraction in `crates/nexus-hooks`
- external JS hook files under `~/.config/nexus-memory-system/hooks`

The Claude low-signal issue is happening in the external JS path, not the Rust-native `crates/nexus-hooks/src/agents/claude.rs` path.

That means the remediation must do both:

1. fix the external Claude hook immediately
2. move enrichment logic into the repo so memory quality is not trapped inside ad hoc user-local scripts

## Target End State

For each Claude Code hook event, the system should do this:

1. Capture the raw hook payload without losing fields.
2. Normalize it into a stable internal event schema.
3. Score whether the event is high-signal enough to produce memories.
4. Derive one or more candidate memories.
5. Ask an LLM to:
   - decide if each candidate is worth storing
   - assign one of the six categories
   - rewrite content into a retrieval-friendly memory
   - generate a comment for every stored memory
6. Persist each accepted memory with:
   - rewritten content
   - category
   - labels
   - optional `memory_lane_type`
   - source evidence
   - raw event references
   - LLM comment in metadata

Important constraint:

- If the LLM is unavailable, do not silently store empty or low-signal memory junk.
- Instead, buffer the normalized event for later enrichment or fail open only into a retry buffer, not into the main memories table.

## Recommended Architecture

### Architectural decision

Do not keep the intelligence in `event-ingest.js`.

Instead:

- make the JS hook a thin transport shim
- move normalization, scoring, categorization, memory derivation, and storage into Rust inside this repository

Reason:

- the repo can be tested and versioned
- the current JS file is outside the repo and easy to drift
- storage, categories, metadata, and search all already live in Rust
- a single Rust ingestion path can later serve Claude, Codex, Gemini, Qwen, and others

## Implementation Plan

## Phase 1: Introduce a real hook-ingestion command in the repo

### Files to add or update

- [`crates/nexus-cli/src/main.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/main.rs)
- [`crates/nexus-cli/src/commands/mod.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/commands/mod.rs)
- new: `crates/nexus-cli/src/commands/ingest_hook_event.rs`
- new: `crates/nexus-hooks/src/ingest.rs`
- new: `crates/nexus-hooks/src/agents/claude_payload.rs`
- possibly update [`crates/nexus-hooks/src/lib.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-hooks/src/lib.rs)

### Why

Right now the external JS hook directly calls:

```bash
nexus store --content ...
```

That is too weak. We need a first-class command that accepts raw hook payloads and performs structured ingestion.

### New CLI subcommand

Add a new CLI command like this:

```rust
IngestHookEvent {
    #[arg(long)]
    agent: String,

    #[arg(long)]
    event: String,

    #[arg(long, default_value = "auto")]
    format: String,
}
```

Expected usage from the JS hook:

```bash
node ~/.config/nexus-memory-system/hooks/event-ingest.js claude-code post-tool-use
```

and inside the JS hook it should call:

```bash
nexus ingest-hook-event --agent claude-code --event post-tool-use --format claude-code
```

with the raw JSON piped on stdin unchanged.

### Command contract

`nexus ingest-hook-event` should:

1. read raw stdin
2. parse JSON
3. normalize the payload into a `NormalizedHookEvent`
4. run high-signal extraction
5. invoke LLM enrichment
6. persist only accepted memories
7. print a concise result summary

Example result:

```text
Stored 2 memories from claude-code/post-tool-use
- facts: 1
- session: 1
- skipped: 3 low-signal candidates
```

## Phase 2: Define a normalized hook event model

### Files to add or update

- new: `crates/nexus-hooks/src/ingest.rs`
- new: `crates/nexus-hooks/src/agents/claude_payload.rs`
- optionally extend [`crates/nexus-hooks/src/session.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-hooks/src/session.rs) if shared helpers are useful

### New Rust types

Add a normalized event model similar to:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedHookEvent {
    pub agent: String,
    pub event_name: String,
    pub observed_at: DateTime<Utc>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_response_text: Option<String>,
    pub assistant_message_text: Option<String>,
    pub user_message_text: Option<String>,
    pub raw_payload: Value,
}
```

### Claude-specific normalization rules

Implement a Claude normalizer that reads both snake_case and camelCase fields:

```rust
fn get_string<'a>(value: &'a Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}
```

Required extraction rules:

- `event_name` from `hook_event_name` or `hookEventName`
- `tool_name` from `tool_name` or `toolName`
- `tool_input` from `tool_input` or `toolInput`
- `session_id` from `session_id`, `sessionId`, `thread_id`, `threadId`, or `conversation_id` if present
- `turn_id` from `turn_id`, `turnId`, `message_id`, or `messageId`
- `tool_response_text` from `tool_response` or `toolResponse`
- `assistant_message_text` from `message.content`
- `cwd` from `cwd`, `directory`, or `workspace`

### Structured message extraction

Claude `message.content` may be a string or a list of blocks. Implement a helper like:

```rust
fn flatten_message_content(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Array(items)) => {
            let text_parts: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    let item_type = item.get("type").and_then(|v| v.as_str())?;
                    if item_type == "text" {
                        item.get("text")
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim().to_string())
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty())
                .collect();

            if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n\n"))
            }
        }
        _ => None,
    }
}
```

### Why this matters

This is the direct fix for the empty fields bug.

The current JS hook expects `payload.arguments`, `payload.output`, and `payload.text`; the Claude payload uses nested `tool_input`, `tool_response`, and `message.content`.

## Phase 3: Make the external JS hook a thin shim

### File to update outside the repo

- `~/.config/nexus-memory-system/hooks/event-ingest.js`

### Required change

Replace most of `summarizePayload()` behavior with a thin passthrough that forwards raw stdin to the new Rust ingestion command.

### Recommended JS implementation

The simplest reliable version is:

```js
#!/usr/bin/env node

const { spawnSync } = require("child_process");
const { mkdirSync, appendFileSync } = require("fs");
const { dirname, join } = require("path");
const os = require("os");

const [, , agent = "generic", eventName = "event"] = process.argv;

function readStdin() {
  return new Promise((resolve) => {
    if (process.stdin.isTTY) {
      resolve("");
      return;
    }

    let data = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => {
      data += chunk;
    });
    process.stdin.on("end", () => resolve(data));
    process.stdin.on("error", () => resolve(""));
  });
}

function logFailure(message) {
  try {
    const logPath = join(
      os.homedir(),
      ".local",
      "state",
      "nexus-memory-system",
      "hook-errors.log",
    );
    mkdirSync(dirname(logPath), { recursive: true });
    appendFileSync(logPath, `[${new Date().toISOString()}] ${message}\n`);
  } catch (_) {
    // fail open
  }
}

(async () => {
  const rawInput = await readStdin();

  const result = spawnSync(
    "nexus",
    [
      "ingest-hook-event",
      "--agent",
      agent,
      "--event",
      eventName,
      "--format",
      agent,
    ],
    {
      input: rawInput,
      encoding: "utf8",
      env: process.env,
    },
  );

  if (result.status !== 0) {
    logFailure(
      `${agent}/${eventName} failed: ${result.stderr || result.stdout || `exit ${result.status}`}`,
    );
  }

  process.exit(0);
})().catch((error) => {
  logFailure(`${agent}/${eventName} crashed: ${error.stack || error.message}`);
  process.exit(0);
});
```

### Important note

This is preferable to trying to keep complex extraction logic in JS.

If an immediate hotfix is needed before the Rust command lands, then at minimum patch the JS hook to read:

- `tool_name`
- `tool_input`
- `tool_response`
- `message.content`

But the final implementation should still move logic into Rust.

## Phase 4: Add high-signal memory candidate derivation

### Files to add

- new: `crates/nexus-hooks/src/candidate.rs`
- new: `crates/nexus-hooks/src/high_signal.rs`

### New types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub candidate_id: String,
    pub source_event_name: String,
    pub source_agent: String,
    pub signal_score: f32,
    pub provisional_category: Option<String>,
    pub memory_text: String,
    pub evidence: serde_json::Value,
    pub labels: Vec<String>,
}
```

### Candidate derivation rules

The system should not store one raw memory per hook event. It should derive candidates only when there is possible retrieval value.

#### Store-worthy examples

- Bash command that reveals a durable fact about the repo or environment
- tool use that changes project direction or confirms a design decision
- user prompt that reveals a stable preference
- completion event that captures accepted requirements or specs
- session summary event with meaningful progress or blockers

#### Skip examples

- bare pings with only timestamp, cwd, session id
- repeated tool calls with no semantic outcome
- generic read/search operations without extracted insight
- empty assistant message or empty tool result

### Suggested heuristics

For v1, derive candidates from:

- `tool_name == "Bash"` and the command output contains concrete facts or decisions
- plan/review related events with `tool_input.plan`
- `user-prompt-submit` with stable user preference or explicit requirement
- assistant messages with clear decisions, commitments, or summarized context

Do not derive candidates for:

- empty `tool_input` and empty `message`
- noise-only events
- duplicate events with identical normalized fingerprints within the same session

### Duplicate suppression

Build a fingerprint from:

- `session_id`
- `event_name`
- `tool_name`
- normalized `tool_input`
- normalized `assistant_message_text`

Use it to suppress repeated low-value event-derived candidates before they ever reach the LLM.

## Phase 5: Introduce LLM enrichment and required comment generation

### Files to add

- new: `crates/nexus-hooks/src/enrichment.rs`
- new: `crates/nexus-hooks/src/llm.rs`
- possibly new: `crates/nexus-core/src/config_llm.rs` or extend existing config

### Core rule

Every memory written to the main memories table must include an LLM-generated comment.

That comment can be free-form, but the persistence schema should still be stable enough for querying. The best balance is:

- fixed outer JSON field names
- free-form LLM-authored comment text

### Metadata shape for persisted memory

Store at least this in `metadata`:

```json
{
  "source": {
    "agent": "claude-code",
    "event_name": "post-tool-use",
    "session_id": "abc",
    "turn_id": "def",
    "cwd": "/repo/path"
  },
  "evidence": {
    "tool_name": "Bash",
    "tool_input": { "command": "cargo test" },
    "tool_response_excerpt": "test result: ok. 42 passed",
    "assistant_message_excerpt": "The tests are green."
  },
  "ingestion": {
    "signal_score": 0.92,
    "normalized_at": "2026-03-24T00:00:00Z",
    "pipeline_version": "hook-ingest-v1"
  },
  "llm_comment": {
    "model": "configured-model-name",
    "generated_at": "2026-03-24T00:00:00Z",
    "text": "This is worth remembering because it confirms the repository is currently passing its test suite after the storage-path change."
  }
}
```

The outer keys are fixed for stability.

The contents of `llm_comment.text` are determined by the model itself.

### Recommended enrichment output contract

Ask the LLM to return strict JSON:

```json
{
  "store": true,
  "category": "facts",
  "memory_text": "The repository test suite passed after the storage-path change.",
  "labels": ["testing", "verification", "storage"],
  "memory_lane_type": "confidence",
  "comment": "Worth storing because it is a concrete verification event that may be used later to explain why the storage fix was considered safe.",
  "confidence": 0.93
}
```

For multiple candidates, return a list:

```json
{
  "memories": [
    {
      "store": true,
      "category": "context",
      "memory_text": "...",
      "labels": ["..."],
      "memory_lane_type": "workflow_note",
      "comment": "...",
      "confidence": 0.81
    }
  ]
}
```

### Prompt requirements

The prompt should explicitly instruct the model:

1. Only store durable or decision-relevant information.
2. Use exactly one category from:
   - `general`
   - `facts`
   - `preferences`
   - `context`
   - `specifications`
   - `session`
3. Produce concise retrieval-friendly memory text.
4. Produce a comment for every stored memory explaining why it matters, how it should be used, or what future retrieval purpose it serves.
5. Reject noise.

### Suggested prompt

```text
You are enriching agent hook events into durable memories for a retrieval system.

Decide whether each candidate is worth storing.

Only keep information that is durable, decision-relevant, preference-revealing, specification-bearing, contextual in a useful way, or session-significant.

Allowed categories:
- general
- facts
- preferences
- context
- specifications
- session

For each accepted memory:
- rewrite the memory into a standalone retrieval-friendly sentence or short paragraph
- assign exactly one allowed category
- produce labels
- optionally assign a memory_lane_type if clearly justified
- produce a comment

The comment must be model-authored and should explain why the memory is worth keeping, what retrieval value it has, or how it should be interpreted later.

Reject low-signal operational noise.

Return strict JSON only.
```

### LLM transport

This repo does not currently have an obvious general-purpose LLM enrichment module. The cleanest implementation is:

- add a small OpenAI-compatible chat client using `reqwest`
- configure via environment variables

Recommended env vars:

- `NEXUS_LLM_BASE_URL`
- `NEXUS_LLM_API_KEY`
- `NEXUS_LLM_MODEL`
- `NEXUS_LLM_TIMEOUT_MS`

If you want compatibility with the current machine setup, the implementation should also support reusing already-set Anthropic-compatible values where possible, but the code should present a clean Nexus-specific config surface.

### Fail-closed behavior

Because the requirement says every stored memory must have an LLM comment:

- if enrichment fails, do not persist candidate memories to `memories`
- instead write the normalized event and candidates to a retry buffer file under a Nexus state directory

Suggested path:

- `~/.local/state/nexus-memory-system/pending-enrichment/`

## Phase 6: Persist rich memory metadata instead of empty JSON

### Files to update

- [`crates/nexus-cli/src/commands/store.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/commands/store.rs)
- [`crates/nexus-cli/src/main.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/main.rs)
- optionally [`crates/nexus-mcp/src/tools.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-mcp/src/tools.rs)

### Required changes

The existing `store` CLI command is too narrow. It should be extended so it can accept:

- metadata JSON
- optional `memory_lane_type`

Suggested CLI shape:

```rust
Store {
    #[arg(short = 'm', long)]
    content: String,

    #[arg(short, long, default_value = "default")]
    agent: String,

    #[arg(short = 'g', long, default_value = "general")]
    category: String,

    #[arg(short, long)]
    labels: Option<String>,

    #[arg(long)]
    metadata_json: Option<String>,

    #[arg(long)]
    memory_lane_type: Option<String>,
}
```

Then in `commands/store.rs` parse and forward them instead of hardcoding `{}`.

### Why this matters

Even if `ingest-hook-event` stores memories directly without reusing the CLI `store` command, the CLI should still support rich metadata so:

- manual ingestion is consistent with automated ingestion
- scripts and operators can inspect and backfill the new model

## Phase 7: Define category assignment rules

The LLM should decide the final category, but the implementation should document intended meanings to keep outputs stable.

### Category definitions

- `general`
  - durable information that matters but does not fit a more specific bucket
- `facts`
  - objective facts about the codebase, environment, APIs, outputs, tool behavior, or system state
- `preferences`
  - stable user or project preferences, coding style choices, operational preferences
- `context`
  - current situational state that may matter later but is not a hard spec
- `specifications`
  - requirements, acceptance criteria, constraints, agreed implementation targets
- `session`
  - important session-level events, major progress, blockers, handoff context, verified completion milestones

### Examples

Input:

```json
{
  "tool_name": "Bash",
  "tool_input": { "command": "cargo test -p nexus-hooks" },
  "tool_response": "running 12 tests\ntest result: ok. 12 passed"
}
```

Possible output:

- category: `facts`
- memory text: `The nexus-hooks crate test suite passed locally with 12 tests.`
- comment: `Useful verification evidence for later debugging or release confidence.`

Input:

```json
{
  "event_name": "user-prompt-submit",
  "user_message_text": "Use LeIndex tools so investigation is conducted in a token efficient manner."
}
```

Possible output:

- category: `preferences`
- memory text: `The user prefers investigations to use LeIndex tooling for token-efficient codebase analysis.`
- comment: `This is a durable workflow preference that should affect future investigation strategy.`

Input:

```json
{
  "assistant_message_text": "We will fix the Claude hook parsing bug and upgrade the memory model in the same plan."
}
```

Possible output:

- category: `specifications`
- memory text: `The approved remediation scope includes both fixing the Claude hook payload parsing and upgrading the memory model in one implementation plan.`
- comment: `This captures accepted scope and prevents partial implementation drift.`

## Phase 8: Add retry buffering for enrichment failures

### Files to add

- new: `crates/nexus-hooks/src/retry_buffer.rs`

### Why

The requirement "every stored memory must have an LLM comment" means enrichment is no longer optional. A network blip or provider failure cannot lead to either:

- empty low-signal memories being stored anyway
- total loss of the event

### Required behavior

If the LLM step fails:

1. write a retry artifact containing:
   - normalized event
   - derived candidates
   - failure reason
   - created timestamp
2. log the failure
3. exit without storing to the main memories table

Suggested retry artifact schema:

```json
{
  "agent": "claude-code",
  "event_name": "post-tool-use",
  "normalized_event": { "...": "..." },
  "candidates": [{ "...": "..." }],
  "error": "timeout calling LLM provider",
  "created_at": "2026-03-24T00:00:00Z"
}
```

## Phase 9: Add tests before wiring the hook over

### Test files to add

- `crates/nexus-hooks/tests/claude_payload_normalization.rs`
- `crates/nexus-hooks/tests/high_signal_extraction.rs`
- `crates/nexus-hooks/tests/enrichment_contract.rs`
- `crates/nexus-cli/tests/ingest_hook_event.rs`

### Required fixtures

Create JSON fixtures for at least:

1. `claude-post-tool-use-bash.json`
2. `claude-user-prompt-submit.json`
3. `claude-post-tool-use-read-noise.json`
4. `claude-plan-event.json`
5. `claude-message-content-array.json`

### Required assertions

#### Normalization tests

- `tool_name` is extracted from `tool_name`
- `tool_input` is extracted from `tool_input`
- `assistant_message_text` is flattened from `message.content[]`
- `tool_response_text` is extracted
- `session_id` and `turn_id` are populated when present

#### High-signal tests

- noise-only event yields zero candidates
- Bash verification event yields at least one candidate
- user preference prompt yields a preference candidate

#### Enrichment contract tests

Mock the LLM client and assert:

- every stored memory has `metadata.llm_comment.text`
- category is one of the six allowed values
- rejected candidates are not stored

#### End-to-end ingest tests

Given a Claude payload fixture:

- `nexus ingest-hook-event` stores structured memories
- categories are set correctly
- metadata includes evidence and source block
- no memory is stored with empty `tool`, `message`, and `input` fields

## Phase 10: Cut over the Claude hook and then backfill

### Cutover steps

1. Implement and test `nexus ingest-hook-event`.
2. Update the external `event-ingest.js` to be a shim.
3. Run the hook locally with fixture payloads first.
4. Enable on live Claude events.
5. Inspect stored rows in the `claude-code` namespace.

### Validation queries

After cutover, verify:

- no new memories are opaque timestamp-only blobs
- `metadata.source.event_name` is present
- `metadata.evidence.tool_input` is populated for tool events
- `metadata.llm_comment.text` exists on every new memory
- categories spread across the six supported values rather than defaulting everything to `session`

### Optional backfill

If desired, write a one-off backfill tool that:

- reads recent low-signal Claude memories
- reconstructs what it can from stored raw content
- re-enriches them through the new pipeline
- archives or deletes superseded junk memories

This is optional because old low-signal rows likely lack enough evidence for high-confidence recovery.

## Concrete Blocking Task List

This is the implementation order a smaller model should follow.

1. Add `ingest-hook-event` CLI command in [`crates/nexus-cli/src/main.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/main.rs) and [`crates/nexus-cli/src/commands/mod.rs`](/mnt/WD-SSD/Moved_Docs/nexus-memory-system/crates/nexus-cli/src/commands/mod.rs).
2. Create `crates/nexus-cli/src/commands/ingest_hook_event.rs` with stdin JSON parsing and orchestration.
3. Add `NormalizedHookEvent` and Claude payload normalization helpers under `crates/nexus-hooks/src/`.
4. Add message flattening helpers for `message.content`.
5. Add high-signal candidate derivation with duplicate suppression.
6. Add an LLM client abstraction and an OpenAI-compatible implementation using env config.
7. Add strict JSON enrichment prompt and parser.
8. Add a persistence adapter that writes `content`, `category`, `labels`, `memory_lane_type`, and rich `metadata`.
9. Extend CLI `store` so metadata and memory lane type can be passed explicitly.
10. Add retry buffering for failed enrichment.
11. Add normalization, candidate, enrichment, and end-to-end ingest tests.
12. Replace the external JS hook body with the thin passthrough shim.
13. Validate live Claude ingestion and inspect stored rows.

## Minimum Viable Acceptance Criteria

The implementation is not done until all of these are true:

1. A real Claude `PostToolUse` payload populates `tool_name`, `tool_input`, and message/response text in the normalized event.
2. The system no longer stores timestamp-only Claude hook pings as primary memories.
3. Every stored memory is assigned exactly one of:
   - `general`
   - `facts`
   - `preferences`
   - `context`
   - `specifications`
   - `session`
4. Every stored memory includes `metadata.llm_comment.text`.
5. The metadata contains enough evidence to explain why the memory exists.
6. Low-signal duplicate hook events are skipped or buffered, not promoted into junk memories.
7. The external Claude JS hook becomes a transport shim, not the home of memory intelligence.

## Non-Goals

These should not block the first implementation:

- perfect historical backfill of old low-signal memories
- multi-provider LLM support beyond one working OpenAI-compatible path
- solving every agent integration at once

The first goal is a correct Claude path plus the new durable memory model.

## Final Recommendation

Implement the Claude fix and memory-model upgrade together, but sequence them like this:

1. introduce the Rust ingestion pipeline
2. move Claude payload parsing into the repo
3. require LLM enrichment plus comment generation before persistence
4. convert the external JS hook into a thin passthrough

That sequence fixes the immediate empty-field bug and prevents the system from regressing back into low-signal event spam.
