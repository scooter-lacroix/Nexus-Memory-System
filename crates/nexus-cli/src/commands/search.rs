//! Search command implementation

use anyhow::Result;
use nexus_core::Config;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

pub async fn execute(query: String, agent: String, limit: usize, include_raw: bool) -> Result<()> {
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

    let rows = memory_repo
        .search_by_text(namespace.id, &query, limit as i32, include_raw)
        .await?;

    if rows.is_empty() {
        println!("No memories matching '{}' for agent '{}'", query, agent);
        return Ok(());
    }

    println!("Found {} memories matching '{}':\n", rows.len(), query);
    for row in &rows {
        println!("──────────────────────────────────────");
        println!(
            "ID: {} | Category: {} | {}",
            row.id, row.category, row.created_at
        );
        let preview: String = row.content.chars().take(300).collect();
        if row.content.chars().count() > 300 {
            println!("  {preview}...");
        } else {
            println!("  {preview}");
        }
        println!();
    }

    Ok(())
}
