//! Lineage command - inspect evidence lineage for a memory.

use anyhow::Result;
use nexus_core::Config;
use nexus_storage::{MemoryRepository, StorageManager};

pub async fn execute(memory_id: i64) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let target = match memory_repo.get_by_id(memory_id).await? {
        Some(m) => m,
        None => {
            println!("Memory #{} not found.", memory_id);
            return Ok(());
        }
    };

    println!("Memory #{}", memory_id);
    println!("=============");
    println!("Content: {}", truncate(target.content.as_str(), 200));
    println!("Created: {}", target.created_at);
    println!(
        "Level: {}",
        target
            .metadata
            .get("cognitive")
            .and_then(|c| c.get("level"))
            .and_then(|v| v.as_str())
            .unwrap_or("(none)")
    );
    println!();

    let lineage = memory_repo.load_lineage(memory_id).await?;

    if lineage.is_empty() {
        println!("No lineage records found for memory #{}.", memory_id);
        println!("This may be an original (non-derived) entry.");
        return Ok(());
    }

    // Separate into upstream (this memory was derived from) and downstream
    // (other memories were derived from this one).
    let upstream: Vec<_> = lineage
        .iter()
        .filter(|e| e.derived_memory_id == memory_id)
        .collect();
    let downstream: Vec<_> = lineage
        .iter()
        .filter(|e| e.source_memory_id == memory_id)
        .collect();

    if !upstream.is_empty() {
        println!("Upstream sources ({} records)", upstream.len());
        println!("--------");
        for entry in &upstream {
            let source = memory_repo
                .get_by_id(entry.source_memory_id)
                .await
                .ok()
                .flatten();

            let source_preview = match &source {
                Some(m) => truncate(m.content.as_str(), 100),
                None => "(deleted or inaccessible)".to_string(),
            };

            println!(
                "  #{} [{}] → #{}",
                entry.source_memory_id, entry.evidence_role, entry.derived_memory_id
            );
            println!("    {}", source_preview);
            println!();
        }
    }

    if !downstream.is_empty() {
        println!(
            "Downstream ({} memories derived from #{}):",
            downstream.len(),
            memory_id
        );
        println!("----------");
        for entry in &downstream {
            let derived = memory_repo
                .get_by_id(entry.derived_memory_id)
                .await
                .ok()
                .flatten();

            let derived_preview = match &derived {
                Some(m) => truncate(m.content.as_str(), 100),
                None => "(deleted or inaccessible)".to_string(),
            };

            println!(
                "  #{} [{}] ← #{}",
                entry.derived_memory_id, entry.evidence_role, entry.source_memory_id
            );
            println!("    {}", derived_preview);
            println!();
        }
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
