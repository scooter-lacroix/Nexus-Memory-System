//! Hooks command implementation

use anyhow::Result;
use clap::Subcommand;
use nexus_hooks::{HookError, HookFactory, LifecycleCapabilities};
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

/// Format lifecycle capabilities as a compact label.
fn format_lifecycle_label(caps: &LifecycleCapabilities) -> String {
    let mut parts = Vec::new();
    if caps.session_start {
        parts.push("start");
    }
    if caps.session_end {
        parts.push("end");
    }
    if caps.checkpoint {
        parts.push("checkpoint");
    }
    if caps.error_hook {
        parts.push("error");
    }
    if caps.compact {
        parts.push("compact");
    }
    if parts.is_empty() {
        "monitor-only".to_string()
    } else {
        parts.join("+")
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct LifecycleSupportSummary {
    start_agents: Vec<String>,
    end_agents: Vec<String>,
    checkpoint_agents: Vec<String>,
    error_agents: Vec<String>,
    compact_agents: Vec<String>,
    monitor_agents: Vec<String>,
}

fn collect_lifecycle_support(factory: &HookFactory) -> Result<LifecycleSupportSummary> {
    let mut summary = LifecycleSupportSummary::default();

    for agent_name in factory.supported_agents() {
        let hook = factory.create_hook_readonly(&agent_name)?;
        let caps = hook.lifecycle_capabilities();
        let label = hook.agent_type().to_string();

        if caps.session_start
            || caps.session_end
            || caps.checkpoint
            || caps.error_hook
            || caps.compact
        {
            if caps.session_start {
                summary.start_agents.push(label.clone());
            }
            if caps.session_end {
                summary.end_agents.push(label.clone());
            }
            if caps.checkpoint {
                summary.checkpoint_agents.push(label.clone());
            }
            if caps.error_hook {
                summary.error_agents.push(label.clone());
            }
            if caps.compact {
                summary.compact_agents.push(label);
            }
        } else {
            summary.monitor_agents.push(label);
        }
    }

    Ok(summary)
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

            for target in &targets {
                let mut hook = factory.create_hook(target)?;
                let caps = hook.lifecycle_capabilities();

                let start_result = if caps.session_start {
                    Some(hook.install_session_start_hook(callback.clone()).await)
                } else {
                    None
                };
                let checkpoint_result = if caps.checkpoint {
                    Some(hook.install_checkpoint_hook(callback.clone()).await)
                } else {
                    None
                };
                let error_result = if caps.error_hook {
                    Some(hook.install_error_hook(callback.clone()).await)
                } else {
                    None
                };
                let compact_result = if caps.compact {
                    Some(hook.install_compact_hook(callback.clone()).await)
                } else {
                    None
                };
                let end_result = if caps.session_end {
                    Some(hook.install_session_end_hook(callback.clone()).await)
                } else {
                    None
                };

                if let Some(Err(err)) = end_result {
                    return Err(anyhow::anyhow!(err.to_string()));
                }

                let start_installed = matches!(start_result, Some(Ok(())));
                let checkpoint_installed = matches!(checkpoint_result, Some(Ok(())));
                let error_installed = matches!(error_result, Some(Ok(())));
                let compact_installed = matches!(compact_result, Some(Ok(())));

                let install_status = if caps == LifecycleCapabilities::monitor_only() {
                    "monitor fallback"
                } else if hook.is_hook_installed() {
                    "installed"
                } else {
                    "available"
                };

                let lifecycle = format_lifecycle_label(&caps);
                let tier = hook.support_tier();

                println!(
                    "  {}: {} [{}] ({})",
                    hook.agent_type(),
                    install_status,
                    lifecycle,
                    tier,
                );

                if start_installed {
                    println!("    session start: enabled");
                }
                if checkpoint_installed {
                    println!("    checkpoint: enabled");
                }
                if error_installed {
                    println!("    error hook: enabled");
                }
                if compact_installed {
                    println!("    compact: enabled");
                }

                // Surface non-NotSupported errors
                if let Some(Err(HookError::InstallationFailed(e))) = start_result {
                    println!("    session start: install failed ({})", e);
                }
                if let Some(Err(HookError::InstallationFailed(e))) = checkpoint_result {
                    println!("    checkpoint: install failed ({})", e);
                }
                if let Some(Err(HookError::InstallationFailed(e))) = error_result {
                    println!("    error hook: install failed ({})", e);
                }
                if let Some(Err(HookError::InstallationFailed(e))) = compact_result {
                    println!("    compact: install failed ({})", e);
                }
            }

            tracing::info!("Hooks installed");
        }
        HooksCommands::Status { verbose } => {
            tracing::info!("Checking hook status");
            let factory = HookFactory::new();
            println!("Hook Status:");
            println!();

            for agent_name in factory.supported_agents() {
                let hook = factory.create_hook_readonly(&agent_name)?;
                let caps = hook.lifecycle_capabilities();
                let installed = hook.is_hook_installed();
                let lifecycle = format_lifecycle_label(&caps);
                let tier = hook.support_tier();

                let status_label = if installed { "installed" } else { "available" };

                println!(
                    "  {}: {} [{}] ({})",
                    hook.agent_type(),
                    status_label,
                    lifecycle,
                    tier,
                );

                if verbose {
                    println!("    reliability: {:.2}", hook.reliability_score());
                    println!(
                        "    capabilities: start={}, end={}, checkpoint={}, error={}, compact={}",
                        caps.session_start,
                        caps.session_end,
                        caps.checkpoint,
                        caps.error_hook,
                        caps.compact,
                    );
                }
            }

            println!();
            println!("Lifecycle support:");

            let summary = collect_lifecycle_support(&factory)?;

            if !summary.start_agents.is_empty() {
                println!("  session start:  {}", summary.start_agents.join(", "));
            }
            if !summary.end_agents.is_empty() {
                println!("  session end:    {}", summary.end_agents.join(", "));
            }
            if !summary.checkpoint_agents.is_empty() {
                println!("  checkpoint:     {}", summary.checkpoint_agents.join(", "));
            }
            if !summary.error_agents.is_empty() {
                println!("  error hook:     {}", summary.error_agents.join(", "));
            }
            if !summary.compact_agents.is_empty() {
                println!("  compact:        {}", summary.compact_agents.join(", "));
            }
            if !summary.monitor_agents.is_empty() {
                println!("  monitor-only:   {}", summary.monitor_agents.join(", "));
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

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_hooks::SupportTier;
    use serial_test::serial;
    use std::sync::{Mutex, OnceLock};

    fn home_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_format_lifecycle_label_includes_compact() {
        let caps = LifecycleCapabilities {
            session_start: true,
            session_end: true,
            checkpoint: true,
            error_hook: false,
            compact: true,
        };

        assert_eq!(
            format_lifecycle_label(&caps),
            "start+end+checkpoint+compact"
        );
    }

    #[test]
    fn test_collect_lifecycle_support_groups_agents_honestly() {
        let factory = HookFactory::new();
        let summary = collect_lifecycle_support(&factory).unwrap();

        // Claude has native lifecycle — start, end, compact, checkpoint
        assert!(summary
            .start_agents
            .iter()
            .any(|agent| agent == "claude-code"));
        assert!(summary
            .compact_agents
            .iter()
            .any(|agent| agent == "claude-code"));
        assert!(summary.end_agents.iter().any(|agent| agent == "pi-mono"));

        // Wrapper-lifecycle agents (Codex, OpenCode, etc.) report session_end
        // via atexit callback, so they appear in end_agents, not monitor_agents.
        assert!(summary.end_agents.iter().any(|agent| agent == "codex"));
        assert!(summary.end_agents.iter().any(|agent| agent == "opencode"));

        // Monitor-only agents (Gemini, Qwen) have no lifecycle capabilities
        assert!(summary.monitor_agents.iter().any(|agent| agent == "gemini"));
        assert!(summary.monitor_agents.iter().any(|agent| agent == "qwen"));
    }

    #[test]
    fn test_support_tier_honesty_via_factory() {
        let factory = HookFactory::new();

        // Native lifecycle agents
        for native in &["claude-code", "pi-mono", "oh-my-pi", "pi-skills"] {
            let hook = factory.create_hook_readonly(native).unwrap();
            assert_eq!(
                hook.support_tier(),
                SupportTier::NativeLifecycle,
                "{} should be native-lifecycle",
                native
            );
        }

        // Monitor-only agents
        for monitor in &["gemini", "qwen"] {
            let hook = factory.create_hook_readonly(monitor).unwrap();
            assert_eq!(
                hook.support_tier(),
                SupportTier::MonitorOnly,
                "{} should be monitor-only",
                monitor
            );
        }

        // Wrapper lifecycle agents
        for wrapper in &["codex", "amp", "opencode", "droid", "hermes"] {
            let hook = factory.create_hook_readonly(wrapper).unwrap();
            assert_eq!(
                hook.support_tier(),
                SupportTier::WrapperLifecycle,
                "{} should be wrapper-lifecycle",
                wrapper
            );
        }
    }

    #[test]
    #[serial]
    fn test_status_inspection_does_not_install_pi_family_skills() {
        let _guard = home_lock().lock().unwrap();
        let original_home = std::env::var_os("HOME");
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", temp.path());

        let factory = HookFactory::new();
        let _ = factory.create_hook_readonly("pi-mono").unwrap();
        let _ = factory.create_hook_readonly("oh-my-pi").unwrap();
        let _ = factory.create_hook_readonly("pi-skills").unwrap();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(!temp
            .path()
            .join(".pi")
            .join("agent")
            .join("skills")
            .join("nexus-memory-extraction")
            .join("SKILL.md")
            .exists());
        assert!(!temp
            .path()
            .join(".omp")
            .join("agent")
            .join("skills")
            .join("nexus-memory-extraction")
            .join("SKILL.md")
            .exists());
        assert!(!temp
            .path()
            .join(".pi-skills")
            .join("nexus-memory-extraction")
            .join("SKILL.md")
            .exists());
    }
}
