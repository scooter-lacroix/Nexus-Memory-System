"""
Orchestrator - Main coordination layer for Nexus Memory System

This module provides the main Orchestrator class that coordinates:
- Session lifecycle management
- Cross-agent synchronization
- Event routing and processing
- Memory consistency enforcement
- Workflow coordination between services

Architecture:
    ┌─────────────────────────────────────────────┐
    │              ORCHESTRATOR                   │
    ├─────────────────────────────────────────────┤
    │                                              │
    │  ┌──────────────┐  ┌──────────────┐        │
    │  │   Session    │  │    Event     │        │
    │  │   Tracker    │◄─┤     Bus      │        │
    │  └──────────────┘  └──────────────┘        │
    │          │                  │               │
    │          ▼                  ▼               │
    │  ┌──────────────┐  ┌──────────────┐        │
    │  │   Cross     │  │   Memory     │        │
    │  │  Agent Sync │  │  Consistency │        │
    │  └──────────────┘  └──────────────┘        │
    │          │                  │               │
    │          └────────┬─────────┘               │
    │                   ▼                         │
    │  ┌─────────────────────────────┐           │
    │  │   Workflow Coordination      │           │
    │  │  (HooksManager, Storage,     │           │
    │   Processing)                   │           │
    │  └─────────────────────────────┘           │
    └─────────────────────────────────────────────┘
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Optional, Any, Callable, Awaitable
from dataclasses import dataclass, field
from pathlib import Path
from loguru import logger

from .session_tracker import (
    SessionTracker,
    SessionInfo,
    SessionEvent,
    SessionEventType,
    SessionState,
)
from .event_bus import (
    EventBus,
    Event,
    EventPriority,
    EventType,
)
from .sync import (
    CrossAgentSync,
    SyncPolicy,
    SyncResult,
    MemoryShareRequest,
)


@dataclass
class OrchestratorConfig:
    """
    Configuration for the Orchestrator

    Attributes:
        # Session tracking
        session_idle_threshold_seconds: Idle threshold before marking session idle
        session_timeout_seconds: Timeout before auto-closing session
        session_persistence_enabled: Enable session state persistence
        session_persistence_dir: Directory for session persistence

        # Event bus
        event_queue_max_size: Maximum event queue size
        event_max_workers: Maximum concurrent event handlers
        event_persistence_enabled: Enable event persistence
        event_persistence_dir: Directory for event persistence

        # Cross-agent sync
        sync_policy: Default sync policy for cross-agent sharing
        auto_share_labels: Labels that trigger auto-sharing
    """
    # Session tracking
    session_idle_threshold_seconds: int = 300
    session_timeout_seconds: int = 3600
    session_persistence_enabled: bool = False
    session_persistence_dir: Optional[Path] = None

    # Event bus
    event_queue_max_size: int = 10000
    event_max_workers: int = 4
    event_persistence_enabled: bool = False
    event_persistence_dir: Optional[Path] = None

    # Cross-agent sync
    sync_policy: SyncPolicy = SyncPolicy.MANUAL
    auto_share_labels: List[str] = field(default_factory=lambda: ["cross-agent", "shared"])

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary"""
        return {
            "session_idle_threshold_seconds": self.session_idle_threshold_seconds,
            "session_timeout_seconds": self.session_timeout_seconds,
            "session_persistence_enabled": self.session_persistence_enabled,
            "session_persistence_dir": str(self.session_persistence_dir) if self.session_persistence_dir else None,
            "event_queue_max_size": self.event_queue_max_size,
            "event_max_workers": self.event_max_workers,
            "event_persistence_enabled": self.event_persistence_enabled,
            "event_persistence_dir": str(self.event_persistence_dir) if self.event_persistence_dir else None,
            "sync_policy": self.sync_policy.name,
            "auto_share_labels": self.auto_share_labels,
        }


class Orchestrator:
    """
    Main coordination layer for Nexus Memory System

    Orchestrates between session tracking, event processing,
    cross-agent sync, and workflow coordination.

    Usage:
        orchestrator = Orchestrator(
            memory_manager=memory_manager,
            db_manager=db_manager,
            hooks_manager=hooks_manager,
            config=config
        )
        await orchestrator.initialize()

        # Start session
        session = await orchestrator.start_session("claude-code")

        # Publish events
        await orchestrator.publish_event(
            EventType.MEMORY_STORED,
            {"memory_id": 123}
        )

        # Share memory
        result = await orchestrator.share_memory(
            source_memory_id=123,
            source_agent_type="claude-code",
            target_agent_types=["gemini"]
        )
    """

    def __init__(
        self,
        memory_manager,
        db_manager,
        hooks_manager=None,
        config: Optional[OrchestratorConfig] = None
    ):
        """
        Initialize Orchestrator

        Args:
            memory_manager: MemoryManager instance
            db_manager: DatabaseManager instance
            hooks_manager: Optional HooksManager instance
            config: Orchestrator configuration
        """
        self.memory_manager = memory_manager
        self.db_manager = db_manager
        self.hooks_manager = hooks_manager
        self._config = config or OrchestratorConfig()

        # Components
        self._session_tracker: Optional[SessionTracker] = None
        self._event_bus: Optional[EventBus] = None
        self._cross_agent_sync: Optional[CrossAgentSync] = None

        # State
        self._initialized = False
        self._closed = False
        self._component_locks: Dict[str, asyncio.Lock] = {}

        # Background tasks
        self._consistency_task: Optional[asyncio.Task] = None
        self._consistency_running = False

    async def initialize(self) -> None:
        """Initialize all orchestrator components"""
        if self._initialized:
            return

        logger.info("Initializing Orchestrator...")

        try:
            # Initialize session tracker
            self._session_tracker = SessionTracker(
                idle_threshold_seconds=self._config.session_idle_threshold_seconds,
                session_timeout_seconds=self._config.session_timeout_seconds,
                persistence_enabled=self._config.session_persistence_enabled,
                persistence_dir=self._config.session_persistence_dir
            )
            await self._session_tracker.initialize()

            # Register session event listener
            self._session_tracker.register_listener(self._on_session_event)

            # Initialize event bus
            self._event_bus = EventBus(
                max_queue_size=self._config.event_queue_max_size,
                max_workers=self._config.event_max_workers,
                persistence_enabled=self._config.event_persistence_enabled,
                persistence_dir=self._config.event_persistence_dir
            )
            await self._event_bus.initialize()

            # Subscribe to events
            self._setup_event_subscriptions()

            # Initialize cross-agent sync
            self._cross_agent_sync = CrossAgentSync(
                memory_manager=self.memory_manager,
                db_manager=self.db_manager,
                sync_policy=self._config.sync_policy,
                auto_share_labels=self._config.auto_share_labels
            )
            await self._cross_agent_sync.initialize()

            # Start event processing
            await self._event_bus.start_processing()

            # Start session monitoring
            await self._session_tracker.start_monitoring()

            # Start consistency checker
            await self._start_consistency_checker()

            self._initialized = True
            logger.info("Orchestrator initialized successfully")

        except Exception as e:
            logger.error(f"Failed to initialize Orchestrator: {e}")
            raise

    async def close(self) -> None:
        """Close orchestrator and cleanup resources"""
        if self._closed:
            return

        logger.info("Closing Orchestrator...")
        self._closed = True

        # Stop consistency checker
        await self._stop_consistency_checker()

        # Stop session monitoring
        if self._session_tracker:
            await self._session_tracker.stop_monitoring()

        # Stop event processing
        if self._event_bus:
            await self._event_bus.stop_processing()

        # Close components
        if self._cross_agent_sync:
            await self._cross_agent_sync.close()

        if self._event_bus:
            await self._event_bus.close()

        if self._session_tracker:
            await self._session_tracker.close()

        self._initialized = False
        logger.info("Orchestrator closed")

    # Session Management

    async def start_session(
        self,
        agent_type: str,
        metadata: Optional[Dict[str, Any]] = None
    ) -> SessionInfo:
        """
        Start a new session for an agent type

        Args:
            agent_type: Agent type identifier
            metadata: Optional session metadata

        Returns:
            SessionInfo for the new session
        """
        await self.ensure_initialized()
        return await self._session_tracker.start_session(agent_type, metadata)

    async def end_session(
        self,
        session_id: str,
        reason: str = "manual",
        trigger_extraction: bool = True
    ) -> Optional[SessionInfo]:
        """
        End an active session

        Args:
            session_id: Session identifier
            reason: Reason for ending
            trigger_extraction: Whether to trigger extraction

        Returns:
            Updated SessionInfo or None
        """
        await self.ensure_initialized()
        return await self._session_tracker.end_session(
            session_id, reason, trigger_extraction
        )

    async def record_activity(
        self,
        session_id: str,
        activity_type: str = "unknown",
        metadata: Optional[Dict[str, Any]] = None
    ) -> Optional[SessionInfo]:
        """Record activity for a session"""
        await self.ensure_initialized()
        return await self._session_tracker.record_activity(
            session_id, activity_type, metadata
        )

    async def get_active_session(self, agent_type: str) -> Optional[SessionInfo]:
        """Get the active session for an agent type"""
        await self.ensure_initialized()
        return await self._session_tracker.get_active_session(agent_type)

    async def get_all_active_sessions(self) -> List[SessionInfo]:
        """Get all currently active sessions"""
        await self.ensure_initialized()
        return await self._session_tracker.get_all_active_sessions()

    # Event Publishing

    async def publish_event(
        self,
        event_type: str,
        data: Dict[str, Any],
        source: str = "orchestrator",
        priority: EventPriority = EventPriority.NORMAL
    ) -> bool:
        """
        Publish an event to the event bus

        Args:
            event_type: Event type string
            data: Event payload data
            source: Event source
            priority: Event priority

        Returns:
            True if event was queued
        """
        await self.ensure_initialized()
        return await self._event_bus.publish(
            event_type=event_type,
            data=data,
            source=source,
            priority=priority
        )

    # Cross-Agent Sync

    async def share_memory(
        self,
        source_memory_id: int,
        source_agent_type: str,
        target_agent_types: Optional[List[str]] = None,
        policy: Optional[SyncPolicy] = None,
        reason: str = "manual"
    ) -> SyncResult:
        """
        Share a memory across agent namespaces

        Args:
            source_memory_id: Source memory ID
            source_agent_type: Source agent type
            target_agent_types: Target agent types
            policy: Sync policy
            reason: Reason for sharing

        Returns:
            SyncResult with sync outcome
        """
        await self.ensure_initialized()

        result = await self._cross_agent_sync.share_memory(
            source_memory_id=source_memory_id,
            source_agent_type=source_agent_type,
            target_agent_types=target_agent_types,
            policy=policy,
            reason=reason
        )

        # Publish sync event
        await self.publish_event(
            EventType.MEMORY_SHARED,
            {
                "source_memory_id": source_memory_id,
                "source_agent_type": source_agent_type,
                "shared_memory_ids": result.shared_memory_ids,
                "status": result.status.value,
            },
            source="orchestrator.sync"
        )

        return result

    async def share_to_all(
        self,
        source_memory_id: int,
        source_agent_type: str,
        exclude_agent_types: Optional[List[str]] = None
    ) -> SyncResult:
        """Share memory to all agent namespaces"""
        await self.ensure_initialized()
        return await self._cross_agent_sync.share_to_all(
            source_memory_id,
            source_agent_type,
            exclude_agent_types
        )

    # Memory Consistency

    async def validate_memory_consistency(
        self,
        memory_id: int,
        agent_type: str
    ) -> Dict[str, Any]:
        """
        Validate memory consistency (embeddings, relationships, etc.)

        Args:
            memory_id: Memory ID to validate
            agent_type: Agent type

        Returns:
            Validation result dictionary
        """
        await self.ensure_initialized()

        result = {
            "memory_id": memory_id,
            "agent_type": agent_type,
            "valid": True,
            "issues": [],
            "warnings": [],
        }

        try:
            # Get memory
            memory = await self._cross_agent_sync._get_memory(memory_id, agent_type)
            if not memory:
                result["valid"] = False
                result["issues"].append("Memory not found")
                return result

            # Check embedding consistency
            if memory.get("content_embedding"):
                # Re-generate embedding and compare
                # This is a simplified check - in production would do actual comparison
                result["warnings"].append("Embedding consistency check not fully implemented")
            else:
                if self.memory_manager.embedding_service:
                    result["warnings"].append("Memory has no embedding")

            # Check relationships
            # Would validate relationship integrity here

        except Exception as e:
            result["valid"] = False
            result["issues"].append(f"Validation error: {e}")

        return result

    # Status and Stats

    async def get_status(self) -> Dict[str, Any]:
        """Get comprehensive orchestrator status"""
        await self.ensure_initialized()

        session_status = await self._session_tracker.get_status()
        event_stats = await self._event_bus.get_stats()
        sync_stats = await self._cross_agent_sync.get_sync_stats()

        return {
            "initialized": self._initialized,
            "config": self._config.to_dict(),
            "sessions": session_status,
            "events": event_stats,
            "sync": sync_stats,
            "consistency_check_running": self._consistency_running,
        }

    async def get_health(self) -> Dict[str, Any]:
        """Get orchestrator health status"""
        await self.ensure_initialized()

        return {
            "healthy": self._initialized and not self._closed,
            "components": {
                "session_tracker": self._session_tracker is not None,
                "event_bus": self._event_bus is not None,
                "cross_agent_sync": self._cross_agent_sync is not None,
            },
            "active_sessions": len(await self.get_all_active_sessions()),
            "pending_events": await self._event_bus.get_queue_size(),
            "pending_syncs": sync_stats.get("pending_syncs", 0)
            if (sync_stats := await self._cross_agent_sync.get_sync_stats()) else 0,
        }

    # Event Handlers

    def _setup_event_subscriptions(self) -> None:
        """Setup event bus subscriptions"""
        # Memory events
        self._event_bus.subscribe(
            EventType.MEMORY_STORED,
            self._on_memory_stored
        )

        # Session events
        self._event_bus.subscribe(
            EventType.SESSION_END,
            self._on_session_end_event
        )

        # Sync events
        self._event_bus.subscribe(
            EventType.MEMORY_SHARED,
            self._on_memory_shared
        )

    async def _on_session_event(self, event: SessionEvent) -> None:
        """Handle session tracker events"""
        # Publish to event bus
        event_type_map = {
            SessionEventType.SESSION_START: EventType.SESSION_START,
            SessionEventType.SESSION_END: EventType.SESSION_END,
            SessionEventType.SESSION_IDLE: EventType.SESSION_IDLE,
            SessionEventType.SESSION_ACTIVE: EventType.SESSION_ACTIVE,
        }

        bus_event_type = event_type_map.get(event.event_type)
        if bus_event_type:
            await self.publish_event(
                bus_event_type,
                {
                    "session_id": event.session_info.session_id,
                    "agent_type": event.session_info.agent_type,
                    "state": event.session_info.state.value,
                },
                source="session_tracker"
            )

    async def _on_memory_stored(self, event: Event) -> None:
        """Handle memory stored event"""
        memory_id = event.data.get("memory_id")
        agent_type = event.data.get("agent_type")

        if memory_id and agent_type:
            # Check for auto-share eligibility
            result = await self._cross_agent_sync.auto_share_if_eligible(
                memory_id, agent_type, event.data
            )

            if result:
                logger.info(f"Auto-shared memory {memory_id} from {agent_type}")

    async def _on_session_end_event(self, event: Event) -> None:
        """Handle session end event from event bus"""
        session_id = event.data.get("session_id")
        reason = event.data.get("reason", "event")

        if session_id:
            session = await self._session_tracker.get_session(session_id)
            if session and session.state != SessionState.CLOSED:
                await self._session_tracker.end_session(
                    session_id,
                    reason=f"event_{reason}",
                    trigger_extraction=True
                )

    async def _on_memory_shared(self, event: Event) -> None:
        """Handle memory shared event"""
        logger.info(
            f"Memory {event.data.get('source_memory_id')} shared to "
            f"{len(event.data.get('shared_memory_ids', {}))} agents"
        )

    # Consistency Checker

    async def _start_consistency_checker(self, interval_seconds: int = 300) -> None:
        """Start background consistency checker"""
        if self._consistency_running:
            return

        self._consistency_running = True
        self._consistency_task = asyncio.create_task(
            self._consistency_check_loop(interval_seconds)
        )
        logger.info("Started consistency checker")

    async def _stop_consistency_checker(self) -> None:
        """Stop background consistency checker"""
        if not self._consistency_running:
            return

        self._consistency_running = False
        if self._consistency_task:
            self._consistency_task.cancel()
            try:
                await self._consistency_task
            except asyncio.CancelledError:
                pass
            self._consistency_task = None

        logger.info("Stopped consistency checker")

    async def _consistency_check_loop(self, interval_seconds: int) -> None:
        """Main consistency check loop"""
        try:
            while self._consistency_running and not self._closed:
                try:
                    # Run consistency checks
                    await self._run_consistency_checks()
                except Exception as e:
                    logger.error(f"Error in consistency check: {e}")

                await asyncio.sleep(interval_seconds)

        except asyncio.CancelledError:
            logger.debug("Consistency checker loop cancelled")

    async def _run_consistency_checks(self) -> None:
        """Run consistency checks on memories"""
        # This would implement actual consistency checks
        # For now, just a placeholder
        pass

    async def ensure_initialized(self) -> None:
        """Ensure orchestrator is initialized"""
        if not self._initialized:
            await self.initialize()
