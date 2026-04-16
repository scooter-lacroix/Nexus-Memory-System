//! Builds context.md from hot and cold caches with budget awareness.

use crate::cognitive_cache::{ConfidenceTier, HotCache, HotCacheEntry};
use crate::token_budget::TokenBudget;

/// Compression level for a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionLevel {
    /// Full content
    None,
    /// First sentence or brief summary
    Light,
    /// One-liner
    Heavy,
}

/// A recalled memory from the cold index or vector search.
#[derive(Debug, Clone)]
pub struct ColdRecall {
    pub memory_id: i64,
    pub content: String,
    pub relevance_score: f32,
    pub tier: ConfidenceTier,
}

/// Builds the context.md string from hot and cold memories.
pub fn build_context_md(hot: &HotCache, cold: &[ColdRecall], max_tokens: usize) -> String {
    let mut output = String::new();
    let mut current_tokens = 0;

    output.push_str("# Nexus Project Context\n\n");

    // Group hot entries by tier
    let loud_entries: Vec<&HotCacheEntry> = hot
        .entries
        .iter()
        .filter(|e| e.tier == ConfidenceTier::Loud)
        .collect();
    let clear_entries: Vec<&HotCacheEntry> = hot
        .entries
        .iter()
        .filter(|e| e.tier == ConfidenceTier::Clear)
        .collect();
    let whisper_entries: Vec<&HotCacheEntry> = hot
        .entries
        .iter()
        .filter(|e| e.tier == ConfidenceTier::Whisper)
        .collect();

    // 1. Add Loud entries (Full content)
    let loud_section = format_tier_section(
        "## High Relevance (Loud)",
        &loud_entries,
        CompressionLevel::None,
    );
    let loud_tokens = TokenBudget::estimate_tokens(&loud_section);
    if current_tokens + loud_tokens <= max_tokens {
        output.push_str(&loud_section);
        current_tokens += loud_tokens;
    } else {
        // Truncation happens if even Loud doesn't fit
        return output;
    }

    // 2. Add Clear entries (Light compression)
    let clear_section = format_tier_section(
        "## Relevant (Clear)",
        &clear_entries,
        CompressionLevel::Light,
    );
    let clear_tokens = TokenBudget::estimate_tokens(&clear_section);
    if current_tokens + clear_tokens <= max_tokens {
        output.push_str(&clear_section);
        current_tokens += clear_tokens;
    }

    // 3. Add Whisper entries (Heavy compression) - only if < 80% budget used
    if (current_tokens as f32 / max_tokens as f32) < 0.80 {
        let whisper_section = format_tier_section(
            "## Low Signal (Whisper)",
            &whisper_entries,
            CompressionLevel::Heavy,
        );
        let whisper_tokens = TokenBudget::estimate_tokens(&whisper_section);
        if current_tokens + whisper_tokens <= max_tokens {
            output.push_str(&whisper_section);
            current_tokens += whisper_tokens;
        }
    }

    // 4. Add Cold Recalls
    if !cold.is_empty() && max_tokens - current_tokens > 200 {
        output.push_str("## Recalled Memories\n\n");
        for recall in cold {
            let entry_str = format!(
                "- [Recall {}] {}\n",
                recall.memory_id,
                compress_text(&recall.content, CompressionLevel::Light)
            );
            let entry_tokens = TokenBudget::estimate_tokens(&entry_str);
            if current_tokens + entry_tokens <= max_tokens {
                output.push_str(&entry_str);
                current_tokens += entry_tokens;
            } else {
                break;
            }
        }
    }

    output
}

fn format_tier_section(
    title: &str,
    entries: &[&HotCacheEntry],
    compression: CompressionLevel,
) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut section = format!("{}\n\n", title);
    for entry in entries {
        let content = compress_text(&entry.content, compression);
        section.push_str(&format!("### Memory {}\n{}\n\n", entry.memory_id, content));
    }
    section
}

fn compress_text(text: &str, level: CompressionLevel) -> String {
    match level {
        CompressionLevel::None => text.to_string(),
        CompressionLevel::Light => text.lines().next().unwrap_or("").to_string(),
        CompressionLevel::Heavy => {
            let first_line = text.lines().next().unwrap_or("");
            if first_line.len() > 80 {
                // UTF-8-safe: find the char boundary at or before byte 77
                let truncate_at = first_line
                    .char_indices()
                    .take_while(|(idx, _)| *idx < 77)
                    .last()
                    .map(|(idx, c)| idx + c.len_utf8())
                    .unwrap_or(0);
                format!("{}...", &first_line[..truncate_at])
            } else {
                first_line.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive_cache::ConfidenceTier;
    use chrono::Utc;

    #[test]
    fn test_build_context_md_ordering_and_budget() {
        let mut hot = HotCache::default();
        hot.entries.push(HotCacheEntry {
            memory_id: 1,
            content: "Loud Content".into(),
            relevance_score: 0.9,
            tier: ConfidenceTier::Loud,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        });
        hot.entries.push(HotCacheEntry {
            memory_id: 2,
            content: "Clear Content Line 1\nLine 2".into(),
            relevance_score: 0.75,
            tier: ConfidenceTier::Clear,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        });

        // 1000 tokens budget (plenty)
        let context = build_context_md(&hot, &[], 1000);
        assert!(context.contains("High Relevance (Loud)"));
        assert!(context.contains("Loud Content"));
        assert!(context.contains("Relevant (Clear)"));
        assert!(context.contains("Clear Content Line 1"));
        assert!(!context.contains("Line 2")); // Light compression

        // Very small budget
        let tight_context = build_context_md(&hot, &[], 10);
        assert!(tight_context.len() < context.len());
    }

    #[test]
    fn test_build_context_md_with_cold_recall() {
        let hot = HotCache::default();
        let cold = vec![ColdRecall {
            memory_id: 99,
            content: "Cold Content".into(),
            relevance_score: 0.68,
            tier: ConfidenceTier::Whisper,
        }];

        let context = build_context_md(&hot, &cold, 1000);
        assert!(context.contains("Recalled Memories"));
        assert!(context.contains("[Recall 99] Cold Content"));
    }

    #[test]
    fn test_compress_text_utf8_multibyte() {
        // Japanese text: each char is 3 bytes. 80+ byte line triggers Heavy truncation.
        let long_multibyte = "ア".repeat(40); // 120 bytes, 40 chars
        let compressed = compress_text(&long_multibyte, CompressionLevel::Heavy);
        assert!(compressed.ends_with("..."));
        // Must not panic and must be valid UTF-8
        assert!(compressed.is_char_boundary(compressed.len()));
        // Should be under 80 bytes + "..."
        assert!(compressed.len() <= 83);
    }
}
