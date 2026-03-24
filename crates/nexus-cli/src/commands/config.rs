//! Interactive configuration wizard and config management

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use dialoguer::{Confirm, Input, Select};
use nexus_core::config::{Config, LlmConfig};
use nexus_llm::{create_client, list_models, ChatMessage, GenerateParams};

// ── Provider definitions ─────────────────────────────────────────────

struct Provider {
    id: &'static str,
    label: &'static str,
    default_model: &'static str,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        label: "OpenAI",
        default_model: "gpt-4o-mini",
    },
    Provider {
        id: "anthropic",
        label: "Anthropic (Claude)",
        default_model: "claude-sonnet-4-20250514",
    },
    Provider {
        id: "gemini",
        label: "Google Gemini",
        default_model: "gemini-2.0-flash",
    },
    Provider {
        id: "openrouter",
        label: "OpenRouter",
        default_model: "openai/gpt-4o-mini",
    },
    Provider {
        id: "groq",
        label: "Groq",
        default_model: "llama-3.3-70b-versatile",
    },
    Provider {
        id: "zai",
        label: "Z.ai",
        default_model: "glm-4-flash",
    },
    Provider {
        id: "minimax",
        label: "Minimax",
        default_model: "minimax-abab6.5s",
    },
    Provider {
        id: "mistral",
        label: "Mistral",
        default_model: "mistral-small-latest",
    },
];

// ── Public entry points ──────────────────────────────────────────────

/// Interactive configuration wizard.
pub async fn execute_wizard() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let env_file = env_file_path();

    // ── Header ────────────────────────────────────────────────────
    println!();
    println!("  Nexus Memory System Configuration");
    println!("  ─────────────────────────────────");
    println!();

    // Show current state
    let key_set = std::env::var(&config.llm.api_key_env).is_ok();
    println!("  Current: {} ({})", config.llm.provider, config.llm.model);
    println!(
        "  API key: {}",
        if key_set { "configured" } else { "not set" }
    );
    println!(
        "  Agent:   {}",
        if config.agent.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!();

    // ── Provider selection ────────────────────────────────────────
    let items: Vec<&str> = PROVIDERS.iter().map(|p| p.label).collect();
    let current_idx = PROVIDERS
        .iter()
        .position(|p| p.id == config.llm.provider)
        .unwrap_or(0);

    let selection = Select::new()
        .with_prompt("Select LLM provider")
        .items(&items)
        .default(current_idx)
        .interact()?;

    let provider = &PROVIDERS[selection];

    // ── API key ───────────────────────────────────────────────────
    let current_key = std::env::var(&config.llm.api_key_env).unwrap_or_default();
    let api_key: String = Input::new()
        .with_prompt(format!("API key for {}", provider.label))
        .allow_empty(true)
        .interact_text()?;

    let api_key = if api_key.is_empty() {
        current_key
    } else {
        api_key
    };

    if api_key.is_empty() {
        eprintln!();
        eprintln!("  No API key provided. Configuration will be saved but LLM");
        eprintln!("  features will not work until you set the key.");
        eprintln!();
    }

    // ── Base URL (optional) ───────────────────────────────────────
    let base_url: String = Input::new()
        .with_prompt("Base URL (press Enter for default)")
        .allow_empty(true)
        .interact_text()?;

    // ── Fetch available models ────────────────────────────────────
    let model = if !api_key.is_empty() {
        // Inject the key so list_models can find it
        std::env::set_var("NEXUS_LLM_API_KEY", &api_key);
        println!();
        println!("  Fetching available models from {}...", provider.label);

        let mut llm_config = LlmConfig {
            provider: provider.id.into(),
            api_key_env: "NEXUS_LLM_API_KEY".into(),
            ..Default::default()
        };
        if !base_url.is_empty() {
            llm_config.base_url = Some(base_url.clone());
        }

        match list_models(&llm_config).await {
            Ok(models) if models.is_empty() => {
                eprintln!("  Warning: provider returned 0 models");
                eprintln!();
                Input::new()
                    .with_prompt("Model")
                    .default(provider.default_model.to_string())
                    .interact_text()?
            }
            Ok(models) => {
                println!("  Found {} models", models.len());
                println!();

                // Find the current/default model in the list for pre-selection
                let current_model = if config.llm.provider == provider.id {
                    &config.llm.model
                } else {
                    provider.default_model
                };
                let default_idx = models.iter().position(|m| m == current_model).unwrap_or(0);

                let items: Vec<&str> = models.iter().map(|s| s.as_str()).collect();
                let selected = Select::new()
                    .with_prompt("Select model")
                    .items(&items)
                    .default(default_idx)
                    .interact()?;
                models[selected].clone()
            }
            Err(e) => {
                eprintln!("  Failed to fetch models: {}", e);
                eprintln!("  Check your API key and base URL, then try again.");
                eprintln!();
                Input::new()
                    .with_prompt("Model (manual entry)")
                    .default(provider.default_model.to_string())
                    .interact_text()?
            }
        }
    } else {
        Input::new()
            .with_prompt("Model")
            .default(provider.default_model.to_string())
            .interact_text()?
    };

    // ── Always-on agent ───────────────────────────────────────────
    let enable_agent = if config.agent.enabled {
        Confirm::new()
            .with_prompt("Keep always-on agent enabled?")
            .default(true)
            .interact()?
    } else {
        Confirm::new()
            .with_prompt("Enable always-on agent?")
            .default(false)
            .interact()?
    };

    // ── Save ──────────────────────────────────────────────────────
    let mut entries = read_env_file(&env_file);

    entries.insert("NEXUS_LLM_PROVIDER".into(), provider.id.into());
    entries.insert("NEXUS_LLM_MODEL".into(), model.clone());
    entries.insert("NEXUS_LLM_API_KEY_ENV".into(), "NEXUS_LLM_API_KEY".into());
    entries.insert("NEXUS_LLM_API_KEY".into(), api_key);
    if !base_url.is_empty() {
        entries.insert("NEXUS_LLM_BASE_URL".into(), base_url);
    } else {
        entries.remove("NEXUS_LLM_BASE_URL");
    }
    entries.insert(
        "NEXUS_AGENT_ENABLED".into(),
        if enable_agent { "true" } else { "false" }.into(),
    );

    write_env_file(&env_file, &entries, true)?;

    println!();
    println!("  Configuration saved to {}", env_file.display());
    println!();

    // ── Test connection ───────────────────────────────────────────
    if !entries
        .get("NEXUS_LLM_API_KEY")
        .map_or(true, |v| v.is_empty())
    {
        let test = Confirm::new()
            .with_prompt("Test connection now?")
            .default(true)
            .interact()?;

        if test {
            println!();
            let key_value = entries
                .get("NEXUS_LLM_API_KEY")
                .cloned()
                .unwrap_or_default();
            test_connection_with_key(provider.id, &model, &key_value).await?;
        }
    }

    println!();
    println!("  Restart your shell or run:");
    println!("    source {}", env_file.display());

    Ok(())
}

/// Show the current effective Nexus configuration.
pub async fn execute_show() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let env_file = env_file_path();

    println!("Nexus Memory System Configuration");
    println!();

    println!("Configuration file: {}", env_file.display());
    if !env_file.exists() {
        println!("  (file does not exist — using defaults and env vars)");
    }
    println!();

    println!("Database:");
    println!("  path:         {}", config.database.path.display());
    println!();

    println!("LLM:");
    println!("  provider:    {}", config.llm.provider);
    println!("  model:       {}", config.llm.model);
    let key_set = std::env::var(&config.llm.api_key_env).is_ok();
    println!("  api_key:     {}", if key_set { "set" } else { "NOT SET" });
    if let Some(ref url) = config.llm.base_url {
        println!("  base_url:    {}", url);
    }
    println!();

    println!("Agent:");
    println!("  enabled:                     {}", config.agent.enabled);
    println!("  namespace:                   {}", config.agent.namespace);
    println!(
        "  consolidation_interval_mins: {}",
        config.agent.consolidation_interval_mins
    );
    println!(
        "  scan_interval_secs:          {}",
        config.agent.scan_interval_secs
    );

    Ok(())
}

/// Set a configuration value in the nexus.env file.
pub async fn execute_set(key: String, value: String) -> anyhow::Result<()> {
    validate_key(&key)?;

    let env_file = env_file_path();
    let mut entries = read_env_file(&env_file);
    entries.insert(key.clone(), value.clone());
    write_env_file(&env_file, &entries, false)?;

    println!("Set {}=\"{}\" in {}", key, value, env_file.display());
    println!();
    println!("Restart your shell or run:");
    println!("  source {}", env_file.display());

    Ok(())
}

/// Test LLM connection, setting the key in the process env first.
async fn test_connection_with_key(
    provider: &str,
    model: &str,
    api_key: &str,
) -> anyhow::Result<()> {
    std::env::set_var("NEXUS_LLM_API_KEY", api_key);

    let llm_config = LlmConfig {
        provider: provider.to_string(),
        model: model.to_string(),
        api_key_env: "NEXUS_LLM_API_KEY".to_string(),
        timeout_secs: 30,
        ..Default::default()
    };

    let client = create_client(&llm_config)?;

    print!("  Testing connection to {} ({})... ", provider, model);
    let start = Instant::now();

    let params = GenerateParams {
        messages: vec![ChatMessage::user("Reply with exactly one word: connected")],
        max_tokens: 10,
        temperature: 0.0,
        json_mode: false,
    };

    match client.generate(params).await {
        Ok(resp) => {
            let elapsed = start.elapsed();
            println!("OK ({:.2}s)", elapsed.as_secs_f64());
            if let Some(usage) = resp.usage {
                println!(
                    "  Tokens: {} prompt + {} completion",
                    usage.prompt_tokens, usage.completion_tokens
                );
            }
            Ok(())
        }
        Err(e) => {
            println!("FAILED");
            Err(anyhow::anyhow!("Connection failed: {}", e))
        }
    }
}

// ── Env file I/O ─────────────────────────────────────────────────────

fn read_env_file(path: &PathBuf) -> HashMap<String, String> {
    let mut entries = HashMap::new();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    if let Some((k, v)) = parse_env_line(trimmed) {
                        entries.insert(k, v);
                    }
                }
            }
        }
    }
    entries
}

fn write_env_file(
    path: &PathBuf,
    entries: &HashMap<String, String>,
    contains_secrets: bool,
) -> anyhow::Result<()> {
    let ordered_keys: &[&str] = &[
        "NEXUS_DATABASE_PATH",
        "NEXUS_SYNC_POLICY",
        "NEXUS_AUTO_INGEST",
        "NEXUS_EMBEDDINGS_ENABLED",
        "NEXUS_LLM_PROVIDER",
        "NEXUS_LLM_MODEL",
        "NEXUS_LLM_API_KEY",
        "NEXUS_LLM_API_KEY_ENV",
        "NEXUS_LLM_BASE_URL",
        "NEXUS_AGENT_ENABLED",
        "NEXUS_AGENT_NAMESPACE",
        "NEXUS_AGENT_INBOX_DIR",
        "NEXUS_AGENT_CONSOLIDATION_INTERVAL",
        "NEXUS_AGENT_SCAN_INTERVAL",
    ];

    let mut lines: Vec<String> = Vec::new();
    lines.push("# Nexus Memory System environment".into());
    lines.push(String::new());

    for k in ordered_keys {
        if let Some(v) = entries.get(*k) {
            lines.push(format!("{}=\"{}\"", k, v));
        }
    }

    // Any remaining keys
    let mut remaining: Vec<_> = entries
        .iter()
        .filter(|(k, _)| !ordered_keys.contains(&k.as_str()))
        .collect();
    remaining.sort_by_key(|(k, _)| k.as_str());
    for (k, v) in remaining {
        lines.push(format!("{}=\"{}\"", k, v));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, lines.join("\n") + "\n")?;

    // Restrict permissions if the file contains secrets
    if contains_secrets {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
    }

    Ok(())
}

fn parse_env_line(line: &str) -> Option<(String, String)> {
    let (k, rest) = line.split_once('=')?;
    let k = k.trim().to_string();
    if k.is_empty() {
        return None;
    }
    let v = rest.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(v);
    Some((k, v.to_string()))
}

fn env_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("NEXUS_CONFIG_FILE") {
        return PathBuf::from(path);
    }
    let config_dir = std::env::var("NEXUS_INSTALL_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let xdg = std::env::var("XDG_CONFIG_HOME")
                .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
            PathBuf::from(xdg).join("nexus-memory-system")
        });
    config_dir.join("nexus.env")
}

fn validate_key(key: &str) -> anyhow::Result<()> {
    let valid_keys = [
        "NEXUS_DATABASE_PATH",
        "NEXUS_SYNC_POLICY",
        "NEXUS_AUTO_INGEST",
        "NEXUS_EMBEDDINGS_ENABLED",
        "NEXUS_LLM_PROVIDER",
        "NEXUS_LLM_MODEL",
        "NEXUS_LLM_API_KEY",
        "NEXUS_LLM_API_KEY_ENV",
        "NEXUS_LLM_BASE_URL",
        "NEXUS_AGENT_ENABLED",
        "NEXUS_AGENT_NAMESPACE",
        "NEXUS_AGENT_INBOX_DIR",
        "NEXUS_AGENT_CONSOLIDATION_INTERVAL",
        "NEXUS_AGENT_SCAN_INTERVAL",
        "NEXUS_LOG_LEVEL",
    ];

    if !valid_keys.contains(&key) {
        anyhow::bail!(
            "Unknown configuration key: {}\n\nRecognized keys:\n{}",
            key,
            valid_keys
                .iter()
                .map(|k| format!("  {}", k))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    Ok(())
}
