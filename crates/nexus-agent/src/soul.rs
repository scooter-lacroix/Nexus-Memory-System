//! Soul management for unified user identity and cross-project learnings.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;

use nexus_llm::{ChatMessage, GenerateParams, LlmClient};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::prompts::{SOUL_EVALUATION_PROMPT, SOUL_NORMALIZATION_PROMPT};

/// Maximum token budget for the soul document.
const SOUL_MAX_TOKENS: usize = 2048;

/// Path to the unified soul.md file.
pub fn soul_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexus")
        .join("soul.md")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SoulCategory {
    IdentityPreference,
    TechnicalLearning,
    WorkingPattern,
    AgentNote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulCandidate {
    pub content: String,
    pub source_project: String,
    pub observation_count: u32,
    pub category: String,
    pub source_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedLearning {
    pub content: String,
    pub category: SoulCategory,
    pub confidence: f32,
    pub observation_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NormalizationResponse {
    pub normalized: Vec<NormalizedLearning>,
    pub discarded_count: usize,
}

pub struct SoulBuilder {
    llm: Arc<dyn LlmClient>,
}

impl SoulBuilder {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// Read the current soul document.
    pub fn read_current_soul(&self) -> String {
        let path = soul_path();
        fs::read_to_string(&path).unwrap_or_default()
    }

    /// Step 3: Normalization Gate
    pub async fn normalize_candidates(
        &self,
        candidates: &[SoulCandidate],
    ) -> anyhow::Result<Vec<NormalizedLearning>> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let candidates_json = serde_json::to_string_pretty(candidates)?;
        let system_prompt = SOUL_NORMALIZATION_PROMPT;
        let user_prompt = format!(
            "Normalize the following project-specific candidates for the user's Soul:\n\n{}",
            candidates_json
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        let params = GenerateParams {
            messages,
            json_mode: true,
            ..Default::default()
        };

        let response = self.llm.generate(params).await?;

        let content = response.content.trim();
        let clean_response = if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                if end > start {
                    &content[start..=end]
                } else {
                    content
                }
            } else {
                content
            }
        } else {
            content
        };

        match serde_json::from_str::<NormalizationResponse>(clean_response) {
            Ok(res) => {
                let filtered = res
                    .normalized
                    .into_iter()
                    .filter(|l| l.confidence >= 0.70)
                    .collect();
                Ok(filtered)
            }
            Err(e) => {
                warn!(
                    "Soul normalization parse failed; response length: {} chars, error: {}",
                    clean_response.len(),
                    e
                );
                Ok(Vec::new())
            }
        }
    }

    /// Step 4: Evaluation Gate & Rebuild
    pub async fn evaluate_and_merge(
        &self,
        current_soul: &str,
        normalized: &[NormalizedLearning],
    ) -> anyhow::Result<String> {
        if normalized.is_empty() {
            return Ok(current_soul.to_string());
        }

        let normalized_json = serde_json::to_string_pretty(normalized)?;
        let system_prompt = SOUL_EVALUATION_PROMPT;
        let user_prompt = format!(
            "CURRENT SOUL:\n{}\n\nNEW CANDIDATES:\n{}",
            current_soul, normalized_json
        );

        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ];

        let params = GenerateParams {
            messages,
            ..Default::default()
        };

        let response = self.llm.generate(params).await?;
        let new_soul = response.content;

        if !new_soul.contains("# Nexus Soul") || new_soul.len() < 50 {
            warn!("LLM returned invalid or empty soul document. Keeping existing.");
            return Ok(current_soul.to_string());
        }

        Ok(new_soul)
    }

    /// Full pipeline: normalize -> evaluate -> write
    pub async fn rebuild_soul(&self, candidates: &[SoulCandidate]) -> anyhow::Result<String> {
        info!(
            "Starting Soul rebuild pipeline with {} candidates",
            candidates.len()
        );

        let normalized = self.normalize_candidates(candidates).await?;
        debug!("Normalized into {} general learnings", normalized.len());

        let current = self.read_current_soul();
        let mut new_soul = self.evaluate_and_merge(&current, &normalized).await?;

        if new_soul.len() / 4 > SOUL_MAX_TOKENS {
            debug!("Soul exceeded budget. Attempting compression.");
            let messages = vec![
                ChatMessage::system("You are a text compression engine. Compress the following markdown soul profile while preserving all high-signal patterns. Keep it under 1500 words."),
                ChatMessage::user(&new_soul),
            ];
            let params = GenerateParams {
                messages,
                ..Default::default()
            };
            if let Ok(res) = self.llm.generate(params).await {
                let compressed = res.content;
                let has_header = compressed.contains("# Nexus Soul");
                let within_budget = compressed.len() / 4 <= SOUL_MAX_TOKENS;
                if has_header && within_budget {
                    new_soul = compressed;
                } else {
                    debug!(
                        has_header,
                        within_budget,
                        compressed_len = compressed.len(),
                        "Soul compression output failed validation; keeping pre-compression version"
                    );
                }
            }
        }

        let path = soul_path();
        if path.exists() {
            let bak = path.with_extension("md.bak");
            fs::copy(&path, &bak).with_context(|| {
                format!(
                    "Failed to create backup at {}. Aborting soul rebuild.",
                    bak.display()
                )
            })?;
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_extension("tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp_path)?;
            f.write_all(new_soul.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp_path, &path)?;

        info!(
            "Soul rebuild complete. Wrote {} bytes to {}",
            new_soul.len(),
            path.display()
        );
        Ok(new_soul)
    }
}
