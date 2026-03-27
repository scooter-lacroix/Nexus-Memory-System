//! Dream command - manually trigger reflection/dreaming cycles.

use anyhow::Result;
use nexus_agent::runtime::{run_dream_cycle, DreamCycleRequest};
use nexus_core::config::AgentConfig;
use nexus_core::Config;
use nexus_core::PerspectiveKey;
use nexus_storage::{NamespaceRepository, StorageManager};

pub async fn execute(agent: String, session_key: Option<String>, format: String) -> Result<()> {
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

    let agent_config = AgentConfig {
        namespace: agent.clone(),
        ..AgentConfig::default()
    };
    let perspective = session_key.as_ref().map(|session_key| PerspectiveKey {
        observer: agent.clone(),
        subject: agent.clone(),
        session_key: Some(session_key.clone()),
    });

    let lease_owner = format!("cli-dream-{}", namespace.id);
    let llm = nexus_llm::create_client_auto_with_fallback()?;
    let embeddings = nexus_agent::create_embedding_service(&config).await;
    let processed = run_dream_cycle(
        storage.pool().clone(),
        &config.cognition,
        &agent_config,
        llm,
        embeddings,
        DreamCycleRequest {
            namespace_id: namespace.id,
            lease_owner: &lease_owner,
            perspective: perspective.as_ref(),
            session_key: session_key.as_deref(),
            reflect_reason: if perspective.is_some() {
                "manual_dream"
            } else {
                "namespace_dream"
            },
            digest_reason: "dream_digest",
        },
    )
    .await?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "processed_jobs": processed,
                "session_key": session_key,
            }))?
        );
    } else {
        println!(
            "Dream cycle complete for '{}': processed_jobs={}",
            agent, processed
        );
    }

    Ok(())
}
