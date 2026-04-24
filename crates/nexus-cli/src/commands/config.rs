//! Interactive configuration wizard and config management

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use dialoguer::{Confirm, Input, Select};
use nexus_core::config::{Config, LlmConfig};
use nexus_llm::{
    create_client_with_fallback, list_models, ChatMessage, GenerateParams, Provider, ALL_PROVIDERS,
};

const LOCAL_LLM_RUNTIME_LABEL: &str =
    "Local OpenAI-compatible runtime (vLLM / LM Studio / llama.cpp)";

struct WizardLlmSelection {
    provider_id: String,
    api_key_env: String,
    api_key: String,
    base_url: Option<String>,
    model: String,
}

struct WizardEmbeddingSelection {
    enabled: bool,
    backend: String,
    provider: String,
    model: String,
    api_key_env: String,
    base_url: Option<String>,
    local_model_path: Option<String>,
    local_tokenizer_path: Option<String>,
}

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

    let stored_entries = read_env_file(&env_file);
    let llm = prompt_llm_selection(&config, &stored_entries).await?;
    let embedding = prompt_embedding_selection(&config, &llm)?;

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

    entries.insert("NEXUS_LLM_PROVIDER".into(), llm.provider_id.clone());
    entries.insert("NEXUS_LLM_MODEL".into(), llm.model.clone());
    entries.insert("NEXUS_LLM_API_KEY_ENV".into(), llm.api_key_env.clone());
    entries.insert("NEXUS_LLM_API_KEY".into(), llm.api_key.clone());

    // Store per-provider key so switching providers doesn't lose it
    let provider_key_var = format!("NEXUS_LLM_KEY_{}", llm.provider_id);
    if !llm.api_key.is_empty() {
        entries.insert(provider_key_var, llm.api_key.clone());
    }

    // Store per-provider base URL
    let provider_base_url_var = format!("NEXUS_LLM_BASE_URL_{}", llm.provider_id);
    if let Some(base_url) = &llm.base_url {
        entries.insert(provider_base_url_var, base_url.clone());
    } else {
        entries.remove(&provider_base_url_var);
    }

    // Active base URL (backward compat)
    if let Some(base_url) = &llm.base_url {
        entries.insert("NEXUS_LLM_BASE_URL".into(), base_url.clone());
    } else {
        entries.remove("NEXUS_LLM_BASE_URL");
    }

    apply_embedding_selection(&mut entries, &embedding);

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
            test_connection_with_key(&llm.provider_id, &llm.model, &key_value).await?;
        }
    }

    println!();
    println!("  Restart your shell or run:");
    println!("    source {}", env_file.display());

    Ok(())
}

async fn prompt_llm_selection(
    config: &Config,
    stored_entries: &HashMap<String, String>,
) -> anyhow::Result<WizardLlmSelection> {
    let mut items: Vec<String> = ALL_PROVIDERS
        .iter()
        .map(|p| p.display_label().to_string())
        .collect();
    items.push(LOCAL_LLM_RUNTIME_LABEL.to_string());
    let item_refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();

    let current_idx = ALL_PROVIDERS
        .iter()
        .position(|p| p.to_string() == config.llm.provider)
        .unwrap_or(0);

    let selection = Select::new()
        .with_prompt("Select LLM provider")
        .items(&item_refs)
        .default(current_idx)
        .interact()?;

    if selection == item_refs.len() - 1 {
        return prompt_local_llm_runtime();
    }

    let provider = ALL_PROVIDERS[selection];
    let provider_id = provider.to_string();
    let stored_key_var = format!("NEXUS_LLM_KEY_{}", provider_id);
    let stored_key = stored_entries.get(&stored_key_var).cloned();

    let prompt = if stored_key.is_some() {
        format!(
            "API key for {} (stored, press Enter to keep)",
            provider.display_label()
        )
    } else {
        format!("API key for {}", provider.display_label())
    };

    let api_key: String = Input::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()?;

    let api_key = if api_key.is_empty() {
        stored_key.unwrap_or_default()
    } else {
        api_key
    };

    if api_key.is_empty() {
        eprintln!();
        eprintln!("  No API key provided. Configuration will be saved but LLM");
        eprintln!("  features will not work until you set the key.");
        eprintln!();
    }

    let default_url = provider.default_base_url();
    let base_url_prompt = if provider.requires_base_url() {
        "Base URL (required for this provider)".to_string()
    } else if default_url.is_empty() {
        "Base URL (press Enter for default)".to_string()
    } else {
        format!("Base URL (press Enter for {})", default_url)
    };
    let base_url_input: String = Input::new()
        .with_prompt(&base_url_prompt)
        .default(default_url.to_string())
        .interact_text()?;
    let base_url = (!base_url_input.trim().is_empty()).then_some(base_url_input.clone());

    let model = if !api_key.is_empty() {
        std::env::set_var("NEXUS_LLM_API_KEY", &api_key);
        println!();
        println!(
            "  Fetching available models from {}...",
            provider.display_label()
        );

        let mut llm_config = LlmConfig {
            provider: provider_id.clone(),
            api_key_env: "NEXUS_LLM_API_KEY".into(),
            ..Default::default()
        };
        if let Some(url) = &base_url {
            llm_config.base_url = Some(url.clone());
        }

        match list_models(&llm_config).await {
            Ok(models) if models.is_empty() => {
                eprintln!("  Warning: provider returned 0 models");
                eprintln!();
                Input::new()
                    .with_prompt("Model")
                    .default(provider.default_model().to_string())
                    .interact_text()?
            }
            Ok(models) => {
                println!("  Found {} models", models.len());
                println!();
                let current_model = if config.llm.provider == provider_id {
                    &config.llm.model
                } else {
                    provider.default_model()
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
                    .default(provider.default_model().to_string())
                    .interact_text()?
            }
        }
    } else {
        Input::new()
            .with_prompt("Model")
            .default(provider.default_model().to_string())
            .interact_text()?
    };

    Ok(WizardLlmSelection {
        provider_id,
        api_key_env: "NEXUS_LLM_API_KEY".to_string(),
        api_key,
        base_url,
        model,
    })
}

fn prompt_local_llm_runtime() -> anyhow::Result<WizardLlmSelection> {
    let runtime_options = ["vLLM", "LM Studio", "llama.cpp", "Custom OpenAI-compatible"];
    let selection = Select::new()
        .with_prompt("Select local LLM runtime")
        .items(&runtime_options)
        .default(0)
        .interact()?;
    let runtime = runtime_options[selection];
    let default_base_url = default_local_runtime_base_url(runtime);

    let base_url: String = Input::new()
        .with_prompt("Base URL")
        .default(default_base_url.to_string())
        .interact_text()?;
    let model: String = Input::new()
        .with_prompt("Model")
        .default("openai/gpt-4o-mini".to_string())
        .interact_text()?;
    let api_key: String = Input::new()
        .with_prompt("API key (press Enter to use a harmless local placeholder)")
        .allow_empty(true)
        .interact_text()?;

    Ok(WizardLlmSelection {
        provider_id: "openai".to_string(),
        api_key_env: "NEXUS_LLM_API_KEY".to_string(),
        api_key: if api_key.is_empty() {
            "local".to_string()
        } else {
            api_key
        },
        base_url: Some(base_url),
        model,
    })
}

fn prompt_embedding_selection(
    config: &Config,
    llm: &WizardLlmSelection,
) -> anyhow::Result<WizardEmbeddingSelection> {
    println!();
    let options = [
        "Disable semantic embeddings",
        "Use the same remote provider as the main LLM",
        "Use a different remote embedding provider",
        "Use a local OpenAI-compatible runtime (vLLM / LM Studio / llama.cpp)",
        "Use a local ONNX embedding model",
    ];
    let selection = Select::new()
        .with_prompt("Select embedding backend")
        .items(&options)
        .default(if config.embedding.enabled { 1 } else { 0 })
        .interact()?;

    match selection {
        0 => Ok(WizardEmbeddingSelection {
            enabled: false,
            backend: "local".to_string(),
            provider: "local".to_string(),
            model: config.embedding.model.clone(),
            api_key_env: String::new(),
            base_url: None,
            local_model_path: config.embedding.local_model_path.clone(),
            local_tokenizer_path: config.embedding.local_tokenizer_path.clone(),
        }),
        1 => Ok(WizardEmbeddingSelection {
            enabled: true,
            backend: "openai-compatible".to_string(),
            provider: "inherit".to_string(),
            model: prompt_embedding_model_for_inherited_provider(
                &llm.model,
                default_embedding_model_for_provider(&llm.provider_id),
            )?,
            api_key_env: llm.api_key_env.clone(),
            base_url: llm.base_url.clone(),
            local_model_path: None,
            local_tokenizer_path: None,
        }),
        2 => prompt_remote_embedding_provider(),
        3 => prompt_local_embedding_runtime(),
        4 => prompt_local_onnx_embeddings(config),
        _ => unreachable!(),
    }
}

fn prompt_remote_embedding_provider() -> anyhow::Result<WizardEmbeddingSelection> {
    let providers = [
        "OpenAI",
        "Google Gemini (OpenAI-compatible endpoint)",
        "OpenRouter",
        "Groq",
        "Mistral",
        "Custom OpenAI-compatible provider",
    ];
    let selection = Select::new()
        .with_prompt("Select remote embedding provider")
        .items(&providers)
        .default(0)
        .interact()?;

    let (provider_id, default_base_url, default_api_env) = match selection {
        0 => (
            "openai",
            Some("https://api.openai.com/v1"),
            "OPENAI_API_KEY",
        ),
        1 => (
            "gemini",
            Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            "GEMINI_API_KEY",
        ),
        2 => (
            "openrouter",
            Some("https://openrouter.ai/api/v1"),
            "OPENROUTER_API_KEY",
        ),
        3 => (
            "groq",
            Some("https://api.groq.com/openai/v1"),
            "GROQ_API_KEY",
        ),
        4 => (
            "mistral",
            Some("https://api.mistral.ai/v1"),
            "MISTRAL_API_KEY",
        ),
        _ => ("custom", None, ""),
    };

    let model = prompt_embedding_model(
        "Embedding model",
        default_embedding_model_for_provider(provider_id),
    )?;
    let base_url_input: String = Input::new()
        .with_prompt("Embedding base URL")
        .default(
            default_base_url
                .unwrap_or("https://api.openai.com/v1")
                .to_string(),
        )
        .interact_text()?;
    let api_key_env: String = Input::new()
        .with_prompt("API key env var (leave empty if the endpoint does not require one)")
        .default(default_api_env.to_string())
        .allow_empty(true)
        .interact_text()?;

    Ok(WizardEmbeddingSelection {
        enabled: true,
        backend: "openai-compatible".to_string(),
        provider: provider_id.to_string(),
        model,
        api_key_env,
        base_url: Some(base_url_input),
        local_model_path: None,
        local_tokenizer_path: None,
    })
}

fn prompt_local_embedding_runtime() -> anyhow::Result<WizardEmbeddingSelection> {
    let runtimes = ["vLLM", "LM Studio", "llama.cpp", "Custom OpenAI-compatible"];
    let selection = Select::new()
        .with_prompt("Select local embedding runtime")
        .items(&runtimes)
        .default(0)
        .interact()?;
    let runtime = runtimes[selection];
    let base_url: String = Input::new()
        .with_prompt("Embedding base URL")
        .default(default_local_runtime_base_url(runtime).to_string())
        .interact_text()?;
    let model = prompt_embedding_model("Embedding model", "text-embedding-3-small")?;

    Ok(WizardEmbeddingSelection {
        enabled: true,
        backend: "openai-compatible".to_string(),
        provider: runtime.to_lowercase().replace(['.', ' '], ""),
        model,
        api_key_env: String::new(),
        base_url: Some(base_url),
        local_model_path: None,
        local_tokenizer_path: None,
    })
}

fn prompt_local_onnx_embeddings(config: &Config) -> anyhow::Result<WizardEmbeddingSelection> {
    let model_path: String = Input::new()
        .with_prompt("ONNX model path")
        .default(
            config
                .embedding
                .local_model_path
                .clone()
                .unwrap_or_else(|| "models/all-MiniLM-L6-v2.onnx".to_string()),
        )
        .interact_text()?;
    let tokenizer_path: String = Input::new()
        .with_prompt("Tokenizer directory")
        .default(
            config
                .embedding
                .local_tokenizer_path
                .clone()
                .unwrap_or_else(|| "models/all-MiniLM-L6-v2-tokenizer".to_string()),
        )
        .interact_text()?;

    Ok(WizardEmbeddingSelection {
        enabled: true,
        backend: "local".to_string(),
        provider: "local".to_string(),
        model: "all-MiniLM-L6-v2".to_string(),
        api_key_env: String::new(),
        base_url: None,
        local_model_path: Some(model_path),
        local_tokenizer_path: Some(tokenizer_path),
    })
}

fn prompt_embedding_model(prompt: &str, default_model: &str) -> anyhow::Result<String> {
    Input::new()
        .with_prompt(prompt)
        .default(default_model.to_string())
        .interact_text()
        .map_err(Into::into)
}

fn prompt_embedding_model_for_inherited_provider(
    llm_model: &str,
    default_embedding_model: &str,
) -> anyhow::Result<String> {
    let options = [
        format!("Use the same model as the main LLM ({})", llm_model),
        format!(
            "Use a different embedding model ({})",
            default_embedding_model
        ),
    ];
    let option_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
    let selection = Select::new()
        .with_prompt("Embedding model strategy")
        .items(&option_refs)
        .default(1)
        .interact()?;

    if selection == 0 {
        Ok("inherit".to_string())
    } else {
        prompt_embedding_model("Embedding model", default_embedding_model)
    }
}

fn default_embedding_model_for_provider(provider: &str) -> &str {
    match provider {
        "openai" => "text-embedding-3-small",
        "gemini" | "google" => "text-embedding-004",
        "openrouter" => "openai/text-embedding-3-small",
        "mistral" => "mistral-embed",
        _ => "text-embedding-3-small",
    }
}

fn default_local_runtime_base_url(runtime: &str) -> &'static str {
    match runtime {
        "vLLM" => "http://127.0.0.1:8000/v1",
        "LM Studio" => "http://127.0.0.1:1234/v1",
        "llama.cpp" => "http://127.0.0.1:8080/v1",
        _ => "http://127.0.0.1:8000/v1",
    }
}

fn apply_embedding_selection(
    entries: &mut HashMap<String, String>,
    embedding: &WizardEmbeddingSelection,
) {
    entries.insert(
        "NEXUS_EMBEDDINGS_ENABLED".into(),
        if embedding.enabled { "true" } else { "false" }.to_string(),
    );
    entries.insert("NEXUS_EMBEDDING_BACKEND".into(), embedding.backend.clone());
    entries.insert(
        "NEXUS_EMBEDDING_PROVIDER".into(),
        embedding.provider.clone(),
    );
    entries.insert("NEXUS_EMBEDDING_MODEL".into(), embedding.model.clone());

    if !embedding.api_key_env.trim().is_empty() {
        entries.insert(
            "NEXUS_EMBEDDING_API_KEY_ENV".into(),
            embedding.api_key_env.clone(),
        );
    } else {
        entries.remove("NEXUS_EMBEDDING_API_KEY_ENV");
    }

    if let Some(base_url) = &embedding.base_url {
        entries.insert("NEXUS_EMBEDDING_BASE_URL".into(), base_url.clone());
    } else {
        entries.remove("NEXUS_EMBEDDING_BASE_URL");
    }

    if let Some(model_path) = &embedding.local_model_path {
        entries.insert("NEXUS_EMBEDDING_MODEL_PATH".into(), model_path.clone());
    } else {
        entries.remove("NEXUS_EMBEDDING_MODEL_PATH");
    }

    if let Some(tokenizer_path) = &embedding.local_tokenizer_path {
        entries.insert("NEXUS_TOKENIZER_PATH".into(), tokenizer_path.clone());
    } else {
        entries.remove("NEXUS_TOKENIZER_PATH");
    }
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

    // Show all stored provider credentials
    let stored = stored_provider_keys();
    if !stored.is_empty() {
        println!();
        println!("  Stored credentials:");
        for (id, masked) in &stored {
            let marker = if *id == config.llm.provider {
                " (active)"
            } else {
                ""
            };
            println!("    {}: {}{}", id, masked, marker);
        }
    }
    println!();

    println!("Embeddings:");
    println!("  enabled:     {}", config.embedding.enabled);
    println!("  backend:     {}", config.embedding.backend);
    println!("  provider:    {}", config.embedding.provider);
    println!("  model:       {}", config.embedding.model);
    if !config.embedding.api_key_env.is_empty() {
        println!("  api_key_env: {}", config.embedding.api_key_env);
    }
    if let Some(ref url) = config.embedding.base_url {
        println!("  base_url:    {}", url);
    }
    if config.embedding.backend.eq_ignore_ascii_case("local")
        || config.embedding.backend.eq_ignore_ascii_case("onnx")
    {
        if let Some(ref path) = config.embedding.local_model_path {
            println!("  model_path:  {}", path);
        }
        if let Some(ref path) = config.embedding.local_tokenizer_path {
            println!("  tokenizer:   {}", path);
        }
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
    let is_secret = key.contains("API_KEY");
    write_env_file(&env_file, &entries, is_secret)?;

    println!("Set {}=\"{}\" in {}", key, value, env_file.display());
    println!();
    println!("Restart your shell or run:");
    println!("  source {}", env_file.display());

    Ok(())
}

/// Interactive model picker — select a provider, then a model from that provider.
///
/// If `provider_name` is given, skip provider selection and use that provider
/// directly (saving it as the new default). If omitted, show the provider list.
pub async fn execute_model_picker(provider_name: Option<String>) -> anyhow::Result<()> {
    let config = Config::from_env()?;

    // ── Provider selection ────────────────────────────────────────
    let provider = if let Some(ref name) = provider_name {
        match Provider::resolve(name) {
            Some(p) => p,
            None => {
                eprintln!("  Unknown provider: '{}'", name);
                eprintln!(
                    "  Available: {}",
                    ALL_PROVIDERS
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Ok(());
            }
        }
    } else {
        // No provider specified — show provider list
        let items: Vec<&str> = ALL_PROVIDERS.iter().map(|p| p.display_label()).collect();
        let current_idx = ALL_PROVIDERS
            .iter()
            .position(|p| p.to_string() == config.llm.provider)
            .unwrap_or(0);

        let selection = Select::new()
            .with_prompt("Select provider")
            .items(&items)
            .default(current_idx)
            .interact()?;

        ALL_PROVIDERS[selection]
    };

    let provider_id = provider.to_string();
    let switching = provider_id != config.llm.provider;

    // ── API key resolution ────────────────────────────────────────
    let stored_key_var = format!("NEXUS_LLM_KEY_{}", provider_id);
    let stored_entries = read_env_file(&env_file_path());
    let stored_key = stored_entries
        .get(&stored_key_var)
        .cloned()
        .unwrap_or_default();

    // Try: per-provider stored key > standard env var > NEXUS_LLM_API_KEY
    let api_key = if !stored_key.is_empty() {
        stored_key
    } else if let Ok(key) = std::env::var(provider.default_api_key_env()) {
        key
    } else {
        std::env::var("NEXUS_LLM_API_KEY").unwrap_or_default()
    };

    if api_key.is_empty() {
        eprintln!("  No API key found for {}.", provider.display_label());
        eprintln!("  Run 'nexus config' to set one up.");
        return Ok(());
    }

    // Inject the key so the LLM client can find it
    std::env::set_var(provider.default_api_key_env(), &api_key);
    std::env::set_var("NEXUS_LLM_API_KEY", &api_key);

    println!();
    if switching {
        println!(
            "  Switching provider: {} -> {}",
            config.llm.provider, provider_id
        );
    }
    println!(
        "  Fetching available models from {}...",
        provider.display_label()
    );

    let mut llm_config = LlmConfig {
        provider: provider_id.clone(),
        api_key_env: provider.default_api_key_env().into(),
        model: provider.default_model().into(),
        ..Default::default()
    };
    if let Ok(url) = std::env::var("NEXUS_LLM_BASE_URL") {
        llm_config.base_url = Some(url);
    }

    let models = match list_models(&llm_config).await {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            eprintln!("  Provider returned 0 models.");
            return Ok(());
        }
        Err(e) => {
            eprintln!("  Failed to fetch models: {}", e);
            eprintln!("  Check your API key and base URL.");
            return Ok(());
        }
    };

    println!("  Found {} models", models.len());
    println!();

    // Pre-select: current model if switching to same provider, else default
    let current_model = if !switching {
        &config.llm.model
    } else {
        provider.default_model()
    };
    let default_idx = models.iter().position(|m| m == current_model).unwrap_or(0);

    let items: Vec<&str> = models.iter().map(|s| s.as_str()).collect();
    let selected = Select::new()
        .with_prompt("Select model")
        .items(&items)
        .default(default_idx)
        .interact()?;

    let new_model = &models[selected];

    // ── Save ──────────────────────────────────────────────────────
    let env_file = env_file_path();
    let mut entries = read_env_file(&env_file);

    let provider_changed = entries
        .get("NEXUS_LLM_PROVIDER")
        .map_or(true, |v| *v != provider_id);
    let model_changed = entries.get("NEXUS_LLM_MODEL") != Some(new_model);

    if !provider_changed && !model_changed {
        println!();
        println!("  Unchanged: {} ({})", provider_id, new_model);
        return Ok(());
    }

    entries.insert("NEXUS_LLM_PROVIDER".into(), provider_id.clone());
    entries.insert("NEXUS_LLM_MODEL".into(), new_model.clone());
    entries.insert(
        "NEXUS_LLM_API_KEY_ENV".into(),
        provider.default_api_key_env().into(),
    );
    write_env_file(&env_file, &entries, false)?;

    println!();
    if switching {
        println!("  Provider: {} -> {}", config.llm.provider, provider_id);
    }
    if provider_changed || model_changed {
        println!("  Model:    {} -> {}", config.llm.model, new_model);
    }
    println!("  Saved to {}", env_file.display());
    println!();
    println!("  Restart your shell or run:");
    println!("    source {}", env_file.display());

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

    let client = create_client_with_fallback(&llm_config)?;

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

// ── Stored credential loading ─────────────────────────────────────────

/// Load all stored configuration from nexus.env into the process environment.
///
/// This makes the env file behave like a sourced shell profile: all NEXUS_*
/// settings (provider, model, agent flags, etc.) are available to
/// `Config::from_env()` without requiring the user to source the file.
///
/// Additionally, per-provider API keys stored as `NEXUS_LLM_KEY_{provider}`
/// are mapped to their standard env var names (e.g., `GEMINI_API_KEY`).
///
/// Idempotent — does not overwrite env vars that are already set in the
/// shell, allowing explicit overrides.
pub fn load_stored_credentials() {
    let env_file = env_file_path();
    let entries = read_env_file(&env_file);

    // Load all NEXUS_* settings from the env file (except per-provider keys,
    // which need special handling below).
    for (key, value) in &entries {
        if key.starts_with("NEXUS_")
            && !key.starts_with("NEXUS_LLM_KEY_")
            && !key.starts_with("NEXUS_LLM_BASE_URL_")
            && std::env::var(key).is_err()
        {
            std::env::set_var(key, value);
        }
    }

    // Map per-provider keys to their standard env var names
    for provider in ALL_PROVIDERS {
        let pid = provider.to_string();
        let stored_key_var = format!("NEXUS_LLM_KEY_{}", pid);
        if let Some(key) = entries.get(&stored_key_var) {
            if !key.is_empty() && std::env::var(provider.default_api_key_env()).is_err() {
                std::env::set_var(provider.default_api_key_env(), key);
            }
        }

        // Set per-provider base URL as NEXUS_LLM_BASE_URL when active
        let stored_url_var = format!("NEXUS_LLM_BASE_URL_{}", pid);
        if let Some(url) = entries.get(&stored_url_var) {
            if !url.is_empty() {
                if let Ok(active) = std::env::var("NEXUS_LLM_PROVIDER") {
                    if active == pid && std::env::var("NEXUS_LLM_BASE_URL").is_err() {
                        std::env::set_var("NEXUS_LLM_BASE_URL", url);
                    }
                }
            }
        }
    }

    // Ensure NEXUS_LLM_API_KEY is set from the active provider's stored key
    if std::env::var("NEXUS_LLM_API_KEY").is_err() {
        if let Ok(active_provider) = std::env::var("NEXUS_LLM_PROVIDER") {
            let stored_key_var = format!("NEXUS_LLM_KEY_{}", active_provider);
            if let Some(key) = entries.get(&stored_key_var) {
                if !key.is_empty() {
                    std::env::set_var("NEXUS_LLM_API_KEY", key);
                }
            }
        }
    }
}

/// Get a list of all providers that have stored API keys.
fn stored_provider_keys() -> Vec<(String, String)> {
    let env_file = env_file_path();
    let entries = read_env_file(&env_file);
    let mut result = Vec::new();
    for provider in ALL_PROVIDERS {
        let pid = provider.to_string();
        let key_var = format!("NEXUS_LLM_KEY_{}", pid);
        if let Some(key) = entries.get(&key_var) {
            if !key.is_empty() {
                result.push((
                    pid,
                    format!(
                        "{}...{}",
                        &key[..6.min(key.len())],
                        &key[key.len().saturating_sub(4)..]
                    ),
                ));
            }
        }
    }
    result
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
        "NEXUS_EMBEDDING_BACKEND",
        "NEXUS_EMBEDDING_PROVIDER",
        "NEXUS_EMBEDDING_MODEL",
        "NEXUS_EMBEDDING_API_KEY_ENV",
        "NEXUS_EMBEDDING_BASE_URL",
        "NEXUS_EMBEDDING_MODEL_PATH",
        "NEXUS_TOKENIZER_PATH",
        "NEXUS_LLM_PROVIDER",
        "NEXUS_LLM_MODEL",
        "NEXUS_LLM_API_KEY",
        "NEXUS_LLM_API_KEY_ENV",
        "NEXUS_LLM_BASE_URL",
        "NEXUS_AGENT_ENABLED",
        "NEXUS_AGENT_NAMESPACE",
        "NEXUS_AGENT_INBOX_DIR",
        "NEXUS_AGENT_CONSOLIDATION_INTERVAL_MINS",
        "NEXUS_AGENT_SCAN_INTERVAL_SECS",
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
    if let Ok(dir) = std::env::var("NEXUS_INSTALL_CONFIG_DIR") {
        return PathBuf::from(dir).join("nexus.env");
    }
    if let Some(dir) = dirs::config_dir() {
        return dir.join("nexus-memory-system").join("nexus.env");
    }
    // Last resort: HOME/.config — never /tmp for config files containing secrets
    std::env::var("HOME")
        .map(|h| {
            PathBuf::from(h)
                .join(".config")
                .join("nexus-memory-system")
                .join("nexus.env")
        })
        .unwrap_or_else(|_| PathBuf::from(".nexus.env"))
}

fn validate_key(key: &str) -> anyhow::Result<()> {
    let valid_prefixes = [
        "NEXUS_DATABASE_PATH",
        "NEXUS_SYNC_POLICY",
        "NEXUS_AUTO_INGEST",
        "NEXUS_EMBEDDINGS_ENABLED",
        "NEXUS_EMBEDDING_BACKEND",
        "NEXUS_EMBEDDING_PROVIDER",
        "NEXUS_EMBEDDING_MODEL",
        "NEXUS_EMBEDDING_API_KEY_ENV",
        "NEXUS_EMBEDDING_BASE_URL",
        "NEXUS_EMBEDDING_MODEL_PATH",
        "NEXUS_TOKENIZER_PATH",
        "NEXUS_LLM_PROVIDER",
        "NEXUS_LLM_MODEL",
        "NEXUS_LLM_API_KEY",
        "NEXUS_LLM_API_KEY_ENV",
        "NEXUS_LLM_BASE_URL",
        "NEXUS_LLM_KEY_",
        "NEXUS_AGENT_ENABLED",
        "NEXUS_AGENT_NAMESPACE",
        "NEXUS_AGENT_INBOX_DIR",
        "NEXUS_AGENT_CONSOLIDATION_INTERVAL_MINS",
        "NEXUS_AGENT_SCAN_INTERVAL_SECS",
        "NEXUS_LOG_LEVEL",
    ];

    if !valid_prefixes
        .iter()
        .any(|p| key.starts_with(p) || key == *p)
    {
        anyhow::bail!(
            "Unknown configuration key: {}\n\nRecognized keys:\n{}",
            key,
            valid_prefixes
                .iter()
                .map(|k| format!(
                    "  {}{}",
                    k,
                    if k.ends_with('_') { "<provider>" } else { "" }
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    Ok(())
}
