//! Recall command - retrieve context-relevant memories for agents

use anyhow::Result;
use nexus_core::Config;
use nexus_storage::repository::ListMemoryFilters;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

pub async fn execute(
    query: String,
    agent: String,
    limit: usize,
    category: Option<String>,
    format: String,
    include_raw: bool,
) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = match namespace_repo.get_by_name(&agent).await? {
        Some(ns) => ns,
        None => {
            if format == "json" {
                println!("[]");
            }
            return Ok(());
        }
    };

    let filtered = memory_repo
        .list_filtered(
            namespace.id,
            ListMemoryFilters {
                category: category.as_deref(),
                since: None,
                until: None,
                content_like: Some(&query),
                include_raw,
                limit: limit as i64,
                offset: 0,
            },
        )
        .await?;

    match format.as_str() {
        "json" => {
            let items: Vec<serde_json::Value> = filtered
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "content": r.content,
                        "category": r.category,
                        "created_at": r.created_at.to_string(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        "compact" => {
            for r in &filtered {
                println!("[{}] {}", r.category, r.content);
            }
        }
        _ => {
            if filtered.is_empty() {
                println!("No relevant memories found.");
                return Ok(());
            }
            println!("Recalled {} relevant memories:\n", filtered.len());
            for r in &filtered {
                println!("──────────────────────────────────────");
                println!("ID: {} | {} | {}", r.id, r.category, r.created_at);
                println!("{}", r.content);
                println!();
            }
        }
    }

    Ok(())
}
