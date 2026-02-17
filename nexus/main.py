"""
Main entry point for Nexus Memory System
"""

import sys
import asyncio
import argparse
from pathlib import Path
from loguru import logger

from .config import config
from .database import setup_database, get_database_info
from .server import mcp, get_memory_manager


def setup_logging():
    """Setup logging configuration"""
    if config.verbose:
        logger.remove()
        logger.add(
            sys.stderr,
            format="<green>{time:YYYY-MM-DD HH:mm:ss}</green> | <level>{level: <8}</level> | <cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> - <level>{message}</level>",
            level="DEBUG"
        )
        logger.info("Verbose logging enabled")
    else:
        logger.remove()
        logger.add(
            sys.stderr,
            format="{time:HH:mm:ss} | {level} | {message}",
            level="INFO"
        )


def setup_agent_routes(app):
    """Setup custom routes for agent script compatibility"""

    @app.get("/health")
    async def health_check():
        """Health check endpoint for agent scripts"""
        return {
            "status": "healthy",
            "server": "nexus-memory-system",
            "transport": "http",
            "url": f"http://{config.host}:{config.port}/mcp",
            "web_ui": f"http://{config.host}:{config.web_port}" if config.is_web_enabled() else None,
        }

    @app.post("/call")
    async def call_tool(request):
        """
        Simple JSON-RPC call endpoint for agent script compatibility

        Expected format:
        {
            "tool": "tool_name",
            "arguments": {...}
        }
        """
        try:
            import json
            from fastapi import HTTPException

            body = await request.json()

            # Extract tool name and arguments
            tool_name = body.get("tool")
            arguments = body.get("arguments", {})

            if not tool_name:
                raise HTTPException(status_code=400, detail="Tool name is required")

            # Get all available tools
            tools = mcp.get_tools()

            # Find the requested tool
            tool = None
            for available_tool in tools:
                if available_tool.name == tool_name:
                    tool = available_tool
                    break

            if not tool:
                raise HTTPException(status_code=404, detail=f"Tool '{tool_name}' not found")

            # Call the tool - properly handle FastMCP tools
            logger.info(f"Calling tool '{tool_name}' with arguments: {arguments}")

            # FastMCP tools need to be called through their function attribute
            if hasattr(tool, 'function') and callable(tool.function):
                try:
                    # Try calling as async function first
                    if asyncio.iscoroutinefunction(tool.function):
                        result = await tool.function(arguments)
                    else:
                        result = tool.function(arguments)
                except Exception as tool_error:
                    logger.error(f"Tool execution error: {tool_error}")
                    return {
                        "success": False,
                        "error": f"Tool execution failed: {str(tool_error)}",
                        "tool": tool_name,
                    }
            else:
                raise HTTPException(status_code=500, detail=f"Tool '{tool_name}' is not callable")

            return {
                "success": True,
                "result": result,
                "tool": tool_name,
                "arguments": arguments
            }

        except HTTPException:
            raise
        except Exception as e:
            logger.error(f"Error calling tool: {e}")
            return {
                "success": False,
                "error": str(e)
            }


def run_agent_interface():
    """Run the agent interface server in background"""
    logger.info(f"Starting agent interface server on port {config.port + 1}")
    import threading
    from .server.agent_interface import run_agent_server

    # Run agent server in separate thread
    agent_thread = threading.Thread(
        target=run_agent_server,
        args=(config.host, config.port + 1),
        daemon=True
    )
    agent_thread.start()
    return agent_thread


async def run_http():
    """Run MCP server with HTTP transport and agent interface"""
    logger.info(f"Starting Nexus MCP server with HTTP transport on {config.host}:{config.port}")

    # Setup custom routes
    mcp.setup_fastapi(app=None)  # Let FastMCP create its own app
    app = mcp.app  # Get the FastAPI app

    # Add our custom routes
    setup_agent_routes(app)

    # Add CORS middleware if configured
    if config.is_web_enabled():
        from fastapi.middleware.cors import CORSMiddleware
        app.add_middleware(
            CORSMiddleware,
            **config.get_cors_config()
        )

    # Start the agent interface server in background
    agent_thread = run_agent_interface()

    # Start the main MCP server
    await mcp.run_http_async(
        host=config.host,
        port=config.port,
        path="/mcp",
        transport="streamable-http"
    )


def run_stdio():
    """Run MCP server with stdio transport"""
    logger.info("Starting Nexus MCP server with stdio communication")
    mcp.run(transport="stdio")


def main():
    """Main entry point for the Nexus MCP server"""
    parser = argparse.ArgumentParser(description="Nexus Memory System")
    parser.add_argument(
        "--transport",
        choices=["stdio", "http"],
        default="stdio",
        help="Transport protocol to use (default: stdio)"
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Enable verbose logging"
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Enable debug mode"
    )
    parser.add_argument(
        "--config",
        type=str,
        help="Path to configuration file"
    )
    parser.add_argument(
        "--init-db",
        action="store_true",
        help="Initialize database and exit"
    )
    parser.add_argument(
        "--version",
        action="version",
        version="Nexus Memory System 1.0.0"
    )

    args = parser.parse_args()

    # Update config based on args
    if args.verbose:
        config.verbose = True
    if args.debug:
        config.debug = True

    setup_logging()
    logger.info("Starting Nexus Memory System")
    logger.info(f"Transport mode: {args.transport}")

    try:
        # Initialize database
        logger.info("Setting up database...")
        db_success = setup_database()

        if not db_success:
            logger.error("Failed to setup database, exiting")
            sys.exit(1)

        logger.info(f"Database setup complete: {config.database_path}")
        logger.info(f"Supported agents: {list(config.AGENT_NAMESPACES.keys())}")
        logger.info(f"Memory modes: conscious={config.conscious_ingest}, auto={config.auto_ingest}")

        if args.init_db:
            logger.info("Database initialization complete, exiting")
            sys.exit(0)

        # Get database info
        db_info = get_database_info()
        if db_info.get("success"):
            logger.info(f"Database info: {db_info.get('tables', {})}")

        # Start server
        if args.transport == "http":
            logger.info(f"Starting HTTP server on http://{config.host}:{config.port}/mcp")
            if config.is_web_enabled():
                logger.info(f"Web UI available at http://{config.host}:{config.web_port}")
            asyncio.run(run_http())
        else:
            run_stdio()

    except KeyboardInterrupt:
        logger.info("Received interrupt signal, shutting down")
    except Exception as e:
        logger.error(f"Server error: {e}")
        if config.debug:
            import traceback
            traceback.print_exc()
        sys.exit(1)


def run():
    """Run the server"""
    main()


if __name__ == "__main__":
    run()