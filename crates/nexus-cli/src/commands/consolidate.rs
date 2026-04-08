//! Consolidate command - manually trigger memory consolidation

use anyhow::Result;
use nexus_agent::dream_cycle::{run_dream_cycle, DreamCycleRequest};
use nexus_core::config::AgentConfig;
use nexus_core::Config;
use nexus_llm::create_client_auto_with_fallback;
use nexus_storage::{NamespaceRepository, StorageManager};

pub async fn execute(agent: String) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());

    let namespace = match namespace_repo.get_by_name(&agent).await? {
        Some(ns) => ns,
        None => {
            println!("No namespace found for agent '{}'", agent);
            return Ok(());
        }
    };

    let llm = create_client_auto_with_fallback()?;
    let embeddings = nexus_agent::create_embedding_service(&config).await;
    let agent_config = AgentConfig::default();

    println!("Running a queued dream cycle for agent '{}'...", agent);

    let lease_owner = format!("cli-consolidate-{}", namespace.id);
    match run_dream_cycle(
        storage.pool().clone(),
        &config.cognition,
        &agent_config,
        llm,
        embeddings,
        DreamCycleRequest {
            namespace_id: namespace.id,
            lease_owner: &lease_owner,
            perspective: None,
            session_key: None,
            reflect_reason: "namespace_dream",
            digest_reason: "dream_digest",
        },
    )
    .await
    {
        Ok(processed) if processed > 0 => {
            println!("Dream cycle processed {} cognition jobs.", processed);
        }
        Ok(_) => {
            println!("No dream work was queued or ready to run.");
        }
        Err(e) => {
            eprintln!("Dream cycle failed: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}
