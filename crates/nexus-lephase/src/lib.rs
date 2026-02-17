//! Nexus LePhase - LePhase integration wrapper
//!
//! This crate provides integration with the LePhase system for advanced
//! memory analysis, phase-based organization, and token-efficient compression.
//!
//! ## Features
//! - **Phase Analysis**: Automatic detection of memory phases (Planning, Execution, etc.)
//! - **Memory Compression**: Token-efficient compression with >50% reduction target
//! - **Phase-based Organization**: Group memories by phase for better context
//!
//! ## Compression Modes
//! - **Ultra**: For tight token budgets (~4000 chars)
//! - **Balanced**: Default mode (~12000 chars)
//! - **Verbose**: Full detail (~24000 chars)
//!
//! ## Usage
//! ```ignore
//! use nexus_lephase::{LePhaseIntegration, CompressionMode};
//!
//! let integration = LePhaseIntegration::with_mode(CompressionMode::Balanced);
//! let compressed = integration.compress(&memory);
//! ```

pub mod analyzer;
pub mod phases;
pub mod integration;

pub use analyzer::PhaseAnalyzer;
pub use phases::{Phase, PhaseType};
pub use integration::{
    LePhaseIntegration, CompressionMode, CompressedMemory, CompressionStats,
    TokenFormatter, TokenSavings,
};

use serde::{Deserialize, Serialize};

/// Phase analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseAnalysis {
    /// Detected phase
    pub phase: Phase,

    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,

    /// Relevant memory IDs
    pub relevant_memories: Vec<i64>,

    /// Analysis timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl PhaseAnalysis {
    pub fn new(phase: Phase, confidence: f32) -> Self {
        Self {
            phase,
            confidence,
            relevant_memories: Vec::new(),
            timestamp: chrono::Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_analysis_new() {
        let phase = Phase::default();
        let analysis = PhaseAnalysis::new(phase.clone(), 0.9);

        assert_eq!(analysis.confidence, 0.9);
        assert!(analysis.relevant_memories.is_empty());
    }
}
