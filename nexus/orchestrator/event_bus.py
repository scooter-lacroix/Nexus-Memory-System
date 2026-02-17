"""
Event Bus - Event routing and processing for Nexus Orchestrator

This module provides event routing capabilities:
- Event queue for reliable async processing
- Event filtering and routing to handlers
- Priority-based event processing
- Event persistence for recovery
- Support for event subscriptions

Architecture:
    Event Producer
        |
    EventBus.publish()
        |
    EventQueue (priority queue)
        |
    EventBus._process_events()
        |
    Route to matching handlers
        |
    Execute handlers asynchronously
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Optional, Any, Callable, Awaitable, Set, Pattern
from enum import Enum, auto
from dataclasses import dataclass, field
from collections import defaultdict
import uuid
import re
from loguru import logger
from pathlib import Path
import json


class EventPriority(Enum):
    """Event priority levels for processing order"""
    CRITICAL = auto()  # Highest priority, process immediately
    HIGH = auto()
    NORMAL = auto()
    LOW = auto()
    BACKGROUND = auto()  # Lowest priority


class EventType:
    """Type system for event filtering and routing"""
    # Memory events
    MEMORY_STORED = "memory.stored"
    MEMORY_SEARCHED = "memory.searched"
    MEMORY_DELETED = "memory.deleted"
    MEMORY_UPDATED = "memory.updated"

    # Session events
    SESSION_START = "session.started"
    SESSION_END = "session.ended"
    SESSION_IDLE = "session.idle"
    SESSION_ACTIVE = "session.active"

    # Sync events
    SYNC_STARTED = "sync.started"
    SYNC_COMPLETE = "sync.complete"
    SYNC_FAILED = "sync.failed"
    MEMORY_SHARED = "memory.shared"

    # Extraction events
    EXTRACTION_TRIGGERED = "extraction.triggered"
    EXTRACTION_COMPLETE = "extraction.complete"
    EXTRACTION_FAILED = "extraction.failed"

    # System events
    SYSTEM_ERROR = "system.error"
    SYSTEM_WARNING = "system.warning"
    SYSTEM_READY = "system.ready"


@dataclass
class Event:
    """
    Event data structure for the event bus

    Attributes:
        id: Unique event identifier
        type: Event type string (e.g., "memory.stored")
        data: Event payload data
        source: Source of the event
        priority: Event priority for processing
        timestamp: Event creation timestamp
        metadata: Additional event metadata
        retries: Number of retry attempts
        max_retries: Maximum retry attempts allowed
    """
    id: str = field(default_factory=lambda: str(uuid.uuid4()))
    type: str = ""
    data: Dict[str, Any] = field(default_factory=dict)
    source: str = "unknown"
    priority: EventPriority = EventPriority.NORMAL
    timestamp: datetime = field(default_factory=lambda: datetime.now(UTC))
    metadata: Dict[str, Any] = field(default_factory=dict)
    retries: int = 0
    max_retries: int = 3

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary representation"""
        return {
            "id": self.id,
            "type": self.type,
            "data": self.data,
            "source": self.source,
            "priority": self.priority.name,
            "timestamp": self.timestamp.isoformat(),
            "metadata": self.metadata,
            "retries": self.retries,
            "max_retries": self.max_retries,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "Event":
        """Create Event from dictionary"""
        return cls(
            id=data["id"],
            type=data["type"],
            data=data["data"],
            source=data["source"],
            priority=EventPriority[data["priority"]],
            timestamp=datetime.fromisoformat(data["timestamp"]),
            metadata=data.get("metadata", {}),
            retries=data.get("retries", 0),
            max_retries=data.get("max_retries", 3),
        )


@dataclass(order=True)
class PriorityEvent:
    """Wrapper for priority queue ordering"""
    priority: int
    timestamp: float
    event: Event = field(compare=False)


EventHandler = Callable[[Event], Awaitable[None]]
EventFilter = Callable[[Event], bool]


class EventQueue:
    """
    Async priority queue for events with persistence support

    Maintains events in priority order and supports persistence
    for crash recovery.
    """

    def __init__(
        self,
        max_size: int = 10000,
        persistence_enabled: bool = False,
        persistence_dir: Optional[Path] = None
    ):
        """
        Initialize EventQueue

        Args:
            max_size: Maximum queue size
            persistence_enabled: Enable event persistence
            persistence_dir: Directory for persistence files
        """
        self._queue: asyncio.PriorityQueue = asyncio.PriorityQueue(maxsize=max_size)
        self._max_size = max_size
        self._persistence_enabled = persistence_enabled
        self._persistence_dir = persistence_dir or Path.home() / ".nexus-memory-system" / "events"
        self._pending_events: Dict[str, Event] = {}

    async def put(self, event: Event) -> bool:
        """Add event to queue"""
        try:
            # Create priority wrapper
            priority_order = {
                EventPriority.CRITICAL: 0,
                EventPriority.HIGH: 1,
                EventPriority.NORMAL: 2,
                EventPriority.LOW: 3,
                EventPriority.BACKGROUND: 4,
            }
            priority_event = PriorityEvent(
                priority=priority_order[event.priority],
                timestamp=event.timestamp.timestamp(),
                event=event
            )

            await self._queue.put(priority_event)
            self._pending_events[event.id] = event

            # Persist if enabled
            if self._persistence_enabled:
                await self._persist_event(event)

            return True

        except asyncio.QueueFull:
            logger.warning(f"Event queue full, dropping event {event.id}")
            return False
        except Exception as e:
            logger.error(f"Error adding event to queue: {e}")
            return False

    async def get(self, timeout: Optional[float] = None) -> Optional[Event]:
        """Get next event from queue"""
        try:
            if timeout:
                priority_event = await asyncio.wait_for(
                    self._queue.get(),
                    timeout=timeout
                )
            else:
                priority_event = await self._queue.get()

            event = priority_event.event
            self._pending_events.pop(event.id, None)

            # Remove from persistence
            if self._persistence_enabled:
                await self._remove_persisted_event(event.id)

            return event

        except asyncio.TimeoutError:
            return None
        except Exception as e:
            logger.error(f"Error getting event from queue: {e}")
            return None

    async def qsize(self) -> int:
        """Get approximate queue size"""
        return self._queue.qsize()

    def empty(self) -> bool:
        """Check if queue is empty"""
        return self._queue.empty()

    async def clear(self) -> None:
        """Clear all events from queue"""
        while not self._queue.empty():
            try:
                self._queue.get_nowait()
            except asyncio.QueueEmpty:
                break
        self._pending_events.clear()

    # Persistence

    async def _persist_event(self, event: Event) -> None:
        """Persist event to disk"""
        if not self._persistence_dir:
            return

        try:
            self._persistence_dir.mkdir(parents=True, exist_ok=True)
            event_file = self._persistence_dir / f"event_{event.id}.json"

            with open(event_file, 'w') as f:
                json.dump(event.to_dict(), f, indent=2, default=str)

        except Exception as e:
            logger.error(f"Failed to persist event {event.id}: {e}")

    async def _remove_persisted_event(self, event_id: str) -> None:
        """Remove persisted event file"""
        if not self._persistence_dir:
            return

        try:
            event_file = self._persistence_dir / f"event_{event_id}.json"
            if event_file.exists():
                event_file.unlink()
        except Exception as e:
            logger.warning(f"Failed to remove persisted event {event_id}: {e}")

    async def load_persisted_events(self) -> List[Event]:
        """Load persisted events from disk"""
        if not self._persistence_dir or not self._persistence_dir.exists():
            return []

        events = []
        try:
            for event_file in self._persistence_dir.glob("event_*.json"):
                try:
                    with open(event_file, 'r') as f:
                        data = json.load(f)
                    events.append(Event.from_dict(data))
                except Exception as e:
                    logger.warning(f"Failed to load event from {event_file}: {e}")

        except Exception as e:
            logger.error(f"Error loading persisted events: {e}")

        return events


class EventBus:
    """
    Central event bus for routing events to handlers

    Supports:
    - Event type-based routing with wildcards
    - Priority-based processing
    - Async event handlers
    - Event filtering
    - Retry logic for failed handlers
    - Event persistence for reliability

    Usage:
        bus = EventBus()
        await bus.initialize()

        # Subscribe to events
        bus.subscribe("memory.stored", my_handler)

        # Publish event
        await bus.publish(Event(
            type="memory.stored",
            data={"memory_id": 123}
        ))

        # Start processing
        await bus.start_processing()
    """

    # Default configuration
    DEFAULT_MAX_QUEUE_SIZE = 10000
    DEFAULT_PROCESSING_INTERVAL = 0.1  # seconds
    DEFAULT_PERSISTENCE_ENABLED = False
    DEFAULT_MAX_WORKERS = 4

    def __init__(
        self,
        max_queue_size: int = DEFAULT_MAX_QUEUE_SIZE,
        max_workers: int = DEFAULT_MAX_WORKERS,
        persistence_enabled: bool = DEFAULT_PERSISTENCE_ENABLED,
        persistence_dir: Optional[Path] = None
    ):
        """
        Initialize EventBus

        Args:
            max_queue_size: Maximum event queue size
            max_workers: Maximum concurrent handler workers
            persistence_enabled: Enable event persistence
            persistence_dir: Directory for persistence files
        """
        self._max_queue_size = max_queue_size
        self._max_workers = max_workers

        # Event queue
        self._event_queue = EventQueue(
            max_size=max_queue_size,
            persistence_enabled=persistence_enabled,
            persistence_dir=persistence_dir
        )

        # Handler registry: pattern -> handlers
        self._handlers: Dict[str, List[EventHandler]] = defaultdict(list)

        # Handler metadata
        self._handler_filters: Dict[EventHandler, EventFilter] = {}
        self._handler_stats: Dict[EventHandler, Dict[str, Any]] = defaultdict(
            lambda: {"calls": 0, "errors": 0, "last_called": None}
        )

        # Processing state
        self._processing_task: Optional[asyncio.Task] = None
        self._worker_tasks: Set[asyncio.Task] = set()
        self._processing = False
        self._initialized = False
        self._closed = False

        # Semaphore for concurrent workers
        self._worker_semaphore = asyncio.Semaphore(max_workers)

    async def initialize(self) -> None:
        """Initialize the event bus"""
        if self._initialized:
            return

        try:
            # Load persisted events
            if self._event_queue._persistence_enabled:
                persisted = await self._event_queue.load_persisted_events()
                for event in persisted:
                    await self._event_queue.put(event)
                logger.info(f"Loaded {len(persisted)} persisted events")

            self._initialized = True
            logger.info("EventBus initialized successfully")

        except Exception as e:
            logger.error(f"Failed to initialize EventBus: {e}")
            raise

    async def close(self) -> None:
        """Close the event bus and cleanup"""
        logger.info("Closing EventBus...")

        self._closed = True

        # Stop processing
        await self.stop_processing()

        # Clear handlers
        self._handlers.clear()
        self._handler_filters.clear()
        self._handler_stats.clear()

        self._initialized = False
        logger.info("EventBus closed")

    # Event Publishing

    async def publish(
        self,
        event_type: str,
        data: Dict[str, Any],
        source: str = "unknown",
        priority: EventPriority = EventPriority.NORMAL,
        metadata: Optional[Dict[str, Any]] = None
    ) -> bool:
        """
        Publish an event to the bus

        Args:
            event_type: Event type string
            data: Event payload data
            source: Event source identifier
            priority: Event priority
            metadata: Optional metadata

        Returns:
            True if event was queued successfully
        """
        if self._closed:
            logger.warning("EventBus is closed, ignoring event")
            return False

        event = Event(
            type=event_type,
            data=data,
            source=source,
            priority=priority,
            metadata=metadata or {}
        )

        return await self._event_queue.put(event)

    async def publish_event(self, event: Event) -> bool:
        """Publish a pre-constructed event"""
        return await self._event_queue.put(event)

    # Handler Subscription

    def subscribe(
        self,
        event_pattern: str,
        handler: EventHandler,
        filter_fn: Optional[EventFilter] = None
    ) -> None:
        """
        Subscribe to events matching a pattern

        Args:
            event_pattern: Event type pattern (supports wildcards: "memory.*")
            handler: Async event handler function
            filter_fn: Optional filter function
        """
        if handler not in self._handlers[event_pattern]:
            self._handlers[event_pattern].append(handler)
            if filter_fn:
                self._handler_filters[handler] = filter_fn
            logger.debug(f"Subscribed handler {handler.__name__} to {event_pattern}")

    def unsubscribe(
        self,
        event_pattern: str,
        handler: EventHandler
    ) -> None:
        """Unsubscribe a handler from an event pattern"""
        if handler in self._handlers[event_pattern]:
            self._handlers[event_pattern].remove(handler)
            self._handler_filters.pop(handler, None)
            logger.debug(f"Unsubscribed handler {handler.__name__} from {event_pattern}")

    def unsubscribe_all(self, handler: EventHandler) -> None:
        """Unsubscribe handler from all patterns"""
        for pattern in list(self._handlers.keys()):
            self.unsubscribe(pattern, handler)

    # Processing

    async def start_processing(self) -> None:
        """Start background event processing"""
        if self._processing:
            logger.warning("Event processing already started")
            return

        self._processing = True
        self._processing_task = asyncio.create_task(self._processing_loop())
        logger.info("Started event processing")

    async def stop_processing(self) -> None:
        """Stop background event processing"""
        if not self._processing:
            return

        logger.info("Stopping event processing...")
        self._processing = False

        # Cancel main processing task
        if self._processing_task:
            self._processing_task.cancel()
            try:
                await self._processing_task
            except asyncio.CancelledError:
                pass
            self._processing_task = None

        # Cancel all worker tasks
        for task in self._worker_tasks:
            task.cancel()
        await asyncio.gather(*self._worker_tasks, return_exceptions=True)
        self._worker_tasks.clear()

        logger.info("Stopped event processing")

    async def _processing_loop(self) -> None:
        """Main event processing loop"""
        try:
            while self._processing and not self._closed:
                event = await self._event_queue.get(timeout=1.0)
                if event:
                    # Spawn worker task
                    task = asyncio.create_task(self._process_event(event))
                    self._worker_tasks.add(task)
                    task.add_done_callback(self._worker_tasks.discard)

        except asyncio.CancelledError:
            logger.debug("Event processing loop cancelled")
        except Exception as e:
            logger.error(f"Error in event processing loop: {e}")

    async def _process_event(self, event: Event) -> None:
        """Process a single event"""
        # Find matching handlers
        matching_handlers = self._find_handlers(event)

        if not matching_handlers:
            logger.debug(f"No handlers for event type: {event.type}")
            return

        # Execute handlers concurrently (with semaphore limit)
        async with self._worker_semaphore:
            tasks = [
                self._execute_handler(handler, event)
                for handler in matching_handlers
            ]
            await asyncio.gather(*tasks, return_exceptions=True)

    def _find_handlers(self, event: Event) -> List[EventHandler]:
        """Find all handlers matching the event type"""
        handlers = []

        for pattern, pattern_handlers in self._handlers.items():
            # Convert pattern to regex
            regex = pattern.replace(".", r"\.").replace("*", ".*")
            if re.match(f"^{regex}$", event.type):
                handlers.extend(pattern_handlers)

        return handlers

    async def _execute_handler(self, handler: EventHandler, event: Event) -> None:
        """Execute a single event handler with retry logic"""
        stats = self._handler_stats[handler]

        try:
            # Check filter
            filter_fn = self._handler_filters.get(handler)
            if filter_fn and not filter_fn(event):
                return

            # Execute handler
            await handler(event)

            # Update stats
            stats["calls"] += 1
            stats["last_called"] = datetime.now(UTC).isoformat()

        except Exception as e:
            stats["errors"] += 1
            logger.error(f"Error in handler {handler.__name__} for event {event.id}: {e}")

            # Retry logic
            if event.retries < event.max_retries:
                event.retries += 1
                await self._event_queue.put(event)
                logger.info(f"Retrying event {event.id} (attempt {event.retries}/{event.max_retries})")
            else:
                logger.error(f"Event {event.id} exceeded max retries, giving up")

    # Query Methods

    async def get_queue_size(self) -> int:
        """Get current event queue size"""
        return await self._event_queue.qsize()

    async def get_stats(self) -> Dict[str, Any]:
        """Get event bus statistics"""
        handler_stats = {}
        for handler, stats in self._handler_stats.items():
            handler_stats[handler.__name__] = stats.copy()

        return {
            "processing": self._processing,
            "queue_size": await self.get_queue_size(),
            "handlers_count": sum(len(h) for h in self._handlers.values()),
            "worker_tasks": len(self._worker_tasks),
            "handler_stats": handler_stats,
        }
