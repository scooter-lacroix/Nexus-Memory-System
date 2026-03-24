//! Store command implementation

use anyhow::{Context, Result};
use nexus_core::{Config, MemoryCategory};
use nexus_storage::repository::StoreMemoryParams;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

/// Execute the store command
pub async fn execute(
    content: String,
    agent: String,
    category: String,
    labels: Option<String>,
    metadata_json: Option<String>,
    memory_lane_type: Option<String>,
) -> Result<()> {
    tracing::info!("Storing memory for agent: {}", agent);
    tracing::debug!("Content: {}", content);
    tracing::debug!("Category: {}", category);

    let config = Config::from_env()?;

    // Parse category
    let category = MemoryCategory::parse(&category).unwrap_or(MemoryCategory::General);

    // Parse labels
    let labels_vec: Vec<String> = labels
        .map(|l| l.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Parse metadata JSON
    let metadata: serde_json::Value = match metadata_json {
        Some(ref json_str) => {
            serde_json::from_str(json_str).context("Failed to parse --metadata-json")?
        }
        None => serde_json::json!({}),
    };

    // Parse memory lane type
    let lane_type = memory_lane_type
        .as_deref()
        .and_then(nexus_core::MemoryLaneType::parse);

    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = namespace_repo.get_or_create(&agent, &agent).await?;
    let memory = memory_repo
        .store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: &content,
            category: &category,
            memory_lane_type: lane_type.as_ref(),
            labels: &labels_vec,
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        })
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
