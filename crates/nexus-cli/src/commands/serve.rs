//! Serve command implementation

use anyhow::Result;
use nexus_mcp::{McpConfig, McpServer};
use tracing::info;

/// Execute the serve command
pub async fn execute(transport: String, port: u16, agent: bool) -> Result<()> {
    info!("Starting Nexus Memory System server");
    info!("Transport: {}", transport);
    info!("Port: {}", port);

    // If --agent flag is set, force-enable the agent regardless of env value.
    if agent {
        std::env::set_var("NEXUS_AGENT_ENABLED", "true");
        info!("Agent mode enabled via --agent flag");
    }

    let config = McpConfig::default()
        .with_transport(&transport)
        .with_port(port);

    let mut server = McpServer::new(config);

    // Initialize server
    server.initialize().await?;

    info!("Server initialized");

    // Start server
    match transport.as_str() {
        "stdio" => {
            info!("Starting stdio transport (MCP protocol)");
            tokio::signal::ctrl_c().await?;
        }
        "http" => {
            info!("Starting HTTP server on port {}", port);
            tokio::signal::ctrl_c().await?;
        }
        "web" => {
            info!("Starting web dashboard on port {}", port);
            if agent {
                info!("Always-on memory agent ENABLED");
                // Agent is initialized inside WebDashboard::new when
                // the agent config has enabled=true
            }
            tokio::signal::ctrl_c().await?;
        }
        _ => {
            anyhow::bail!("Unknown transport: {}", transport);
        }
    }

    // Shutdown
    info!("Shutting down server");
    server.stop().await?;

    Ok(())
}
