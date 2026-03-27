//! Nexus CLI - Command-line interface for Nexus Memory System
//!
//! This is the main entry point for the Nexus command-line interface.

use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod star;

#[derive(Parser)]
#[command(name = "nexus")]
#[command(author, version, about = "Nexus Memory System CLI", long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(long, global = true)]
    config: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, global = true, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the database
    Init {
        /// Reset the database if it exists
        #[arg(short, long)]
        reset: bool,
    },

    /// Start the server
    Serve {
        /// Transport type (stdio, http, web)
        #[arg(short = 't', long, default_value = "stdio")]
        transport: String,

        /// Port for HTTP transport
        #[arg(short, long, default_value = "8768")]
        port: u16,

        /// Enable the always-on memory agent
        #[arg(long)]
        agent: bool,
    },

    /// Store a memory
    Store {
        /// Memory content
        #[arg(short = 'm', long)]
        content: String,

        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Memory category
        #[arg(short = 'g', long, default_value = "general")]
        category: String,

        /// Memory labels (comma-separated)
        #[arg(short, long)]
        labels: Option<String>,

        /// Metadata JSON object
        #[arg(long)]
        metadata_json: Option<String>,

        /// Memory lane type (e.g., confidence, decision, workflow_note)
        #[arg(long)]
        memory_lane_type: Option<String>,
    },

    /// Search memories
    Search {
        /// Search query
        #[arg(short, long)]
        query: String,

        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Include raw operational activity memories
        #[arg(long)]
        include_raw: bool,
    },

    /// Show statistics
    Stats {
        /// Agent/namespace name
        #[arg(short, long)]
        agent: Option<String>,
    },

    /// Manage hooks for agent integration
    Hooks {
        #[command(subcommand)]
        command: commands::hooks::HooksCommands,
    },

    /// Inspect available Nexus tool definitions
    #[command(visible_alias = "tool")]
    Tools {
        #[command(subcommand)]
        command: commands::tools::ToolsCommands,
    },

    /// Run migration workflows for existing Nexus data
    Migrate {
        #[command(subcommand)]
        command: commands::migrate::MigrateCommands,
    },

    /// Ingest a hook event with LLM enrichment
    IngestHookEvent {
        /// Agent/namespace name (e.g., claude-code)
        #[arg(long)]
        agent: String,

        /// Hook event name (e.g., post-tool-use)
        #[arg(long)]
        event: String,

        /// Payload format (auto, claude-code)
        #[arg(long, default_value = "auto")]
        format: String,

        /// Effective session key for correlating fallback-scoped events
        #[arg(long)]
        session_key: Option<String>,

        /// Working directory associated with the event
        #[arg(long)]
        cwd: Option<String>,
    },

    /// Configure Nexus (interactive wizard, or use show/set subcommands)
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommands>,
    },

    /// Test LLM provider connectivity
    Llm {
        #[command(subcommand)]
        command: LlmCommands,
    },

    /// Evaluate a model against all aspects of the memory system
    Eval {
        /// Override provider (uses current config if omitted)
        #[arg(long)]
        provider: Option<String>,

        /// Override model (uses current config if omitted)
        #[arg(long)]
        model: Option<String>,
    },

    /// List memories with filters
    List {
        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Filter by category (general, facts, preferences, context, specifications, session)
        #[arg(short = 'g', long)]
        category: Option<String>,

        /// Show memories since (e.g., 1h, 24h, 7d, 2w, 2026-03-24)
        #[arg(long)]
        since: Option<String>,

        /// Show memories until (e.g., 1h, 24h, 7d, 2w, 2026-03-24)
        #[arg(long)]
        until: Option<String>,

        /// Maximum results
        #[arg(short, long, default_value = "20")]
        limit: usize,

        /// Offset for pagination
        #[arg(short, long, default_value = "0")]
        offset: usize,

        /// Include raw operational activity memories
        #[arg(long)]
        include_raw: bool,
    },

    /// Prune archived raw activity after it has been distilled and lineaged
    Clean {
        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Remove memories older than this cutoff (e.g. 7d, 2026-03-20)
        #[arg(long)]
        older_than: Option<String>,

        /// Maximum candidates to inspect or delete in one run
        #[arg(short, long, default_value = "100")]
        limit: usize,

        /// Apply the deletion. Default mode is dry-run.
        #[arg(long)]
        apply: bool,
    },

    /// Recall relevant memories for agent context
    Recall {
        /// Context query to match against
        #[arg(short, long)]
        query: String,

        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Maximum results
        #[arg(short, long, default_value = "5")]
        limit: usize,

        /// Filter by category
        #[arg(short = 'g', long)]
        category: Option<String>,

        /// Output format (human, json, compact)
        #[arg(short, long, default_value = "human")]
        format: String,

        /// Include raw operational activity memories
        #[arg(long)]
        include_raw: bool,
    },

    /// Consolidate memories (find patterns and relationships)
    Consolidate {
        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,
    },

    /// Distill raw hook events into meaningful session summaries
    Distill {
        /// Agent/namespace name
        #[arg(short, long, default_value = "claude-code")]
        agent: String,

        /// Max events per session to send to LLM
        #[arg(long, default_value = "100")]
        batch_size: usize,

        /// Show what would be distilled without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Inspect or rebuild session digests
    Digest {
        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Session key to inspect
        #[arg(long)]
        session_key: String,

        /// Rebuild digests before showing them
        #[arg(long)]
        force: bool,

        /// Output format (human, json)
        #[arg(short, long, default_value = "human")]
        format: String,
    },

    /// Manually run a dream/reflection cycle
    Dream {
        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Optional session key to scope the dream cycle
        #[arg(long)]
        session_key: Option<String>,

        /// Output format (human, json)
        #[arg(short, long, default_value = "human")]
        format: String,
    },

    /// Build and inspect a working representation for a query
    Represent {
        /// Agent/namespace name
        #[arg(short, long, default_value = "default")]
        agent: String,

        /// Query text to guide semantic search
        #[arg(short, long)]
        query: Option<String>,

        /// Perspective observer
        #[arg(long)]
        observer: Option<String>,

        /// Perspective subject
        #[arg(long)]
        subject: Option<String>,

        /// Session key to scope the perspective
        #[arg(long)]
        session_key: Option<String>,

        /// Maximum items across all buckets
        #[arg(short = 'm', long, default_value = "24")]
        max_items: usize,

        /// Include raw memories in the recent bucket
        #[arg(long)]
        include_raw: bool,

        /// Show ranking introspection (excluded candidates, inclusion reasons, reflections)
        #[arg(long)]
        introspect: bool,
    },

    /// Inspect evidence lineage for a memory
    Lineage {
        /// Memory ID whose lineage should be shown
        memory_id: i64,
    },

    /// Session lifecycle commands used by hook integrations
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current effective configuration
    Show,

    /// Set a configuration value, or pick a model interactively
    Set {
        /// Configuration key (e.g., NEXUS_LLM_PROVIDER)
        /// If omitted, launches interactive model selector
        key: Option<String>,

        /// Configuration value
        value: Option<String>,
    },
}

#[derive(Subcommand)]
enum LlmCommands {
    /// Test LLM provider connection
    Test {
        /// Override provider (e.g., openai, anthropic, gemini)
        #[arg(long)]
        provider: Option<String>,

        /// Override model (e.g., gpt-4o-mini, claude-sonnet-4-20250514)
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Start or resume session-scoped runtime state
    Start {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        session_key: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long, default_value = "session")]
        mode: String,
    },
    /// Record a non-terminal lifecycle event like compact/checkpoint
    Event {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        session_key: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        kind: String,
    },
    /// Finalize a session and run bounded shutdown work
    End {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        session_key: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    star::star_repo_background();

    // Initialize logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load stored per-provider credentials before any command runs
    commands::config::load_stored_credentials();

    // Execute command
    match cli.command {
        Commands::Init { reset } => {
            commands::init::execute(reset).await?;
        }
        Commands::Serve {
            transport,
            port,
            agent,
        } => {
            commands::serve::execute(transport, port, agent).await?;
        }
        Commands::Store {
            content,
            agent,
            category,
            labels,
            metadata_json,
            memory_lane_type,
        } => {
            commands::store::execute(
                content,
                agent,
                category,
                labels,
                metadata_json,
                memory_lane_type,
            )
            .await?;
        }
        Commands::Search {
            query,
            agent,
            limit,
            include_raw,
        } => {
            commands::search::execute(query, agent, limit, include_raw).await?;
        }
        Commands::Stats { agent } => {
            commands::stats::execute(agent).await?;
        }
        Commands::Hooks { command } => {
            commands::hooks::execute(command).await?;
        }
        Commands::Tools { command } => {
            commands::tools::execute(command).await?;
        }
        Commands::Migrate { command } => {
            commands::migrate::execute(command).await?;
        }
        Commands::IngestHookEvent {
            agent,
            event,
            format,
            session_key,
            cwd,
        } => {
            commands::ingest_hook_event::execute(agent, event, format, session_key, cwd).await?;
        }
        Commands::Config { command } => match command {
            Some(ConfigCommands::Show) => {
                commands::config::execute_show().await?;
            }
            Some(ConfigCommands::Set { key, value }) => match (key, value) {
                (Some(k), Some(v)) => {
                    commands::config::execute_set(k, v).await?;
                }
                (Some(provider_hint), None) => {
                    // Treat as provider name — launch model picker for that provider
                    commands::config::execute_model_picker(Some(provider_hint)).await?;
                }
                (None, _) => {
                    commands::config::execute_model_picker(None).await?;
                }
            },
            None => {
                commands::config::execute_wizard().await?;
            }
        },
        Commands::Llm { command } => match command {
            LlmCommands::Test { provider, model } => {
                commands::llm::execute_test(provider, model).await?;
            }
        },
        Commands::Eval { provider, model } => {
            commands::eval::execute(provider, model).await?;
        }
        Commands::List {
            agent,
            category,
            since,
            until,
            limit,
            offset,
            include_raw,
        } => {
            commands::list::execute(agent, category, since, until, limit, offset, include_raw)
                .await?;
        }
        Commands::Clean {
            agent,
            older_than,
            limit,
            apply,
        } => {
            commands::clean::execute(agent, older_than, limit, apply).await?;
        }
        Commands::Recall {
            query,
            agent,
            limit,
            category,
            format,
            include_raw,
        } => {
            commands::recall::execute(query, agent, limit, category, format, include_raw).await?;
        }
        Commands::Consolidate { agent } => {
            commands::consolidate::execute(agent).await?;
        }
        Commands::Distill {
            agent,
            batch_size,
            dry_run,
        } => {
            commands::distill::execute(agent, batch_size, dry_run).await?;
        }
        Commands::Digest {
            agent,
            session_key,
            force,
            format,
        } => {
            commands::digest::execute(agent, session_key, force, format).await?;
        }
        Commands::Dream {
            agent,
            session_key,
            format,
        } => {
            commands::dream::execute(agent, session_key, format).await?;
        }
        Commands::Represent {
            agent,
            query,
            observer,
            subject,
            session_key,
            max_items,
            include_raw,
            introspect,
        } => {
            commands::represent::execute(
                agent,
                query,
                observer,
                subject,
                session_key,
                max_items,
                include_raw,
                introspect,
            )
            .await?;
        }
        Commands::Lineage { memory_id } => {
            commands::lineage::execute(memory_id).await?;
        }
        Commands::Session { command } => match command {
            SessionCommands::Start {
                agent,
                session_key,
                cwd,
                mode,
            } => {
                commands::session::execute_start(agent, session_key, cwd, mode).await?;
            }
            SessionCommands::Event {
                agent,
                session_key,
                cwd,
                kind,
            } => {
                commands::session::execute_event(agent, session_key, cwd, kind).await?;
            }
            SessionCommands::End {
                agent,
                session_key,
                cwd,
                reason,
            } => {
                commands::session::execute_end(agent, session_key, cwd, reason).await?;
            }
        },
    }

    Ok(())
}
