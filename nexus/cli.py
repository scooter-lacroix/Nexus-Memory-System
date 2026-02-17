"""
Command Line Interface for Nexus Memory System
"""

import click
import asyncio
import sys
from pathlib import Path
from typing import Optional
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.progress import track
from loguru import logger

from .config import config
from .database import setup_database, get_database_info
from .server import get_memory_manager


console = Console()


@click.group()
@click.option('--verbose', '-v', is_flag=True, help='Enable verbose logging')
@click.option('--config', '-c', type=click.Path(exists=True), help='Configuration file path')
@click.version_option(version="1.0.0", prog_name="Nexus Memory System")
def cli(verbose: bool, config: Optional[str]):
    """Nexus Memory System - Cross-agent memory management platform"""
    if verbose:
        logger.remove()
        logger.add(sys.stderr, level="DEBUG")
    console.print("[bold blue]Nexus Memory System[/bold blue] - Cross-agent memory management")


@cli.command()
@click.option('--transport', type=click.Choice(['stdio', 'http', 'web']), default='stdio', help='Transport protocol')
@click.option('--host', default=None, help='Host address (HTTP/Web mode)')
@click.option('--port', type=int, default=None, help='Port number')
@click.option('--web-port', type=int, default=None, help='Web dashboard port (overrides config)')
@click.option('--debug', is_flag=True, help='Enable debug mode')
def serve(transport: str, host: Optional[str], port: Optional[int], web_port: Optional[int], debug: bool):
    """Start the Nexus Memory server

    \b
    Transport options:
        stdio: MCP stdio transport (default)
        http:  MCP HTTP transport
        web:   Web dashboard (FastAPI)
    """
    try:
        # Update config if provided
        if host:
            config.host = host
        if port:
            config.port = port
        if web_port:
            config.web_port = web_port
        if debug:
            config.debug = True

        if transport == 'web':
            # Start web dashboard
            console.print(f"[green]Starting Nexus Web Dashboard[/green]")
            console.print(f"Host: {host or '0.0.0.0'}, Port: {config.web_port}")
            console.print(f"Dashboard URL: http://localhost:{config.web_port}")
            console.print(f"API Docs: http://localhost:{config.web_port}/api/docs")

            from .server.nexus_manager import run_web_server
            run_web_server(
                host=host or '0.0.0.0',
                port=config.web_port,
                log_level='debug' if debug else 'info'
            )
        else:
            # MCP transport (stdio or http)
            console.print(f"[green]Starting Nexus server with {transport} transport[/green]")
            console.print(f"Host: {config.host}, Port: {config.port}")

            # Import and run main server
            from .main import run
            sys.argv = ['nexus', '--transport', transport]
            if debug:
                sys.argv.append('--debug')
            run()

    except Exception as e:
        console.print(f"[red]Error starting server: {e}[/red]")
        sys.exit(1)


@cli.command()
@click.option('--reset', is_flag=True, help='Reset database before initialization')
def init(reset: bool):
    """Initialize the Nexus database"""
    try:
        console.print("[blue]Initializing Nexus database...[/blue]")

        if reset:
            db_path = Path(config.database_path)
            if db_path.exists():
                console.print(f"[yellow]Removing existing database: {db_path}[/yellow]")
                db_path.unlink()

        success = setup_database()

        if success:
            console.print("[green]✓ Database initialized successfully[/green]")

            # Show database info
            db_info = get_database_info()
            if db_info.get("success"):
                console.print(f"[green]Database location: {db_info['database_path']}[/green]")
                console.print(f"[green]Tables created: {list(db_info['tables'].keys())}[/green]")
        else:
            console.print("[red]✗ Failed to initialize database[/red]")
            sys.exit(1)

    except Exception as e:
        console.print(f"[red]Error initializing database: {e}[/red]")
        sys.exit(1)


@cli.command()
def status():
    """Show system status and statistics"""
    try:
        console.print("[blue]Nexus System Status[/blue]")
        console.print()

        # Database status
        db_info = get_database_info()
        if db_info.get("success"):
            console.print("[green]✓ Database: Connected[/green]")
            console.print(f"  Location: {db_info['database_path']}")

            tables = db_info.get('tables', {})
            console.print("  Tables:")
            for table_name, count in tables.items():
                console.print(f"    {table_name}: {count:,} records")
        else:
            console.print("[red]✗ Database: Connection failed[/red]")

        console.print()

        # Configuration
        console.print("[blue]Configuration:[/blue]")
        console.print(f"  Host: {config.host}")
        console.print(f"  Port: {config.port}")
        console.print(f"  Web Port: {config.web_port}")
        console.print(f"  Conscious Ingest: {config.conscious_ingest}")
        console.print(f"  Auto Ingest: {config.auto_ingest}")
        console.print(f"  Embeddings Enabled: {config.embeddings_enabled}")

        console.print()

        # Supported agents
        from .config.agent_namespaces import list_supported_agents, get_agent_description
        agents = list_supported_agents()
        console.print(f"[blue]Supported Agents ({len(agents)}):[/blue]")

        table = Table()
        table.add_column("Agent Type", style="cyan")
        table.add_column("Description", style="white")

        for agent in sorted(agents):
            table.add_row(agent, get_agent_description(agent))

        console.print(table)

    except Exception as e:
        console.print(f"[red]Error getting status: {e}[/red]")
        sys.exit(1)


@cli.command()
@click.argument('query')
@click.option('--agent', '-a', default='general', help='Agent type to search')
@click.option('--limit', '-l', type=int, default=5, help='Maximum results')
@click.option('--category', help='Filter by category')
def search(query: str, agent: str, limit: int, category: Optional[str]):
    """Search memories"""
    try:
        console.print(f"[blue]Searching memories for agent '{agent}'[/blue]")
        console.print(f"Query: {query}")
        console.print()

        manager = get_memory_manager()
        results = asyncio.run(manager.search_memories_sync(
            query=query,
            agent_type=agent,
            limit=limit,
            category=category
        ))

        if results.get("success"):
            memories = results.get("results", [])
            console.print(f"[green]Found {len(memories)} memories[/green]")
            console.print()

            for i, memory in enumerate(memories, 1):
                console.print(f"[cyan]{i}. Memory ID: {memory['id']}[/cyan]")
                console.print(f"Category: {memory['category']}")
                console.print(f"Created: {memory['created_at']}")
                console.print(f"Access Count: {memory['access_count']}")

                if memory.get('labels'):
                    console.print(f"Labels: {', '.join(memory['labels'])}")

                console.print(f"Content: {memory['content'][:200]}{'...' if len(memory['content']) > 200 else ''}")
                console.print("-" * 50)
        else:
            console.print(f"[red]Search failed: {results.get('error')}[/red]")

    except Exception as e:
        console.print(f"[red]Error searching memories: {e}[/red]")


@cli.command()
@click.argument('content')
@click.option('--agent', '-a', default='general', help='Agent type')
@click.option('--category', default='general', help='Memory category')
@click.option('--labels', help='Comma-separated labels')
def store(content: str, agent: str, category: str, labels: Optional[str]):
    """Store a memory"""
    try:
        console.print("[blue]Storing memory...[/blue]")

        label_list = []
        if labels:
            label_list = [label.strip() for label in labels.split(',') if label.strip()]

        manager = get_memory_manager()
        result = asyncio.run(manager.store_memory_sync(
            content=content,
            agent_type=agent,
            category=category,
            labels=label_list
        ))

        if result.get("success"):
            console.print(f"[green]✓ Memory stored successfully[/green]")
            console.print(f"Memory ID: {result.get('memory_id')}")
            console.print(f"Agent: {agent}")
            console.print(f"Category: {category}")
        else:
            console.print(f"[red]✗ Failed to store memory: {result.get('error')}[/red]")

    except Exception as e:
        console.print(f"[red]Error storing memory: {e}[/red]")


@cli.command()
@click.option('--agent', '-a', help='Agent type (default: all agents)')
def stats(agent: Optional[str]):
    """Show memory statistics"""
    try:
        console.print("[blue]Memory Statistics[/blue]")
        console.print()

        manager = get_memory_manager()
        result = asyncio.run(manager.get_memory_stats_sync(agent))

        if result.get("success"):
            total_memories = result.get("total_memories", 0)
            console.print(f"[green]Total Memories: {total_memories:,}[/green]")
            console.print()

            categories = result.get("categories", {})
            if categories:
                console.print("[blue]Memories by Category:[/blue]")
                table = Table()
                table.add_column("Category", style="cyan")
                table.add_column("Count", style="white", justify="right")

                for category, count in sorted(categories.items()):
                    table.add_row(category, f"{count:,}")

                console.print(table)

            if agent:
                console.print(f"[blue]Agent: {agent}[/blue]")
            else:
                console.print("[blue]All Agents[/blue]")

        else:
            console.print(f"[red]Failed to get statistics: {result.get('error')}[/red]")

    except Exception as e:
        console.print(f"[red]Error getting statistics: {e}[/red]")


@cli.group()
def config():
    """Configuration management commands"""
    pass


@config.command('show')
def config_show():
    """Show current configuration"""
    try:
        console.print("[blue]Current Configuration:[/blue]")
        console.print()

        config_dict = config.to_dict()
        for key, value in config_dict.items():
            if 'key' in key.lower() and 'api' in key.lower():
                # Hide sensitive values
                console.print(f"{key}: {'*' * 8 if value else 'None'}")
            else:
                console.print(f"{key}: {value}")

    except Exception as e:
        console.print(f"[red]Error showing configuration: {e}[/red]")


@config.command('set')
@click.argument('key')
@click.argument('value')
def config_set(key: str, value: str):
    """Set configuration value"""
    try:
        if hasattr(config, key):
            # Convert string value to appropriate type
            current_value = getattr(config, key)
            if isinstance(current_value, bool):
                value = value.lower() in ('true', '1', 'yes', 'on')
            elif isinstance(current_value, int):
                value = int(value)
            elif isinstance(current_value, float):
                value = float(value)

            setattr(config, key, value)
            console.print(f"[green]✓ Set {key} = {value}[/green]")
        else:
            console.print(f"[red]Unknown configuration key: {key}[/red]")

    except Exception as e:
        console.print(f"[red]Error setting configuration: {e}[/red]")


@cli.group()
def hooks():
    """Agent hooks management commands"""
    pass


@hooks.command('install')
@click.argument('agent', required=False)
@click.option('--all', 'install_all', is_flag=True, help='Install hooks for all supported agents')
@click.option('--no-monitor', is_flag=True, help='Install without starting monitoring')
def hooks_install(agent: Optional[str], install_all: bool, no_monitor: bool):
    """Install agent hooks for automated memory extraction

    \b
    Examples:
        nexus hooks install --all           # Install for all agents
        nexus hooks install claude-code     # Install for specific agent
        nexus hooks install --no-monitor    # Install without monitoring
    """
    try:
        from .server import get_memory_manager

        manager = get_memory_manager()

        async def do_install():
            await manager.ensure_initialized()
            enable_monitoring = not no_monitor

            if install_all:
                console.print("[blue]Installing hooks for all supported agents...[/blue]")
                results = await manager.hooks_manager.install_all_hooks(
                    enable_monitoring=enable_monitoring
                )

                # Display results
                for agent_type, result in results.items():
                    status_color = {
                        "success": "green",
                        "failed": "red",
                        "already_installed": "yellow",
                        "disabled": "yellow",
                        "not_supported": "yellow"
                    }.get(result.status.value, "white")

                    console.print(
                        f"[{status_color}]  {agent_type}: {result.status.value}[/{status_color}]"
                    )
                    if result.message:
                        console.print(f"    {result.message}")
                    if result.error:
                        console.print(f"    Error: {result.error}", style="red")

            elif agent:
                console.print(f"[blue]Installing hooks for {agent}...[/blue]")
                result = await manager.hooks_manager.install_hooks(
                    agent,
                    enable_monitoring=enable_monitoring
                )

                status_color = {
                    "success": "green",
                    "failed": "red",
                    "already_installed": "yellow",
                    "disabled": "yellow",
                    "not_supported": "yellow"
                }.get(result.status.value, "white")

                console.print(f"Status: {result.status.value}", style=status_color if status_color != "white" else None)
                if result.message:
                    console.print(result.message)
                if result.error:
                    console.print(f"Error: {result.error}", style="red")

            else:
                # Show available agents
                from .hooks.factory import list_supported_agents
                console.print("[yellow]Please specify an agent type or use --all[/yellow]")
                console.print("\n[cyan]Supported agents:[/cyan]")
                for agent_type in sorted(list_supported_agents()):
                    console.print(f"  - {agent_type}")
                console.print("\nExample: nexus hooks install claude-code")
                console.print("Example: nexus hooks install --all")
                return

            # Start monitoring summary
            if enable_monitoring:
                monitoring_status = manager.hooks_manager.is_monitoring()
                if monitoring_status:
                    console.print("\n[green]✓ Monitoring is active[/green]")
                else:
                    console.print("\n[yellow]⚠ Monitoring not started (no hooks installed)[/yellow]")

        asyncio.run(do_install())

    except Exception as e:
        console.print(f"[red]Error installing hooks: {e}[/red]")
        import traceback
        console.print(traceback.format_exc())


@hooks.command('uninstall')
@click.argument('agent')
def hooks_uninstall(agent: str):
    """Uninstall hooks for an agent"""
    try:
        from .server import get_memory_manager

        manager = get_memory_manager()

        async def do_uninstall():
            await manager.ensure_initialized()
            success = await manager.hooks_manager.uninstall_hooks(agent)

            if success:
                console.print(f"[green]✓ Uninstalled hooks for {agent}[/green]")
            else:
                console.print(f"[red]✗ Failed to uninstall hooks for {agent}[/red]")

        asyncio.run(do_uninstall())

    except Exception as e:
        console.print(f"[red]Error uninstalling hooks: {e}[/red]")


@hooks.command('status')
@click.option('--verbose', '-v', is_flag=True, help='Show detailed status including statistics')
def hooks_status(verbose: bool):
    """Show hooks installation and monitoring status"""
    try:
        from .server import get_memory_manager

        manager = get_memory_manager()

        async def do_status():
            await manager.ensure_initialized()

            console.print("[blue]Agent Hooks Status[/blue]\n")

            # Check if hooks manager is initialized
            if not manager.hooks_manager:
                console.print("[yellow]Hooks manager not initialized[/yellow]")
                return

            # Get installed agents
            installed = manager.hooks_manager.get_installed_agents()

            if not installed:
                console.print("[yellow]No hooks installed[/yellow]")
                console.print("\n[cyan]To install hooks:[/cyan]")
                console.print("  nexus hooks install --all")
                console.print("  nexus hooks install <agent>")
                return

            # Monitoring status
            is_monitoring = manager.hooks_manager.is_monitoring()
            monitoring_color = "green" if is_monitoring else "yellow"
            monitoring_text = "Active" if is_monitoring else "Inactive"
            console.print(f"[{monitoring_color}]Monitoring: {monitoring_text}[/{monitoring_color}]")

            # Auto extraction status
            auto_enabled = manager.hooks_manager.is_auto_extraction_enabled()
            auto_color = "green" if auto_enabled else "yellow"
            auto_text = "Enabled" if auto_enabled else "Disabled"
            console.print(f"[{auto_color}]Auto Extraction: {auto_text}[/{auto_color}]")

            console.print()

            # Get hook status
            status_dict = await manager.hooks_manager.get_hooks_status()

            # Create status table
            table = Table()
            table.add_column("Agent", style="cyan")
            table.add_column("Status", style="white")
            table.add_column("Hook Type", style="white")
            table.add_column("Extractions", style="white", justify="right")
            table.add_column("Last Extraction", style="white")

            for agent_type in sorted(installed):
                status = status_dict.get(agent_type, {})
                hook_status = "Installed" if status.get("installed") else "Not Installed"
                hook_type = status.get("hook_type", "unknown")
                extraction_count = status.get("extraction_count", 0)
                last_extraction = status.get("last_extraction") or "Never"

                # Format last extraction
                if last_extraction != "Never":
                    try:
                        from datetime import datetime
                        dt = datetime.fromisoformat(last_extraction.replace('Z', '+00:00'))
                        last_extraction = dt.strftime("%Y-%m-%d %H:%M")
                    except:
                        pass

                table.add_row(
                    agent_type,
                    hook_status,
                    hook_type,
                    str(extraction_count),
                    last_extraction
                )

            console.print(table)

            # Show detailed statistics if verbose
            if verbose:
                console.print("\n[blue]Detailed Statistics:[/blue]\n")

                for agent_type in sorted(installed):
                    stats = await manager.hooks_manager.get_extraction_stats(agent_type)

                    if stats:
                        console.print(f"[cyan]{agent_type}:[/cyan]")
                        console.print(f"  Total Extractions: {stats.get('total_extractions', 0)}")
                        console.print(f"  Successful: {stats.get('successful_extractions', 0)}")
                        console.print(f"  Failed: {stats.get('failed_extractions', 0)}")

                        sources = stats.get('sources', {})
                        if sources:
                            console.print(f"  Sources:")
                            for source, count in sources.items():
                                console.print(f"    {source}: {count}")

                        console.print()

        asyncio.run(do_status())

    except Exception as e:
        console.print(f"[red]Error getting hooks status: {e}[/red]")
        import traceback
        console.print(traceback.format_exc())


@hooks.command('start')
def hooks_start():
    """Start hooks monitoring (if installed but not monitoring)"""
    try:
        from .server import get_memory_manager

        manager = get_memory_manager()

        async def do_start():
            await manager.ensure_initialized()

            if manager.hooks_manager.is_monitoring():
                console.print("[yellow]Monitoring is already active[/yellow]")
                return

            await manager.hooks_manager.start_monitoring()
            console.print("[green]✓ Started hooks monitoring[/green]")

        asyncio.run(do_start())

    except Exception as e:
        console.print(f"[red]Error starting monitoring: {e}[/red]")


@hooks.command('stop')
def hooks_stop():
    """Stop hooks monitoring"""
    try:
        from .server import get_memory_manager

        manager = get_memory_manager()

        async def do_stop():
            await manager.ensure_initialized()

            if not manager.hooks_manager.is_monitoring():
                console.print("[yellow]Monitoring is not active[/yellow]")
                return

            await manager.hooks_manager.stop_monitoring()
            console.print("[green]✓ Stopped hooks monitoring[/green]")

        asyncio.run(do_stop())

    except Exception as e:
        console.print(f"[red]Error stopping monitoring: {e}[/red]")


@hooks.command('extract')
@click.argument('agent', required=False)
@click.option('--all', 'extract_all', is_flag=True, help='Extract from all active agents')
def hooks_extract(agent: Optional[str], extract_all: bool):
    """Manually trigger memory extraction

    \b
    Examples:
        nexus hooks extract claude-code     # Extract from specific agent
        nexus hooks extract --all           # Extract from all active agents
    """
    try:
        from .server import get_memory_manager

        manager = get_memory_manager()

        async def do_extract():
            await manager.ensure_initialized()

            if extract_all:
                console.print("[blue]Extracting from all active sessions...[/blue]")
                results = await manager.hooks_manager.extract_all_active_sessions()

                for agent_type, result in results.items():
                    if result.success:
                        console.print(f"[green]✓ {agent_type}: Extracted successfully[/green]")
                    else:
                        console.print(f"[red]✗ {agent_type}: {result.error}[/red]")

            elif agent:
                console.print(f"[blue]Extracting from {agent}...[/blue]")
                result = await manager.hooks_manager.trigger_extraction(agent)

                if result.success:
                    console.print("[green]✓ Extraction successful[/green]")
                else:
                    console.print(f"[red]✗ Extraction failed: {result.error}[/red]")

            else:
                console.print("[yellow]Please specify an agent or use --all[/yellow]")
                console.print("Example: nexus hooks extract claude-code")
                console.print("Example: nexus hooks extract --all")

        asyncio.run(do_extract())

    except Exception as e:
        console.print(f"[red]Error triggering extraction: {e}[/red]")


def main():
    """Main CLI entry point"""
    try:
        cli()
    except KeyboardInterrupt:
        console.print("\n[yellow]Operation cancelled by user[/yellow]")
        sys.exit(0)
    except Exception as e:
        console.print(f"\n[red]Unexpected error: {e}[/red]")
        sys.exit(1)


if __name__ == '__main__':
    main()