//! Serve command implementation

use anyhow::Result;
use nexus_mcp::{McpConfig, McpServer};

/// Execute the serve command
pub async fn execute(transport: String, port: u16) -> Result<()> {
    tracing::info!("Starting Nexus Memory System server");
    tracing::info!("Transport: {}", transport);
    tracing::info!("Port: {}", port);

    let config = McpConfig::default()
        .with_transport(&transport)
        .with_port(port);

    let mut server = McpServer::new(config);

    // Initialize server
    server.initialize().await?;

    tracing::info!("Server initialized");

    // Start server
    match transport.as_str() {
        "stdio" => {
            tracing::info!("Starting stdio transport (MCP protocol)");
            // TODO: Implement stdio transport
            // For now, just wait
            tokio::signal::ctrl_c().await?;
        }
        "http" => {
            tracing::info!("Starting HTTP server on port {}", port);
            // TODO: Implement HTTP transport
            tokio::signal::ctrl_c().await?;
        }
        "web" => {
            tracing::info!("Starting web dashboard on port {}", port);
            // TODO: Implement web dashboard
            tokio::signal::ctrl_c().await?;
        }
        _ => {
            anyhow::bail!("Unknown transport: {}", transport);
        }
    }

    // Shutdown
    tracing::info!("Shutting down server");
    server.stop().await?;

    Ok(())
}
