//! Nexus CLI - Command-line interface for Nexus Memory System
//!
//! This is the main entry point for the Nexus command-line interface.

use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Execute command
    match cli.command {
        Commands::Init { reset } => {
            commands::init::execute(reset).await?;
        }
        Commands::Serve { transport, port } => {
            commands::serve::execute(transport, port).await?;
        }
        Commands::Store {
            content,
            agent,
            category,
            labels,
        } => {
            commands::store::execute(content, agent, category, labels).await?;
        }
        Commands::Search {
            query,
            agent,
            limit,
        } => {
            commands::search::execute(query, agent, limit).await?;
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
    }

    Ok(())
}
