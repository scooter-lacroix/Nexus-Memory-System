//! Phase analyzer for memory analysis

use crate::{Phase, PhaseAnalysis, PhaseType};
use nexus_core::Memory;

/// Phase analyzer for detecting and analyzing memory phases
pub struct PhaseAnalyzer {
    /// Minimum confidence threshold
    confidence_threshold: f32,
}

impl PhaseAnalyzer {
    /// Create a new phase analyzer
    pub fn new() -> Self {
        Self {
            confidence_threshold: 0.5,
        }
    }

    /// Create with custom threshold
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            confidence_threshold: threshold.clamp(0.0, 1.0),
        }
    }

    /// Analyze a memory and detect its phase
    pub fn analyze(&self, memory: &Memory) -> PhaseAnalysis {
        let phase = self.detect_phase(memory);
        let confidence = self.calculate_confidence(memory, &phase);

        let mut analysis = PhaseAnalysis::new(phase, confidence);

        // Add memory to relevant memories if confidence is high enough
        if analysis.confidence >= self.confidence_threshold {
            analysis.relevant_memories.push(memory.id);
        }

        analysis
    }

    /// Analyze multiple memories
    pub fn analyze_batch(&self, memories: &[Memory]) -> Vec<PhaseAnalysis> {
        memories.iter().map(|m| self.analyze(m)).collect()
    }

    /// Detect phase from memory content
    fn detect_phase(&self, memory: &Memory) -> Phase {
        let content = memory.content.to_lowercase();

        // Simple keyword-based phase detection
        // Order matters: more specific checks should come first
        let phase_type = if content.contains("todo")
            || content.contains("task")
            || content.contains("plan")
        {
            PhaseType::Planning
        } else if content.contains("test")
            || content.contains("verify")
            || content.contains("check")
        {
            PhaseType::Verification
        } else if content.contains("implement")
            || content.contains("code")
            || content.contains("write")
        {
            PhaseType::Execution
        } else if content.contains("fix") || content.contains("bug") || content.contains("error") {
            PhaseType::Debugging
        } else if content.contains("refactor")
            || content.contains("improve")
            || content.contains("optimize")
        {
            PhaseType::Refinement
        } else {
            PhaseType::General
        };

        Phase::new(phase_type)
    }

    /// Calculate confidence for phase detection
    fn calculate_confidence(&self, memory: &Memory, phase: &Phase) -> f32 {
        // Base confidence
        let mut confidence: f32 = 0.5;

        // Boost confidence if memory has relevant category
        if phase.phase_type == PhaseType::Planning
            && memory.category.to_string() == "specifications"
        {
            confidence += 0.2;
        }

        // Boost confidence based on memory lane type
        if let Some(ref lane_type) = memory.memory_lane_type {
            let lane_str = lane_type.to_string();
            if phase.phase_type == PhaseType::Debugging && lane_str == "correction" {
                confidence += 0.3;
            } else if phase.phase_type == PhaseType::Execution && lane_str == "decision" {
                confidence += 0.2;
            }
        }

        // Boost confidence if memory has labels
        if !memory.labels.is_empty() {
            confidence += 0.1;
        }

        confidence.clamp(0.0, 1.0)
    }

    /// Get confidence threshold
    pub fn threshold(&self) -> f32 {
        self.confidence_threshold
    }
}

impl Default for PhaseAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_memory(content: &str) -> Memory {
        let mut memory = Memory::default();
        memory.content = content.to_string();
        memory
    }

    #[test]
    fn test_analyzer_new() {
        let analyzer = PhaseAnalyzer::new();
        assert!((analyzer.threshold() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_analyzer_with_threshold() {
        let analyzer = PhaseAnalyzer::with_threshold(0.8);
        assert!((analyzer.threshold() - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_detect_planning_phase() {
        let analyzer = PhaseAnalyzer::new();
        let memory = create_test_memory("I need to plan the next tasks for the project");

        let analysis = analyzer.analyze(&memory);

        assert_eq!(analysis.phase.phase_type, PhaseType::Planning);
    }

    #[test]
    fn test_detect_execution_phase() {
        let analyzer = PhaseAnalyzer::new();
        let memory = create_test_memory("Implementing the new feature code");

        let analysis = analyzer.analyze(&memory);

        assert_eq!(analysis.phase.phase_type, PhaseType::Execution);
    }

    #[test]
    fn test_detect_verification_phase() {
        let analyzer = PhaseAnalyzer::new();
        let memory = create_test_memory("Testing and verifying the implementation");

        let analysis = analyzer.analyze(&memory);

        assert_eq!(analysis.phase.phase_type, PhaseType::Verification);
    }

    #[test]
    fn test_detect_debugging_phase() {
        let analyzer = PhaseAnalyzer::new();
        let memory = create_test_memory("Fix the bug in the error handling");

        let analysis = analyzer.analyze(&memory);

        assert_eq!(analysis.phase.phase_type, PhaseType::Debugging);
    }

    #[test]
    fn test_analyze_batch() {
        let analyzer = PhaseAnalyzer::new();
        let memories = vec![
            create_test_memory("Plan the project"),
            create_test_memory("Implement feature"),
        ];

        let analyses = analyzer.analyze_batch(&memories);

        assert_eq!(analyses.len(), 2);
    }
}
