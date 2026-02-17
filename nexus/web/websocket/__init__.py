"""
WebSocket Manager for Nexus Memory System

This module provides real-time event broadcasting for:
- Memory created/updated/deleted events
- Session lifecycle events
- Statistics updates
- Hooks extraction events
"""

from .manager import WebSocketManager, broadcast_event, get_websocket_manager

__all__ = [
    "WebSocketManager",
    "broadcast_event",
    "get_websocket_manager",
]
