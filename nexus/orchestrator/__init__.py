"""
Orchestrator Package for Nexus Memory System

The Orchestrator is the coordination layer that manages:
- Session lifecycle management across agent types
- Cross-agent synchronization and memory sharing
- Event routing and async event processing
- Memory consistency enforcement
- Workflow coordination between services

Components:
- Orchestrator: Main coordination class
- SessionTracker: Session lifecycle management
- EventBus: Event routing and processing
- CrossAgentSync: Cross-agent memory synchronization
"""

from .orchestrator import Orchestrator, OrchestratorConfig
from .session_tracker import (
    SessionTracker,
    SessionInfo,
    SessionState,
    SessionEventType,
)
from .event_bus import (
    EventBus,
    Event,
    EventHandler,
    EventPriority,
    EventQueue,
)
from .sync import (
    CrossAgentSync,
    SyncPolicy,
    SyncResult,
    MemoryShareRequest,
)

__all__ = [
    # Main orchestrator
    "Orchestrator",
    "OrchestratorConfig",
    # Session tracking
    "SessionTracker",
    "SessionInfo",
    "SessionState",
    "SessionEventType",
    # Event bus
    "EventBus",
    "Event",
    "EventHandler",
    "EventPriority",
    "EventQueue",
    # Cross-agent sync
    "CrossAgentSync",
    "SyncPolicy",
    "SyncResult",
    "MemoryShareRequest",
]
