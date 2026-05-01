//! Derive command - convert a raw memory into explicit observations

use anyhow::Result;
use nexus_agent::{create_embedding_service, DeriveService};
use nexus_core::config::AgentConfig;
use nexus_core::Config;
use nexus_llm::create_client_auto_with_fallback;
use nexus_storage::{MemoryRepository, StorageManager};

/// Execute the derive command
///
/// Derives explicit observations from a raw memory using LLM-based analysis.
/// The memory must exist and belong to an accessible namespace.
pub async fn execute(memory_id: i64) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let memory = match memory_repo.get_by_id(memory_id).await? {
        Some(m) => m,
        None => {
            println!("Memory #{} not found.", memory_id);
            return Ok(());
        }
    };

    tracing::info!(
        "Deriving observations from memory #{} (namespace ID: {})",
        memory_id,
        memory.namespace_id
    );

    let llm = create_client_auto_with_fallback()?;
    let embeddings = create_embedding_service(&config).await;
    let agent_config = AgentConfig::default();

    let derive_service = DeriveService::new(agent_config, llm, embeddings);
    let derived_ids = derive_service.derive_memory(&memory, &memory_repo).await?;

    if derived_ids.is_empty() {
        println!(
            "No new observations derived from memory #{} (already derived or not derivable).",
            memory_id
        );
    } else {
        println!(
            "Derived {} observation(s) from memory #{}:",
            derived_ids.len(),
            memory_id
        );
        for id in &derived_ids {
            println!("  #{}", id);
        }
    }

    Ok(())
}
