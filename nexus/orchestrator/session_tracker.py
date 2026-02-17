"""
Session Tracker - Session lifecycle management for Nexus Orchestrator

This module provides session tracking capabilities:
- Track active sessions per agent type
- Detect session start/end events
- Maintain session state and metadata
- Coordinate with HooksManager for extraction triggers
- Support session recovery and persistence

Architecture:
    Session Event (start/end)
        |
    SessionTracker._handle_event()
        |
    Update state + Notify listeners
        |
    Trigger extraction on end
"""

import asyncio
from datetime import datetime, UTC, timedelta
from typing import Dict, List, Optional, Any, Callable, Awaitable, Set
from enum import Enum
from dataclasses import dataclass, field
from collections import defaultdict
import uuid
from loguru import logger
from pathlib import Path
import json


class SessionState(Enum):
    """Session states for lifecycle tracking"""
    INITIALIZING = "initializing"
    ACTIVE = "active"
    IDLE = "idle"
    CLOSING = "closing"
    CLOSED = "closed"
    ERROR = "error"


class SessionEventType(Enum):
    """Types of session events"""
    SESSION_START = "session_start"
    SESSION_END = "session_end"
    SESSION_ACTIVE = "session_active"
    SESSION_IDLE = "session_idle"
    SESSION_ERROR = "session_error"
    EXTRACTION_TRIGGERED = "extraction_triggered"
    EXTRACTION_COMPLETE = "extraction_complete"
    EXTRACTION_FAILED = "extraction_failed"


@dataclass
class SessionInfo:
    """
    Container for session information and state

    Attributes:
        session_id: Unique session identifier
        agent_type: Agent type (claude-code, gemini, etc.)
        state: Current session state
        started_at: Session start timestamp
        last_activity: Last activity timestamp
        activity_count: Number of activities detected
        metadata: Additional session metadata
        error_count: Number of errors encountered
        last_error: Last error message
        extraction_pending: Whether extraction is pending
    """
    session_id: str
    agent_type: str
    state: SessionState = SessionState.INITIALIZING
    started_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    last_activity: datetime = field(default_factory=lambda: datetime.now(UTC))
    activity_count: int = 0
    metadata: Dict[str, Any] = field(default_factory=dict)
    error_count: int = 0
    last_error: Optional[str] = None
    extraction_pending: bool = False

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary representation"""
        return {
            "session_id": self.session_id,
            "agent_type": self.agent_type,
            "state": self.state.value,
            "started_at": self.started_at.isoformat(),
            "last_activity": self.last_activity.isoformat(),
            "activity_count": self.activity_count,
            "duration_seconds": (datetime.now(UTC) - self.started_at).total_seconds(),
            "idle_seconds": (datetime.now(UTC) - self.last_activity).total_seconds(),
            "metadata": self.metadata,
            "error_count": self.error_count,
            "last_error": self.last_error,
            "extraction_pending": self.extraction_pending,
        }

    def update_activity(self) -> None:
        """Update activity timestamp and count"""
        self.last_activity = datetime.now(UTC)
        self.activity_count += 1

    def is_idle(self, threshold_seconds: int = 300) -> bool:
        """Check if session is idle beyond threshold"""
        idle_time = (datetime.now(UTC) - self.last_activity).total_seconds()
        return idle_time > threshold_seconds

    def mark_error(self, error: str) -> None:
        """Mark error on session"""
        self.error_count += 1
        self.last_error = error


@dataclass
class SessionEvent:
    """Event data for session lifecycle changes"""
    event_type: SessionEventType
    session_info: SessionInfo
    timestamp: datetime = field(default_factory=lambda: datetime.now(UTC))
    source: str = "session_tracker"
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary"""
        return {
            "event_type": self.event_type.value,
            "session_id": self.session_info.session_id,
            "agent_type": self.session_info.agent_type,
            "state": self.session_info.state.value,
            "timestamp": self.timestamp.isoformat(),
            "source": self.source,
            "metadata": self.metadata,
        }


SessionEventHandler = Callable[[SessionEvent], Awaitable[None]]


class SessionTracker:
    """
    Tracks session lifecycle for all agent types

    Manages active sessions, detects state changes, and coordinates
    with HooksManager for extraction triggers on session end.

    Usage:
        tracker = SessionTracker()
        await tracker.initialize()

        # Register event listener
        tracker.register_listener(my_handler)

        # Track session
        session = await tracker.start_session("claude-code")
        await tracker.record_activity(session.session_id)
        await tracker.end_session(session.session_id)

        # Get status
        status = await tracker.get_status()
    """

    # Default configuration
    DEFAULT_IDLE_THRESHOLD_SECONDS = 300  # 5 minutes
    DEFAULT_SESSION_TIMEOUT_SECONDS = 3600  # 1 hour
    DEFAULT_PERSISTENCE_ENABLED = False
    DEFAULT_PERSISTENCE_DIR: Optional[Path] = None

    def __init__(
        self,
        idle_threshold_seconds: int = DEFAULT_IDLE_THRESHOLD_SECONDS,
        session_timeout_seconds: int = DEFAULT_SESSION_TIMEOUT_SECONDS,
        persistence_enabled: bool = DEFAULT_PERSISTENCE_ENABLED,
        persistence_dir: Optional[Path] = None
    ):
        """
        Initialize SessionTracker

        Args:
            idle_threshold_seconds: Seconds before session considered idle
            session_timeout_seconds: Seconds before session auto-closes
            persistence_enabled: Enable session state persistence
            persistence_dir: Directory for persistence files
        """
        self._idle_threshold_seconds = idle_threshold_seconds
        self._session_timeout_seconds = session_timeout_seconds
        self._persistence_enabled = persistence_enabled
        self._persistence_dir = persistence_dir or Path.home() / ".nexus-memory-system" / "sessions"

        # Session storage
        self._sessions: Dict[str, SessionInfo] = {}  # session_id -> SessionInfo
        self._agent_sessions: Dict[str, Set[str]] = defaultdict(set)  # agent_type -> session_ids
        self._active_session: Dict[str, Optional[str]] = {}  # agent_type -> current session_id

        # Event listeners
        self._listeners: List[SessionEventHandler] = []

        # Background tasks
        self._monitoring_task: Optional[asyncio.Task] = None
        self._monitoring = False
        self._initialized = False

        # Locks
        self._lock = asyncio.Lock()

    async def initialize(self) -> None:
        """Initialize the session tracker"""
        if self._initialized:
            return

        try:
            # Ensure persistence directory exists
            if self._persistence_enabled and self._persistence_dir:
                self._persistence_dir.mkdir(parents=True, exist_ok=True)
                # Load existing sessions
                await self._load_persisted_sessions()

            self._initialized = True
            logger.info("SessionTracker initialized successfully")

        except Exception as e:
            logger.error(f"Failed to initialize SessionTracker: {e}")
            raise

    async def close(self) -> None:
        """Close the session tracker and cleanup"""
        logger.info("Closing SessionTracker...")

        # Stop monitoring
        await self.stop_monitoring()

        # Persist active sessions if enabled
        if self._persistence_enabled:
            await self._persist_sessions()

        # Clear sessions
        async with self._lock:
            self._sessions.clear()
            self._agent_sessions.clear()
            self._active_session.clear()

        self._initialized = False
        logger.info("SessionTracker closed")

    # Session Lifecycle

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

        async with self._lock:
            # Generate session ID
            session_id = str(uuid.uuid4())

            # Create session info
            session = SessionInfo(
                session_id=session_id,
                agent_type=agent_type,
                state=SessionState.ACTIVE,
                metadata=metadata or {}
            )

            # Store session
            self._sessions[session_id] = session
            self._agent_sessions[agent_type].add(session_id)
            self._active_session[agent_type] = session_id

            logger.info(f"Started session {session_id} for {agent_type}")

            # Emit event
            await self._emit_event(SessionEvent(
                event_type=SessionEventType.SESSION_START,
                session_info=session,
                source="session_tracker"
            ))

            # Persist if enabled
            if self._persistence_enabled:
                await self._persist_session(session)

            return session

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
            reason: Reason for ending session
            trigger_extraction: Whether to trigger extraction

        Returns:
            Updated SessionInfo or None if not found
        """
        await self.ensure_initialized()

        async with self._lock:
            session = self._sessions.get(session_id)
            if not session:
                logger.warning(f"Session {session_id} not found")
                return None

            if session.state == SessionState.CLOSED:
                logger.warning(f"Session {session_id} already closed")
                return session

            # Update state
            session.state = SessionState.CLOSING
            session.metadata["end_reason"] = reason
            session.metadata["ended_at"] = datetime.now(UTC).isoformat()

            # Trigger extraction if requested
            if trigger_extraction:
                session.extraction_pending = True
                await self._emit_event(SessionEvent(
                    event_type=SessionEventType.EXTRACTION_TRIGGERED,
                    session_info=session,
                    source="session_tracker",
                    metadata={"reason": reason}
                ))

            # Close session
            session.state = SessionState.CLOSED

            # Update tracking
            agent_type = session.agent_type
            if session_id in self._agent_sessions[agent_type]:
                self._agent_sessions[agent_type].remove(session_id)
            if self._active_session.get(agent_type) == session_id:
                self._active_session[agent_type] = None

            logger.info(f"Ended session {session_id} for {agent_type} (reason: {reason})")

            # Emit event
            await self._emit_event(SessionEvent(
                event_type=SessionEventType.SESSION_END,
                session_info=session,
                source="session_tracker",
                metadata={"reason": reason, "triggered_extraction": trigger_extraction}
            ))

            # Persist if enabled
            if self._persistence_enabled:
                await self._persist_session(session)
                await self._remove_persisted_session(session_id)

            return session

    async def record_activity(
        self,
        session_id: str,
        activity_type: str = "unknown",
        metadata: Optional[Dict[str, Any]] = None
    ) -> Optional[SessionInfo]:
        """
        Record activity for a session

        Args:
            session_id: Session identifier
            activity_type: Type of activity
            metadata: Optional activity metadata

        Returns:
            Updated SessionInfo or None if not found
        """
        await self.ensure_initialized()

        async with self._lock:
            session = self._sessions.get(session_id)
            if not session:
                logger.warning(f"Session {session_id} not found for activity recording")
                return None

            # Update session
            session.update_activity()
            session.metadata["last_activity_type"] = activity_type
            if metadata:
                session.metadata.update(metadata)

            # Transition from idle to active
            if session.state == SessionState.IDLE:
                session.state = SessionState.ACTIVE
                await self._emit_event(SessionEvent(
                    event_type=SessionEventType.SESSION_ACTIVE,
                    session_info=session,
                    source="session_tracker"
                ))

            # Persist if enabled
            if self._persistence_enabled:
                await self._persist_session(session)

            return session

    async def mark_session_idle(self, session_id: str) -> Optional[SessionInfo]:
        """Mark a session as idle"""
        await self.ensure_initialized()

        async with self._lock:
            session = self._sessions.get(session_id)
            if not session or session.state != SessionState.ACTIVE:
                return None

            session.state = SessionState.IDLE
            await self._emit_event(SessionEvent(
                event_type=SessionEventType.SESSION_IDLE,
                session_info=session,
                source="session_tracker"
            ))

            if self._persistence_enabled:
                await self._persist_session(session)

            return session

    # Query Methods

    async def get_session(self, session_id: str) -> Optional[SessionInfo]:
        """Get session by ID"""
        async with self._lock:
            return self._sessions.get(session_id)

    async def get_active_session(self, agent_type: str) -> Optional[SessionInfo]:
        """Get the active session for an agent type"""
        async with self._lock:
            session_id = self._active_session.get(agent_type)
            if session_id:
                return self._sessions.get(session_id)
            return None

    async def get_sessions_by_agent(self, agent_type: str) -> List[SessionInfo]:
        """Get all sessions for an agent type"""
        async with self._lock:
            session_ids = self._agent_sessions.get(agent_type, set())
            return [self._sessions[sid] for sid in session_ids if sid in self._sessions]

    async def get_all_active_sessions(self) -> List[SessionInfo]:
        """Get all currently active sessions"""
        async with self._lock:
            return [
                s for s in self._sessions.values()
                if s.state in (SessionState.ACTIVE, SessionState.IDLE)
            ]

    async def get_status(self) -> Dict[str, Any]:
        """Get comprehensive session tracker status"""
        async with self._lock:
            active_sessions = [
                s for s in self._sessions.values()
                if s.state in (SessionState.ACTIVE, SessionState.IDLE)
            ]
            idle_sessions = [
                s for s in self._sessions.values()
                if s.state == SessionState.IDLE
            ]

            return {
                "total_sessions": len(self._sessions),
                "active_sessions": len(active_sessions),
                "idle_sessions": len(idle_sessions),
                "sessions_by_agent": {
                    agent_type: len(session_ids)
                    for agent_type, session_ids in self._agent_sessions.items()
                },
                "monitoring": self._monitoring,
                "persistence_enabled": self._persistence_enabled,
                "idle_threshold_seconds": self._idle_threshold_seconds,
                "session_timeout_seconds": self._session_timeout_seconds,
            }

    # Event Handling

    def register_listener(self, handler: SessionEventHandler) -> None:
        """Register a session event listener"""
        if handler not in self._listeners:
            self._listeners.append(handler)
            logger.debug(f"Registered session event listener: {handler.__name__}")

    def unregister_listener(self, handler: SessionEventHandler) -> None:
        """Unregister a session event listener"""
        if handler in self._listeners:
            self._listeners.remove(handler)
            logger.debug(f"Unregistered session event listener: {handler.__name__}")

    async def _emit_event(self, event: SessionEvent) -> None:
        """Emit event to all registered listeners"""
        for listener in self._listeners:
            try:
                await listener(event)
            except Exception as e:
                logger.error(f"Error in session event listener {listener.__name__}: {e}")

    # Monitoring

    async def start_monitoring(self, interval_seconds: int = 30) -> None:
        """Start background session monitoring"""
        if self._monitoring:
            logger.warning("Session monitoring already started")
            return

        self._monitoring = True
        self._monitoring_task = asyncio.create_task(
            self._monitoring_loop(interval_seconds)
        )
        logger.info("Started session monitoring")

    async def stop_monitoring(self) -> None:
        """Stop background session monitoring"""
        if not self._monitoring:
            return

        self._monitoring = False
        if self._monitoring_task:
            self._monitoring_task.cancel()
            try:
                await self._monitoring_task
            except asyncio.CancelledError:
                pass
            self._monitoring_task = None

        logger.info("Stopped session monitoring")

    async def _monitoring_loop(self, interval_seconds: int) -> None:
        """Main monitoring loop"""
        try:
            while self._monitoring:
                await self._check_idle_sessions()
                await self._check_timeout_sessions()
                await asyncio.sleep(interval_seconds)
        except asyncio.CancelledError:
            logger.debug("Session monitoring loop cancelled")
        except Exception as e:
            logger.error(f"Error in session monitoring loop: {e}")

    async def _check_idle_sessions(self) -> None:
        """Check for idle sessions and mark them"""
        try:
            sessions = await self.get_all_active_sessions()
            for session in sessions:
                if session.state == SessionState.ACTIVE and session.is_idle(self._idle_threshold_seconds):
                    await self.mark_session_idle(session.session_id)
        except Exception as e:
            logger.error(f"Error checking idle sessions: {e}")

    async def _check_timeout_sessions(self) -> None:
        """Check for timed out sessions and end them"""
        try:
            sessions = await self.get_all_active_sessions()
            for session in sessions:
                if session.is_idle(self._session_timeout_seconds):
                    await self.end_session(
                        session.session_id,
                        reason="timeout",
                        trigger_extraction=True
                    )
        except Exception as e:
            logger.error(f"Error checking timeout sessions: {e}")

    # Persistence

    async def _load_persisted_sessions(self) -> None:
        """Load sessions from persistent storage"""
        if not self._persistence_dir or not self._persistence_dir.exists():
            return

        try:
            for session_file in self._persistence_dir.glob("session_*.json"):
                try:
                    with open(session_file, 'r') as f:
                        data = json.load(f)

                    # Reconstruct session
                    session = SessionInfo(
                        session_id=data["session_id"],
                        agent_type=data["agent_type"],
                        state=SessionState(data["state"]),
                        started_at=datetime.fromisoformat(data["started_at"]),
                        last_activity=datetime.fromisoformat(data["last_activity"]),
                        activity_count=data["activity_count"],
                        metadata=data.get("metadata", {}),
                        error_count=data.get("error_count", 0),
                        last_error=data.get("last_error"),
                        extraction_pending=data.get("extraction_pending", False),
                    )

                    # Only restore if not closed
                    if session.state != SessionState.CLOSED:
                        self._sessions[session.session_id] = session
                        self._agent_sessions[session.agent_type].add(session.session_id)
                        if session.state in (SessionState.ACTIVE, SessionState.IDLE):
                            self._active_session[session.agent_type] = session.session_id

                    logger.debug(f"Loaded session {session.session_id} from persistence")

                except Exception as e:
                    logger.warning(f"Failed to load session from {session_file}: {e}")

        except Exception as e:
            logger.error(f"Error loading persisted sessions: {e}")

    async def _persist_session(self, session: SessionInfo) -> None:
        """Persist a single session to disk"""
        if not self._persistence_dir:
            return

        try:
            self._persistence_dir.mkdir(parents=True, exist_ok=True)
            session_file = self._persistence_dir / f"session_{session.session_id}.json"

            with open(session_file, 'w') as f:
                json.dump(session.to_dict(), f, indent=2, default=str)

        except Exception as e:
            logger.error(f"Failed to persist session {session.session_id}: {e}")

    async def _persist_sessions(self) -> None:
        """Persist all active sessions"""
        for session in self._sessions.values():
            if session.state != SessionState.CLOSED:
                await self._persist_session(session)

    async def _remove_persisted_session(self, session_id: str) -> None:
        """Remove a persisted session file"""
        if not self._persistence_dir:
            return

        try:
            session_file = self._persistence_dir / f"session_{session_id}.json"
            if session_file.exists():
                session_file.unlink()
        except Exception as e:
            logger.warning(f"Failed to remove persisted session {session_id}: {e}")

    async def ensure_initialized(self) -> None:
        """Ensure the tracker is initialized"""
        if not self._initialized:
            await self.initialize()
