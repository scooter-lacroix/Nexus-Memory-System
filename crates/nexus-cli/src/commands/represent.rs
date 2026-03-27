//! Represent command - build and inspect a working representation for a query.

use anyhow::Result;
use nexus_agent::RepresentationService;
use nexus_core::{Config, PerspectiveKey, WorkingRepresentationRequest};
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

pub async fn execute(
    agent: String,
    query: Option<String>,
    observer: Option<String>,
    subject: Option<String>,
    session_key: Option<String>,
    max_items: usize,
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
            println!("No namespace found for agent '{}'.", agent);
            return Ok(());
        }
    };

    let perspective = if let (Some(obs), Some(sub)) = (observer, subject) {
        Some(PerspectiveKey::new(
            obs,
            sub,
            session_key.filter(|s| !s.is_empty()),
        ))
    } else {
        None
    };

    let request = WorkingRepresentationRequest {
        namespace_id: namespace.id,
        perspective,
        query,
        max_items,
        include_raw,
        include_recent: true,
        include_semantic: true,
        include_derived: true,
        include_digests: true,
        include_contradictions: true,
    };

    let service = RepresentationService::new();
    let representation = service.build(&request, &memory_repo).await?;

    let total = representation.digests.len()
        + representation.recent.len()
        + representation.semantic.len()
        + representation.derived.len()
        + representation.contradictions.len();

    if total == 0 {
        println!("No memories found for the given representation parameters.");
        return Ok(());
    }

    println!(
        "Working Representation ({} items, max {})",
        total, max_items
    );
    println!("=========================================\n");

    if !representation.digests.is_empty() {
        println!("Digests ({})", representation.digests.len());
        println!("--------");
        for m in &representation.digests {
            println!("  [#{}] {}", m.id, truncate(m.content.as_str(), 120));
        }
        println!();
    }

    if !representation.derived.is_empty() {
        println!("Derived Insights ({})", representation.derived.len());
        println!("-----------------");
        for m in &representation.derived {
            println!("  [#{}] {}", m.id, truncate(m.content.as_str(), 120));
        }
        println!();
    }

    if !representation.contradictions.is_empty() {
        println!("Contradictions ({})", representation.contradictions.len());
        println!("---------------");
        for m in &representation.contradictions {
            println!("  [#{}] {}", m.id, truncate(m.content.as_str(), 120));
        }
        println!();
    }

    if !representation.semantic.is_empty() {
        println!("Semantic Matches ({})", representation.semantic.len());
        println!("----------------");
        for m in &representation.semantic {
            println!(
                "  [#{}] {} (score: {:.3})",
                m.id,
                truncate(m.content.as_str(), 120),
                m.similarity_score.unwrap_or(0.0)
            );
        }
        println!();
    }

    if include_raw || !representation.recent.is_empty() {
        println!("Recent ({})", representation.recent.len());
        println!("------");
        for m in &representation.recent {
            println!("  [#{}] {}", m.id, truncate(m.content.as_str(), 120));
        }
        println!();
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
