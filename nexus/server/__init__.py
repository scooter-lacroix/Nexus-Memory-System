"""
Server module for Nexus Memory System
"""

from .mcp_server import mcp, get_memory_manager, NexusManager
from .nexus_manager import run_web_server

# agent_interface module not implemented yet, commented out
# from .agent_interface import run_agent_server, AgentInterfaceServer

__all__ = [
    "mcp",
    "get_memory_manager",
    "NexusManager",
    "run_web_server",
    # "run_agent_server",
    # "AgentInterfaceServer",
]