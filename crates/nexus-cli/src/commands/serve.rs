//! Serve command implementation

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use nexus_core::Config;
use nexus_mcp::{McpConfig, McpServer};
use nexus_orchestrator::Orchestrator;
use nexus_storage::StorageManager;
use tracing::info;

/// Execute the serve command
pub async fn execute(transport: String, port: u16, agent: bool) -> Result<()> {
    info!("Starting Nexus Memory System server");
    info!("Transport: {}", transport);
    info!("Port: {}", port);

    if agent {
        std::env::set_var("NEXUS_AGENT_ENABLED", "true");
        info!("Agent mode enabled via --agent flag");
    }

    let config = Config::from_env().context("failed to load Nexus configuration")?;

    match transport.as_str() {
        "stdio" => serve_stdio().await,
        "http" => {
            eprintln!(
                "warning: --transport http is deprecated and serves the web dashboard, not MCP.\n\
                 Use --transport web instead. The http alias will be removed in a future release."
            );
            tracing::warn!(
                "deprecated transport 'http' used; serving web dashboard (not MCP over HTTP)"
            );
            serve_web_surface(&config, port).await
        }
        "web" => {
            if agent {
                info!("Always-on memory agent ENABLED");
            }
            serve_web_surface(&config, port).await
        }
        _ => anyhow::bail!("Unknown transport: {}", transport),
    }
}

async fn serve_stdio() -> Result<()> {
    let mut server = McpServer::new(McpConfig::stdio());
    server
        .start()
        .await
        .context("failed to start stdio MCP server")
}

async fn serve_web_surface(config: &Config, port: u16) -> Result<()> {
    let mut storage = StorageManager::from_url(&config.database_url())
        .await
        .context("failed to open storage for web server")?;
    storage
        .initialize()
        .await
        .context("failed to initialize storage for web server")?;

    let dashboard = nexus_web::create_dashboard(storage, Orchestrator::default())
        .await
        .context("failed to initialize web dashboard")?;

    let host: IpAddr = config
        .server
        .host
        .parse()
        .with_context(|| format!("invalid NEXUS_HOST value: {}", config.server.host))?;
    let addr = SocketAddr::new(host, port);

    dashboard
        .serve(addr)
        .await
        .context("web server exited with an error")
}
