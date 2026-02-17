"""
WebSocket Connection Manager for Real-time Updates

Provides:
- Connection lifecycle management
- Event broadcasting to all connected clients
- Support for different event types
- Automatic cleanup of disconnected clients
"""

import asyncio
import json
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional, Set
from dataclasses import dataclass, field
from enum import Enum

from fastapi import WebSocket
from loguru import logger


class EventType(str, Enum):
    """Types of events that can be broadcast"""
    # Memory events
    MEMORY_CREATED = "memory_created"
    MEMORY_UPDATED = "memory_updated"
    MEMORY_DELETED = "memory_deleted"

    # Session events
    SESSION_STARTED = "session_started"
    SESSION_ENDED = "session_ended"
    SESSION_IDLE = "session_idle"

    # Hooks events
    EXTRACTION_TRIGGERED = "extraction_triggered"
    EXTRACTION_COMPLETED = "extraction_completed"
    EXTRACTION_FAILED = "extraction_failed"

    # Statistics events
    STATS_UPDATED = "stats_updated"

    # Orchestrator events
    ORCHESTRATOR_STATUS_CHANGED = "orchestrator_status_changed"


@dataclass
class WebSocketClient:
    """Represents a connected WebSocket client"""
    websocket: WebSocket
    client_id: str
    connected_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    subscribed_events: Set[EventType] = field(default_factory=lambda: set(EventType))

    async def send(self, data: Dict[str, Any]) -> bool:
        """Send data to this client"""
        try:
            await self.websocket.send_text(json.dumps(data))
            return True
        except Exception as e:
            logger.warning(f"Failed to send to client {self.client_id}: {e}")
            return False


class WebSocketManager:
    """
    Manages WebSocket connections and broadcasts events

    Usage:
        manager = WebSocketManager()
        await manager.connect(websocket)
        await manager.broadcast({"type": "memory_created", "data": {...}})
        await manager.disconnect(websocket)
    """

    def __init__(self):
        self.active_connections: Dict[str, WebSocketClient] = {}
        self._client_id_counter = 0
        self._lock = asyncio.Lock()

    def _generate_client_id(self) -> str:
        """Generate a unique client ID"""
        self._client_id_counter += 1
        return f"client_{self._client_id_counter}_{datetime.now(UTC).timestamp()}"

    async def connect(self, websocket: WebSocket) -> str:
        """
        Connect a new WebSocket client

        Args:
            websocket: The WebSocket connection

        Returns:
            Client ID for the connected client
        """
        await websocket.accept()

        async with self._lock:
            client_id = self._generate_client_id()
            client = WebSocketClient(websocket=websocket, client_id=client_id)
            self.active_connections[client_id] = client

        logger.info(f"WebSocket client connected: {client_id}")
        logger.info(f"Total active connections: {len(self.active_connections)}")

        # Send welcome message
        await client.send({
            "type": "connected",
            "client_id": client_id,
            "timestamp": datetime.now(UTC).isoformat(),
            "message": "Connected to Nexus Memory System"
        })

        return client_id

    async def disconnect(self, websocket: WebSocket) -> Optional[str]:
        """
        Disconnect a WebSocket client

        Args:
            websocket: The WebSocket connection to disconnect

        Returns:
            The client ID if found, None otherwise
        """
        async with self._lock:
            # Find the client by websocket
            client_id = None
            for cid, client in list(self.active_connections.items()):
                if client.websocket == websocket:
                    client_id = cid
                    break

            if client_id:
                del self.active_connections[client_id]
                logger.info(f"WebSocket client disconnected: {client_id}")
                logger.info(f"Total active connections: {len(self.active_connections)}")
            else:
                logger.warning("Attempted to disconnect unknown WebSocket client")

        return client_id

    async def broadcast(self, data: Dict[str, Any], event_type: Optional[EventType] = None) -> int:
        """
        Broadcast data to all connected clients

        Args:
            data: The data to broadcast
            event_type: Optional event type for filtering

        Returns:
            Number of clients the data was sent to
        """
        if not self.active_connections:
            return 0

        # Add timestamp if not present
        if "timestamp" not in data:
            data["timestamp"] = datetime.now(UTC).isoformat()

        # Add event type if specified
        if event_type and "type" not in data:
            data["type"] = event_type.value

        message = json.dumps(data)
        sent_count = 0
        failed_clients = []

        async with self._lock:
            for client_id, client in list(self.active_connections.items()):
                # Skip if client has subscriptions and this event doesn't match
                if client.subscribed_events and event_type and event_type not in client.subscribed_events:
                    continue

                try:
                    await client.websocket.send_text(message)
                    sent_count += 1
                except Exception as e:
                    logger.warning(f"Failed to broadcast to {client_id}: {e}")
                    failed_clients.append(client_id)

        # Remove failed clients
        for client_id in failed_clients:
            await self._cleanup_client(client_id)

        if sent_count > 0:
            logger.debug(f"Broadcast event to {sent_count} clients: {data.get('type', 'unknown')}")

        return sent_count

    async def send_to_client(self, client_id: str, data: Dict[str, Any]) -> bool:
        """
        Send data to a specific client

        Args:
            client_id: The client ID to send to
            data: The data to send

        Returns:
            True if sent successfully, False otherwise
        """
        async with self._lock:
            client = self.active_connections.get(client_id)
            if not client:
                logger.warning(f"Client not found: {client_id}")
                return False

            if "timestamp" not in data:
                data["timestamp"] = datetime.now(UTC).isoformat()

            return await client.send(data)

    async def _cleanup_client(self, client_id: str):
        """Remove a failed client"""
        async with self._lock:
            if client_id in self.active_connections:
                del self.active_connections[client_id]
                logger.info(f"Cleaned up failed client: {client_id}")

    def get_connection_count(self) -> int:
        """Get the number of active connections"""
        return len(self.active_connections)

    def get_client_ids(self) -> List[str]:
        """Get list of connected client IDs"""
        return list(self.active_connections.keys())


# Global WebSocket manager instance
_websocket_manager: Optional[WebSocketManager] = None


def get_websocket_manager() -> WebSocketManager:
    """Get the global WebSocket manager instance"""
    global _websocket_manager
    if _websocket_manager is None:
        _websocket_manager = WebSocketManager()
    return _websocket_manager


async def broadcast_event(data: Dict[str, Any], event_type: Optional[EventType] = None) -> int:
    """
    Broadcast an event to all connected WebSocket clients

    This is a convenience function that can be called from anywhere
    in the application to broadcast events.

    Args:
        data: The event data to broadcast
        event_type: Optional event type for filtering

    Returns:
        Number of clients the event was sent to

    Example:
        from nexus.web.websocket import broadcast_event, EventType

        await broadcast_event({
            "type": "memory_created",
            "data": {
                "memory_id": 123,
                "content": "User prefers dark mode"
            }
        }, EventType.MEMORY_CREATED)
    """
    manager = get_websocket_manager()
    return await manager.broadcast(data, event_type)


# Convenience functions for common events
async def broadcast_memory_created(memory_id: int, agent_type: str, content: str):
    """Broadcast memory created event"""
    await broadcast_event({
        "type": EventType.MEMORY_CREATED.value,
        "data": {
            "memory_id": memory_id,
            "agent_type": agent_type,
            "content": content[:200] + "..." if len(content) > 200 else content
        }
    }, EventType.MEMORY_CREATED)


async def broadcast_memory_updated(memory_id: int):
    """Broadcast memory updated event"""
    await broadcast_event({
        "type": EventType.MEMORY_UPDATED.value,
        "data": {
            "memory_id": memory_id
        }
    }, EventType.MEMORY_UPDATED)


async def broadcast_memory_deleted(memory_id: int):
    """Broadcast memory deleted event"""
    await broadcast_event({
        "type": EventType.MEMORY_DELETED.value,
        "data": {
            "memory_id": memory_id
        }
    }, EventType.MEMORY_DELETED)


async def broadcast_session_started(session_id: str, agent_type: str):
    """Broadcast session started event"""
    await broadcast_event({
        "type": EventType.SESSION_STARTED.value,
        "data": {
            "session_id": session_id,
            "agent_type": agent_type
        }
    }, EventType.SESSION_STARTED)


async def broadcast_session_ended(session_id: str, agent_type: str):
    """Broadcast session ended event"""
    await broadcast_event({
        "type": EventType.SESSION_ENDED.value,
        "data": {
            "session_id": session_id,
            "agent_type": agent_type
        }
    }, EventType.SESSION_ENDED)


async def broadcast_extraction_completed(agent_type: str, memory_count: int):
    """Broadcast extraction completed event"""
    await broadcast_event({
        "type": EventType.EXTRACTION_COMPLETED.value,
        "data": {
            "agent_type": agent_type,
            "memory_count": memory_count
        }
    }, EventType.EXTRACTION_COMPLETED)


__all__ = [
    "WebSocketManager",
    "WebSocketClient",
    "EventType",
    "get_websocket_manager",
    "broadcast_event",
    "broadcast_memory_created",
    "broadcast_memory_updated",
    "broadcast_memory_deleted",
    "broadcast_session_started",
    "broadcast_session_ended",
    "broadcast_extraction_completed",
]
