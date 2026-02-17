"""
Web Dashboard Module for Nexus Memory System

This module provides:
- FastAPI REST API for memory management
- WebSocket support for real-time updates
- Static files serving for admin UI
- Integration with NexusManager, HooksManager, and Orchestrator
"""

from .app import create_app, get_web_app

__all__ = [
    "create_app",
    "get_web_app",
]
