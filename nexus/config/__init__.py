"""
Configuration management for Nexus Memory System
"""

from .settings import config, ServerConfig
from .agent_namespaces import AGENT_NAMESPACES, get_agent_namespace

__all__ = [
    "config",
    "ServerConfig",
    "AGENT_NAMESPACES",
    "get_agent_namespace",
]