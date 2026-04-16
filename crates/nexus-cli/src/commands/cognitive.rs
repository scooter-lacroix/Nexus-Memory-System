//! Cognitive system inspection commands.

use anyhow::Result;
use nexus_agent::activity_monitor::ActivityMonitor;
use nexus_agent::soul::soul_path;
use nexus_agent::CognitiveCache;
use nexus_core::ProjectIdentity;

#[derive(clap::Subcommand)]
pub enum CognitiveCommands {
    /// Display cognitive system status
    Status {
        /// Output format (human, json)
        #[arg(short, long, default_value = "human")]
        format: String,
    },
    /// Show cognitive cache contents for the current project
    CacheShow {
        /// Show entries for a specific project path (defaults to CWD)
        #[arg(long)]
        project: Option<String>,
        /// Output format (human, json)
        #[arg(short, long, default_value = "human")]
        format: String,
    },
}

pub async fn execute(command: CognitiveCommands) -> Result<()> {
    match command {
        CognitiveCommands::Status { format } => execute_status(&format).await,
        CognitiveCommands::CacheShow { project, format } => {
            execute_cache_show(project.as_deref(), &format).await
        }
    }
}

async fn execute_status(format: &str) -> Result<()> {
    // 1. Soul.md info
    let soul = soul_path();
    let soul_exists = soul.exists();
    let soul_size = if soul_exists {
        std::fs::metadata(&soul).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    // 2. Activity monitor
    let monitor = ActivityMonitor::load();
    let last_activity = monitor.activity_log.last().copied();
    let detected_sleep_hour = monitor.detected_sleep_hour;
    let last_deep_dream = monitor.last_deep_dream;
    let deep_dream_cooldown_secs = monitor.deep_dream_cooldown.num_seconds();
    let activity_log_count = monitor.activity_log.len();
    let should_dream = monitor.should_deep_dream();

    // 3. Per-project cache stats (current project)
    let cwd = std::env::current_dir()?;
    let project = ProjectIdentity::resolve(&cwd);
    let nexus_dir = project.root_dir.join(".nexus");
    let cache = CognitiveCache::load_or_init(&nexus_dir);
    let hot_count = cache.hot_cache.entries.len();
    let cold_count = cache.cold_index.entries.len();

    if format == "json" {
        let output = serde_json::json!({
            "soul": {
                "path": soul.to_string_lossy(),
                "exists": soul_exists,
                "size_bytes": soul_size,
            },
            "activity_monitor": {
                "last_activity": last_activity.map(|t| t.to_rfc3339()),
                "detected_sleep_hour": detected_sleep_hour,
                "last_deep_dream": last_deep_dream.map(|t| t.to_rfc3339()),
                "deep_dream_cooldown_secs": deep_dream_cooldown_secs,
                "activity_log_count": activity_log_count,
                "should_deep_dream": should_dream,
            },
            "project": {
                "name": project.display_name,
                "root_dir": project.root_dir.to_string_lossy(),
                "git_remote": project.git_remote,
            },
            "cache": {
                "hot_entries": hot_count,
                "cold_entries": cold_count,
            },
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("=== Cognitive System Status ===\n");

        println!("Soul Document:");
        println!("  Path:    {}", soul.display());
        println!("  Exists:  {}", soul_exists);
        if soul_exists {
            println!("  Size:    {} bytes", soul_size);
        }
        println!();

        println!("Activity Monitor:");
        println!(
            "  Last Activity:       {}",
            last_activity
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "none".to_string())
        );
        println!(
            "  Detected Sleep Hour: {}",
            detected_sleep_hour
                .map(|h| format!("{:02}:00", h))
                .unwrap_or_else(|| "not detected".to_string())
        );
        println!(
            "  Last Deep Dream:     {}",
            last_deep_dream
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "never".to_string())
        );
        println!(
            "  Deep Dream Cooldown: {}h",
            deep_dream_cooldown_secs / 3600
        );
        println!("  Activity Log Size:   {} entries", activity_log_count);
        println!("  Should Deep Dream:   {}", should_dream);
        println!();

        println!(
            "Project: {} ({})",
            project.display_name,
            project.root_dir.display()
        );
        if let Some(ref remote) = project.git_remote {
            println!("  Git Remote: {}", remote);
        }
        println!();

        println!("Cognitive Cache:");
        println!("  Hot Entries:  {}", hot_count);
        println!("  Cold Entries: {}", cold_count);
    }

    Ok(())
}

async fn execute_cache_show(project_path: Option<&str>, format: &str) -> Result<()> {
    let cwd = match project_path {
        Some(p) => std::path::PathBuf::from(p),
        None => std::env::current_dir()?,
    };
    let project = ProjectIdentity::resolve(&cwd);
    let nexus_dir = project.root_dir.join(".nexus");
    let cache = CognitiveCache::load_or_init(&nexus_dir);

    if format == "json" {
        let output = serde_json::json!({
            "project": {
                "name": project.display_name,
                "root_dir": project.root_dir.to_string_lossy(),
            },
            "hot_cache": cache.hot_cache.entries.iter().map(|e| {
                serde_json::json!({
                    "memory_id": e.memory_id,
                    "tier": format!("{:?}", e.tier),
                    "relevance_score": e.relevance_score,
                    "pinned": e.pinned,
                    "hot_streak": e.hot_streak,
                    "content_preview": truncate_str(&e.content, 80),
                })
            }).collect::<Vec<_>>(),
            "cold_index_size": cache.cold_index.entries.len(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("=== Cognitive Cache: {} ===\n", project.display_name);

        if cache.hot_cache.entries.is_empty() {
            println!("Hot cache: (empty)");
        } else {
            println!("Hot Cache ({} entries):", cache.hot_cache.entries.len());
            println!(
                "{:<12} {:<8} {:<10} {:<7} {:<10} Content",
                "Memory ID", "Tier", "Score", "Pinned", "Streak"
            );
            println!("{}", "-".repeat(80));
            for entry in &cache.hot_cache.entries {
                let preview = truncate_str(&entry.content, 80);
                println!(
                    "{:<12} {:<8} {:<10.3} {:<7} {:<10} {}",
                    entry.memory_id,
                    format!("{:?}", entry.tier),
                    entry.relevance_score,
                    entry.pinned,
                    entry.hot_streak,
                    preview
                );
            }
        }

        println!();
        println!("Cold Index: {} entries", cache.cold_index.entries.len());
        if let Some(last_reindexed) = cache.cold_index.last_reindexed {
            println!("  Last Reindexed: {}", last_reindexed.to_rfc3339());
        }
    }

    Ok(())
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.replace('\n', " ")
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated.replace('\n', " "))
    }
}
