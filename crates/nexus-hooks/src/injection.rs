//! Reference injection system for agent configuration files.

use nexus_agent::soul::soul_path;
use nexus_core::fsutil::atomic_write;
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

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

/// Check if a JSON value is a Nexus-managed entry.
///
/// Nexus-owned entries have `"source": "nexus-memory"` at the top level.
fn is_nexus_owned(value: &Value) -> bool {
    value
        .get("source")
        .and_then(|v| v.as_str())
        .map(|s| s == "nexus-memory")
        .unwrap_or(false)
}

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
            .find(|t| t.agent_type == agent_type)
    }
}

/// Inject Nexus references into a config file.
///
/// # Parameters
/// - `config_file`: Path to the configuration file
/// - `soul_path`: Path to the soul.md reference
/// - `context_path`: Path to the context.md reference  
/// - `agent_type`: Reserved for future agent-specific logic (currently unused)
pub fn inject_reference(
    config_file: &Path,
    soul_path: &Path,
    context_path: &Path,
    _agent_type: Option<&str>,
) -> io::Result<()> {
    if !config_file.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_file)?;
    let original_content = content.clone();

    // Detect JSON config files (Droid's settings.json, etc.)
    let is_json = config_file
        .extension()
        .map(|ext| ext == "json")
        .unwrap_or(false);

    let new_content = if is_json {
        // For JSON, add settings if not already present
        inject_into_json(&content, config_file, soul_path, context_path)?
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
    config_file: &Path,
    soul_path: &Path,
    context_path: &Path,
) -> io::Result<String> {
    let json: Value = serde_json::from_str(content).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse JSON in {}: {}", config_file.display(), e),
        )
    })?;

    let soul_name = "Soul";
    let context_name = "Project Context";

    // Create the nexus reference object
    let nexus_obj = if let Some(context_path) = context_path {
        serde_json::json!({
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
        })
    } else {
        serde_json::json!({
            "identity": {
                "name": soul_name,
                "path": soul_path.to_string_lossy(),
                "source": "soul.md"
            },
            "source": "nexus-memory",
            "version": env!("CARGO_PKG_VERSION"),
        })
    };

    insert_nexus_into_json(json, config_file, nexus_obj)
}

/// Shared logic for inserting a Nexus reference object into a JSON config.
fn insert_nexus_into_json(
    mut json: Value,
    config_file: &Path,
    nexus_obj: serde_json::Value,
) -> io::Result<String> {
    use serde_json::Value;

    // Handle settings.json-style structure with "hooks" key
    if let Value::Object(ref mut map) = json {
        // Determine if hooks exists and is an object
        let has_hooks = matches!(map.get("hooks"), Some(Value::Object(_)));

        // Determine target location:
        // Use hooks if has_hooks AND root contains no keys other than "hooks" and "nexus"
        let target_hooks = if has_hooks {
            let mut only_hooks = true;
            for key in map.keys() {
                if key != "hooks" && key != "nexus" {
                    only_hooks = false;
                    break;
                }
            }
            only_hooks
        } else {
            false
        };

        if target_hooks {
            // Clean root nexus before borrowing hooks (only if Nexus-owned)
            if map.get("nexus").map(is_nexus_owned).unwrap_or(false) {
                map.remove("nexus");
            }

            // Insert into hooks
            if let Some(hooks) = map.get_mut("hooks").and_then(|v| v.as_object_mut()) {
                if let Some(existing) = hooks.get("nexus") {
                    if !is_nexus_owned(existing) {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!(
                                "Refusing to overwrite non-Nexus-managed hooks.nexus in {}",
                                config_file.display()
                            ),
                        ));
                    }
                }
                hooks.insert("nexus".to_string(), nexus_obj);
            } else {
                // hooks existed but not an object; unexpected
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Expected hooks to be an object in {}",
                        config_file.display()
                    ),
                ));
            }
        } else {
            // Root target: clean hooks.nexus if present (only if Nexus-owned)
            if let Some(hooks) = map.get_mut("hooks").and_then(|v| v.as_object_mut()) {
                if hooks.get("nexus").map(is_nexus_owned).unwrap_or(false) {
                    hooks.remove("nexus");
                }
            }
            if let Some(existing) = map.get("nexus") {
                if !is_nexus_owned(existing) {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "Refusing to overwrite non-Nexus-managed nexus key in {}",
                            config_file.display()
                        ),
                    ));
                }
            }
            map.insert("nexus".to_string(), nexus_obj);
        }
    } else {
        // Non-object JSON: this configuration format cannot support Nexus injection
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Expected top-level JSON object for Nexus injection in {}",
                config_file.display()
            ),
        ));
    }

    serde_json::to_string_pretty(&json).map_err(std::io::Error::other)
}

/// Inject only the soul identity reference into a config file (no project context).
///
/// # Parameters
/// - `config_file`: Path to the configuration file to modify
/// - `soul_path`: Path to the soul.md file to reference
/// - `agent_type`: Reserved for future agent-specific injection logic (currently unused)
pub fn inject_soul_only(
    config_file: &Path,
    soul_path: &Path,
    _agent_type: Option<&str>,
) -> io::Result<()> {
    if !config_file.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(config_file)?;
    let original_content = content.clone();

    let is_json = config_file
        .extension()
        .map(|ext| ext == "json")
        .unwrap_or(false);

    let new_content = if is_json {
        inject_into_json_soul_only(&content, soul_path, config_file)?
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
    config_file: &Path,
) -> io::Result<String> {
    let json: Value = serde_json::from_str(content).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse JSON in {}: {}", config_file.display(), e),
        )
    })?;

    let nexus_obj = serde_json::json!({
        "identity": {
            "name": "Soul",
            "path": soul_path.to_string_lossy(),
            "source": "soul.md"
        },
        "source": "nexus-memory",
        "version": env!("CARGO_PKG_VERSION"),
    });

    insert_nexus_into_json(json, config_file, nexus_obj)
}

/// Remove Nexus references from a config file.
pub fn remove_reference(config_file: &Path) -> io::Result<()> {
    if !config_file.exists() {
        return Ok(());
    }

    let is_json = config_file
        .extension()
        .map(|ext| ext == "json")
        .unwrap_or(false);

    if is_json {
        let content = fs::read_to_string(config_file)?;
        let mut json: Value = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut changed = false;

        // Handle wrapper case: if the file was wrapped by inject_into_json for non-object original,
        // the structure is { "nexus": ..., "original": <original_value> }.
        // After removing nexus, we should unwrap back to the original.
        if let Value::Object(ref mut map) = json {
            // Check if this is a wrapper (has both "nexus" and "original" keys)
            let is_wrapper = map.contains_key("nexus") && map.contains_key("original");

            // Remove nexus key from wrapper root (Nexus-owned check not needed here - this nexus was created by us)
            if is_wrapper {
                map.remove("nexus");
                changed = true;
            } else {
                // Normal case: remove nexus from root (only if Nexus-owned)
                if map.get("nexus").map(is_nexus_owned).unwrap_or(false) {
                    map.remove("nexus");
                    changed = true;
                }
            }

            // Remove nexus key from hooks if present (only if Nexus-owned)
            if let Some(hooks) = map.get_mut("hooks").and_then(|v| v.as_object_mut()) {
                if hooks.get("nexus").map(is_nexus_owned).unwrap_or(false) {
                    hooks.remove("nexus");
                    changed = true;
                }
            }

            // If this was a wrapper and nexus was removed, unwrap to original
            if is_wrapper && !map.contains_key("nexus") {
                if let Some(original) = map.remove("original") {
                    json = original;
                    // Note: changed already true
                }
            }
        }

        if changed {
            let updated = serde_json::to_string_pretty(&json).map_err(std::io::Error::other)?;
            atomic_write(config_file, &updated)?;
        }
    } else {
        let content = fs::read_to_string(config_file)?;
        let start_pos = content.find(NEXUS_BLOCK_START);
        let end_pos = content.find(NEXUS_BLOCK_END);

        match (start_pos, end_pos) {
            // Both markers present
            (Some(start), Some(end)) => {
                if start < end {
                    // Valid order: remove from start to end+len
                    let mut updated = content[..start].to_string();
                    let remaining = &content[end + NEXUS_BLOCK_END.len()..];
                    updated.push_str(remaining);
                    // Collapse trailing double newlines
                    while updated.ends_with("\n\n") {
                        updated.pop();
                    }
                    atomic_write(config_file, &updated)?;
                } else {
                    // Malordered: end before start, treat as orphaned markers, remove individually
                    let mut updated = content;
                    updated = updated.replace(NEXUS_BLOCK_START, "");
                    updated = updated.replace(NEXUS_BLOCK_END, "");
                    // Collapse trailing double newlines if any
                    while updated.ends_with("\n\n") {
                        updated.pop();
                    }
                    atomic_write(config_file, &updated)?;
                }
            }
            // Only START marker present
            (Some(start), None) => {
                let mut updated = content[..start].to_string();
                let remaining = &content[start + NEXUS_BLOCK_START.len()..];
                updated.push_str(remaining);
                // Collapse trailing double newlines
                while updated.ends_with("\n\n") {
                    updated.pop();
                }
                atomic_write(config_file, &updated)?;
            }
            // Only END marker present
            (None, Some(end)) => {
                let mut updated = content[..end].to_string();
                let remaining = &content[end + NEXUS_BLOCK_END.len()..];
                updated.push_str(remaining);
                // Collapse trailing double newlines
                while updated.ends_with("\n\n") {
                    updated.pop();
                }
                atomic_write(config_file, &updated)?;
            }
            // Neither marker present: nothing to do
            (None, None) => {}
        }
    }

    Ok(())
}

/// Extract project identity from CLAUDE.md and AGENTS.md to auto-seed soul.md
/// if it doesn't exist or is empty.
pub fn auto_seed_soul(project_root: &Path) -> Option<String> {
    let soul_path = soul_path();

    // Check if soul already exists and has content
    if soul_path.exists() {
        if let Ok(content) = fs::read_to_string(&soul_path) {
            let trimmed = content.trim();
            // Check for empty headers only (the default template)
            if trimmed.is_empty() || trimmed == "# Nexus Soul" {
                // Will regenerate below
            } else if !trimmed.is_empty() {
                // Soul already has real content - don't overwrite
                debug!("Soul already has content, skipping auto-seed");
                return None;
            }
        }
    }

    let mut extracts = Vec::new();

    // Read CLAUDE.md
    let claude_md = project_root.join("CLAUDE.md");
    if claude_md.exists() {
        if let Ok(content) = fs::read_to_string(&claude_md) {
            extracts.push(("CLAUDE.md".to_string(), content));
        }
    }

    // Read AGENTS.md
    let agents_md = project_root.join("AGENTS.md");
    if agents_md.exists() {
        if let Ok(content) = fs::read_to_string(&agents_md) {
            extracts.push(("AGENTS.md".to_string(), content));
        }
    }

    // Also check for any .md files in .config/nexus/ project context
    let project_context_path = project_root.join(".nexus").join("context.md");
    if project_context_path.exists() {
        if let Ok(content) = fs::read_to_string(&project_context_path) {
            extracts.push(("context.md".to_string(), content));
        }
    }

    if extracts.is_empty() {
        debug!("No CLAUDE.md or AGENTS.md found for soul auto-seeding");
        return None;
    }

    // Extract key patterns from the files
    let mut patterns = Vec::new();
    let mut tool_preferences = Vec::new();
    let mut coding_conventions = Vec::new();
    let mut testing_notes = Vec::new();

    for (_source, content) in &extracts {
        let lines: Vec<&str> = content.lines().collect();

        // Extract build/test commands
        for line in &lines {
            let lower = line.to_lowercase();
            if (lower.contains("cargo")
                || lower.contains("npm")
                || lower.contains("python")
                || lower.contains("uv"))
                && (lower.contains("build")
                    || lower.contains("test")
                    || lower.contains("lint")
                    || lower.contains("format"))
            {
                tool_preferences.push(line.trim().to_string());
            }
        }

        // Extract coding patterns from code blocks
        let in_code_block = content.contains("```");
        if in_code_block {
            // Look for common patterns like use statements, imports, etc.
            if content.contains("use anyhow") || content.contains("anyhow::Result") {
                coding_conventions.push("Uses anyhow for error handling".to_string());
            }
            if content.contains("use serde") || content.contains("#[derive(Serialize") {
                coding_conventions.push("Uses serde for serialization".to_string());
            }
            if content.contains("#[cfg(") {
                coding_conventions.push("Uses feature gating (#[cfg])".to_string());
            }
        }

        // Extract testing patterns
        if content.to_lowercase().contains("test") {
            if content.to_lowercase().contains("tdd")
                || content.to_lowercase().contains("test-driven")
            {
                testing_notes.push("Test-Driven Development approach".to_string());
            }
            if content.to_lowercase().contains("integration") {
                testing_notes.push("Integration tests".to_string());
            }
        }

        // Extract general preferences
        if content.to_lowercase().contains("convention") || content.to_lowercase().contains("style")
        {
            for line in &lines {
                if (line.to_lowercase().contains("prefer")
                    || line.to_lowercase().contains("always"))
                    && line.len() > 10
                    && line.len() < 200
                {
                    patterns.push(line.trim().to_string());
                }
            }
        }
    }

    // Build a soul.md from extracted patterns
    let mut soul_content = String::new();
    soul_content.push_str("# Nexus Soul\n\n");

    // Identity & Preferences section
    soul_content.push_str("## Identity & Preferences\n\n");
    if !tool_preferences.is_empty() {
        soul_content.push_str("### Build & Tool Preferences\n");
        for pref in tool_preferences.iter().take(5) {
            if !pref.is_empty() {
                soul_content.push_str(&format!("- {}\n", pref));
            }
        }
        soul_content.push('\n');
    }
    if !patterns.is_empty() {
        soul_content.push_str("### Project Conventions\n");
        for pat in patterns.iter().take(5) {
            if !pat.is_empty() {
                soul_content.push_str(&format!("- {}\n", pat));
            }
        }
        soul_content.push('\n');
    }

    // Technical Learnings section
    soul_content.push_str("## Technical Learnings\n\n");
    if !coding_conventions.is_empty() {
        for conv in coding_conventions.iter() {
            soul_content.push_str(&format!("- {}\n", conv));
        }
    }
    // Add auto-detected patterns
    if content_contains_rust(&extracts) {
        soul_content.push_str("- Project uses Rust (Cargo)\n");
    }
    if content_contains_warnings_policy(&extracts) {
        soul_content.push_str("- Zero warnings policy enforced\n");
    }
    soul_content.push('\n');

    // Working Patterns section
    soul_content.push_str("## Working Patterns\n\n");
    for note in testing_notes.iter().take(3) {
        soul_content.push_str(&format!("- {}\n", note));
    }
    soul_content.push('\n');

    // Agent Notes section
    soul_content.push_str("## Agent Notes\n\n");
    soul_content.push_str("- Auto-generated from project CLAUDE.md/AGENTS.md\n");
    soul_content.push_str("- Update manually with additional learnings\n");
    soul_content.push('\n');

    // Ensure we have some actual content beyond headers
    let trimmed = soul_content.trim();
    let has_real_content = trimmed
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .count()
        > 3;

    if has_real_content {
        // Create parent directories if needed
        if let Some(parent) = soul_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Write the auto-seeded soul
        if let Err(e) = atomic_write(&soul_path, &soul_content) {
            tracing::warn!("Failed to write auto-seeded soul: {}", e);
            return None;
        }

        info!("Auto-seeded soul.md from project config files");
        Some(soul_content)
    } else {
        None
    }
}

fn content_contains_rust(extracts: &[(String, String)]) -> bool {
    for (_, content) in extracts {
        let lower = content.to_lowercase();
        if lower.contains("cargo") || lower.contains("rust") || lower.contains(".rs") {
            return true;
        }
    }
    false
}

fn content_contains_warnings_policy(extracts: &[(String, String)]) -> bool {
    for (_, content) in extracts {
        let lower = content.to_lowercase();
        if lower.contains("warning")
            && (lower.contains("error") || lower.contains("strict") || lower.contains("deny"))
        {
            return true;
        }
    }
    false
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
        fs::create_dir_all(parent)?;
    }
    let mut storage = nexus_storage::StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;
    let memory_repo = nexus_storage::repository::MemoryRepository::new(storage.pool().clone());
    let ns_repo = nexus_storage::repository::NamespaceRepository::new(storage.pool().clone());
    let namespace = ns_repo.get_or_create(agent_type, agent_type).await?;

    // 3. Load Cache and Perform Morning Recall
    let mut cache = nexus_agent::cognitive_cache::CognitiveCache::load_or_init(&nexus_dir);
    // Remove internal session lifecycle memories from hot cache (cleanup from previous runs)
    cache
        .hot_cache
        .entries
        .retain(|e| !e.content.contains("Session lifecycle event"));

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

    // Promote morning recall results to hot cache for future sessions
    let hot_cache_max = config.cognitive_system.hot_cache_max_entries;
    for recall in &recalls {
        let entry = nexus_agent::cognitive_cache::HotCacheEntry {
            memory_id: recall.memory_id,
            content: recall.content.clone(),
            relevance_score: recall.relevance_score,
            tier: recall.tier,
            promoted_at: chrono::Utc::now(),
            last_surfaced: chrono::Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: Some(agent_type.to_string()),
        };
        cache.hot_cache.promote(entry, hot_cache_max);
    }

    // Save updated hot cache to disk (so future sessions see these memories)
    cache.save(&nexus_dir)?;

    // 4b. Auto-seed soul if empty (extract identity from CLAUDE.md/AGENTS.md)
    let _ = auto_seed_soul(&project.root_dir);

    // 5. Compute soul path for injection reference
    // (now includes auto-seeded content from project config files)
    let soul_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexus")
        .join("soul.md");

    // 6. Inject references into agent configs
    if let Some(target) = AgentInjectionTarget::find(agent_type) {
        // Project config
        let project_config = project.root_dir.join(&target.project_config_filename);
        if let Err(e) =
            inject_reference(&project_config, &soul_path, &context_path, Some(agent_type))
        {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                // Non-Nexus-owned config already present; log and continue
                warn!(file=?project_config, error=?e, "Skipping injection: config already contains non-Nexus reference");
            } else {
                return Err(e.into());
            }
        }

        // Global config - skip if it's the same file as project config (Droid edge case)
        if let Some(global_config) = target.global_config {
            // Guard against overlap: if global_config resolves to the same path as project_config, skip
            if global_config == project_config {
                debug!(file=?global_config, "Skipping global soul-only injection: same as project config");
            } else if let Err(e) = inject_soul_only(&global_config, &soul_path, Some(agent_type)) {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    warn!(file=?global_config, error=?e, "Skipping injection: global config already contains non-Nexus reference");
                } else {
                    return Err(e.into());
                }
            }
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
        "Nexus session start pipeline completed in {:?} (hot cache: {} entries)",
        start_time.elapsed(),
        cache.hot_cache.entries.len()
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
        let db_path = dir.path().join("test.db");
        let original_db = std::env::var("NEXUS_DATABASE_PATH").ok();
        std::env::set_var("NEXUS_DATABASE_PATH", &db_path);

        let result = on_session_start(dir.path(), "claude-code", "test-session").await;

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
    fn test_droid_injection_target_registered() {
        let target = AgentInjectionTarget::find("droid");
        assert!(target.is_some(), "droid must be in known_agents()");

        let target = target.unwrap();
        assert_eq!(target.agent_type, "droid");
        assert!(target.global_config.is_some());
        assert_eq!(target.project_config_filename, ".factory/settings.json");

        let global = target.global_config.unwrap();
        assert!(global.to_string_lossy().contains(".factory/settings.json"));
    }

    // --- Additional remediation tests ---

    #[test]
    fn test_inject_into_json_hooks_path() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("settings.json");
        let soul = Path::new("/tmp/soul.md");
        let context = Path::new("/tmp/context.md");
        let initial_json = r#"{"hooks": {}}"#;
        fs::write(&config, initial_json).unwrap();

        let result = inject_into_json(initial_json, &config, soul, context).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        let hooks = parsed.get("hooks").and_then(|v| v.as_object()).unwrap();
        assert!(hooks.contains_key("nexus"));
        let nexus = hooks.get("nexus").unwrap();
        assert_eq!(
            nexus.get("source").and_then(|v| v.as_str()),
            Some("nexus-memory")
        );
        // Idempotency: second injection should yield same result
        let result2 = inject_into_json(initial_json, &config, soul, context).unwrap();
        assert_eq!(result, result2);
    }

    #[test]
    fn test_inject_into_json_root_path() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("settings.json");
        let soul = Path::new("/tmp/soul.md");
        let context = Path::new("/tmp/context.md");
        let initial_json = r#"{"some_other_key": "value"}"#;
        fs::write(&config, initial_json).unwrap();

        let result = inject_into_json(initial_json, &config, soul, context).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert!(parsed.get("nexus").is_some());
        let nexus = parsed.get("nexus").unwrap();
        assert_eq!(
            nexus.get("source").and_then(|v| v.as_str()),
            Some("nexus-memory")
        );
    }

    #[test]
    fn test_inject_into_json_duplicate_cleanup() {
        let dir = tempdir().unwrap();
        let soul = Path::new("/tmp/soul.md");
        let context = Path::new("/tmp/context.md");

        // Hooks injection: non-owned root nexus should be preserved, hooks.nexus added
        {
            let config = dir.path().join("hooks_cleanup.json");
            let initial_json = r#"{"hooks": {}, "nexus": {"source": "other"}}"#;
            fs::write(&config, initial_json).unwrap();
            let result = inject_into_json(initial_json, &config, soul, context).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            // Non-owned root nexus must be preserved (not removed)
            assert!(
                parsed.get("nexus").is_some(),
                "root nexus (non-owned) should be preserved"
            );
            // Verify root nexus still has original source
            let root_nexus = parsed.get("nexus").unwrap();
            assert_eq!(
                root_nexus.get("source").and_then(|v| v.as_str()),
                Some("other")
            );
            let hooks = parsed.get("hooks").and_then(|v| v.as_object()).unwrap();
            assert!(
                hooks.contains_key("nexus"),
                "hooks.nexus should exist (Nexus-owned)"
            );
        }

        // Root injection: non-owned hooks.nexus should be preserved, root.nexus added
        {
            let config = dir.path().join("root_cleanup.json");
            let initial_json = r#"{"hooks": {"nexus": {"source": "other"}}, "other": 1}"#;
            fs::write(&config, initial_json).unwrap();
            let result = inject_into_json(initial_json, &config, soul, context).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
            // Root nexus should exist (Nexus-owned)
            assert!(parsed.get("nexus").is_some(), "root nexus should exist");
            let hooks = parsed.get("hooks").and_then(|v| v.as_object()).unwrap();
            // Non-owned hooks.nexus must be preserved (not removed)
            assert!(
                hooks.contains_key("nexus"),
                "hooks.nexus (non-owned) should be preserved"
            );
            // Verify the hooks.nexus source remains "other"
            let hooks_nexus = hooks.get("nexus").unwrap();
            assert_eq!(
                hooks_nexus.get("source").and_then(|v| v.as_str()),
                Some("other")
            );
        }
    }

    #[test]
    fn test_inject_into_json_ownership_check() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("ownership.json");
        let soul = Path::new("/tmp/soul.md");
        let context = Path::new("/tmp/context.md");

        // Non-Nexus owned entry in hooks should fail
        let initial_json = r#"{"hooks": {"nexus": {"source": "something-else"}}}"#;
        fs::write(&config, initial_json).unwrap();
        let err = inject_into_json(initial_json, &config, soul, context).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

        // Non-Nexus owned entry in root should fail
        let initial_json2 = r#"{"nexus": {"source": "other-source"}}"#;
        fs::write(&config, initial_json2).unwrap();
        let err2 = inject_into_json(initial_json2, &config, soul, context).unwrap_err();
        assert_eq!(err2.kind(), std::io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn test_inject_soul_only_json() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("soul_only.json");
        let soul = Path::new("/tmp/soul.md");

        // Hooks branch
        let initial_json = r#"{"hooks": {}}"#;
        fs::write(&config, initial_json).unwrap();
        let result = inject_into_json_soul_only(initial_json, soul, &config).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let hooks = parsed.get("hooks").and_then(|v| v.as_object()).unwrap();
        let nexus = hooks.get("nexus").unwrap();
        assert_eq!(
            nexus
                .get("identity")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str()),
            Some("Soul")
        );
        assert!(nexus.get("projectContext").is_none());

        // Root branch
        let initial_json2 = r#"{"other": "val"}"#;
        fs::write(&config, initial_json2).unwrap();
        let result2 = inject_into_json_soul_only(initial_json2, soul, &config).unwrap();
        let parsed2: serde_json::Value = serde_json::from_str(&result2).unwrap();
        let nexus2 = parsed2.get("nexus").unwrap();
        assert_eq!(
            nexus2
                .get("identity")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str()),
            Some("Soul")
        );
    }

    #[test]
    fn test_remove_reference_json() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("remove.json");
        let initial_json = r#"{"hooks": {"nexus": {"source": "nexus-memory"}}, "nexus": {"source": "nexus-memory"}, "other": 1}"#;
        fs::write(&config, initial_json).unwrap();

        remove_reference(&config).unwrap();
        let content = fs::read_to_string(&config).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        let map = parsed.as_object().unwrap();
        assert!(!map.contains_key("nexus"));
        if let Some(hooks) = map.get("hooks").and_then(|v| v.as_object()) {
            assert!(!hooks.contains_key("nexus"));
        }
        assert_eq!(map.get("other").and_then(|v| v.as_i64()), Some(1));
    }

    #[test]
    fn test_remove_reference_partial_markers() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("partial.md");

        // Only START
        {
            let content = format!("before{}after", NEXUS_BLOCK_START);
            fs::write(&config, &content).unwrap();
            remove_reference(&config).unwrap();
            let result = fs::read_to_string(&config).unwrap();
            assert!(!result.contains(NEXUS_BLOCK_START));
            assert_eq!(result, "beforeafter");
        }

        // Only END
        {
            let content = format!("before{}after", NEXUS_BLOCK_END);
            fs::write(&config, &content).unwrap();
            remove_reference(&config).unwrap();
            let result = fs::read_to_string(&config).unwrap();
            assert!(!result.contains(NEXUS_BLOCK_END));
            assert_eq!(result, "beforeafter");
        }

        // Malordered: END before START
        {
            let content = format!("a{}b{}c", NEXUS_BLOCK_END, NEXUS_BLOCK_START);
            fs::write(&config, &content).unwrap();
            remove_reference(&config).unwrap();
            let result = fs::read_to_string(&config).unwrap();
            assert!(!result.contains(NEXUS_BLOCK_START));
            assert!(!result.contains(NEXUS_BLOCK_END));
            assert_eq!(result, "a b c".replace(" ", "")); // both removed
        }

        // Neither marker: file unchanged
        {
            let content = "plain text".to_string();
            fs::write(&config, &content).unwrap();
            remove_reference(&config).unwrap();
            let result = fs::read_to_string(&config).unwrap();
            assert_eq!(result, "plain text");
        }
    }
}
