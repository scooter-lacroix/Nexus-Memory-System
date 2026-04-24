//! Token budget management for context window allocation.

/// Token budget allocation for a model's context window.
#[derive(Debug, Clone, Copy)]
pub struct TokenBudget {
    /// Total context window of the model
    pub model_context_window: usize,
    /// Percentage of window allocated to Nexus (0.0 - 1.0)
    pub nexus_allocation_pct: f32,
    /// Budget for the soul.md document
    pub soul_budget: usize,
    /// Budget for the project context.md (hot cache + cold recall)
    pub context_budget: usize,
}

impl TokenBudget {
    /// Create a token budget based on a model's context window.
    pub fn for_model(context_window: usize) -> Self {
        // Default allocation: 10% of window to Nexus
        let nexus_pct = 0.10;
        let nexus_budget = (context_window as f32 * nexus_pct) as usize;

        // Of the Nexus budget: 30% for soul, 70% for context
        let soul_budget = (nexus_budget as f32 * 0.30) as usize;
        let context_budget = nexus_budget - soul_budget;

        Self {
            model_context_window: context_window,
            nexus_allocation_pct: nexus_pct,
            soul_budget,
            context_budget,
        }
    }

    /// Estimate context window for known agent types/models.
    pub fn estimate_window(agent_type: &str) -> usize {
        match agent_type.to_lowercase().as_str() {
            "claude-code" | "amp" | "codex" | "sonnet-specialist" => 200_000,
            "gemini" | "gemini-analyzer" => 1_000_000,
            "opus-specialist" => 200_000,
            _ => 128_000, // Conservative default for GPT-4/Turbo/o1
        }
    }

    /// Rough estimation of tokens from text length (approx 4 chars per token).
    pub fn estimate_tokens(text: &str) -> usize {
        text.len() / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_budget_math() {
        let budget = TokenBudget::for_model(100_000);
        assert_eq!(budget.model_context_window, 100_000);
        // 10% of 100k = 10k
        // soul = 30% of 10k = 3k
        // context = 7k
        assert_eq!(budget.soul_budget, 3000);
        assert_eq!(budget.context_budget, 7000);
    }

    #[test]
    fn test_estimate_window() {
        assert_eq!(TokenBudget::estimate_window("claude-code"), 200_000);
        assert_eq!(TokenBudget::estimate_window("gemini"), 1_000_000);
        assert_eq!(TokenBudget::estimate_window("unknown"), 128_000);
    }
}
