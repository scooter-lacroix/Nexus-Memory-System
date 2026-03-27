//! Digest command - inspect or rebuild session digests.

use anyhow::Result;
use nexus_agent::{create_embedding_service, DigestService};
use nexus_core::config::AgentConfig;
use nexus_core::Config;
use nexus_llm::create_client_auto_with_fallback;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

const DIGEST_KIND_SHORT: &str = "short";
const DIGEST_KIND_LONG: &str = "long";

pub async fn execute(
    agent: String,
    session_key: String,
    force: bool,
    format: String,
) -> Result<()> {
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

    if force {
        let llm = create_client_auto_with_fallback()?;
        let embeddings = create_embedding_service(&config).await;
        let agent_config = AgentConfig {
            namespace: agent.clone(),
            ..AgentConfig::default()
        };
        let service = DigestService::new(agent_config, llm, embeddings);
        let result = service
            .digest_session(namespace.id, &session_key, &memory_repo, true)
            .await?;

        println!(
            "Rebuilt digests for session '{}' in '{}': short=#{}, long=#{}, sources={}",
            session_key, agent, result.short_id, result.long_id, result.source_count
        );
    }

    let short = memory_repo
        .latest_digest_for_session(namespace.id, &session_key, DIGEST_KIND_SHORT)
        .await?;
    let long = memory_repo
        .latest_digest_for_session(namespace.id, &session_key, DIGEST_KIND_LONG)
        .await?;

    match format.as_str() {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "agent": agent,
                    "session_key": session_key,
                    "short": short,
                    "long": long,
                }))?
            );
        }
        _ => {
            if let Some(short) = short {
                println!("Short digest (#{}):\n{}\n", short.id, short.content);
            } else {
                println!("No short digest found for session '{}'.", session_key);
            }

            if let Some(long) = long {
                println!("Long digest (#{}):\n{}", long.id, long.content);
            } else {
                println!("No long digest found for session '{}'.", session_key);
            }
        }
    }

    Ok(())
}
