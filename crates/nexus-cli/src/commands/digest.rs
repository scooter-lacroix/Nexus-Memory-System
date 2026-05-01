//! Digest command - produce short and long session summaries

use anyhow::Result;
use nexus_agent::{create_embedding_service, DigestResult, DigestService};
use nexus_core::config::AgentConfig;
use nexus_core::Config;
use nexus_llm::create_client_auto_with_fallback;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

/// Execute the digest command
///
/// Generates or retrieves short and long digests summarizing the session.
/// If digests already exist, they are returned without regeneration.
pub async fn execute(agent: String, session_key: String) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = match namespace_repo.get_by_name(&agent).await? {
        Some(ns) => ns,
        None => {
            println!("No namespace found for agent '{}'", agent);
            return Ok(());
        }
    };

    tracing::info!(
        "Creating digests for agent '{}', session '{}'",
        agent,
        session_key
    );

    let llm = create_client_auto_with_fallback()?;
    let embeddings = create_embedding_service(&config).await;
    let agent_config = AgentConfig::default();

    let digest_service = DigestService::new(agent_config, llm, embeddings);
    let DigestResult {
        short_id,
        long_id,
        source_count,
    } = digest_service
        .digest_session(namespace.id, &session_key, &memory_repo, false)
        .await?;

    println!(
        "Session Digest — agent: {}, session: {}",
        agent, session_key
    );
    println!("  Short digest (ID #{})", short_id);
    println!("  Long digest  (ID #{})", long_id);
    println!("  Source memories: {}", source_count);

    Ok(())
}
