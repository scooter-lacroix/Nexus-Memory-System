//! Store command implementation

use anyhow::Result;
use nexus_core::MemoryCategory;

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

    // Parse category
    let category = MemoryCategory::from_str(&category)
        .unwrap_or(MemoryCategory::General);

    // Parse labels
    let labels_vec: Vec<String> = labels
        .map(|l| l.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // TODO: Actually store the memory
    // For now, just print what we would store
    println!("Storing memory:");
    println!("  Agent: {}", agent);
    println!("  Category: {}", category);
    println!("  Content: {}", content);
    if !labels_vec.is_empty() {
        println!("  Labels: {}", labels_vec.join(", "));
    }

    tracing::info!("Memory stored successfully");
    Ok(())
}
