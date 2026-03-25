//! CLI commands for testing LLM provider connectivity

use std::time::Instant;

use nexus_core::config::{Config, LlmConfig};
use nexus_llm::{create_client_with_fallback, ChatMessage, GenerateParams};

/// Test the LLM connection using current configuration.
pub async fn execute_test(
    provider_override: Option<String>,
    model_override: Option<String>,
) -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let llm_config = build_test_config(&config, provider_override, model_override);

    println!("Testing LLM connection...");
    println!();
    println!("  provider:    {}", llm_config.provider);
    println!("  model:       {}", llm_config.model);
    println!("  api_key_env: {}", llm_config.api_key_env);

    // Check if the API key is actually set
    match std::env::var(&llm_config.api_key_env) {
        Ok(_) => println!("  api_key:     set"),
        Err(_) => {
            eprintln!();
            eprintln!(
                "  ERROR: Environment variable {} is not set.",
                llm_config.api_key_env
            );
            eprintln!();
            eprintln!("  Set it in your shell or run:");
            eprintln!(
                "    nexus config set {} YOUR_API_KEY",
                llm_config.api_key_env
            );
            eprintln!();
            eprintln!("  Or set the key env var to reference, e.g.:");
            eprintln!("    nexus config set NEXUS_LLM_API_KEY_ENV OPENAI_API_KEY");
            eprintln!("    export OPENAI_API_KEY=sk-...");
            std::process::exit(1);
        }
    }

    if let Some(ref url) = llm_config.base_url {
        println!("  base_url:    {}", url);
    }
    println!("  timeout:     {}s", llm_config.timeout_secs);
    println!();

    // Create client
    let client = create_client_with_fallback(&llm_config)?;

    println!("  Connecting to {}...", llm_config.provider);
    let start = Instant::now();

    let params = GenerateParams {
        messages: vec![ChatMessage::user("Reply with exactly one word: connected")],
        max_tokens: 10,
        temperature: 0.0,
        json_mode: false,
    };

    let response = client.generate(params).await;
    let elapsed = start.elapsed();

    match response {
        Ok(resp) => {
            println!();
            println!("  Connection:   OK");
            println!("  Model:        {}", resp.model);
            println!("  Latency:      {:.2}s", elapsed.as_secs_f64());
            println!("  Response:     {}", resp.content.trim());
            if let Some(usage) = resp.usage {
                println!(
                    "  Tokens:       {} prompt + {} completion = {} total",
                    usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
                );
            }
            println!();
            println!("  LLM provider is working correctly.");
        }
        Err(e) => {
            eprintln!();
            eprintln!("  Connection:   FAILED");
            eprintln!("  Error:        {}", e);
            eprintln!();
            eprintln!("  Troubleshooting:");
            eprintln!(
                "    1. Verify your API key is correct: echo ${}",
                llm_config.api_key_env
            );
            eprintln!("    2. Check the provider and model are valid");
            eprintln!("    3. If using a custom base_url, verify it is reachable");
            if let Some(ref url) = llm_config.base_url {
                eprintln!("       curl -s {} >/dev/null", url);
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Map a provider ID to its standard API key env var name.
fn provider_key_env(provider: &str) -> &'static str {
    match provider {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "groq" => "GROQ_API_KEY",
        "zai" | "z.ai" => "ZAI_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        _ => "NEXUS_LLM_API_KEY",
    }
}

/// Build an LlmConfig for testing, applying any CLI overrides.
pub fn build_test_config(
    config: &Config,
    provider_override: Option<String>,
    model_override: Option<String>,
) -> LlmConfig {
    let mut llm = config.llm.clone();
    if let Some(provider) = &provider_override {
        llm.provider = provider.clone();
        llm.api_key_env = provider_key_env(provider).to_string();
    }
    if let Some(model) = model_override {
        llm.model = model;
    }
    // Use a short timeout for the test
    llm.timeout_secs = llm.timeout_secs.min(30);
    llm
}
