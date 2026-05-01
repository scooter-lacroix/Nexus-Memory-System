//! Reference injection system for agent configuration files.

use nexus_core::fsutil::atomic_write;
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Target for injecting Nexus references into an agent's configuration.
#[derive(Debug, Clone)]
pub struct AgentInjectionTarget {
    pub agent_type: String,
    pub global_config: Option<PathBuf>,
    pub project_config_filename: String,
}

/// Sentinel markers for Nexus-managed blocks.
pub const NEXUS_BLOCK_START: &str = "<!-- NEXUS:START -->";
pub const NEXUS_BLOCK_END: &str = "<!-- NEXUS:END -->";

impl AgentInjectionTarget {
    /// Get known agent injection targets.
    pub fn known_agents() -> Vec<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        vec![
            Self {
                agent_type: "claude-code".to_string(),
                global_config: Some(home.join(".claude").join("CLAUDE.md")),
                project_config_filename: "CLAUDE.md".to_string(),
            },
            Self {
                agent_type: "amp".to_string(),
                global_config: Some(home.join(".config").join("amp").join("AGENTS.md")),
                project_config_filename: "AGENTS.md".to_string(),
            },
            Self {
                agent_type: "codex".to_string(),
                global_config: Some(home.join(".config").join("codex").join("AGENTS.md")),
                project_config_filename: "AGENTS.md".to_string(),
            },
            Self {
                agent_type: "gemini".to_string(),
                global_config: Some(home.join(".gemini").join("GEMINI.md")),
                project_config_filename: "GEMINI.md".to_string(),
            },
            Self {
                agent_type: "pi-mono".to_string(),
                global_config: Some(home.join(".pi").join("agent").join("AGENTS.md")),
                project_config_filename: ".pi/AGENTS.md".to_string(),
            },
            Self {
                agent_type: "droid".to_string(),
                global_config: Some(home.join(".factory").join("settings.json")),
                project_config_filename: ".factory/settings.json".to_string(),
            },
        ]
    }

    /// Find injection target by agent type.
    pub fn find(agent_type: &str) -> Option<Self> {
        Self::known_agents()
            .into_iter()
            .find(|t| t.agent_type == agent_type || agent_type.contains(&t.agent_type))
    }
}

/// Inject Nexus references into a config file.
pub fn inject_reference(
    config_file: &Path,
    soul_path: &Path,
    context_path: &Path,
    agent_type: Option<&str>,
) -> io::Result<()> {
    if !config_file.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_file)?;
    let original_content = content.clone();

    // Detect JSON config files (Droid's settings.json, etc.)
    let is_json = config_file.extension()
        .map(|ext| ext == "json")
        .unwrap_or(false);

    let new_content = if is_json {
        // For JSON, add settings if not already present
        inject_into_json(&content, config_file, soul_path, context_path, agent_type)?
    } else {
        // Standard markdown-style injection
        let block = format!(
            "{}\n\
            ## Nexus Memory Substrate\n\
            - Identity: [{soul_name}]({soul_path})\n\
            - Project Context: [{context_name}]({context_path})\n\
            {}",
            NEXUS_BLOCK_START,
            NEXUS_BLOCK_END,
            soul_name = "Soul",
            soul_path = soul_path.to_string_lossy(),
            context_name = "Project Context",
            context_path = context_path.to_string_lossy(),
        );

        if let (Some(start), Some(end)) = (
            content.find(NEXUS_BLOCK_START),
            content.find(NEXUS_BLOCK_END),
        ) {
            if start >= end {
                let stripped = content
                    .replace(NEXUS_BLOCK_START, "")
                    .replace(NEXUS_BLOCK_END, "");
                let mut updated = stripped.trim_end().to_string();
                updated.push('\n');
                updated.push_str(&block);
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated
            } else {
                let mut updated = content[..start].to_string();
                updated.push_str(&block);
                updated.push_str(&content[end + NEXUS_BLOCK_END.len()..]);
                updated
            }
        } else {
            let mut updated = content;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(&block);
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated
        }
    };

    if new_content != original_content {
        atomic_write(config_file, &new_content)?;
        debug!("Injected Nexus reference into {}", config_file.display());
    }

    Ok(())
}

/// Inject Nexus references into a JSON config file (e.g., Droid's settings.json).
/// For JSON files, we add a `"nexus"` meta field or add to hooks structure.
fn inject_into_json(
    content: &str,
    _config_file: &Path,
    soul_path: &Path,
    context_path: &Path,
    _agent_type: Option<&str>,
) -> io::Result<String> {
    use serde_json::{Map, Value};

    let mut json: Value = serde_json::from_str(content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Determine relative paths for injection
    let soul_name = "Soul";
    let context_name = "Project Context";

    // Create the nexus reference object
    let nexus_obj = serde_json::json!({
        "identity": {
            "name": soul_name,
            "path": soul_path.to_string_lossy(),
            "source": "soul.md"
        },
        "projectContext": {
            "name": context_name,
            "path": context_path.to_string_lossy(),
            "source": "context.md"
        },
        "source": "nexus-memory",
        "version": env!("CARGO_PKG_VERSION"),
    });

    // Handle settings.json-style structure with "hooks" key
    // We inject nexus reference either in the root or in hooks section
    match json {
        Value::Object(ref mut map) => {
            // Check if there's a "hooks" section - Droid's convention
            if let Some(Value::Object(hooks)) = map.get_mut("hooks") {
                // Add nexus reference to hooks structure
                hooks.insert("nexus".to_string(), nexus_obj);
            } else {
                // Add to root level
                map.insert("nexus".to_string(), nexus_obj);
            }
        }
        _ => {
            // Non-object JSON - create wrapper
            let mut wrapper = Map::new();
            wrapper.insert("nexus".to_string(), nexus_obj);
            wrapper.insert("original".to_string(), json.clone());
            json = Value::Object(wrapper);
        }
    }

    serde_json::to_string_pretty(&json).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

/// Inject only the soul identity reference into a config file (no project context).
/// Used for global config files that should not reference project-specific context.md.
pub fn inject_soul_only(config_file: &Path, soul_path: &Path) -> io::Result<()> {
    if !config_file.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_file)?;
    let original_content = content.clone();

    let is_json = config_file.extension()
        .map(|ext| ext == "json")
        .unwrap_or(false);

    let new_content = if is_json {
        inject_into_json_soul_only(&content, soul_path)?
    } else {
        let block = format!(
            "{}\n\
            ## Nexus Memory Substrate\n\
            - Identity: [Soul]({soul_path_val})\n\
            {}",
            NEXUS_BLOCK_START,
            NEXUS_BLOCK_END,
            soul_path_val = soul_path.to_string_lossy(),
        );

        if let (Some(start), Some(end)) = (
            content.find(NEXUS_BLOCK_START),
            content.find(NEXUS_BLOCK_END),
        ) {
            if start >= end {
                let stripped = content
                    .replace(NEXUS_BLOCK_START, "")
                    .replace(NEXUS_BLOCK_END, "");
                let mut updated = stripped.trim_end().to_string();
                updated.push('\n');
                updated.push_str(&block);
                if !updated.ends_with('\n') {
                    updated.push('\n');
                }
                updated
            } else {
                let mut updated = content[..start].to_string();
                updated.push_str(&block);
                updated.push_str(&content[end + NEXUS_BLOCK_END.len()..]);
                updated
            }
        } else {
            let mut updated = content;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(&block);
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated
        }
    };

    if new_content != original_content {
        atomic_write(config_file, &new_content)?;
        debug!(
            "Injected soul-only Nexus reference into {}",
            config_file.display()
        );
    }

    Ok(())
}

/// Inject soul-only reference into JSON config
fn inject_into_json_soul_only(
    content: &str,
    soul_path: &Path,
) -> io::Result<String> {
    use serde_json::{Map, Value};

    let mut json: Value = serde_json::from_str(content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let nexus_obj = serde_json::json!({
        "identity": {
            "name": "Soul",
            "path": soul_path.to_string_lossy(),
            "source": "soul.md"
        },
        "source": "nexus-memory",
    });

    match json {
        Value::Object(ref mut map) => {
            if let Some(Value::Object(hooks)) = map.get_mut("hooks") {
                hooks.insert("nexus".to_string(), nexus_obj);
            } else {
                map.insert("nexus".to_string(), nexus_obj);
            }
        }
        _ => {
            let mut wrapper = Map::new();
            wrapper.insert("nexus".to_string(), nexus_obj);
            wrapper.insert("original".to_string(), json.clone());
            json = Value::Object(wrapper);
        }
    }

    serde_json::to_string_pretty(&json).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}
/// Remove Nexus references from a config file.
pub fn remove_reference(config_file: &Path) -> io::Result<()> {
    if !config_file.exists() {
        return Ok(());
    }

    let is_json = config_file.extension()
        .map(|ext| ext == "json")
        .unwrap_or(false);

    if is_json {
        let content = fs::read_to_string(config_file)?;
        let mut json: Value = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        if let Value::Object(ref mut map) = json {
            // Remove nexus key from root
            map.remove("nexus");
            // Remove nexus key from hooks if present
            if let Some(Value::Object(hooks)) = map.get_mut("hooks") {
                hooks.remove("nexus");
            }
        }

        let updated = serde_json::to_string_pretty(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        atomic_write(config_file, &updated)?;
    } else {
        let content = fs::read_to_string(config_file)?;
        if let (Some(start), Some(end)) = (
            content.find(NEXUS_BLOCK_START),
            content.find(NEXUS_BLOCK_END),
        ) {
            let mut updated = content[..start].to_string();
            let remaining = &content[end + NEXUS_BLOCK_END.len()..];
            updated.push_str(remaining);

            // Final cleanup: if we're left with multiple newlines at the end, collapse them
            while updated.ends_with("\n\n") {
                updated.pop();
            }

            atomic_write(config_file, &updated)?;
        }
    }

    Ok(())
}

/// Pipeline executed when a new agent session starts.
pub async fn on_session_start(
    cwd: &Path,
    agent_type: &str,
    session_id: &str,
) -> anyhow::Result<()> {
    let start_time = std::time::Instant::now();
    info!(
        "Starting Nexus session start pipeline for {} ({})",
        agent_type, session_id
    );

    // 1. Resolve Project Identity
    let project = nexus_core::ProjectIdentity::resolve(cwd);
    let nexus_dir = project.root_dir.join(".nexus");
    fs::create_dir_all(&nexus_dir)?;
    fs::create_dir_all(nexus_dir.join("cache"))?;
    fs::create_dir_all(nexus_dir.join("sessions"))?;

    // 2. Initialize Repositories (for Morning Recall)
    let config = nexus_core::Config::from_env().unwrap_or_default();
    // SQLite create_if_missing won't create parent dirs
    if let Some(parent) = config.database.path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut storage = nexus_storage::StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;
    let memory_repo = nexus_storage::repository::MemoryRepository::new(storage.pool().clone());
    let ns_repo = nexus_storage::repository::NamespaceRepository::new(storage.pool().clone());
    let namespace = ns_repo.get_or_create(agent_type, agent_type).await?;

    // 3. Load Cache and Perform Morning Recall
    let cache = nexus_agent::cognitive_cache::CognitiveCache::load_or_init(&nexus_dir);

    // Attempt to get embedder if available
    let embedder = if config.embedding.enabled {
        nexus_agent::runtime::create_embedding_service(&config).await
    } else {
        None
    };

    let recalls = cache
        .morning_recall(
            &project,
            namespace.id,
            &memory_repo,
            embedder
                .as_ref()
                .map(|e| e.as_ref() as &dyn nexus_core::EmbeddingService),
        )
        .await;

    // 4. Build and Write context.md
    let window_size = nexus_agent::TokenBudget::estimate_window(agent_type) as f32;
    let max_context_tokens =
        (window_size * config.cognitive_system.context_allocation_pct) as usize;
    let context_md = nexus_agent::context_builder::build_context_md(
        &cache.hot_cache,
        &recalls,
        max_context_tokens,
    );

    let context_path = nexus_dir.join("context.md");
    atomic_write(&context_path, &context_md)?;

    // 5. Compute soul path for injection reference
    // (soul.md is only created/modified during deep dream cycles, per spec)
    let soul_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexus")
        .join("soul.md");

    // 6. Inject references into agent configs
    if let Some(target) = AgentInjectionTarget::find(agent_type) {
        // Project config
        let project_config = project.root_dir.join(&target.project_config_filename);
        inject_reference(&project_config, &soul_path, &context_path, Some(agent_type))?;

        // Global config
        if let Some(global_config) = target.global_config {
            inject_soul_only(&global_config, &soul_path)?;
        }
    }

    // 7. Start session scratch file
    let session_manager = nexus_agent::session_manager::SessionManager::new(&project.root_dir);
    session_manager.start_session(session_id, agent_type)?;

    // 8. Hardening: .gitignore — ensure .nexus/ is always ignored
    let gitignore = project.root_dir.join(".gitignore");
    let gitignore_content = fs::read_to_string(&gitignore).unwrap_or_default();
    let has_nexus_entry = gitignore_content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == ".nexus" || trimmed == ".nexus/" || trimmed == "/.nexus" || trimmed == "/.nexus/"
    });
    if !has_nexus_entry {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gitignore)?;
        if !gitignore_content.is_empty() && !gitignore_content.ends_with('\n') {
            writeln!(f)?;
        }
        writeln!(f, ".nexus/")?;
    }

    info!(
        "Nexus session start pipeline completed in {:?}",
        start_time.elapsed()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_inject_reference_idempotency() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("CLAUDE.md");
        fs::write(&config, "# Existing Content\n").unwrap();

        let soul = PathBuf::from("/tmp/soul.md");
        let context = PathBuf::from("/tmp/context.md");

        // 1. First injection
        inject_reference(&config, &soul, &context, None).unwrap();
        let content1 = fs::read_to_string(&config).unwrap();
        assert!(content1.contains(NEXUS_BLOCK_START));

        // 2. Second injection (idempotent)
        inject_reference(&config, &soul, &context, None).unwrap();
        let content2 = fs::read_to_string(&config).unwrap();
        assert_eq!(content1, content2);
    }

    #[test]
    fn test_remove_reference() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("AGENTS.md");
        fs::write(
            &config,
            "# Top\n<!-- NEXUS:START -->\n- Ref\n<!-- NEXUS:END -->\n# Bottom",
        )
        .unwrap();

        remove_reference(&config).unwrap();
        let content = fs::read_to_string(&config).unwrap();
        assert!(!content.contains("NEXUS:START"));
        assert!(content.contains("# Top"));
        assert!(content.contains("# Bottom"));
    }

    #[tokio::test]
    async fn test_on_session_start_creates_structure() {
        let dir = tempdir().unwrap();
        // Use NEXUS_DATABASE_PATH to isolate DB instead of HOME manipulation
        let db_path = dir.path().join("test.db");
        let original_db = std::env::var("NEXUS_DATABASE_PATH").ok();
        std::env::set_var("NEXUS_DATABASE_PATH", &db_path);

        let result = on_session_start(dir.path(), "claude-code", "test-session").await;

        // Restore before assertions
        if let Some(orig) = original_db {
            std::env::set_var("NEXUS_DATABASE_PATH", orig);
        } else {
            std::env::remove_var("NEXUS_DATABASE_PATH");
        }

        result.unwrap();

        assert!(dir.path().join(".nexus").exists());
        assert!(dir.path().join(".nexus/context.md").exists());
        assert!(dir.path().join(".nexus/sessions/test-session.md").exists());
    }

    #[test]
    fn test_pi_mono_injection_target_exists() {
        let target = AgentInjectionTarget::find("pi-mono");
        assert!(target.is_some(), "pi-mono must be in known_agents()");

        let target = target.unwrap();
        assert_eq!(target.agent_type, "pi-mono");
        assert!(target.global_config.is_some());
        assert_eq!(target.project_config_filename, ".pi/AGENTS.md");

        let global = target.global_config.unwrap();
        assert!(
            global.ends_with(".pi/agent/AGENTS.md")
                || global.to_string_lossy().contains(".pi/agent/AGENTS.md")
        );
    }
}
