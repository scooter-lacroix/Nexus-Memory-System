//! Reflect command - deterministic reinforcement and contradiction detection

use anyhow::Result;
use nexus_agent::{create_embedding_service, ReflectService};
use nexus_core::config::AgentConfig;
use nexus_core::Config;
use nexus_core::PerspectiveKey;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

/// Execute the reflect command
///
/// Runs a reflection cycle over memories aligned to the given perspective
/// (observer, subject, optional session_key). Reports reinforcement and
/// contradiction findings.
pub async fn execute(
    agent: String,
    observer: String,
    subject: String,
    session_key: Option<String>,
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

    let perspective = PerspectiveKey::new(observer, subject, session_key.filter(|s| !s.is_empty()));

    tracing::info!(
        "Running reflect cycle for namespace {} (perspective: {}/{})",
        namespace.name,
        perspective.observer,
        perspective.subject
    );

    let embeddings = create_embedding_service(&config).await;
    let reflect_service =
        ReflectService::new(AgentConfig::default(), config.cognition.clone(), embeddings);

    let result = reflect_service
        .reflect_perspective_cycle(namespace.id, &perspective, &memory_repo)
        .await?;

    println!(
        "Reflection complete — perspective: {} observing {}",
        perspective.observer, perspective.subject
    );
    println!("  Memories scanned:       {}", result.memories_scanned);
    println!("  Pairs compared:         {}", result.pairs_compared);
    println!("  Reinforcements found:   {}", result.reinforcements);
    println!("  Insights created:       {}", result.insights_created);
    println!(
        "  Contradictions found:   {}",
        result.contradictions_created
    );

    Ok(())
}
