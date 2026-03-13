//! Tool introspection commands

use anyhow::{bail, Result};
use clap::Subcommand;

#[derive(Subcommand)]
#[command(disable_help_subcommand = true)]
pub enum ToolsCommands {
    /// List available tools or explain one tool
    Help {
        /// Optional tool name
        tool: Option<String>,
    },
    /// Print JSON schema for one tool or all tools
    Schema {
        /// Optional tool name
        tool: Option<String>,
    },
}

pub async fn execute(cmd: ToolsCommands) -> Result<()> {
    match cmd {
        ToolsCommands::Help { tool } => print_help(tool.as_deref()),
        ToolsCommands::Schema { tool } => print_schema(tool.as_deref()),
    }
}

fn print_help(tool: Option<&str>) -> Result<()> {
    let tools = nexus_mcp::get_tools();

    if let Some(name) = tool {
        let tool = tools
            .into_iter()
            .find(|tool| tool.name == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", name))?;

        println!("{}", tool.name);
        println!("{}", "=".repeat(tool.name.len()));
        println!("{}", tool.description);
        println!();
        println!("Schema:");
        println!("{}", serde_json::to_string_pretty(&tool.input_schema)?);
        return Ok(());
    }

    println!("Available Nexus tools");
    println!("====================");
    for tool in tools {
        println!("- {}: {}", tool.name, tool.description);
    }
    println!();
    println!("Use `nexus tools help <tool>` for details.");
    println!("Use `nexus tools schema <tool>` for JSON schema.");
    Ok(())
}

fn print_schema(tool: Option<&str>) -> Result<()> {
    if let Some(name) = tool {
        let tool = nexus_mcp::find_tool(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {}", name))?;
        println!("{}", serde_json::to_string_pretty(&tool.input_schema)?);
        return Ok(());
    }

    let schemas: serde_json::Map<String, serde_json::Value> = nexus_mcp::get_tools()
        .into_iter()
        .map(|tool| (tool.name, tool.input_schema))
        .collect();

    if schemas.is_empty() {
        bail!("No tools are registered");
    }

    println!("{}", serde_json::to_string_pretty(&schemas)?);
    Ok(())
}
