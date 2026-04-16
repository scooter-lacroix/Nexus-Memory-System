//! Soul management commands — show, edit, and rebuild the unified soul.md identity file.

use std::fs;
use std::process::Command;

use anyhow::{bail, Context, Result};
use nexus_agent::activity_monitor::ActivityMonitor;
use nexus_agent::soul::{soul_path, SoulBuilder, SoulCandidate};
use nexus_core::types::CognitiveLevel;
use nexus_core::Config;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

#[derive(clap::Subcommand)]
pub enum SoulCommands {
    /// Display current soul.md content
    Show,

    /// Open soul.md in $EDITOR
    Edit,

    /// Trigger a soul rebuild from cross-project patterns
    Rebuild {
        /// Force rebuild even if cooldown hasn't elapsed
        #[arg(long)]
        force: bool,
    },
}

pub async fn execute(command: SoulCommands) -> Result<()> {
    match command {
        SoulCommands::Show => execute_show(),
        SoulCommands::Edit => execute_edit(),
        SoulCommands::Rebuild { force } => execute_rebuild(force).await,
    }
}

fn execute_show() -> Result<()> {
    let path = soul_path();

    if !path.exists() {
        println!("No soul.md found at {}", path.display());
        println!("Run `nexus soul rebuild` to generate one from your cross-project learnings.");
        return Ok(());
    }

    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;

    if content.trim().is_empty() {
        println!("soul.md exists but is empty.");
        println!("Run `nexus soul rebuild` to populate it.");
    } else {
        println!("{}", content);
    }

    Ok(())
}

fn execute_edit() -> Result<()> {
    let path = soul_path();

    // Ensure the file exists before opening the editor.
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::write(&path, "").with_context(|| format!("Failed to create {}", path.display()))?;
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

    // Split editor command to handle cases like "code --wait"
    let mut parts = editor.split_whitespace();
    let editor_bin = parts.next().unwrap_or("vi");
    let status = Command::new(editor_bin)
        .args(parts)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to launch editor '{}'", editor))?;

    if !status.success() {
        bail!("Editor exited with non-zero status");
    }

    Ok(())
}

async fn execute_rebuild(force: bool) -> Result<()> {
    // Check cooldown unless forced.
    if !force {
        let monitor = ActivityMonitor::load();
        if !monitor.should_deep_dream() {
            println!("Soul rebuild cooldown has not elapsed. Use --force to override.");
            return Ok(());
        }
    }

    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let ns_repo = NamespaceRepository::new(storage.pool().clone());
    let mem_repo = MemoryRepository::new(storage.pool().clone());

    let namespaces = ns_repo.list_all().await?;
    if namespaces.is_empty() {
        println!("No namespaces found. Store some memories first.");
        return Ok(());
    }

    // Gather derived and contradiction memories across all namespaces as soul candidates.
    let mut candidates = Vec::new();
    for ns in &namespaces {
        for level in [CognitiveLevel::Derived, CognitiveLevel::Contradiction] {
            let memories = mem_repo
                .get_by_cognitive_level(ns.id, level, 50)
                .await
                .with_context(|| {
                    format!("Failed to fetch {:?} memories for '{}'", level, ns.name)
                })?;

            for mem in memories {
                candidates.push(SoulCandidate {
                    content: mem.content.clone(),
                    source_project: ns.name.clone(),
                    observation_count: mem.access_count.max(1) as u32,
                    category: mem.category.to_string(),
                    source_agent: nexus_core::CognitiveMetadata::from_metadata(&mem.metadata)
                        .map(|c| c.observer.clone())
                        .unwrap_or_else(|| ns.name.clone()),
                });
            }
        }
    }

    if candidates.is_empty() {
        println!(
            "No derived or contradiction memories found across {} namespace(s).",
            namespaces.len()
        );
        println!("Run dream cycles first to produce higher-order memories.");
        return Ok(());
    }

    println!(
        "Rebuilding soul from {} candidate(s) across {} namespace(s)...",
        candidates.len(),
        namespaces.len()
    );

    let llm = nexus_llm::create_client_auto_with_fallback()?;
    let builder = SoulBuilder::new(llm);
    let result = builder.rebuild_soul(&candidates).await?;

    // Update activity monitor to record this deep dream.
    let mut monitor = ActivityMonitor::load();
    monitor.last_deep_dream = Some(chrono::Utc::now());
    monitor.save()?;

    println!(
        "Soul rebuild complete. Wrote {} bytes to {}",
        result.len(),
        soul_path().display()
    );

    Ok(())
}
