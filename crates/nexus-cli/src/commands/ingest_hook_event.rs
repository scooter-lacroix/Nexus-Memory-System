//! Ingest hook event command implementation
//!
//! Reads raw JSON from stdin, normalizes, derives candidates,
//! enriches via LLM, and persists accepted memories.

use std::collections::HashSet;
use std::io::Read;

use anyhow::{Context, Result};
use nexus_core::Config;
use nexus_hooks::candidate::derive_candidates;
use nexus_hooks::claude_payload::normalize_claude_payload;
use nexus_hooks::enrichment::EnrichmentService;
use nexus_hooks::retry_buffer::RetryBuffer;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

/// Execute the ingest-hook-event command
pub async fn execute(agent: String, event: String, format: String) -> Result<()> {
    // 1. Read raw JSON from stdin
    let mut raw_input = String::new();
    std::io::stdin()
        .read_to_string(&mut raw_input)
        .context("Failed to read stdin")?;

    let raw: serde_json::Value =
        serde_json::from_str(&raw_input).context("Failed to parse stdin as JSON")?;

    tracing::info!(agent = %agent, event = %event, format = %format, "Processing hook event");

    // 2. Normalize the payload
    let normalized = if format.is_empty() || format == "claude" {
        normalize_claude_payload(&agent, &event, &raw)
    } else {
        tracing::warn!(
            format = %format,
            "No normalizer for format '{}', falling back to Claude normalization",
            format
        );
        normalize_claude_payload(&agent, &event, &raw)
    };
    tracing::debug!(tool_name = ?normalized.tool_name, "Normalized event");

    // 3. Initialize storage
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = namespace_repo.get_or_create(&agent, &agent).await?;

    // 4. Derive high-signal candidates
    let mut fingerprints = HashSet::new();
    let candidates = derive_candidates(&normalized, &mut fingerprints);

    if candidates.is_empty() {
        println!("No high-signal candidates derived from {}/{}", agent, event);
        return Ok(());
    }

    tracing::info!(candidate_count = candidates.len(), "Derived candidates");

    // 5. Enrich via LLM
    let enrichment_service: EnrichmentService = match EnrichmentService::new() {
        Ok(svc) => svc,
        Err(e) => {
            tracing::warn!(error = %e, "LLM enrichment unavailable, buffering for retry");
            let retry_buffer = RetryBuffer::new();
            retry_buffer.write_failed(&normalized, &candidates, &e.to_string())?;
            println!(
                "LLM enrichment unavailable. {} candidates buffered for retry.",
                candidates.len()
            );
            return Ok(());
        }
    };

    let batch = match enrichment_service
        .enrich_candidates(&candidates, &normalized)
        .await
    {
        Ok(batch) => batch,
        Err(e) => {
            tracing::warn!(error = %e, "LLM enrichment failed, buffering for retry");
            let retry_buffer = RetryBuffer::new();
            retry_buffer.write_failed(&normalized, &candidates, &e.to_string())?;
            println!(
                "LLM enrichment failed. {} candidates buffered for retry.",
                candidates.len()
            );
            return Ok(());
        }
    };

    // 6. Persist accepted memories
    let model_name = enrichment_service.model_name();
    let result = nexus_hooks::persistence::persist_enriched_memories(
        namespace.id,
        &normalized,
        &batch,
        &memory_repo,
        model_name,
    )
    .await
    .context("Failed to persist enriched memories")?;

    // 7. Print summary
    println!("Stored {} memories from {}/{}", result.stored, agent, event);
    for (cat, count) in &result.categories {
        println!("- {}: {}", cat, count);
    }
    if result.skipped > 0 {
        println!("- skipped: {} rejected by LLM", result.skipped);
    }

    tracing::info!(
        stored = result.stored,
        skipped = result.skipped,
        "Ingestion complete"
    );
    Ok(())
}
