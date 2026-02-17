"""
Nexus Memory System

A comprehensive, cross-agent memory management platform that enables
intelligent memory storage, retrieval, and sharing across multiple AI agents.
"""

__version__ = "1.0.0"
__author__ = "scooter-lacroix"
__email__ = "scooter.lacroix@example.com"
__description__ = "A comprehensive, cross-agent memory management platform"

# Core exports
from .config import config, ServerConfig
from .database import setup_database, get_database_info

# Server exports
from .server import mcp, get_memory_manager

# Version information
VERSION_INFO = {
    "major": 1,
    "minor": 0,
    "patch": 0,
    "release": "stable"
}

def get_version():
    """Get the current version string"""
    return __version__

def get_version_info():
    """Get detailed version information"""
    return VERSION_INFO.copy()

# Package metadata
__all__ = [
    "__version__",
    "__author__",
    "__email__",
    "__description__",
    "config",
    "ServerConfig",
    "setup_database",
    "get_database_info",
    "mcp",
    "get_memory_manager",
    "get_version",
    "get_version_info",
    "VERSION_INFO",
]