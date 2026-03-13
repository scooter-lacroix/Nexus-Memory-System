//! Hooks command implementation

use anyhow::Result;
use clap::Subcommand;
use nexus_hooks::HookFactory;
use std::sync::Arc;

/// Hooks commands
#[derive(Debug, Clone, Subcommand)]
pub enum HooksCommands {
    /// Install hooks for an agent
    Install {
        /// Agent name (or "all" for all agents)
        #[arg(short, long, default_value = "all")]
        agent: String,
    },

    /// Check hook status
    Status {
        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Uninstall hooks
    Uninstall {
        /// Agent name
        #[arg(short, long)]
        agent: String,
    },
}

/// Execute the hooks command
pub async fn execute(command: HooksCommands) -> Result<()> {
    match command {
        HooksCommands::Install { agent } => {
            tracing::info!("Installing hooks for agent: {}", agent);
            let factory = HookFactory::new();
            let callback = Arc::new(|_ctx| {});
            let targets = if agent == "all" {
                factory.supported_agents()
            } else {
                vec![agent]
            };

            for target in targets {
                let mut hook = factory.create_hook(&target)?;
                hook.install_session_end_hook(callback.clone()).await?;
                println!(
                    "{}: {}",
                    hook.agent_type(),
                    if hook.is_hook_installed() {
                        "installed"
                    } else {
                        "configured (monitor fallback)"
                    }
                );
            }

            tracing::info!("Hooks installed");
        }
        HooksCommands::Status { verbose } => {
            tracing::info!("Checking hook status");
            let factory = HookFactory::new();
            println!("Hook Status:");
            println!();

            for agent_name in factory.supported_agents() {
                let hook = factory.create_hook(&agent_name)?;
                let status = if hook.is_hook_installed() {
                    "installed"
                } else {
                    "available"
                };
                println!("  {}: {}", hook.agent_type(), status);

                if verbose {
                    println!("    reliability: {:.2}", hook.reliability_score());
                }
            }

            if verbose {
                println!();
                println!("Agent support:");
                println!("  native hooks: claude-code, gemini, qwen, pi-mono, oh-my-pi, pi-skills");
                println!("  cli monitoring: opencode, codex, amp, droid, generic");
            }
        }
        HooksCommands::Uninstall { agent } => {
            tracing::info!("Uninstalling hooks for agent: {}", agent);
            let factory = HookFactory::new();
            let mut hook = factory.create_hook(&agent)?;
            hook.uninstall_hooks().await?;
            println!("Uninstalled hooks for: {}", hook.agent_type());

            tracing::info!("Hooks uninstalled");
        }
    }

    Ok(())
}
