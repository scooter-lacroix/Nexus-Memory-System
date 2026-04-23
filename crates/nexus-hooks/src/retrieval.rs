//! Subconscious retrieval engine — surfaces relevant memories for injection.
//!
//! Queries cognitive cache, soul.md, and embedding search to build an XML
//! context payload for hook stdout injection (UserPromptSubmit / PreToolUse).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::debug;

use nexus_core::Config;
use nexus_memory_agent::cognitive_cache::{CognitiveCache, ConfidenceTier};
use nexus_memory_agent::soul::soul_path;

use crate::sync_state::{self, SyncState};

/// Operating mode for the subconscious pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SubconsciousMode {
    /// Inject minimal guidance via stdout XML (default).
    #[default]
    Whisper,
    /// Inject full soul.md + memory blocks + active guidance.
    Full,
    /// Disable all subconscious hooks.
    Off,
}

impl SubconsciousMode {
    /// Read mode from `NEXUS_SUBCONSCIOUS_MODE` env var.
    pub fn from_env() -> Self {
        match std::env::var("NEXUS_SUBCONSCIOUS_MODE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "full" => SubconsciousMode::Full,
            "off" => SubconsciousMode::Off,
            _ => SubconsciousMode::Whisper,
        }
    }
}

/// Result of a memory retrieval operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    /// Soul.md content (truncated to budget).
    pub soul_content: Option<String>,
    /// Top relevant memories by embedding similarity.
    pub recalled: Vec<RecalledMemory>,
    /// Hot cache entries that have changed since last sync.
    pub active_guidance: Vec<String>,
    /// Metadata line for the context header.
    pub stats: RetrievalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecalledMemory {
    pub content: String,
    pub relevance: f32,
    pub tier: ConfidenceTier,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalStats {
    pub total_memories: usize,
    pub hot_cache_entries: usize,
    pub soul_md_exists: bool,
    pub soul_md_age_minutes: Option<i64>,
}

/// The retrieval engine drives subconscious memory injection.
pub struct RetrievalEngine {
    project_root: PathBuf,
    mode: SubconsciousMode,
    /// Maximum tokens for the entire injection payload.
    token_budget: usize,
}

/// Token budget allocations for different injection sections.
struct TokenBudgets {
    soul: usize,
    recall: usize,
    guidance: usize,
}

impl RetrievalEngine {
    /// Create a new retrieval engine for a project.
    pub fn new(project_root: &Path, _config: Config) -> Self {
        let mode = SubconsciousMode::from_env();
        let token_budget = match mode {
            SubconsciousMode::Whisper => 512,
            SubconsciousMode::Full => 1024,
            SubconsciousMode::Off => 0,
        };
        Self {
            project_root: project_root.to_path_buf(),
            mode,
            token_budget,
        }
    }

    /// Retrieve memories relevant to a prompt for UserPromptSubmit injection.
    pub async fn retrieve_for_prompt(
        &self,
        prompt: &str,
        sync_state: &SyncState,
    ) -> RetrievalResult {
        let stats = self.gather_stats();
        let soul_content = self.load_soul_content();
        let hot_cache = self.load_hot_cache();
        let recalled = self.search_memories(prompt, &hot_cache).await;
        let active_guidance = self.compute_guidance(&hot_cache, sync_state);

        RetrievalResult {
            soul_content,
            recalled,
            active_guidance,
            stats,
        }
    }

    /// Lightweight check for updates since last sync (PreToolUse hook).
    pub fn check_for_updates(&self, sync_state: &mut SyncState) -> Option<String> {
        let soul_content = self.load_soul_content();
        let soul_hash = sync_state::soul_content_hash(soul_content.as_deref().unwrap_or(""));
        let hot_cache = self.load_hot_cache();
        let hot_cache_ids: Vec<String> = hot_cache
            .hot_cache
            .entries
            .iter()
            .map(|e| e.memory_id.to_string())
            .collect();
        let hot_cache_hash = sync_state::hot_cache_hash(&hot_cache_ids);

        if !sync_state.has_updates(
            &soul_hash,
            hot_cache.hot_cache.entries.len(),
            &hot_cache_hash,
        ) {
            return None;
        }

        // Build a lightweight delta injection
        let mut parts = Vec::new();

        if soul_hash != sync_state.last_soul_hash {
            if let Some(soul) = soul_content {
                let truncated = truncate_to_chars(&soul, 300);
                parts.push(format!(
                    "<soul_update>\n{}\n</soul_update>",
                    escape_xml(&truncated)
                ));
            }
        }

        if hot_cache.hot_cache.entries.len() > sync_state.last_hot_cache_count {
            // Cache grew: list the newly promoted entries
            let new_count = hot_cache
                .hot_cache
                .entries
                .len()
                .saturating_sub(sync_state.last_hot_cache_count);
            let new_entries: Vec<_> = hot_cache
                .hot_cache
                .entries
                .iter()
                .rev()
                .take(new_count)
                .map(|e| {
                    let tier = match e.tier {
                        ConfidenceTier::Loud => "LOUD",
                        ConfidenceTier::Clear => "CLEAR",
                        ConfidenceTier::Whisper => "WHISPER",
                    };
                    format!(
                        "[{}] {}",
                        tier,
                        escape_xml(&truncate_to_chars(&e.content, 120))
                    )
                })
                .collect();
            parts.push(format!(
                "<cache_promotions count=\"{new_count}\">\n{}\n</cache_promotions>",
                new_entries.join("\n")
            ));
        } else if hot_cache_hash != sync_state.last_hot_cache_hash {
            // Cache content changed without net growth (eviction at capacity,
            // or count decreased). Emit a summary of the current state.
            let entries: Vec<_> = hot_cache
                .hot_cache
                .entries
                .iter()
                .rev()
                .take(5)
                .map(|e| {
                    let tier = match e.tier {
                        ConfidenceTier::Loud => "LOUD",
                        ConfidenceTier::Clear => "CLEAR",
                        ConfidenceTier::Whisper => "WHISPER",
                    };
                    format!(
                        "[{}] {}",
                        tier,
                        escape_xml(&truncate_to_chars(&e.content, 120))
                    )
                })
                .collect();
            parts.push(format!(
                "<cache_update count=\"{}\">\n{}\n</cache_update>",
                hot_cache.hot_cache.entries.len(),
                entries.join("\n")
            ));
        }

        if parts.is_empty() {
            return None;
        }

        // Advance the sync state watermark to prevent re-emission
        sync_state.advance(
            soul_hash,
            hot_cache.hot_cache.entries.len(),
            hot_cache_hash,
            None,
        );

        Some(format!(
            "<nexus_delta>\n{}\n</nexus_delta>",
            parts.join("\n")
        ))
    }

    /// Format a retrieval result as XML for stdout injection.
    pub fn format_for_stdout(&self, result: &RetrievalResult) -> String {
        if self.mode == SubconsciousMode::Off {
            return String::new();
        }

        let budgets = self.compute_budgets();
        let mut sections = Vec::new();

        // Context header
        let memory_count = result.stats.total_memories;
        let hot_count = result.stats.hot_cache_entries;
        let soul_status = if result.stats.soul_md_exists {
            "synthesized"
        } else {
            "not yet generated"
        };

        sections.push(format!(
            "<nexus_context>\n\
             Subconscious memory active. {memory_count} memories indexed, \
             {hot_count} in hot cache, soul.md {soul_status}.\n\
             </nexus_context>"
        ));

        // Soul section (full mode only, or whisper if explicitly requested)
        if self.mode == SubconsciousMode::Full {
            if let Some(ref soul) = result.soul_content {
                let truncated = truncate_to_chars(soul, budgets.soul * 4);
                sections.push(format!(
                    "<nexus_soul>\n{}\n</nexus_soul>",
                    escape_xml(&truncated)
                ));
            }
        }

        // Recall section
        if !result.recalled.is_empty() {
            let mut entries = Vec::new();
            for mem in &result.recalled {
                let tier = match mem.tier {
                    ConfidenceTier::Loud => "LOUD",
                    ConfidenceTier::Clear => "CLEAR",
                    ConfidenceTier::Whisper => "WHISPER",
                };
                let truncated = truncate_to_chars(
                    &mem.content,
                    budgets.recall * 4 / result.recalled.len().max(1),
                );
                entries.push(format!(
                    "<memory relevance=\"{:.2}\" tier=\"{tier}\" source=\"{}\">\n{}\n</memory>",
                    mem.relevance,
                    escape_xml(&mem.source),
                    escape_xml(&truncated)
                ));
            }
            sections.push(format!(
                "<nexus_recall>\n{}\n</nexus_recall>",
                entries.join("\n")
            ));
        }

        // Active guidance section
        if !result.active_guidance.is_empty() {
            let truncated_guidance: Vec<_> = result
                .active_guidance
                .iter()
                .map(|g| {
                    escape_xml(&truncate_to_chars(
                        g,
                        budgets.guidance * 4 / result.active_guidance.len().max(1),
                    ))
                })
                .collect();
            sections.push(format!(
                "<nexus_guidance>\n{}\n</nexus_guidance>",
                truncated_guidance.join("\n")
            ));
        }

        sections.join("\n\n")
    }

    /// Format the initial session-start injection.
    pub fn format_session_start(
        &self,
        hot_cache: &CognitiveCache,
        soul_content: Option<&str>,
    ) -> String {
        if self.mode == SubconsciousMode::Off {
            return String::new();
        }

        let mut parts = Vec::new();

        parts.push(format!(
            "<nexus_context>\n\
             Subconscious memory active. {} entries in hot cache.\n\
             Soul.md {}.\n\
             </nexus_context>",
            hot_cache.hot_cache.entries.len(),
            if soul_content.is_some() {
                "loaded"
            } else {
                "not yet generated"
            }
        ));

        if self.mode == SubconsciousMode::Full {
            if let Some(soul) = soul_content {
                let truncated = truncate_to_chars(soul, 400);
                parts.push(format!(
                    "<nexus_soul>\n{}\n</nexus_soul>",
                    escape_xml(&truncated)
                ));
            }

            // Show all hot cache entries in full mode
            if !hot_cache.hot_cache.entries.is_empty() {
                let entries: Vec<_> = hot_cache
                    .hot_cache
                    .entries
                    .iter()
                    .map(|e| {
                        let tier = match e.tier {
                            ConfidenceTier::Loud => "LOUD",
                            ConfidenceTier::Clear => "CLEAR",
                            ConfidenceTier::Whisper => "WHISPER",
                        };
                        format!(
                            "[{tier}] {}",
                            escape_xml(&truncate_to_chars(&e.content, 120))
                        )
                    })
                    .collect();
                parts.push(format!(
                    "<nexus_hot_cache>\n{}\n</nexus_hot_cache>",
                    entries.join("\n")
                ));
            }
        } else {
            // Whisper mode: show top 3 by relevance
            let mut sorted = hot_cache.hot_cache.entries.clone();
            sorted.sort_by(|a, b| {
                b.relevance_score
                    .partial_cmp(&a.relevance_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let top: Vec<_> = sorted
                .iter()
                .take(3)
                .map(|e| {
                    let tier = match e.tier {
                        ConfidenceTier::Loud => "LOUD",
                        ConfidenceTier::Clear => "CLEAR",
                        ConfidenceTier::Whisper => "WHISPER",
                    };
                    format!(
                        "[{tier}] {}",
                        escape_xml(&truncate_to_chars(&e.content, 80))
                    )
                })
                .collect();
            if !top.is_empty() {
                parts.push(format!(
                    "<nexus_whisper>\n{}\n</nexus_whisper>",
                    top.join("\n")
                ));
            }
        }

        parts.join("\n\n")
    }

    // ── Internal helpers ──────────────────────────────────────────────

    fn gather_stats(&self) -> RetrievalStats {
        let hot_cache = self.load_hot_cache();
        let soul_path = soul_path();

        let (soul_md_exists, soul_md_age_minutes) = if soul_path.exists() {
            let age = std::fs::metadata(&soul_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|modified| {
                    let modified: chrono::DateTime<chrono::Local> = modified.into();
                    chrono::Utc::now()
                        .signed_duration_since(modified.with_timezone(&chrono::Utc))
                        .num_minutes()
                });
            (true, age)
        } else {
            (false, None)
        };

        // For total_memories, we'd need a DB query; approximate from hot+cold cache
        let total = hot_cache.hot_cache.entries.len() + hot_cache.cold_index.entries.len();

        RetrievalStats {
            total_memories: total,
            hot_cache_entries: hot_cache.hot_cache.entries.len(),
            soul_md_exists,
            soul_md_age_minutes,
        }
    }

    pub fn load_soul_content(&self) -> Option<String> {
        let path = soul_path();
        if !path.exists() {
            return None;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    None
                } else {
                    Some(content)
                }
            }
            Err(e) => {
                debug!("Failed to read soul.md: {e}");
                None
            }
        }
    }

    fn load_hot_cache(&self) -> CognitiveCache {
        let nexus_dir = self.project_root.join(".nexus");
        CognitiveCache::load_or_init(&nexus_dir)
    }

    /// Hot-cache-only search. Prompt-based semantic retrieval is wired in the CLI
    /// layer (`subconscious.rs`) which has async DB access.
    async fn search_memories(
        &self,
        _prompt: &str,
        hot_cache: &CognitiveCache,
    ) -> Vec<RecalledMemory> {
        let mut entries: Vec<_> = hot_cache
            .hot_cache
            .entries
            .iter()
            .filter(|e| e.relevance_score >= 0.5)
            .map(|e| RecalledMemory {
                content: e.content.clone(),
                relevance: e.relevance_score,
                tier: e.tier,
                source: "hot_cache".to_string(),
            })
            .collect();

        entries.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries.truncate(5);
        entries
    }

    fn compute_guidance(&self, hot_cache: &CognitiveCache, sync_state: &SyncState) -> Vec<String> {
        // Find entries surfaced since last sync
        let mut guidance = Vec::new();
        for entry in &hot_cache.hot_cache.entries {
            if entry.promoted_at > sync_state.last_sync_timestamp {
                let tier = match entry.tier {
                    ConfidenceTier::Loud => "LOUD",
                    ConfidenceTier::Clear => "CLEAR",
                    ConfidenceTier::Whisper => "WHISPER",
                };
                guidance.push(format!(
                    "[{tier}] {}",
                    escape_xml(&truncate_to_chars(&entry.content, 120))
                ));
            }
        }
        guidance.truncate(3);
        guidance
    }

    fn compute_budgets(&self) -> TokenBudgets {
        let total = self.token_budget;
        match self.mode {
            SubconsciousMode::Whisper => TokenBudgets {
                soul: 0, // No soul in whisper mode
                recall: total * 3 / 4,
                guidance: total / 4,
            },
            SubconsciousMode::Full => TokenBudgets {
                soul: total / 2,
                recall: total / 3,
                guidance: total / 6,
            },
            SubconsciousMode::Off => TokenBudgets {
                soul: 0,
                recall: 0,
                guidance: 0,
            },
        }
    }
}

/// Truncate text to approximately `max_chars` bytes, preserving UTF-8 and word boundaries.
fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Find a char boundary at or before max_chars
    let mut end = max_chars;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    // Try to break at a word boundary
    if let Some(pos) = text[..end].rfind(' ') {
        end = pos;
    }

    format!("{}…", &text[..end])
}

/// Escape special XML characters in a string.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config() -> Config {
        Config::default()
    }

    #[test]
    fn mode_from_env_default_is_whisper() {
        // Ensure no env var set
        std::env::remove_var("NEXUS_SUBCONSCIOUS_MODE");
        assert_eq!(SubconsciousMode::from_env(), SubconsciousMode::Whisper);
    }

    #[test]
    fn truncate_preserves_short_text() {
        assert_eq!(truncate_to_chars("hello", 10), "hello");
    }

    #[test]
    fn truncate_truncates_long_text() {
        let result = truncate_to_chars("hello world this is a test of truncation", 15);
        assert!(result.ends_with('…'));
        assert!(result.len() < 20);
    }

    #[test]
    fn truncate_handles_multibyte() {
        let text = "héllo wörld";
        let result = truncate_to_chars(text, 5);
        assert!(result.len() < 15);
        // Should not panic and should produce valid UTF-8
        assert!(result.ends_with('…') || result == "héllo");
    }

    #[test]
    fn escape_xml_handles_special_chars() {
        assert_eq!(
            escape_xml("a<b>c&d\"e'f"),
            "a&lt;b&gt;c&amp;d&quot;e&apos;f"
        );
    }

    #[test]
    fn format_off_mode_returns_empty() {
        let dir = TempDir::new().unwrap();
        let mut engine = RetrievalEngine::new(dir.path(), test_config());
        engine.mode = SubconsciousMode::Off;
        let result = RetrievalResult {
            soul_content: Some("test".to_string()),
            recalled: vec![],
            active_guidance: vec![],
            stats: RetrievalStats {
                total_memories: 10,
                hot_cache_entries: 3,
                soul_md_exists: true,
                soul_md_age_minutes: Some(5),
            },
        };
        assert!(engine.format_for_stdout(&result).is_empty());
    }

    #[test]
    fn format_whisper_mode_includes_context() {
        let dir = TempDir::new().unwrap();
        let mut engine = RetrievalEngine::new(dir.path(), test_config());
        engine.mode = SubconsciousMode::Whisper;
        let result = RetrievalResult {
            soul_content: None,
            recalled: vec![RecalledMemory {
                content: "test memory".to_string(),
                relevance: 0.9,
                tier: ConfidenceTier::Loud,
                source: "hot_cache".to_string(),
            }],
            active_guidance: vec![],
            stats: RetrievalStats {
                total_memories: 10,
                hot_cache_entries: 3,
                soul_md_exists: false,
                soul_md_age_minutes: None,
            },
        };
        let output = engine.format_for_stdout(&result);
        assert!(output.contains("<nexus_context>"));
        assert!(output.contains("<nexus_recall>"));
        assert!(!output.contains("<nexus_soul>"));
    }

    #[test]
    fn format_full_mode_includes_soul() {
        let dir = TempDir::new().unwrap();
        let mut engine = RetrievalEngine::new(dir.path(), test_config());
        engine.mode = SubconsciousMode::Full;
        let result = RetrievalResult {
            soul_content: Some("I am a helpful assistant".to_string()),
            recalled: vec![],
            active_guidance: vec![],
            stats: RetrievalStats {
                total_memories: 10,
                hot_cache_entries: 0,
                soul_md_exists: true,
                soul_md_age_minutes: Some(5),
            },
        };
        let output = engine.format_for_stdout(&result);
        assert!(output.contains("<nexus_soul>"));
        assert!(output.contains("I am a helpful assistant"));
    }

    #[test]
    fn check_for_updates_returns_none_when_unchanged() {
        let dir = TempDir::new().unwrap();
        let engine = RetrievalEngine::new(dir.path(), test_config());
        let mut state = SyncState::new("test");
        let soul_content = engine.load_soul_content();
        state.last_soul_hash = sync_state::soul_content_hash(soul_content.as_deref().unwrap_or(""));
        state.last_hot_cache_count = engine.load_hot_cache().hot_cache.entries.len();
        let hot_cache_ids: Vec<String> = engine
            .load_hot_cache()
            .hot_cache
            .entries
            .iter()
            .map(|e| e.memory_id.to_string())
            .collect();
        state.last_hot_cache_hash = sync_state::hot_cache_hash(&hot_cache_ids);
        assert!(engine.check_for_updates(&mut state).is_none());
    }

    #[test]
    fn session_start_format_contains_context() {
        let dir = TempDir::new().unwrap();
        let mut engine = RetrievalEngine::new(dir.path(), test_config());
        engine.mode = SubconsciousMode::Whisper;
        let cache = CognitiveCache::default();
        let output = engine.format_session_start(&cache, None);
        assert!(output.contains("<nexus_context>"));
    }
}
