//! Clean command - prune archived raw activity that has already been distilled.

use crate::commands::list::parse_time_filter;
use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use nexus_core::Config;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

pub async fn execute(
    agent: String,
    older_than: Option<String>,
    limit: usize,
    apply: bool,
) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = match namespace_repo.get_by_name(&agent).await? {
        Some(ns) => ns,
        None => {
            println!("No memories found for agent '{}'", agent);
            return Ok(());
        }
    };

    let cutoff = resolve_cutoff(older_than.as_deref())?;
    let total = memory_repo
        .count_archived_raw_cleanup_candidates(namespace.id, cutoff)
        .await?;
    let candidates = memory_repo
        .list_archived_raw_cleanup_candidates(namespace.id, cutoff, limit as i64)
        .await?;

    if total == 0 {
        println!(
            "No archived raw-activity memories are eligible for cleanup in '{}'.",
            agent
        );
        return Ok(());
    }

    println!(
        "Cleanup candidates for '{}' older than {}: {} total (showing {}).",
        agent,
        cutoff.format("%Y-%m-%d %H:%M:%S UTC"),
        total,
        candidates.len()
    );

    for memory in candidates.iter().take(10) {
        let summary_id = memory
            .metadata
            .get("distillation")
            .and_then(|v| v.get("summary_memory_id"))
            .and_then(|v| v.as_i64())
            .unwrap_or_default();
        let preview: String = memory.content.chars().take(120).collect();
        println!(
            "  #{} -> summary #{} [{}] {}",
            memory.id,
            summary_id,
            memory.created_at.format("%Y-%m-%d %H:%M"),
            preview
        );
    }

    if !apply {
        println!();
        println!("Dry run only. Re-run with --apply to delete the listed candidates.");
        return Ok(());
    }

    let ids: Vec<i64> = candidates.iter().map(|memory| memory.id).collect();
    let deleted = memory_repo.delete_batch(&ids).await?;
    println!(
        "Deleted {} archived raw-activity memories from '{}'.",
        deleted, agent
    );

    Ok(())
}

fn resolve_cutoff(value: Option<&str>) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Some(parsed) = parse_time_filter(value)? {
        return Ok(parsed);
    }

    if value.is_some() {
        Err(anyhow!("invalid older-than filter"))
    } else {
        Ok(Utc::now() - Duration::days(7))
    }
}
