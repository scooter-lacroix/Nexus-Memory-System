//! LePhase integration for Nexus Memory System
//!
//! Provides token-efficient memory compression and phase-based organization.
//!
//! ## Compression Modes
//! - **Ultra**: For tight token budgets (~4000 chars)
//! - **Balanced**: Default mode (~12000 chars)
//! - **Verbose**: Full detail (~24000 chars)
//!
//! ## Targets
//! - Token reduction: >50%
//! - Phase detection accuracy: >80%

use crate::{Phase, PhaseAnalysis, PhaseAnalyzer, PhaseType};
use nexus_core::Memory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Compression format modes (aligned with LePhase FormatMode)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompressionMode {
    /// Ultra-compact format for tight token budgets
    Ultra,
    /// Balanced detail and compactness
    #[default]
    Balanced,
    /// Most detailed format
    Verbose,
}

impl CompressionMode {
    /// Get suggested max characters for this mode
    pub fn max_chars(&self) -> usize {
        match self {
            CompressionMode::Ultra => 4_000,
            CompressionMode::Balanced => 12_000,
            CompressionMode::Verbose => 24_000,
        }
    }

    /// Get max content length for compression
    pub fn max_content_len(&self) -> usize {
        match self {
            CompressionMode::Ultra => 500,
            CompressionMode::Balanced => 1000,
            CompressionMode::Verbose => 2000,
        }
    }

    /// Parse mode from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ultra" => Some(Self::Ultra),
            "balanced" => Some(Self::Balanced),
            "verbose" => Some(Self::Verbose),
            _ => None,
        }
    }
}

/// Token-aware formatter utilities (aligned with LePhase TokenFormatter)
#[derive(Debug, Clone)]
pub struct TokenFormatter {
    /// Maximum characters allowed
    max_chars: usize,
    /// Truncation marker
    truncation_marker: String,
}

impl Default for TokenFormatter {
    fn default() -> Self {
        Self {
            max_chars: 12_000,
            truncation_marker: "\n\n...[truncated]".to_string(),
        }
    }
}

impl TokenFormatter {
    /// Create a new formatter with max chars
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            truncation_marker: "\n\n...[truncated]".to_string(),
        }
    }

    /// Create with compression mode
    pub fn from_mode(mode: CompressionMode) -> Self {
        Self::new(mode.max_chars())
    }

    /// Truncate a string to max characters while preserving UTF-8 boundaries
    pub fn truncate(&self, input: &str) -> String {
        if input.chars().count() <= self.max_chars {
            return input.to_string();
        }

        let mut out = String::new();
        for (i, ch) in input.chars().enumerate() {
            if i >= self.max_chars {
                break;
            }
            out.push(ch);
        }
        out.push_str(&self.truncation_marker);
        out
    }

    /// Truncate to specific max chars (override instance max_chars)
    pub fn truncate_to(&self, input: &str, max_chars: usize) -> String {
        if input.chars().count() <= max_chars {
            return input.to_string();
        }

        let mut out = String::new();
        for (i, ch) in input.chars().enumerate() {
            if i >= max_chars {
                break;
            }
            out.push(ch);
        }
        out.push_str(&self.truncation_marker);
        out
    }

    /// Estimate token count (rough: 4 chars per token average)
    pub fn estimate_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }

    /// Check if text would be truncated
    pub fn would_truncate(&self, text: &str) -> bool {
        text.chars().count() > self.max_chars
    }

    /// Get the max chars setting
    pub fn max_chars(&self) -> usize {
        self.max_chars
    }
}

/// Compressed memory representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedMemory {
    /// Original memory ID
    pub memory_id: i64,

    /// Original content
    pub original_content: String,

    /// Compressed content
    pub compressed_content: String,

    /// Compression ratio (compressed/original)
    pub compression_ratio: f64,

    /// Detected phase
    pub phase: Phase,

    /// Memory category
    pub category: String,

    /// Memory lane type
    pub memory_lane_type: Option<String>,

    /// Labels
    pub labels: Vec<String>,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl CompressedMemory {
    /// Check if compression achieved the target ratio (>50%)
    pub fn meets_target(&self) -> bool {
        self.compression_ratio < 0.5
    }
}

/// Statistics about compression performance
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressionStats {
    /// Number of memories processed
    pub memory_count: usize,

    /// Total bytes before compression
    pub total_original_bytes: usize,

    /// Total bytes after compression
    pub total_compressed_bytes: usize,

    /// Overall compression ratio
    pub compression_ratio: f64,

    /// Total bytes saved
    pub bytes_saved: usize,
}

impl CompressionStats {
    /// Check if average compression meets the >50% target
    pub fn meets_target(&self) -> bool {
        self.compression_ratio < 0.5
    }

    /// Get percentage saved
    pub fn percentage_saved(&self) -> f64 {
        if self.total_original_bytes == 0 {
            return 0.0;
        }
        (1.0 - self.compression_ratio) * 100.0
    }
}

/// LePhase integration for advanced memory management
pub struct LePhaseIntegration {
    /// Phase analyzer
    analyzer: PhaseAnalyzer,

    /// Phase-indexed memories
    phase_memories: HashMap<PhaseType, Vec<i64>>,

    /// Memory to phase mapping
    memory_phases: HashMap<i64, Phase>,

    /// Compression mode
    compression_mode: CompressionMode,

    /// Enable compression
    compression_enabled: bool,
}

impl LePhaseIntegration {
    /// Create a new LePhase integration
    pub fn new() -> Self {
        Self {
            analyzer: PhaseAnalyzer::new(),
            phase_memories: HashMap::new(),
            memory_phases: HashMap::new(),
            compression_mode: CompressionMode::default(),
            compression_enabled: true,
        }
    }

    /// Create with custom analyzer
    pub fn with_analyzer(analyzer: PhaseAnalyzer) -> Self {
        Self {
            analyzer,
            phase_memories: HashMap::new(),
            memory_phases: HashMap::new(),
            compression_mode: CompressionMode::default(),
            compression_enabled: true,
        }
    }

    /// Create with custom compression mode
    pub fn with_mode(mode: CompressionMode) -> Self {
        Self {
            analyzer: PhaseAnalyzer::new(),
            phase_memories: HashMap::new(),
            memory_phases: HashMap::new(),
            compression_mode: mode,
            compression_enabled: true,
        }
    }

    /// Enable or disable compression
    pub fn set_compression_enabled(&mut self, enabled: bool) {
        self.compression_enabled = enabled;
    }

    /// Set compression mode
    pub fn set_compression_mode(&mut self, mode: CompressionMode) {
        self.compression_mode = mode;
    }

    /// Get compression mode
    pub fn compression_mode(&self) -> CompressionMode {
        self.compression_mode
    }

    /// Register a memory with phase analysis
    pub fn register_memory(&mut self, memory: &Memory) -> PhaseAnalysis {
        let analysis = self.analyzer.analyze(memory);

        // Update phase index
        let phase_type = analysis.phase.phase_type;
        self.phase_memories
            .entry(phase_type)
            .or_default()
            .push(memory.id);

        // Store memory-phase mapping
        self.memory_phases.insert(memory.id, analysis.phase.clone());

        analysis
    }

    /// Unregister a memory
    pub fn unregister_memory(&mut self, memory_id: i64) -> Option<Phase> {
        if let Some(phase) = self.memory_phases.remove(&memory_id) {
            // Remove from phase index
            if let Some(memories) = self.phase_memories.get_mut(&phase.phase_type) {
                memories.retain(|&id| id != memory_id);
            }
            Some(phase)
        } else {
            None
        }
    }

    /// Get the phase for a memory
    pub fn get_phase(&self, memory_id: i64) -> Option<&Phase> {
        self.memory_phases.get(&memory_id)
    }

    /// Get all memories in a phase
    pub fn get_memories_by_phase(&self, phase_type: PhaseType) -> Vec<i64> {
        self.phase_memories
            .get(&phase_type)
            .cloned()
            .unwrap_or_default()
    }

    /// Get all phase types with memory counts
    pub fn phase_summary(&self) -> HashMap<PhaseType, usize> {
        self.phase_memories
            .iter()
            .map(|(phase_type, memories)| (*phase_type, memories.len()))
            .collect()
    }

    /// Get total registered memory count
    pub fn total_memories(&self) -> usize {
        self.memory_phases.len()
    }

    /// Clear all registered memories
    pub fn clear(&mut self) {
        self.phase_memories.clear();
        self.memory_phases.clear();
    }

    /// Re-analyze all memories
    pub fn reanalyze(&mut self, memories: &[Memory]) -> Vec<PhaseAnalysis> {
        // Clear existing data
        self.clear();

        // Re-analyze all memories
        memories.iter().map(|m| self.register_memory(m)).collect()
    }

    /// Get the analyzer
    pub fn analyzer(&self) -> &PhaseAnalyzer {
        &self.analyzer
    }

    /// Get mutable analyzer
    pub fn analyzer_mut(&mut self) -> &mut PhaseAnalyzer {
        &mut self.analyzer
    }

    // === Compression Methods ===

    /// Compress memory content for storage
    ///
    /// This reduces token usage while preserving key information.
    /// Target: >50% compression ratio
    pub fn compress(&self, memory: &Memory) -> CompressedMemory {
        let original_len = memory.content.len();

        let compressed_content = if self.compression_enabled {
            self.compress_content(&memory.content)
        } else {
            memory.content.clone()
        };

        let compression_ratio = if original_len > 0 {
            compressed_content.len() as f64 / original_len as f64
        } else {
            1.0
        };

        // Get phase from registered memories or analyze
        let phase = self
            .memory_phases
            .get(&memory.id)
            .cloned()
            .unwrap_or_else(|| {
                let analysis = self.analyzer.analyze(memory);
                analysis.phase
            });

        CompressedMemory {
            memory_id: memory.id,
            original_content: memory.content.clone(),
            compressed_content,
            compression_ratio,
            phase,
            category: memory.category.to_string(),
            memory_lane_type: memory.memory_lane_type.as_ref().map(|t| t.to_string()),
            labels: memory.labels.clone(),
            created_at: memory.created_at,
        }
    }

    /// Decompress memory content for retrieval
    pub fn decompress(&self, compressed: &CompressedMemory) -> String {
        if self.compression_enabled {
            // Remove truncation marker if present
            compressed
                .compressed_content
                .trim_end_matches("...")
                .to_string()
        } else {
            compressed.compressed_content.clone()
        }
    }

    /// Format memories for presentation to a model
    ///
    /// This creates a token-efficient summary of multiple memories.
    pub fn format_for_model(&self, memories: &[Memory], max_tokens: Option<usize>) -> String {
        let max_chars = max_tokens
            .unwrap_or_else(|| self.compression_mode.max_chars())
            .min(self.compression_mode.max_chars());

        let mut output = String::new();
        output.push_str("# Memory Context\n\n");

        // Group memories by phase
        let mut by_phase: HashMap<PhaseType, Vec<&Memory>> = HashMap::new();

        for memory in memories {
            let phase = self
                .memory_phases
                .get(&memory.id)
                .map(|p| p.phase_type)
                .unwrap_or_else(|| {
                    let analysis = self.analyzer.analyze(memory);
                    analysis.phase.phase_type
                });
            by_phase.entry(phase).or_default().push(memory);
        }

        // Format each phase group in priority order
        for phase_type in [
            PhaseType::Debugging,
            PhaseType::Execution,
            PhaseType::Planning,
            PhaseType::Verification,
            PhaseType::Refinement,
            PhaseType::General,
        ] {
            if let Some(phase_memories) = by_phase.get(&phase_type) {
                if phase_memories.is_empty() {
                    continue;
                }

                output.push_str(&format!(
                    "## {} ({} items)\n",
                    phase_type,
                    phase_memories.len()
                ));

                for memory in phase_memories {
                    let formatted = self.format_single_memory(memory);
                    if output.len() + formatted.len() > max_chars {
                        output.push_str("\n...[truncated]\n");
                        break;
                    }
                    output.push_str(&formatted);
                }
                output.push('\n');

                if output.len() >= max_chars {
                    break;
                }
            }
        }

        // Truncate if needed
        if output.len() > max_chars {
            output.truncate(max_chars);
            output.push_str("\n\n...[truncated]");
        }

        output
    }

    /// Get compression statistics
    pub fn compression_stats(&self, memories: &[Memory]) -> CompressionStats {
        if memories.is_empty() {
            return CompressionStats::default();
        }

        let mut total_original = 0usize;
        let mut total_compressed = 0usize;
        let mut count = 0;

        for memory in memories {
            let compressed = self.compress(memory);
            total_original += compressed.original_content.len();
            total_compressed += compressed.compressed_content.len();
            count += 1;
        }

        let ratio = if total_original > 0 {
            total_compressed as f64 / total_original as f64
        } else {
            1.0
        };

        CompressionStats {
            memory_count: count,
            total_original_bytes: total_original,
            total_compressed_bytes: total_compressed,
            compression_ratio: ratio,
            bytes_saved: total_original.saturating_sub(total_compressed),
        }
    }

    // Private methods

    fn compress_content(&self, content: &str) -> String {
        let max_len = self.compression_mode.max_content_len();

        // Advanced compression pipeline:
        // 1. Normalize whitespace
        // 2. Remove redundant phrases
        // 3. Compress common patterns
        // 4. Truncate if still too long

        let normalized: String = content.split_whitespace().collect::<Vec<_>>().join(" ");

        // Apply pattern-based compression
        let compressed = self.apply_compression_patterns(&normalized);

        if compressed.len() > max_len {
            // Truncate at word boundary
            let truncated = self.truncate_at_word_boundary(&compressed, max_len);
            format!("{}...", truncated)
        } else {
            compressed
        }
    }

    /// Apply compression patterns to reduce token count
    fn apply_compression_patterns(&self, content: &str) -> String {
        let mut result = content.to_string();

        // Common phrase contractions (token-efficient)
        let contractions = [
            ("I need to", "Need to"),
            ("I want to", "Want to"),
            ("I have to", "Have to"),
            ("I am going to", "Will"),
            ("I will", "Will"),
            ("you need to", "Need to"),
            ("you should", "Should"),
            ("please ", ""),
            ("basically ", ""),
            ("actually ", ""),
            ("just ", ""),
            ("really ", ""),
            ("very ", ""),
            (" in order to", " to"),
            (" due to the fact that", " because"),
            (" at this point in time", " now"),
            (" in the event that", " if"),
            (" for the purpose of", " to"),
        ];

        for (pattern, replacement) in contractions {
            result = result.replace(pattern, replacement);
        }

        result
    }

    /// Truncate at word boundary to avoid cutting words
    fn truncate_at_word_boundary(&self, content: &str, max_len: usize) -> String {
        if content.len() <= max_len {
            return content.to_string();
        }

        // Find the last space before max_len
        let mut end = max_len;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }

        let substring = &content[..end];

        // Find last space
        if let Some(last_space) = substring.rfind(' ') {
            substring[..last_space].to_string()
        } else {
            substring.to_string()
        }
    }

    fn format_single_memory(&self, memory: &Memory) -> String {
        let category = &memory.category;
        let lane_type = memory
            .memory_lane_type
            .as_ref()
            .map(|t| format!(" [{}]", t))
            .unwrap_or_default();

        let content = self.compress_content(&memory.content);
        let labels = if memory.labels.is_empty() {
            String::new()
        } else {
            format!(" ({})", memory.labels.join(", "))
        };

        format!("- [{}]{}{}: {}\n", category, lane_type, labels, content)
    }

    // === Advanced Compression Methods ===

    /// Compress multiple memories into a summary
    pub fn compress_batch(&self, memories: &[Memory]) -> Vec<CompressedMemory> {
        memories.iter().map(|m| self.compress(m)).collect()
    }

    /// Create a condensed summary of memories by phase
    pub fn summarize_by_phase(&self, memories: &[Memory]) -> HashMap<PhaseType, String> {
        let mut by_phase: HashMap<PhaseType, Vec<&Memory>> = HashMap::new();

        for memory in memories {
            let phase = self
                .memory_phases
                .get(&memory.id)
                .map(|p| p.phase_type)
                .unwrap_or_else(|| {
                    let analysis = self.analyzer.analyze(memory);
                    analysis.phase.phase_type
                });
            by_phase.entry(phase).or_default().push(memory);
        }

        by_phase
            .into_iter()
            .map(|(phase_type, phase_memories)| {
                let summary = self.create_phase_summary(&phase_type, &phase_memories);
                (phase_type, summary)
            })
            .collect()
    }

    fn create_phase_summary(&self, phase_type: &PhaseType, memories: &[&Memory]) -> String {
        let count = memories.len();
        let total_content: String = memories
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        // Extract key terms (simple implementation)
        let key_terms = self.extract_key_terms(&total_content, 5);

        format!(
            "{}: {} memories. Key: {}",
            phase_type,
            count,
            key_terms.join(", ")
        )
    }

    fn extract_key_terms(&self, content: &str, max_terms: usize) -> Vec<String> {
        // Simple keyword extraction based on word frequency
        let mut word_counts: HashMap<String, usize> = HashMap::new();

        let stop_words = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
            "by", "from", "is", "are", "was", "were", "be", "been", "have", "has", "had", "do",
            "does", "did", "will", "would", "could", "should", "may", "might", "must", "this",
            "that", "these", "those", "i", "you", "he", "she", "it", "we", "they", "what", "which",
            "who",
        ];

        for word in content.split_whitespace() {
            let lower = word.to_lowercase();
            let trimmed: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();

            if trimmed.len() > 2 && !stop_words.contains(&trimmed.as_str()) {
                *word_counts.entry(trimmed).or_insert(0) += 1;
            }
        }

        let mut sorted: Vec<_> = word_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        sorted
            .into_iter()
            .take(max_terms)
            .map(|(word, _)| word)
            .collect()
    }

    /// Get a token formatter for this integration
    pub fn formatter(&self) -> TokenFormatter {
        TokenFormatter::from_mode(self.compression_mode)
    }

    /// Estimate token savings from compression
    pub fn estimate_token_savings(&self, memories: &[Memory]) -> TokenSavings {
        let stats = self.compression_stats(memories);

        let original_tokens = stats.total_original_bytes / 4; // Rough estimate
        let compressed_tokens = stats.total_compressed_bytes / 4;
        let tokens_saved = original_tokens.saturating_sub(compressed_tokens);

        TokenSavings {
            original_tokens,
            compressed_tokens,
            tokens_saved,
            percentage_saved: if original_tokens > 0 {
                (tokens_saved as f64 / original_tokens as f64) * 100.0
            } else {
                0.0
            },
        }
    }
}

/// Token savings information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSavings {
    /// Estimated original token count
    pub original_tokens: usize,
    /// Estimated compressed token count
    pub compressed_tokens: usize,
    /// Tokens saved
    pub tokens_saved: usize,
    /// Percentage saved
    pub percentage_saved: f64,
}

impl Default for LePhaseIntegration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_memory(id: i64, content: &str) -> Memory {
        Memory {
            id,
            content: content.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_integration_new() {
        let integration = LePhaseIntegration::new();
        assert_eq!(integration.total_memories(), 0);
        assert!(integration.compression_enabled);
    }

    #[test]
    fn test_register_memory() {
        let mut integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, "Plan the project tasks");

        let analysis = integration.register_memory(&memory);

        assert_eq!(integration.total_memories(), 1);
        assert_eq!(analysis.phase.phase_type, PhaseType::Planning);
    }

    #[test]
    fn test_unregister_memory() {
        let mut integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, "Debug the error");

        integration.register_memory(&memory);
        let removed = integration.unregister_memory(1);

        assert!(removed.is_some());
        assert_eq!(integration.total_memories(), 0);
    }

    #[test]
    fn test_get_phase() {
        let mut integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, "Test the implementation");

        integration.register_memory(&memory);

        let phase = integration.get_phase(1);
        assert!(phase.is_some());
        assert_eq!(phase.unwrap().phase_type, PhaseType::Verification);
    }

    #[test]
    fn test_get_memories_by_phase() {
        let mut integration = LePhaseIntegration::new();

        integration.register_memory(&create_test_memory(1, "Plan feature"));
        integration.register_memory(&create_test_memory(2, "Plan another"));
        integration.register_memory(&create_test_memory(3, "Test code"));

        let planning = integration.get_memories_by_phase(PhaseType::Planning);

        assert_eq!(planning.len(), 2);
        assert!(planning.contains(&1));
        assert!(planning.contains(&2));
    }

    #[test]
    fn test_phase_summary() {
        let mut integration = LePhaseIntegration::new();

        integration.register_memory(&create_test_memory(1, "Plan"));
        integration.register_memory(&create_test_memory(2, "Code"));
        integration.register_memory(&create_test_memory(3, "Test"));

        let summary = integration.phase_summary();

        assert!(summary.contains_key(&PhaseType::Planning));
        assert!(summary.contains_key(&PhaseType::Execution));
        assert!(summary.contains_key(&PhaseType::Verification));
    }

    #[test]
    fn test_clear() {
        let mut integration = LePhaseIntegration::new();

        integration.register_memory(&create_test_memory(1, "Plan"));
        integration.clear();

        assert_eq!(integration.total_memories(), 0);
    }

    // Compression tests

    #[test]
    fn test_compression_mode_default() {
        let mode = CompressionMode::default();
        assert_eq!(mode.max_chars(), 12_000);
    }

    #[test]
    fn test_compress_memory() {
        let integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, "This is a test memory with some content");

        let compressed = integration.compress(&memory);

        assert_eq!(compressed.memory_id, 1);
        assert!(compressed.compression_ratio <= 1.0);
    }

    #[test]
    fn test_compress_long_memory() {
        let integration = LePhaseIntegration::new();
        let long_content = "word ".repeat(1000);
        let memory = create_test_memory(1, &long_content);

        let compressed = integration.compress(&memory);

        // Long content should be truncated
        assert!(compressed.compressed_content.len() < long_content.len());
    }

    #[test]
    fn test_decompress_memory() {
        let integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, "Test content");

        let compressed = integration.compress(&memory);
        let decompressed = integration.decompress(&compressed);

        assert!(!decompressed.is_empty());
    }

    #[test]
    fn test_format_for_model() {
        let integration = LePhaseIntegration::new();
        let memories = vec![
            create_test_memory(1, "Debug the error in the code"),
            create_test_memory(2, "Implement the new feature"),
        ];

        let formatted = integration.format_for_model(&memories, None);

        assert!(formatted.starts_with("# Memory Context"));
    }

    #[test]
    fn test_format_for_model_with_limit() {
        let integration = LePhaseIntegration::new();
        let memories = vec![
            create_test_memory(1, "Debug the error in the code"),
            create_test_memory(2, "Implement the new feature"),
        ];

        let formatted = integration.format_for_model(&memories, Some(50));

        assert!(formatted.len() <= 70); // 50 + some buffer for truncation marker
    }

    #[test]
    fn test_compression_stats() {
        let integration = LePhaseIntegration::new();
        let memories = vec![
            create_test_memory(1, &"word ".repeat(100)),
            create_test_memory(2, &"another ".repeat(100)),
        ];

        let stats = integration.compression_stats(&memories);

        assert_eq!(stats.memory_count, 2);
        assert!(stats.total_compressed_bytes < stats.total_original_bytes);
    }

    #[test]
    fn test_compressed_memory_meets_target() {
        let mut compressed = CompressedMemory {
            memory_id: 1,
            original_content: "original content".to_string(),
            compressed_content: "compressed".to_string(),
            compression_ratio: 0.4,
            phase: Phase::default(),
            category: "general".to_string(),
            memory_lane_type: None,
            labels: vec![],
            created_at: chrono::Utc::now(),
        };

        assert!(compressed.meets_target());

        compressed.compression_ratio = 0.6;
        assert!(!compressed.meets_target());
    }

    #[test]
    fn test_compression_disabled() {
        let mut integration = LePhaseIntegration::new();
        integration.set_compression_enabled(false);

        let memory = create_test_memory(1, "Test content");
        let compressed = integration.compress(&memory);

        // Should return original content when compression is disabled
        assert_eq!(compressed.original_content, compressed.compressed_content);
    }

    #[test]
    fn test_compression_stats_percentage_saved() {
        let stats = CompressionStats {
            memory_count: 10,
            total_original_bytes: 1000,
            total_compressed_bytes: 400,
            compression_ratio: 0.4,
            bytes_saved: 600,
        };

        assert!((stats.percentage_saved() - 60.0).abs() < 0.1);
    }

    #[test]
    fn test_compression_stats_meets_target() {
        let good_stats = CompressionStats {
            memory_count: 10,
            total_original_bytes: 1000,
            total_compressed_bytes: 400,
            compression_ratio: 0.4,
            bytes_saved: 600,
        };
        assert!(good_stats.meets_target());

        let bad_stats = CompressionStats {
            memory_count: 10,
            total_original_bytes: 1000,
            total_compressed_bytes: 600,
            compression_ratio: 0.6,
            bytes_saved: 400,
        };
        assert!(!bad_stats.meets_target());
    }

    // New tests for enhanced functionality

    #[test]
    fn test_token_formatter_truncate() {
        let formatter = TokenFormatter::new(10);
        let short = formatter.truncate("short");
        let long = formatter.truncate("this is a very long string that should be truncated");

        assert_eq!(short, "short");
        assert!(long.contains("truncated"));
        assert!(long.len() < 50);
    }

    #[test]
    fn test_token_formatter_from_mode() {
        let formatter = TokenFormatter::from_mode(CompressionMode::Ultra);
        assert_eq!(formatter.max_chars(), 4_000);

        let formatter = TokenFormatter::from_mode(CompressionMode::Verbose);
        assert_eq!(formatter.max_chars(), 24_000);
    }

    #[test]
    fn test_token_formatter_would_truncate() {
        let formatter = TokenFormatter::new(10);
        assert!(!formatter.would_truncate("short"));
        assert!(formatter.would_truncate("this is a long string"));
    }

    #[test]
    fn test_token_formatter_estimate_tokens() {
        let formatter = TokenFormatter::new(100);
        let tokens = formatter.estimate_tokens("this is a test string with some words");
        // Rough estimate: 4 chars per token
        assert!(tokens > 0);
    }

    #[test]
    fn test_compression_mode_parse() {
        assert_eq!(
            CompressionMode::parse("ultra"),
            Some(CompressionMode::Ultra)
        );
        assert_eq!(
            CompressionMode::parse("balanced"),
            Some(CompressionMode::Balanced)
        );
        assert_eq!(
            CompressionMode::parse("verbose"),
            Some(CompressionMode::Verbose)
        );
        assert_eq!(CompressionMode::parse("invalid"), None);
    }

    #[test]
    fn test_compress_batch() {
        let integration = LePhaseIntegration::new();
        let memories = vec![
            create_test_memory(1, "First memory content"),
            create_test_memory(2, "Second memory content"),
            create_test_memory(3, "Third memory content"),
        ];

        let compressed = integration.compress_batch(&memories);
        assert_eq!(compressed.len(), 3);
    }

    #[test]
    fn test_summarize_by_phase() {
        let mut integration = LePhaseIntegration::new();

        integration.register_memory(&create_test_memory(1, "Plan the project architecture"));
        integration.register_memory(&create_test_memory(2, "Implement the feature code"));
        integration.register_memory(&create_test_memory(3, "Test the implementation"));

        let summaries = integration.summarize_by_phase(&[
            create_test_memory(1, "Plan the project architecture"),
            create_test_memory(2, "Implement the feature code"),
            create_test_memory(3, "Test the implementation"),
        ]);

        assert!(!summaries.is_empty());
    }

    #[test]
    fn test_estimate_token_savings() {
        let integration = LePhaseIntegration::new();
        let memories = vec![
            create_test_memory(
                1,
                &"I need to implement the feature because it is very important ".repeat(20),
            ),
            create_test_memory(
                2,
                &"Another memory with some content to compress ".repeat(15),
            ),
        ];

        let savings = integration.estimate_token_savings(&memories);

        assert!(savings.original_tokens > 0);
        assert!(savings.compressed_tokens > 0);
        assert!(savings.percentage_saved >= 0.0);
    }

    #[test]
    fn test_formatter_method() {
        let integration = LePhaseIntegration::new();
        let formatter = integration.formatter();

        assert!(formatter.max_chars() > 0);
    }

    #[test]
    fn test_compression_patterns() {
        let integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, "I need to implement the feature in order to test it");

        let compressed = integration.compress(&memory);

        // Should have removed "I need to" -> "Need to" and "in order to" -> "to"
        assert!(compressed.compressed_content.len() < memory.content.len());
    }

    #[test]
    fn test_truncate_at_word_boundary() {
        let integration = LePhaseIntegration::new();
        let memory = create_test_memory(1, &"word ".repeat(300));

        let compressed = integration.compress(&memory);

        // Should truncate at word boundary, not mid-word
        assert!(!compressed.compressed_content.ends_with("wo"));
        assert!(compressed.compressed_content.ends_with("..."));
    }
}
