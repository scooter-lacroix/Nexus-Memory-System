//! Store command implementation

use anyhow::Result;
use nexus_core::{Config, MemoryCategory};
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

/// Execute the store command
pub async fn execute(
    content: String,
    agent: String,
    category: String,
    labels: Option<String>,
) -> Result<()> {
    tracing::info!("Storing memory for agent: {}", agent);
    tracing::debug!("Content: {}", content);
    tracing::debug!("Category: {}", category);

    let config = Config::from_env()?;

    // Parse category
    let category = MemoryCategory::from_str(&category).unwrap_or(MemoryCategory::General);

    // Parse labels
    let labels_vec: Vec<String> = labels
        .map(|l| l.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = namespace_repo.get_or_create(&agent, &agent).await?;
    let memory = memory_repo
        .store(
            namespace.id,
            &content,
            &category,
            None,
            &labels_vec,
            &serde_json::json!({}),
            None,
            None,
        )
        .await?;

    println!("Stored memory:");
    println!("  ID: {}", memory.id);
    println!("  Agent: {}", agent);
    println!("  Category: {}", category);
    if !labels_vec.is_empty() {
        println!("  Labels: {}", labels_vec.join(", "));
    }

    tracing::info!("Memory stored successfully");
    Ok(())
}
