# Nexus Autonomous Cognitive System — Spec Bible

> **Date**: 2026-04-10
> **Status**: Implemented
> **Scope**: Memory retrieval/injection, soul.md, per-project cognitive cache, dream automation, model failover

---

## 1. Executive Summary

The Nexus Memory System currently captures memories but does not autonomously feed them back into active agent sessions. This spec defines the architecture for a fully autonomous cognitive substrate that:

1. **Automatically injects relevant memories** into any CLI agent session without requiring explicit tool calls
2. **Builds and maintains a unified `soul.md`** — a project-agnostic identity document reflecting the user's patterns, preferences, and cross-project learnings
3. **Manages per-project cognitive caches** with hot/cold tiering and continuous relevance scoring
4. **Automates dream cycles** at three tiers (nap, dream, deep dream) calibrated to the user's activity patterns
5. **Handles model failover** on rate limits (already implemented via `FallbackClient`)

The system operates like a human memory/subconscious — it just works, learning, growing, and adapting across every session without requiring direct interaction.

---

## 2. Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                        USER'S CLI AGENTS                                │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐              │
│   │Claude Code│  │   Amp    │  │  Codex   │  │  Gemini  │  ...         │
│   └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘              │
│        │              │              │              │                    │
│   Reads CLAUDE.md  AGENTS.md    AGENTS.md      GEMINI.md               │
│   (includes ref   (includes ref (includes ref (includes ref            │
│    to nexus files)  to nexus)    to nexus)     to nexus)               │
└────────┼──────────────┼──────────────┼──────────────┼───────────────────┘
         │              │              │              │
         ▼              ▼              ▼              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     NEXUS INJECTION LAYER                               │
│                                                                         │
│  ~/.config/nexus/soul.md  ◄── Global identity (one per user)           │
│  .nexus/context.md        ◄── Per-project cognitive cache              │
│  .nexus/sessions/<id>.md  ◄── Session scratch files                    │
│  .nexus/project.toml      ◄── Project identity marker                  │
│                                                                         │
│  ┌─────────────────┐  ┌──────────────────┐  ┌───────────────────┐      │
│  │ Reference        │  │ Context Builder  │  │ Relevance Scorer  │      │
│  │ Injector (Hooks) │  │ (Hot/Cold/Rank)  │  │ (Embeddings)      │      │
│  └────────┬────────┘  └────────┬─────────┘  └────────┬──────────┘      │
└───────────┼─────────────────────┼──────────────────────┼────────────────┘
            │                     │                      │
            ▼                     ▼                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     NEXUS COGNITIVE ENGINE                              │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │ Dream Cycle Orchestrator                                         │   │
│  │  ┌─────────┐  ┌──────────────┐  ┌────────────────────────────┐  │   │
│  │  │  Nap    │  │    Dream     │  │      Deep Dream            │  │   │
│  │  │(session │  │ (threshold)  │  │  (24hr/user-calibrated)    │  │   │
│  │  │  end)   │  │              │  │                            │  │   │
│  │  └─────────┘  └──────────────┘  └────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌──────────────────┐  ┌───────────────────┐  ┌─────────────────┐      │
│  │ Soul Builder     │  │ Normalization Gate │  │ Activity Monitor│      │
│  │ (Deep Dream)     │  │ (LLM Evaluation)  │  │ (Sleep Detector)│      │
│  └──────────────────┘  └───────────────────┘  └─────────────────┘      │
│                                                                         │
│  Existing Nexus: StorageManager, EmbeddingService, RepresentationSvc   │
│                  LlmClient (FallbackClient), CognitionConfig           │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Component Design

### 3.1 Project Identity System

**Purpose**: Deterministically identify which project a session belongs to.

**New crate**: None — lives in `nexus-core` as a utility module.

**File**: `crates/nexus-core/src/project_identity.rs`

**Resolution order** (local-first, always):
1. Check for `.nexus/project.toml` in cwd or parents (explicit override)
2. Use canonical absolute path of the working directory
3. Enrich with git remote URL if available (for cross-machine correlation)

```rust
// crates/nexus-core/src/project_identity.rs

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Unique identity for a project, used as the cache key for per-project memories.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectIdentity {
    /// Canonical absolute path to the project root directory.
    /// This is ALWAYS the primary key.
    pub root_dir: PathBuf,

    /// Git remote origin URL, if available. Used for cross-machine
    /// correlation but never as the primary identity.
    pub git_remote: Option<String>,

    /// Human-readable project name derived from directory name
    /// or project.toml override.
    pub display_name: String,
}

/// Marker file for explicit project identity override.
/// Lives at `.nexus/project.toml` in the project root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMarker {
    pub name: Option<String>,
    pub aliases: Vec<String>,
}

impl ProjectIdentity {
    /// Resolve the project identity for the given working directory.
    ///
    /// Resolution order:
    /// 1. Walk up from `cwd` looking for `.nexus/project.toml`
    /// 2. Walk up from `cwd` looking for `.git/` directory
    /// 3. Fall back to `cwd` itself as project root
    ///
    /// GUARD RAIL: Never panic. If git commands fail, proceed without
    /// git_remote. If filesystem access fails, use cwd as-is.
    pub fn resolve(cwd: &Path) -> Self {
        let root_dir = Self::find_project_root(cwd);
        let display_name = Self::derive_display_name(&root_dir);
        let git_remote = Self::detect_git_remote(&root_dir);

        Self {
            root_dir,
            git_remote,
            display_name,
        }
    }

    /// Walk up directory tree looking for `.nexus/project.toml` or `.git/`.
    fn find_project_root(start: &Path) -> PathBuf {
        let mut current = start.to_path_buf();
        loop {
            if current.join(".nexus").join("project.toml").exists() {
                return current;
            }
            if current.join(".git").exists() {
                return current;
            }
            if !current.pop() {
                // Reached filesystem root, use original cwd
                return start.to_path_buf();
            }
        }
    }

    /// Extract git remote origin URL. Never fails — returns None on error.
    fn detect_git_remote(root: &Path) -> Option<String> {
        std::process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(root)
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    String::from_utf8(output.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
    }

    fn derive_display_name(root: &Path) -> String {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown-project")
            .to_string()
    }

    /// Stable hash key for database lookups and cache keying.
    /// Uses the canonical root_dir path.
    pub fn cache_key(&self) -> String {
        self.root_dir.to_string_lossy().to_string()
    }
}
```

**GUARD RAILS**:
- `find_project_root` must have a loop bound (max 256 iterations) to prevent infinite loops on broken symlinks
- `detect_git_remote` must timeout after 2 seconds — git can hang on network-mounted filesystems
- Never store absolute paths in the database for cross-machine portability; store the `cache_key` but also `git_remote` for correlation

---

### 3.2 Soul.md Pipeline

**Purpose**: Build and maintain a unified `~/.config/nexus/soul.md` that reflects the user's identity across all agents and projects.

**Location**: `~/.config/nexus/soul.md`

**New module**: `crates/nexus-agent/src/soul.rs`

#### 3.2.1 Soul Document Structure

The soul.md is a markdown document with natural prose. It is NOT a structured config file. It reads like a personality profile that any agent can understand:

```markdown
# Nexus Soul

## Identity & Preferences
<!-- Naturally emerging observations about the user -->
- Prefers minimal, surgical diffs over full file rewrites
- Values being corrected when wrong — never wants false agreement
- Thinks through problems out loud before wanting action taken
- Prefers Rust for systems work; reaches for Python for scripting

## Technical Learnings
<!-- Generalized insights stripped of project-specific details -->
- SQLite merge triggers with RAISE(IGNORE) break last_insert_rowid()
  — always implement content-based fallback lookups
- When designing async pipelines, prefer channel-based communication
  over shared mutable state behind locks
- Embedding cache hit rates above 85% indicate the working set is
  stable enough for aggressive caching

## Working Patterns
<!-- How the user works, their rhythms and style -->
- Most productive in late-night sessions (typically 10pm-5am)
- Prefers brainstorming before implementation — resist jumping to code
- Iterates rapidly: small change → verify → next change
- Values cross-project consistency in naming conventions

## Agent Notes
<!-- How to express through different interfaces — emerges naturally -->
- In Amp sessions: be more concise, user expects density
- In Claude Code: streaming explanations are preferred
- User responds well to analogies and metaphors when explaining architecture
```

**CRITICAL GUARD RAIL**: The soul.md must NEVER contain:
- Project-specific file paths, variable names, or architecture details
- Session IDs, timestamps, or ephemeral context
- Forced personality traits — everything emerges from observed patterns
- Anything the user explicitly asked to forget

#### 3.2.2 Soul Builder Service

```rust
// crates/nexus-agent/src/soul.rs

use std::path::PathBuf;
use std::sync::Arc;

use nexus_llm::{ChatMessage, GenerateParams, LlmClient};
use nexus_storage::repository::MemoryRepository;
use tracing::{debug, info, warn};

use crate::error::AgentError;

/// Path to the soul document.
pub fn soul_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexus")
        .join("soul.md")
}

/// Maximum token budget for the soul document.
/// ENGINEERING JUDGEMENT: 2048 tokens ≈ ~1500 words. Large enough for
/// a rich personality profile, small enough to leave room for project context.
const SOUL_MAX_TOKENS: usize = 2048;

/// Minimum sessions before a pattern is considered soul-worthy.
/// ENGINEERING JUDGEMENT: 2 means the pattern must be observed at least twice
/// across different sessions/projects before it enters the soul.
const SOUL_MIN_PATTERN_OBSERVATIONS: usize = 2;

pub struct SoulBuilder {
    llm: Arc<dyn LlmClient>,
}

impl SoulBuilder {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// Read the current soul document from disk.
    /// Returns empty string if file doesn't exist yet (first run).
    pub fn read_current_soul(&self) -> String {
        let path = soul_path();
        std::fs::read_to_string(&path).unwrap_or_default()
    }

    /// Rebuild the soul document during a deep dream cycle.
    ///
    /// This is the ONLY entry point for soul modifications.
    /// Never call this outside of deep dream.
    ///
    /// Pipeline:
    /// 1. Gather soul-candidate memories (cross-project, high-confidence derived/distilled)
    /// 2. Run normalization gate (strip project specifics)
    /// 3. Merge with existing soul (LLM evaluates additions/removals)
    /// 4. Write updated soul to disk
    pub async fn rebuild_soul(
        &self,
        memory_repo: &MemoryRepository,
        candidate_memories: &[SoulCandidate],
    ) -> Result<String, AgentError> {
        let current_soul = self.read_current_soul();

        if candidate_memories.is_empty() && !current_soul.is_empty() {
            debug!("No new soul candidates, keeping existing soul");
            return Ok(current_soul);
        }

        // Step 1: Normalize candidates — strip project-specific details
        let normalized = self.normalize_candidates(candidate_memories).await?;

        // Step 2: Evaluate and merge with existing soul
        let updated_soul = self
            .evaluate_and_merge(&current_soul, &normalized)
            .await?;

        // Step 3: Write to disk
        let path = soul_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AgentError::Storage(format!("Failed to create soul directory: {}", e))
            })?;
        }
        std::fs::write(&path, &updated_soul).map_err(|e| {
            AgentError::Storage(format!("Failed to write soul.md: {}", e))
        })?;

        info!("Soul updated ({} bytes)", updated_soul.len());
        Ok(updated_soul)
    }

    /// Normalization gate: use LLM to strip project-specific details
    /// from candidate memories, extracting only generalizable learnings.
    async fn normalize_candidates(
        &self,
        candidates: &[SoulCandidate],
    ) -> Result<Vec<NormalizedLearning>, AgentError> {
        // ... LLM call with SOUL_NORMALIZATION_PROMPT
        // See §3.2.3 for prompt definition
        todo!()
    }

    /// Evaluation gate: LLM decides which normalized learnings to add,
    /// which existing soul entries to update/remove, and produces
    /// the final merged soul document.
    async fn evaluate_and_merge(
        &self,
        current_soul: &str,
        normalized: &[NormalizedLearning],
    ) -> Result<String, AgentError> {
        // ... LLM call with SOUL_EVALUATION_PROMPT
        // See §3.2.3 for prompt definition
        todo!()
    }
}

/// A memory that has been identified as a potential soul contribution.
#[derive(Debug, Clone)]
pub struct SoulCandidate {
    /// The memory content (already at derived/distilled cognitive level)
    pub content: String,
    /// Which project this came from (for normalization stripping)
    pub source_project: String,
    /// How many times this pattern has been observed
    pub observation_count: usize,
    /// The category of the original memory
    pub category: String,
    /// Agent that generated this memory
    pub source_agent: String,
}

/// A learning that has been normalized (project details stripped).
#[derive(Debug, Clone)]
pub struct NormalizedLearning {
    pub content: String,
    pub category: SoulCategory,
    pub confidence: f32,
    pub observation_count: usize,
}

/// Categories within the soul document.
#[derive(Debug, Clone, Copy)]
pub enum SoulCategory {
    IdentityPreference,
    TechnicalLearning,
    WorkingPattern,
    AgentNote,
}
```

#### 3.2.3 Soul Prompts

```rust
// Add to crates/nexus-agent/src/prompts.rs

/// System prompt for soul normalization — stripping project specifics.
pub const SOUL_NORMALIZATION_PROMPT: &str = r#"You are a memory normalization engine.

Your job is to take project-specific memories and extract generalizable learnings.

Rules:
- Strip ALL project-specific details: file paths, variable names, project names,
  specific architecture choices tied to one project
- Preserve the UNIVERSAL truth: the pattern, the insight, the principle
- If a memory is entirely project-specific with no generalizable kernel, discard it
- Keep technical terms that are universal (e.g., "SQLite", "async", "connection pooling")
- Discard technical terms that are project-specific (e.g., "MemoryRepository", "nexus-agent")
- Preserve user preferences and working style observations exactly
- Each output should be a standalone insight that applies across projects

Output valid JSON only:
{
  "normalized": [
    {
      "content": "string — the generalized learning",
      "category": "identity_preference|technical_learning|working_pattern|agent_note",
      "confidence": 0.0-1.0,
      "reasoning": "string — why this is generalizable"
    }
  ],
  "discarded_count": integer
}"#;

/// System prompt for soul evaluation — deciding what enters the soul.
pub const SOUL_EVALUATION_PROMPT: &str = r#"You are the guardian of a unified identity document (soul.md).

You will receive:
1. The CURRENT soul document
2. A list of normalized candidate learnings

Your job is to produce an UPDATED soul document that integrates worthy candidates.

Evaluation criteria for each candidate:
1. DURABILITY: Will this still be true in 6 months? (Reject ephemeral observations)
2. PATTERN STRENGTH: Has this been observed multiple times? (Prefer multi-observation patterns)
3. CONTRADICTION CHECK: Does this contradict existing soul content? If so, which is better supported? Update accordingly.
4. REDUNDANCY: Is this already captured in the soul? If so, strengthen existing wording, don't duplicate.
5. GENERALIZABILITY: Is this truly project-agnostic? (Reject anything that only applies to one context)

Personality emergence rules:
- NEVER force personality traits. Only record what is genuinely observed.
- NEVER add aspirational statements. Only record actual patterns.
- Agent behavioral notes should reflect OBSERVED differences in how the user interacts with different agents.
- Technical preferences (e.g., language preferences) CAN emerge if the model genuinely tends toward them across sessions.

The soul document MUST:
- Stay under ~1500 words
- Use natural prose with markdown formatting
- Feel like a personality profile, NOT a config file
- Organize into sections: Identity & Preferences, Technical Learnings, Working Patterns, Agent Notes
- Preserve existing content that is still valid

Output the complete updated soul.md content as a markdown string.
Do NOT wrap in JSON. Output raw markdown only."#;
```

**GUARD RAILS**:
- Soul is ONLY modified during deep dream cycles — never during naps or mid-session
- The LLM evaluation gate is mandatory — no direct writes to soul.md from memory pipeline
- Soul.md must be backed up before each update: `soul.md.bak` (single-generation backup)
- If the LLM call fails during soul update, keep the existing soul unchanged — never corrupt

---

### 3.3 Per-Project Cognitive Cache

**Purpose**: Maintain a per-project context file (`.nexus/context.md`) that is automatically loaded by agents, containing hot-cache learnings, cold-cache recalls, and relevance-scored memories.

**New module**: `crates/nexus-agent/src/cognitive_cache.rs`

#### 3.3.1 Directory Structure

```text
project-root/
├── .nexus/
│   ├── project.toml          # Optional project identity override
│   ├── context.md             # The injected context file (agent reads this)
│   ├── cache/
│   │   ├── hot.json           # Hot cache state (recent + key learnings, full fidelity)
│   │   └── cold_index.json    # Cold cache index (memory IDs + relevance scores)
│   └── sessions/
│       ├── <session-id-1>.md  # Session scratch (active session)
│       └── <session-id-2>.md  # Session scratch (previous, awaiting merge)
```

#### 3.3.2 Hot/Cold Cache Data Model

```rust
// crates/nexus-agent/src/cognitive_cache.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Maximum number of entries in the hot cache.
/// ENGINEERING JUDGEMENT: 20 entries provides rich context without
/// overwhelming the token budget. Each entry is a distilled/derived
/// memory, typically 1-3 sentences.
const HOT_CACHE_MAX_ENTRIES: usize = 20;

/// Minimum relevance score for a cold-cache memory to be surfaced
/// during morning recall.
/// ENGINEERING JUDGEMENT: 0.65 is below the standard semantic search
/// threshold (0.7) to allow "whisper" level recalls. Memories between
/// 0.58-0.65 are available but not surfaced by default.
const COLD_RECALL_THRESHOLD: f32 = 0.65;

/// Confidence tiers for context injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidenceTier {
    /// High relevance, high confidence — injected directly.
    /// "You decided to use PostgreSQL for this project."
    Loud,
    /// High relevance, medium confidence — present but not emphasized.
    /// "In a previous session, you explored caching strategies."
    Clear,
    /// Medium relevance, lower confidence — subtle, model decides.
    /// "There may be a relevant pattern from your work on project X."
    Whisper,
}

impl ConfidenceTier {
    /// Determine tier from relevance score.
    pub fn from_score(score: f32) -> Self {
        if score >= 0.85 {
            Self::Loud
        } else if score >= 0.72 {
            Self::Clear
        } else {
            Self::Whisper
        }
    }
}

/// A single entry in the hot cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotCacheEntry {
    /// Database memory ID for traceability
    pub memory_id: i64,
    /// The memory content at full fidelity
    pub content: String,
    /// When this entry was promoted to hot
    pub promoted_at: DateTime<Utc>,
    /// Last session this was relevant in
    pub last_relevant_session: String,
    /// Current relevance score (updated on re-scoring)
    pub relevance_score: f32,
    /// Number of sessions this has been continuously hot
    pub hot_streak: u32,
    /// Whether this is pinned (never auto-demoted)
    pub pinned: bool,
    /// The confidence tier for rendering
    pub tier: ConfidenceTier,
    /// Source cognitive level
    pub cognitive_level: String,
}

/// The hot cache state persisted to `.nexus/cache/hot.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotCache {
    pub entries: Vec<HotCacheEntry>,
    pub last_updated: DateTime<Utc>,
    pub last_session_id: Option<String>,
}

/// Index entry for cold cache (lightweight — actual content in SQLite).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdIndexEntry {
    pub memory_id: i64,
    /// Pre-computed relevance to this project's identity
    pub project_relevance: f32,
    /// Last time this was surfaced to hot
    pub last_surfaced: Option<DateTime<Utc>>,
}

/// The cold cache index persisted to `.nexus/cache/cold_index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdCacheIndex {
    pub entries: Vec<ColdIndexEntry>,
    pub last_reindexed: DateTime<Utc>,
}
```

#### 3.3.3 Context Builder

The context builder assembles `context.md` from hot cache, cold recalls, and relevance scoring:

```rust
// crates/nexus-agent/src/context_builder.rs

use crate::cognitive_cache::*;
use crate::soul;

/// Build the complete context.md content from cache state.
///
/// STRUCTURE of output context.md:
/// 1. Soul reference (always included)
/// 2. Hot cache entries (loud tier first, then clear, then whisper)
/// 3. Cold recalls from morning recall (if any)
///
/// TOKEN BUDGET: The builder respects `max_context_tokens` and
/// compresses lower-tier entries when budget is tight.
pub fn build_context_md(
    hot_cache: &HotCache,
    cold_recalls: &[ColdRecall],
    max_context_tokens: usize,
) -> String {
    let mut sections = Vec::new();

    // Section 1: Hot cache — grouped by tier
    let loud: Vec<_> = hot_cache.entries.iter()
        .filter(|e| e.tier == ConfidenceTier::Loud)
        .collect();
    let clear: Vec<_> = hot_cache.entries.iter()
        .filter(|e| e.tier == ConfidenceTier::Clear)
        .collect();
    let whisper: Vec<_> = hot_cache.entries.iter()
        .filter(|e| e.tier == ConfidenceTier::Whisper)
        .collect();

    if !loud.is_empty() {
        sections.push(format_tier_section(
            "Key Context",
            &loud,
            CompressionLevel::None,
        ));
    }

    if !clear.is_empty() {
        sections.push(format_tier_section(
            "Recent Learnings",
            &clear,
            CompressionLevel::Light,
        ));
    }

    // Budget check: only include whispers if we have room
    let current_tokens = estimate_tokens(&sections.join("\n"));
    if current_tokens < max_context_tokens * 80 / 100 && !whisper.is_empty() {
        sections.push(format_tier_section(
            "Related Notes",
            &whisper,
            CompressionLevel::Heavy,
        ));
    }

    // Section 2: Cold recalls (morning recall results)
    if !cold_recalls.is_empty() {
        let current_tokens = estimate_tokens(&sections.join("\n"));
        let remaining = max_context_tokens.saturating_sub(current_tokens);
        if remaining > 200 {
            sections.push(format_cold_recalls(cold_recalls, remaining));
        }
    }

    sections.join("\n\n")
}

/// Compression levels for token budget management.
#[derive(Debug, Clone, Copy)]
enum CompressionLevel {
    /// Full content, no compression
    None,
    /// Truncate to first sentence
    Light,
    /// Single-line summary
    Heavy,
}

/// ENGINEERING JUDGEMENT: Token estimation uses the 4-chars-per-token
/// heuristic. This is intentionally conservative (real tokenizers give
/// ~3.5 chars/token for English) to avoid overflowing context windows.
fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}
```

#### 3.3.4 Morning Recall

At session start, the system performs a cold-cache scan to surface foundational project knowledge:

```rust
// In crates/nexus-agent/src/cognitive_cache.rs

impl CognitiveCache {
    /// Morning recall: scan cold cache for relevant memories at session start.
    ///
    /// This runs ONCE at session start (triggered by hooks detecting a new session).
    /// It surfaces memories from cold storage that are relevant to the current
    /// project but not currently in the hot cache.
    ///
    /// GUARD RAIL: This must complete in under 500ms to avoid delaying session start.
    /// If embedding service is unavailable, fall back to text-based search.
    pub async fn morning_recall(
        &self,
        project: &ProjectIdentity,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        embedder: Option<&dyn EmbeddingService>,
    ) -> Vec<ColdRecall> {
        let started = std::time::Instant::now();

        // Build query from project identity
        let query = format!(
            "{} {} project context",
            project.display_name,
            project.git_remote.as_deref().unwrap_or("")
        );

        // Get hot cache IDs to exclude (already loaded)
        let hot_ids: HashSet<i64> = self.hot_cache.entries
            .iter()
            .map(|e| e.memory_id)
            .collect();

        // Semantic search against cold cache
        let candidates = if let Some(embedder) = embedder {
            // Preferred: embedding-based semantic search
            self.semantic_cold_recall(&query, namespace_id, memory_repo, embedder).await
        } else {
            // Fallback: text-based search
            self.text_cold_recall(&query, namespace_id, memory_repo).await
        };

        // Filter out already-hot entries and apply threshold
        let recalls: Vec<ColdRecall> = candidates
            .into_iter()
            .filter(|c| !hot_ids.contains(&c.memory_id))
            .filter(|c| c.relevance_score >= COLD_RECALL_THRESHOLD)
            .take(10)  // Max 10 cold recalls per session
            .collect();

        let elapsed = started.elapsed();
        if elapsed.as_millis() > 500 {
            warn!(
                elapsed_ms = elapsed.as_millis(),
                "Morning recall exceeded 500ms budget"
            );
        }

        debug!(
            recalls = recalls.len(),
            elapsed_ms = elapsed.as_millis(),
            "Morning recall complete"
        );

        recalls
    }
}
```

---

### 3.4 Context Injection System

**Purpose**: Automatically inject soul.md and context.md into agent sessions via file references and hooks.

**Modified crate**: `nexus-hooks`

#### 3.4.1 Agent Config Registry

```rust
// crates/nexus-hooks/src/injection.rs

use std::path::{Path, PathBuf};

/// How each agent discovers its context files.
#[derive(Debug, Clone)]
pub struct AgentInjectionTarget {
    /// Agent identifier
    pub agent_type: String,
    /// Global config file where soul.md reference is injected.
    /// Example: ~/.claude/CLAUDE.md
    pub global_config: PathBuf,
    /// Per-project config file where context.md reference is injected.
    /// Example: ./CLAUDE.md (relative to project root)
    pub project_config_filename: String,
    /// The include/reference syntax this agent understands.
    /// Most agents auto-read referenced files when they see a path.
    pub reference_format: ReferenceFormat,
}

/// How to reference an external file in the agent's config.
#[derive(Debug, Clone)]
pub enum ReferenceFormat {
    /// Markdown comment block with file path.
    /// Used by: Claude Code, Amp, Codex
    /// ```
    /// <!-- nexus:soul ~/.config/nexus/soul.md -->
    /// <!-- nexus:context .nexus/context.md -->
    /// ```
    MarkdownComment,
    /// Direct file include/import.
    /// Used by: agents that support @import
    DirectInclude,
}

/// GUARD RAIL: Known agent configurations.
/// Adding a new agent requires ONLY adding an entry here.
/// No other code changes needed — the injection logic is generic.
pub fn known_agents() -> Vec<AgentInjectionTarget> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    vec![
        AgentInjectionTarget {
            agent_type: "claude-code".to_string(),
            global_config: home.join(".claude").join("CLAUDE.md"),
            project_config_filename: "CLAUDE.md".to_string(),
            reference_format: ReferenceFormat::MarkdownComment,
        },
        AgentInjectionTarget {
            agent_type: "amp".to_string(),
            global_config: home.join(".config").join("amp").join("AGENTS.md"),
            project_config_filename: "AGENTS.md".to_string(),
            reference_format: ReferenceFormat::MarkdownComment,
        },
        AgentInjectionTarget {
            agent_type: "codex".to_string(),
            global_config: home.join(".config").join("codex").join("AGENTS.md"),
            project_config_filename: "AGENTS.md".to_string(),
            reference_format: ReferenceFormat::MarkdownComment,
        },
        AgentInjectionTarget {
            agent_type: "gemini".to_string(),
            global_config: home.join(".gemini").join("GEMINI.md"),
            project_config_filename: "GEMINI.md".to_string(),
            reference_format: ReferenceFormat::MarkdownComment,
        },
    ]
}
```

#### 3.4.2 Reference Injection Logic

```rust
// crates/nexus-hooks/src/injection.rs (continued)

/// Sentinel markers for Nexus-managed content blocks.
const NEXUS_BLOCK_START: &str = "<!-- NEXUS:START -->";
const NEXUS_BLOCK_END: &str = "<!-- NEXUS:END -->";

/// Inject or update the Nexus reference block in an agent config file.
///
/// GUARD RAIL: This function is IDEMPOTENT. Calling it multiple times
/// produces the same result. It never duplicates the block.
///
/// GUARD RAIL: This function NEVER modifies content outside the
/// NEXUS:START/NEXUS:END markers. User's own config is preserved.
pub fn inject_reference(
    config_file: &Path,
    soul_path: &Path,
    context_path: &Path,
) -> std::io::Result<()> {
    let nexus_block = format!(
        "{}\n\
         # Nexus Memory Context\n\
         \n\
         The following files contain context from the Nexus Memory System.\n\
         Read them for project-specific context and cross-project learnings.\n\
         \n\
         - Global identity & learnings: {}\n\
         - Project-specific context: {}\n\
         {}\n",
        NEXUS_BLOCK_START,
        soul_path.display(),
        context_path.display(),
        NEXUS_BLOCK_END,
    );

    let existing = std::fs::read_to_string(config_file).unwrap_or_default();

    if let (Some(start), Some(end)) = (
        existing.find(NEXUS_BLOCK_START),
        existing.find(NEXUS_BLOCK_END),
    ) {
        // Replace existing block
        let end = end + NEXUS_BLOCK_END.len();
        let mut updated = String::with_capacity(existing.len());
        updated.push_str(&existing[..start]);
        updated.push_str(&nexus_block);
        if end < existing.len() {
            updated.push_str(&existing[end..]);
        }
        std::fs::write(config_file, updated)?;
    } else {
        // Append block to end of file
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push('\n');
        content.push_str(&nexus_block);
        std::fs::write(config_file, content)?;
    }

    Ok(())
}
```

#### 3.4.3 Hook Session Lifecycle Integration

```rust
// Additions to crates/nexus-hooks/src/base.rs or per-agent hooks

/// Called by hooks when a new session is detected.
/// This is the main entry point for the injection pipeline.
///
/// EXECUTION ORDER (blocking, before agent reads config):
/// 1. Resolve project identity
/// 2. Ensure .nexus/ directory exists
/// 3. Run morning recall (cold → hot promotion)
/// 4. Build context.md from hot cache + recalls + soul reference
/// 5. Inject references into agent config files
pub async fn on_session_start(
    cwd: &Path,
    agent_type: &str,
    session_id: &str,
) -> Result<(), HookError> {
    let project = ProjectIdentity::resolve(cwd);

    // Ensure .nexus/ directory structure
    let nexus_dir = cwd.join(".nexus");
    std::fs::create_dir_all(nexus_dir.join("cache"))?;
    std::fs::create_dir_all(nexus_dir.join("sessions"))?;

    // Load or initialize cognitive cache
    let mut cache = CognitiveCache::load_or_init(&nexus_dir);

    // Morning recall: surface cold cache memories
    let recalls = cache.morning_recall(
        &project, namespace_id, &memory_repo, embedder.as_deref()
    ).await;

    // Build context.md
    let context_content = build_context_md(
        &cache.hot_cache,
        &recalls,
        max_context_tokens_for_agent(agent_type),
    );
    std::fs::write(nexus_dir.join("context.md"), &context_content)?;

    // Inject references into agent config
    let soul = soul::soul_path();
    let context = nexus_dir.join("context.md");
    if let Some(target) = find_injection_target(agent_type) {
        inject_reference(&target.project_config(cwd), &soul, &context)?;

        // Also ensure global config has soul reference
        if !target.global_config.exists()
            || !std::fs::read_to_string(&target.global_config)
                .unwrap_or_default()
                .contains(NEXUS_BLOCK_START)
        {
            inject_reference(&target.global_config, &soul, &context)?;
        }
    }

    // Create session scratch file
    let scratch = nexus_dir.join("sessions").join(format!("{}.md", session_id));
    std::fs::write(&scratch, format!(
        "# Session: {}\n# Started: {}\n# Agent: {}\n\n",
        session_id,
        chrono::Utc::now().to_rfc3339(),
        agent_type,
    ))?;

    info!(
        project = %project.display_name,
        agent = agent_type,
        hot_entries = cache.hot_cache.entries.len(),
        cold_recalls = recalls.len(),
        "Session injection complete"
    );

    Ok(())
}
```

**GUARD RAILS**:
- `on_session_start` must complete in under 2 seconds total. If any step exceeds its budget, log a warning and skip (but never block the agent from starting)
- Morning recall: 500ms budget
- Context build: 100ms budget
- File writes: 200ms budget
- Reference injection: 100ms budget
- If any file write fails, log error but do NOT prevent the agent session from starting
- Never inject into a config file that doesn't already exist (don't create CLAUDE.md if the user doesn't have one — only inject into existing files)

---

### 3.5 Dream Cycle Automation

**Purpose**: Three-tier automatic dream cycles that consolidate, derive, and refine memories without user intervention.

**Modified modules**: `crates/nexus-agent/src/dream_cycle.rs`, `crates/nexus-agent/src/supervisor.rs`

**New module**: `crates/nexus-agent/src/activity_monitor.rs`

#### 3.5.1 Activity Monitor — Sleep Detection

```rust
// crates/nexus-agent/src/activity_monitor.rs

use chrono::{DateTime, Utc, Duration as ChronoDuration};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Tracks user activity patterns to determine "overnight" timing.
///
/// ENGINEERING JUDGEMENT: We use a simple rolling window of activity
/// timestamps. "Sleep" is detected as the longest gap in activity
/// within a 24-hour period. This naturally adapts to users who work
/// at unconventional hours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityMonitor {
    /// Recent activity timestamps (last 7 days, sampled hourly)
    pub activity_log: Vec<DateTime<Utc>>,
    /// Detected typical inactivity start hour (0-23, in local time)
    pub detected_sleep_hour: Option<u8>,
    /// Last deep dream execution
    pub last_deep_dream: Option<DateTime<Utc>>,
    /// Minimum 24 hours between deep dreams
    pub deep_dream_cooldown: ChronoDuration,
}

impl ActivityMonitor {
    pub fn new() -> Self {
        Self {
            activity_log: Vec::new(),
            detected_sleep_hour: None,
            last_deep_dream: None,
            deep_dream_cooldown: ChronoDuration::hours(24),
        }
    }

    /// Record that the user was active at this moment.
    pub fn record_activity(&mut self) {
        let now = Utc::now();
        // Sample at most once per 10 minutes to avoid log bloat
        if self.activity_log.last()
            .map(|last| now - *last > ChronoDuration::minutes(10))
            .unwrap_or(true)
        {
            self.activity_log.push(now);
        }
        // Keep only last 7 days
        let cutoff = now - ChronoDuration::days(7);
        self.activity_log.retain(|t| *t > cutoff);
    }

    /// Check if it's time for a deep dream.
    ///
    /// Returns true when:
    /// 1. At least 24 hours since last deep dream
    /// 2. User has been inactive for at least 30 minutes
    /// 3. Current time falls within detected sleep window (or no pattern yet)
    pub fn should_deep_dream(&self) -> bool {
        let now = Utc::now();

        // Cooldown check
        if let Some(last) = self.last_deep_dream {
            if now - last < self.deep_dream_cooldown {
                return false;
            }
        }

        // Inactivity check: last activity > 30 min ago
        let inactive_duration = self.activity_log.last()
            .map(|last| now - *last)
            .unwrap_or(ChronoDuration::hours(24));

        if inactive_duration < ChronoDuration::minutes(30) {
            return false;
        }

        true
    }

    /// Persistence path for the activity monitor state.
    pub fn state_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("nexus-memory-system")
            .join("activity_monitor.json")
    }

    /// Save state to disk.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load state from disk.
    pub fn load() -> Self {
        let path = Self::state_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(Self::new)
    }
}
```

#### 3.5.2 Three-Tier Dream Orchestration

```rust
// Additions to crates/nexus-agent/src/dream_cycle.rs

/// Nap: lightweight consolidation at session end.
///
/// MUST complete within `session_end_dream_timeout_secs` (default 8s).
/// If it can't finish, write remaining work to the cognition job queue.
///
/// Actions:
/// 1. Process raw → derived for this session's memories
/// 2. Merge session scratch into hot cache
/// 3. Quick relevance re-score of hot cache
/// 4. Update .nexus/context.md
pub async fn run_nap(
    session_id: &str,
    project_root: &Path,
    namespace_id: i64,
    memory_repo: &MemoryRepository,
    llm: &dyn LlmClient,
    timeout: std::time::Duration,
) -> Result<NapResult, AgentError> {
    // ... implementation leveraging existing dream_cycle infrastructure
    todo!()
}

/// Dream: medium consolidation triggered by memory threshold.
///
/// Actions:
/// 1. Everything in Nap
/// 2. Promote important derived → distilled
/// 3. Discover cross-memory connections (existing reflect pipeline)
/// 4. Re-rank hot/cold boundaries
/// 5. Update .nexus/context.md with new context
pub async fn run_dream(
    project_root: &Path,
    namespace_id: i64,
    memory_repo: &MemoryRepository,
    relation_repo: &MemoryRelationRepository<'_>,
    llm: &dyn LlmClient,
    embedder: Option<&dyn EmbeddingService>,
) -> Result<DreamResult, AgentError> {
    // ... implementation leveraging existing dream_cycle + reflect pipelines
    todo!()
}

/// Deep dream: full consolidation during user inactivity.
///
/// Actions:
/// 1. Everything in Dream
/// 2. Cross-project synthesis (find patterns across all projects)
/// 3. Soul.md rebuild via normalization gate + LLM evaluation
/// 4. Memory pruning (remove low-value, old, unaccessed memories)
/// 5. Re-index cold cache relevance graph for all projects
/// 6. Update activity monitor with detected sleep patterns
pub async fn run_deep_dream(
    memory_repo: &MemoryRepository,
    relation_repo: &MemoryRelationRepository<'_>,
    llm: &dyn LlmClient,
    embedder: Option<&dyn EmbeddingService>,
    soul_builder: &SoulBuilder,
    activity_monitor: &mut ActivityMonitor,
) -> Result<DeepDreamResult, AgentError> {
    // ... implementation
    todo!()
}
```

#### 3.5.3 Dream Trigger Configuration

```rust
// Additions to crates/nexus-core/src/config.rs

/// Configuration for autonomous dream cycle triggers.
/// Added to CognitionConfig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamTriggerConfig {
    /// Enable nap at session end (default: true)
    pub nap_on_session_end: bool,
    /// Idle timeout in seconds before triggering nap (default: 600 = 10 min)
    pub nap_idle_timeout_secs: u64,
    /// Unprocessed memory threshold to trigger a dream (default: 20)
    pub dream_memory_threshold: usize,
    /// Minimum hours between deep dreams (default: 24)
    pub deep_dream_cooldown_hours: u64,
    /// Minimum inactivity minutes before deep dream can start (default: 30)
    pub deep_dream_inactivity_mins: u64,
}

impl Default for DreamTriggerConfig {
    fn default() -> Self {
        Self {
            nap_on_session_end: true,
            nap_idle_timeout_secs: 600,
            dream_memory_threshold: 20,
            deep_dream_cooldown_hours: 24,
            deep_dream_inactivity_mins: 30,
        }
    }
}
```

**GUARD RAILS**:
- Nap MUST respect the existing `session_end_dream_timeout_secs` (default 8s). If it can't finish, enqueue remaining work.
- Dream cycles MUST NOT run during an active session. The activity monitor prevents this.
- Deep dream MUST back up soul.md before modifying it.
- All dream tiers MUST use `create_client_auto_with_fallback()` for LLM calls — model failover is automatic.
- If the LLM is unreachable during any dream tier, log the failure, skip LLM-dependent steps, and complete what can be done locally (cache updates, file writes, re-scoring).

---

### 3.6 Mid-Session Re-Scoring

**Purpose**: Continuously update the project context as the session's topic drifts, using embedding-based relevance detection.

**New module**: `crates/nexus-hooks/src/rescorer.rs`

```rust
// crates/nexus-hooks/src/rescorer.rs

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Debounced re-scorer that updates context.md when topic drift is detected.
///
/// ENGINEERING JUDGEMENT: We use turn counting (not time-based) as the
/// debounce mechanism. This ensures the system only re-scores after
/// meaningful conversation progress, not during idle periods.
pub struct SessionRescorer {
    /// Turns since last re-score
    turns_since_rescore: AtomicU32,
    /// Minimum turns between re-scores
    rescore_interval: u32,
    /// Embedding of the current hot cache topics (for drift detection)
    current_topic_embedding: tokio::sync::RwLock<Option<Vec<f32>>>,
    /// Drift threshold — if cosine similarity drops below this,
    /// trigger re-score even before the turn interval.
    drift_threshold: f32,
}

impl SessionRescorer {
    /// Default: re-score every 5 turns.
    /// ENGINEERING JUDGEMENT: 5 turns ≈ 2-3 minutes of active conversation.
    /// Frequent enough to catch topic shifts, infrequent enough to avoid
    /// false positives and unnecessary I/O.
    pub fn new() -> Self {
        Self {
            turns_since_rescore: AtomicU32::new(0),
            rescore_interval: 5,
            current_topic_embedding: tokio::sync::RwLock::new(None),
            drift_threshold: 0.70,
        }
    }

    /// Called by hooks on each conversation turn.
    /// Returns true if a re-score should be triggered.
    pub async fn on_turn(&self, turn_content: &str, embedder: &dyn EmbeddingService) -> bool {
        let count = self.turns_since_rescore.fetch_add(1, Ordering::Relaxed) + 1;

        // Check turn-based threshold
        if count >= self.rescore_interval {
            self.turns_since_rescore.store(0, Ordering::Relaxed);
            return true;
        }

        // Check drift-based threshold (only if we have a baseline)
        let current = self.current_topic_embedding.read().await;
        if let Some(ref baseline) = *current {
            if let Ok(turn_embedding) = embedder.encode(turn_content).await {
                let similarity = cosine_similarity(baseline, &turn_embedding);
                if similarity < self.drift_threshold {
                    self.turns_since_rescore.store(0, Ordering::Relaxed);
                    return true;
                }
            }
        }

        false
    }

    /// Perform the actual re-score: re-rank hot cache entries,
    /// optionally promote cold entries, rebuild context.md.
    pub async fn rescore(
        &self,
        project_root: &Path,
        recent_turns: &[String],
        cache: &mut CognitiveCache,
        memory_repo: &MemoryRepository,
        embedder: &dyn EmbeddingService,
    ) -> Result<(), AgentError> {
        // 1. Build embedding from recent turns
        let combined = recent_turns.join(" ");
        let topic_embedding = embedder.encode(&combined).await?;

        // 2. Re-score hot cache entries against new topic
        for entry in &mut cache.hot_cache.entries {
            if let Ok(mem_embedding) = embedder.encode(&entry.content).await {
                entry.relevance_score = cosine_similarity(&topic_embedding, &mem_embedding);
                entry.tier = ConfidenceTier::from_score(entry.relevance_score);
            }
        }

        // 3. Check if any cold entries should be promoted
        // (lightweight: only check top 5 from cold index)
        let cold_candidates = cache.cold_index.entries.iter()
            .take(5)
            .collect::<Vec<_>>();
        // ... promote if relevance > hot cache minimum

        // 4. Update topic embedding baseline
        *self.current_topic_embedding.write().await = Some(topic_embedding);

        // 5. Rebuild context.md
        let context = build_context_md(
            &cache.hot_cache, &[], // No morning recall during re-score
            max_context_tokens,
        );
        std::fs::write(
            project_root.join(".nexus").join("context.md"),
            context,
        )?;

        Ok(())
    }
}

/// Standard cosine similarity between two embedding vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
```

**GUARD RAILS**:
- Re-scoring MUST NOT use LLM calls — embeddings only (local ONNX, free, fast)
- If the embedding service is unavailable, fall back to turn-count-only debouncing (no drift detection)
- Context.md writes during re-score must use atomic write (write to `.nexus/context.md.tmp` then rename) to prevent agents from reading partial files
- The re-scorer holds no locks on the database — it only reads

---

### 3.7 Concurrent Session Management

**Purpose**: Handle multiple agents working on the same project simultaneously.

Each session writes to its own scratch file. The shared `context.md` is only updated during naps (session end) or scheduled re-scores.

```rust
// crates/nexus-agent/src/session_manager.rs

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Manage session scratch files for concurrent session isolation.
pub struct SessionManager {
    nexus_dir: PathBuf,
}

impl SessionManager {
    pub fn new(project_root: &Path) -> Self {
        Self {
            nexus_dir: project_root.join(".nexus"),
        }
    }

    /// Create a new session scratch file.
    pub fn start_session(&self, session_id: &str, agent_type: &str) -> std::io::Result<PathBuf> {
        let sessions_dir = self.nexus_dir.join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;

        let scratch_path = sessions_dir.join(format!("{}.md", session_id));
        let header = format!(
            "# Session Scratch\n\
             - id: {}\n\
             - agent: {}\n\
             - started: {}\n\
             - status: active\n\n\
             ## Learnings\n\n",
            session_id,
            agent_type,
            Utc::now().to_rfc3339(),
        );
        std::fs::write(&scratch_path, header)?;
        Ok(scratch_path)
    }

    /// Append a learning to the session scratch file.
    /// Called during mid-session when new insights are captured.
    pub fn append_learning(
        &self,
        session_id: &str,
        content: &str,
        confidence: f32,
    ) -> std::io::Result<()> {
        let scratch_path = self.nexus_dir
            .join("sessions")
            .join(format!("{}.md", session_id));

        let entry = format!(
            "- [confidence: {:.2}] {}\n",
            confidence, content
        );

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&scratch_path)?;
        std::io::Write::write_all(&mut file, entry.as_bytes())?;
        Ok(())
    }

    /// Merge a completed session's scratch into the hot cache.
    /// Called during nap (session end).
    ///
    /// GUARD RAIL: After successful merge, rename scratch file to
    /// `<session_id>.merged.md` (don't delete — useful for debugging).
    pub fn merge_session(
        &self,
        session_id: &str,
        hot_cache: &mut HotCache,
    ) -> Result<usize, AgentError> {
        let scratch_path = self.nexus_dir
            .join("sessions")
            .join(format!("{}.md", session_id));

        if !scratch_path.exists() {
            return Ok(0);
        }

        let content = std::fs::read_to_string(&scratch_path)
            .map_err(|e| AgentError::Storage(e.to_string()))?;

        let learnings = parse_scratch_learnings(&content);
        let merged_count = learnings.len();

        for learning in learnings {
            promote_to_hot_cache(hot_cache, learning);
        }

        // Rename to .merged.md
        let merged_path = self.nexus_dir
            .join("sessions")
            .join(format!("{}.merged.md", session_id));
        std::fs::rename(&scratch_path, &merged_path)
            .map_err(|e| AgentError::Storage(e.to_string()))?;

        Ok(merged_count)
    }

    /// Clean up old merged session files (older than 7 days).
    pub fn cleanup_old_sessions(&self) -> std::io::Result<usize> {
        let sessions_dir = self.nexus_dir.join("sessions");
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let cutoff = Utc::now() - chrono::Duration::days(7);
        let mut cleaned = 0;

        for entry in std::fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md")
                && path.to_string_lossy().contains(".merged.")
            {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        let modified: DateTime<Utc> = modified.into();
                        if modified < cutoff {
                            std::fs::remove_file(&path)?;
                            cleaned += 1;
                        }
                    }
                }
            }
        }

        Ok(cleaned)
    }
}
```

---

### 3.8 Token Budget Management

```rust
// crates/nexus-agent/src/token_budget.rs

/// Token budget allocation for different agent/model combinations.
///
/// ENGINEERING JUDGEMENT: We allocate 8-12% of the model's context window
/// for Nexus memory injection. This is enough for rich context without
/// meaningfully reducing the user's working space.
pub struct TokenBudget {
    /// Total context window size for the model
    pub model_context_window: usize,
    /// Percentage of window allocated to Nexus (0.08-0.12)
    pub nexus_allocation_pct: f32,
    /// Calculated max tokens for soul.md
    pub soul_budget: usize,
    /// Calculated max tokens for context.md
    pub context_budget: usize,
}

impl TokenBudget {
    /// Create a budget for a known model context window.
    ///
    /// Split: soul gets 30% of Nexus budget, context gets 70%.
    /// RATIONALE: Soul is a compact identity doc. Project context
    /// needs more room for hot cache + recalls.
    pub fn for_model(context_window: usize) -> Self {
        let nexus_pct = 0.10; // 10% default
        let total_nexus = (context_window as f32 * nexus_pct) as usize;
        let soul_budget = total_nexus * 30 / 100;
        let context_budget = total_nexus * 70 / 100;

        Self {
            model_context_window: context_window,
            nexus_allocation_pct: nexus_pct,
            soul_budget,
            context_budget,
        }
    }

    /// Known model context window sizes.
    /// GUARD RAIL: When model is unknown, default to 128K (conservative).
    pub fn estimate_window(agent_type: &str) -> usize {
        match agent_type {
            "claude-code" => 200_000,
            "amp" => 200_000,
            "codex" => 200_000,
            "gemini" => 1_000_000,
            _ => 128_000, // Conservative default
        }
    }
}
```

---

### 3.9 Bootstrap & Configuration

```rust
// Additions to crates/nexus-core/src/config.rs

/// Configuration for the autonomous cognitive system.
/// Added as a field on `Config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveSystemConfig {
    /// Enable the autonomous cognitive system (default: true)
    pub enabled: bool,

    /// Bootstrap mode for new projects
    /// "silent" = empty context, learn from scratch (default)
    /// "scan" = scan project files to seed initial context
    pub bootstrap_mode: String,

    /// Dream trigger configuration
    pub dream_triggers: DreamTriggerConfig,

    /// Maximum hot cache entries per project
    pub hot_cache_max_entries: usize,

    /// Nexus allocation percentage of context window (0.0-1.0)
    pub context_allocation_pct: f32,

    /// Enable mid-session re-scoring via embeddings
    pub mid_session_rescore_enabled: bool,

    /// Turns between re-scores
    pub rescore_turn_interval: u32,

    /// Drift threshold for early re-score trigger
    pub rescore_drift_threshold: f32,
}

impl Default for CognitiveSystemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bootstrap_mode: "silent".to_string(),
            dream_triggers: DreamTriggerConfig::default(),
            hot_cache_max_entries: 20,
            context_allocation_pct: 0.10,
            mid_session_rescore_enabled: true,
            rescore_turn_interval: 5,
            rescore_drift_threshold: 0.70,
        }
    }
}
```

**Environment variables**:
```bash
NEXUS_COGNITIVE_ENABLED=true
NEXUS_BOOTSTRAP_MODE=silent  # or "scan"
NEXUS_HOT_CACHE_MAX=20
NEXUS_CONTEXT_ALLOCATION_PCT=0.10
NEXUS_RESCORE_ENABLED=true
NEXUS_RESCORE_TURN_INTERVAL=5
NEXUS_DREAM_THRESHOLD=20
NEXUS_DEEP_DREAM_COOLDOWN_HOURS=24
```

---

## 4. Guard Rails & Engineering Judgement Calls

### 4.1 Performance Budgets

| Operation | Budget | Fallback if exceeded |
|-----------|--------|---------------------|
| `on_session_start` total | 2 seconds | Log warning, skip slow steps, never block agent |
| Morning recall | 500ms | Skip semantic search, use text-based |
| Context.md build | 100ms | Use cached version |
| Mid-session re-score | 200ms | Skip this turn, try next interval |
| Nap (session end) | 8 seconds (configurable) | Enqueue remaining work to job queue |
| Dream (threshold) | 60 seconds | Partial completion acceptable |
| Deep dream | 5 minutes | Can be interrupted, will resume next cycle |

### 4.2 Failure Modes

| Failure | Response |
|---------|----------|
| LLM unreachable during dream | Skip LLM steps, do local-only work (cache updates, re-scoring) |
| LLM unreachable during soul rebuild | Keep existing soul.md unchanged |
| Embedding service unavailable | Fall back to text-based search/scoring |
| Disk write fails for context.md | Log error, agent works without context (degrades gracefully) |
| Database unreachable | No memory operations possible — log and continue. Agent works without memories. |
| Agent config file not found | Skip injection for that agent (don't create files that don't exist) |
| Concurrent write conflict | Scratch files isolate sessions — no conflict possible for context.md (only written during nap when session is ending) |

### 4.3 Security

- Soul.md and context.md should NEVER contain API keys, secrets, or credentials
- The normalization gate must explicitly filter for secret-like patterns
- Session scratch files should be user-readable only (0600 permissions on Unix)
- The `.nexus/` directory should be added to the project's `.gitignore` automatically on first creation

### 4.4 Invariants

1. **Soul.md is only modified during deep dream cycles.** No exceptions.
2. **context.md is only modified by: session start, nap, re-scorer.** Never by direct user action through Nexus.
3. **Hot cache never exceeds `hot_cache_max_entries`.** When full, lowest-relevance non-pinned entry is demoted to cold.
4. **Injection references are idempotent.** Running injection twice produces the same result.
5. **The system never creates agent config files.** It only modifies existing ones.
6. **All LLM calls use `create_client_auto_with_fallback()`.** Model failover is automatic and transparent.
7. **No dream cycle runs during an active session** (except nap at session end).

---

## 5. Blocking Implementation Plan

### Phase 1: Foundation (No LLM Required)
**Crate**: `nexus-core`
**Files**: `project_identity.rs`, additions to `config.rs`
**Depends on**: Nothing
**Estimated effort**: 1-2 sessions

**Tasks**:
1. Create `crates/nexus-core/src/project_identity.rs` with `ProjectIdentity::resolve()`
2. Add `CognitiveSystemConfig` and `DreamTriggerConfig` to `crates/nexus-core/src/config.rs`
3. Add environment variable parsing for new config fields in `Config::from_env()`
4. Add `pub mod project_identity;` to `crates/nexus-core/src/lib.rs`
5. Add `.nexus/` to `.gitignore` template
6. Unit tests for `ProjectIdentity::resolve()` (with tempdir, with/without git, with marker file)
7. Unit tests for config defaults and env var parsing

**Verification**: `cargo test -p nexus-core`

---

### Phase 2: Cognitive Cache Data Model (No LLM Required)
**Crate**: `nexus-agent`
**Files**: `cognitive_cache.rs`, `token_budget.rs`
**Depends on**: Phase 1

**Tasks**:
1. Create `crates/nexus-agent/src/cognitive_cache.rs` — `HotCache`, `HotCacheEntry`, `ColdCacheIndex`, `ColdIndexEntry`, `ConfidenceTier`, `CognitiveCache`
2. Implement `CognitiveCache::load_or_init()` — reads from `.nexus/cache/hot.json` or creates empty
3. Implement `CognitiveCache::save()` — writes hot.json and cold_index.json
4. Create `crates/nexus-agent/src/token_budget.rs` — `TokenBudget`
5. Create `crates/nexus-agent/src/context_builder.rs` — `build_context_md()` with tier formatting and compression
6. Add modules to `crates/nexus-agent/src/lib.rs`
7. Unit tests for hot cache CRUD, tier scoring, context building with different budgets
8. Unit tests for token estimation accuracy

**Verification**: `cargo test -p nexus-memory-agent`

---

### Phase 3: Session Manager & Scratch Files (No LLM Required)
**Crate**: `nexus-agent`
**Files**: `session_manager.rs`
**Depends on**: Phase 2

**Tasks**:
1. Create `crates/nexus-agent/src/session_manager.rs` — full implementation per §3.7
2. Implement scratch file creation, append, merge, cleanup
3. Implement `parse_scratch_learnings()` — parse markdown scratch format
4. Implement `promote_to_hot_cache()` — add learning to hot cache, evict if full
5. Integration test: create session → append learnings → merge → verify hot cache
6. Test concurrent session isolation (two sessions writing to different scratch files)

**Verification**: `cargo test -p nexus-memory-agent`

---

### Phase 4: Context Injection System (No LLM Required)
**Crate**: `nexus-hooks`
**Files**: `injection.rs`, modifications to per-agent hooks
**Depends on**: Phase 2, Phase 3

**Tasks**:
1. Create `crates/nexus-hooks/src/injection.rs` — `AgentInjectionTarget`, `known_agents()`, `inject_reference()`
2. Implement `inject_reference()` with NEXUS:START/NEXUS:END block management
3. Add `on_session_start()` integration to `crates/nexus-hooks/src/base.rs`
4. Modify `ClaudeCodeHook` to call `on_session_start()` during session detection
5. Modify other agent hooks similarly (Amp, Codex, Gemini)
6. Create initial soul.md template at `~/.config/nexus/soul.md` if it doesn't exist (empty with section headers only)
7. Unit tests for reference injection (idempotency, existing content preservation)
8. Integration test: simulate session start → verify .nexus/ structure created, context.md written, reference injected

**Verification**: `cargo test -p nexus-hooks`, `cargo test -p nexus-cli`

---

### Phase 5: Morning Recall & Relevance Scoring (Embeddings Required)
**Crate**: `nexus-agent`
**Files**: additions to `cognitive_cache.rs`, `context_builder.rs`
**Depends on**: Phase 4

**Tasks**:
1. Implement `CognitiveCache::morning_recall()` per §3.3.4
2. Wire embedding service into morning recall (use existing `RepresentationService::resolve()` pattern for lazy init)
3. Implement cold-cache semantic search (reuse existing `SemanticSearch` from `nexus-vectors`)
4. Implement text-based fallback search when embeddings unavailable
5. Wire morning recall into `on_session_start()` pipeline
6. Integration test with mock embeddings: verify cold → hot promotion on relevance match
7. Performance test: morning recall must complete within 500ms

**Verification**: `cargo test -p nexus-memory-agent`

---

### Phase 6: Mid-Session Re-Scorer (Embeddings Required)
**Crate**: `nexus-hooks`
**Files**: `rescorer.rs`, modifications to hook event processing
**Depends on**: Phase 5

**Tasks**:
1. Create `crates/nexus-hooks/src/rescorer.rs` — full implementation per §3.6
2. Implement `cosine_similarity()` (standalone, no external dependency)
3. Wire re-scorer into hook event processing — call `on_turn()` for each assistant/user message
4. Implement atomic context.md writes (write to .tmp, rename)
5. Add `SessionRescorer` to the hook's session state
6. Integration test: simulate 5 turns → verify re-score triggers → verify context.md updated
7. Test drift detection: simulate topic change → verify early re-score

**Verification**: `cargo test -p nexus-hooks`

---

### Phase 7: Nap & Dream Automation (LLM Required)
**Crate**: `nexus-agent`
**Files**: modifications to `dream_cycle.rs`, `supervisor.rs`
**Depends on**: Phase 3, Phase 5

**Tasks**:
1. Implement `run_nap()` per §3.5.2 — leverage existing dream cycle infrastructure
2. Wire nap into session-end hooks (modify `crates/nexus-hooks/src/base.rs` session_end callback)
3. Implement dream threshold monitoring in supervisor — count unprocessed memories, trigger `run_dream()` at threshold
4. Wire `run_dream()` into existing supervisor consolidation loop
5. Implement `ActivityMonitor` per §3.5.1
6. Wire activity monitor recording into all hook event handlers
7. Integration test: simulate session end → verify nap runs → verify hot cache updated
8. Test threshold trigger: add memories → verify dream triggers at threshold

**Verification**: `cargo test -p nexus-memory-agent`, `cargo test -p nexus-hooks`

---

### Phase 8: Soul.md Pipeline (LLM Required)
**Crate**: `nexus-agent`
**Files**: `soul.rs`, additions to `prompts.rs`
**Depends on**: Phase 7

**Tasks**:
1. Create `crates/nexus-agent/src/soul.rs` — full implementation per §3.2.2
2. Add `SOUL_NORMALIZATION_PROMPT` and `SOUL_EVALUATION_PROMPT` to `crates/nexus-agent/src/prompts.rs`
3. Implement `SoulBuilder::normalize_candidates()` — LLM call with structured JSON output
4. Implement `SoulBuilder::evaluate_and_merge()` — LLM call producing markdown
5. Implement soul candidate gathering — query across all namespaces for cross-project patterns
6. Wire soul rebuild into `run_deep_dream()`
7. Implement soul.md backup (copy to soul.md.bak before update)
8. Add `pub mod soul;` to `crates/nexus-agent/src/lib.rs`
9. Unit test with mock LLM: verify normalization strips project details
10. Unit test with mock LLM: verify evaluation gate rejects non-durable learnings
11. Integration test: full deep dream cycle → verify soul.md created/updated

**Verification**: `cargo test -p nexus-memory-agent`

---

### Phase 9: Deep Dream & Cross-Project Synthesis (LLM Required)
**Crate**: `nexus-agent`
**Files**: modifications to `dream_cycle.rs`, `supervisor.rs`
**Depends on**: Phase 8

**Tasks**:
1. Implement `run_deep_dream()` per §3.5.2
2. Implement cross-project memory scanning (query all namespaces, group by project)
3. Implement soul candidate extraction from cross-project patterns
4. Wire deep dream trigger into supervisor using `ActivityMonitor::should_deep_dream()`
5. Implement cold cache re-indexing during deep dream
6. Implement old session cleanup during deep dream
7. Integration test: full deep dream with multiple project namespaces → verify cross-project learnings extracted
8. End-to-end test: session start → work → session end (nap) → threshold (dream) → inactivity (deep dream) → next session start (verify enriched context)

**Verification**: `cargo test --workspace`

---

### Phase 10: Polish & Hardening
**Depends on**: All previous phases

**Tasks**:
1. Add `.nexus/` to `.gitignore` auto-injection on first `.nexus/` creation
2. Add `nexus cognitive status` CLI command showing cache stats, soul status, dream history
3. Add `nexus soul show` CLI command to display current soul.md
4. Add `nexus soul edit` CLI command for manual soul adjustments
5. Performance benchmarks: session start latency, re-score latency, dream cycle duration
6. Documentation updates: ARCHITECTURE.md, README.md, HOOKS.md
7. Full workspace clippy + fmt + test pass

**Verification**: Full validation checklist:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

---

## 6. File Index — New & Modified Files

### New Files

| File | Phase | Purpose |
|------|-------|---------|
| `crates/nexus-core/src/project_identity.rs` | 1 | Project identity resolution |
| `crates/nexus-agent/src/cognitive_cache.rs` | 2 | Hot/cold cache data model |
| `crates/nexus-agent/src/token_budget.rs` | 2 | Token budget management |
| `crates/nexus-agent/src/context_builder.rs` | 2 | Context.md assembly |
| `crates/nexus-agent/src/session_manager.rs` | 3 | Session scratch file management |
| `crates/nexus-hooks/src/injection.rs` | 4 | Agent config reference injection |
| `crates/nexus-hooks/src/rescorer.rs` | 6 | Mid-session re-scoring |
| `crates/nexus-agent/src/activity_monitor.rs` | 7 | User activity / sleep detection |
| `crates/nexus-agent/src/soul.rs` | 8 | Soul.md pipeline |

### Modified Files
| File | Phase | Changes |

|------|-------|---------|
| `crates/nexus-core/src/config.rs` | 1 | Add `CognitiveSystemConfig`, `DreamTriggerConfig` |
| `crates/nexus-core/src/lib.rs` | 1 | Add `pub mod project_identity` |
| `crates/nexus-agent/src/lib.rs` | 2,3,7,8 | Add new module exports |
| `crates/nexus-agent/src/prompts.rs` | 8 | Add soul prompts |
| `crates/nexus-agent/src/dream_cycle.rs` | 7,9 | Add nap/dream/deep_dream tiers |
| `crates/nexus-agent/src/supervisor.rs` | 7,9 | Wire dream triggers, activity monitor |
| `crates/nexus-hooks/src/base.rs` | 4,6,7 | Add injection hooks, re-scorer, session lifecycle |
| `crates/nexus-hooks/src/agents/claude.rs` | 4 | Wire session_start injection |
| `crates/nexus-hooks/src/agents/*.rs` | 4 | Wire session_start injection (all agents) |
| `crates/nexus-hooks/src/lib.rs` | 4,6 | Add new module exports |

---

## 7. Glossary

| Term | Definition |
|------|-----------|
| **Soul** | The unified identity document (`soul.md`) capturing cross-project, project-agnostic learnings and personality |
| **Hot cache** | Recent, high-relevance memories at full fidelity, always loaded into context |
| **Cold cache** | Full project memory archive in SQLite, queried on-demand |
| **Morning recall** | Cold cache scan at session start to surface foundational project knowledge |
| **Nap** | Lightweight dream at session end (raw→derived, merge scratch, re-score) |
| **Dream** | Medium dream triggered by memory threshold (derive→distill, connections, re-rank) |
| **Deep dream** | Full dream during user inactivity (cross-project synthesis, soul rebuild, pruning) |
| **Whisper** | Low-confidence memory surfaced subtly — model decides whether to pursue |
| **Normalization gate** | LLM-evaluated pipeline that strips project-specific details from memories before soul entry |
| **Re-scorer** | Embedding-based system that detects topic drift and updates context mid-session |
| **Injection** | The process of writing Nexus reference blocks into agent config files |
| **Scratch file** | Per-session temporary file for capturing learnings before merge |
