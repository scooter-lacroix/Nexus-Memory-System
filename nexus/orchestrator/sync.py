"""
Cross-Agent Sync - Memory synchronization across agent namespaces

This module provides cross-agent synchronization capabilities:
- Share memories between agent namespaces
- Handle cross_agent memory type sharing
- Coordinate memory consistency
- Support sync policies and rules
- Track sync status and history

Architecture:
    Memory Share Request
        |
    CrossAgentSync.share_memory()
        |
    Apply Sync Policy
        |
    Copy to target namespaces
        |
    Emit sync events
        |
    Track sync history
"""

import asyncio
from collections import defaultdict
from datetime import datetime, UTC
from typing import Dict, List, Optional, Any, Set, Tuple
from enum import Enum, auto
from dataclasses import dataclass, field
from loguru import logger
import uuid

from ..database.managers import MemoryManager, DatabaseManager
from ..config.agent_namespaces import AGENT_NAMESPACES


class SyncPolicy(Enum):
    """Synchronization policies for memory sharing"""
    # Manual: Only explicit shares
    MANUAL = auto()
    # Auto: Automatically share cross_agent type memories
    AUTO = auto()
    # Selective: Share based on labels/categories
    SELECTIVE = auto()
    # Bidirectional: Share both ways between agents
    BIDIRECTIONAL = auto()


class SyncStatus(Enum):
    """Status of sync operations"""
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    FAILED = "failed"
    PARTIAL = "partial"


@dataclass
class MemoryShareRequest:
    """
    Request to share a memory across agent namespaces

    Attributes:
        request_id: Unique request identifier
        source_memory_id: Source memory ID to share
        source_agent_type: Source agent type
        target_agent_types: Target agent types to share with
        policy: Sync policy to apply
        reason: Reason for sharing
        metadata: Additional metadata
        created_at: Request creation timestamp
    """
    request_id: str = field(default_factory=lambda: str(uuid.uuid4()))
    source_memory_id: int = 0
    source_agent_type: str = ""
    target_agent_types: List[str] = field(default_factory=list)
    policy: SyncPolicy = SyncPolicy.MANUAL
    reason: str = ""
    metadata: Dict[str, Any] = field(default_factory=dict)
    created_at: datetime = field(default_factory=lambda: datetime.now(UTC))

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary"""
        return {
            "request_id": self.request_id,
            "source_memory_id": self.source_memory_id,
            "source_agent_type": self.source_agent_type,
            "target_agent_types": self.target_agent_types,
            "policy": self.policy.name,
            "reason": self.reason,
            "metadata": self.metadata,
            "created_at": self.created_at.isoformat(),
        }


@dataclass
class SyncResult:
    """
    Result of a sync operation

    Attributes:
        request_id: Associated request ID
        status: Sync status
        shared_memory_ids: Map of agent_type -> new_memory_id
        failures: List of failed targets with reasons
        started_at: Operation start time
        completed_at: Operation completion time
        metadata: Additional result metadata
    """
    request_id: str
    status: SyncStatus
    shared_memory_ids: Dict[str, int] = field(default_factory=dict)
    failures: Dict[str, str] = field(default_factory=dict)
    started_at: datetime = field(default_factory=lambda: datetime.now(UTC))
    completed_at: Optional[datetime] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary"""
        return {
            "request_id": self.request_id,
            "status": self.status.value,
            "shared_memory_ids": self.shared_memory_ids,
            "failures": self.failures,
            "duration_seconds": (
                (self.completed_at - self.started_at).total_seconds()
                if self.completed_at else None
            ),
            "started_at": self.started_at.isoformat(),
            "completed_at": self.completed_at.isoformat() if self.completed_at else None,
            "metadata": self.metadata,
        }


@dataclass
class SyncHistory:
    """Historical record of sync operations"""
    history_id: str
    request: MemoryShareRequest
    result: SyncResult
    timestamp: datetime = field(default_factory=lambda: datetime.now(UTC))

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary"""
        return {
            "history_id": self.history_id,
            "request": self.request.to_dict(),
            "result": self.result.to_dict(),
            "timestamp": self.timestamp.isoformat(),
        }


class CrossAgentSync:
    """
    Manages cross-agent memory synchronization

    Handles sharing memories between agent namespaces while maintaining
    consistency and tracking sync history.

    Usage:
        sync = CrossAgentSync(memory_manager)
        await sync.initialize()

        # Share memory
        result = await sync.share_memory(
            source_memory_id=123,
            source_agent_type="claude-code",
            target_agent_types=["gemini", "qwen"]
        )

        # Get history
        history = await sync.get_sync_history()
    """

    # Default configuration
    DEFAULT_SYNC_POLICY = SyncPolicy.MANUAL
    DEFAULT_AUTO_SHARE_LABELS = ["cross-agent", "shared"]
    DEFAULT_MAX_RETRIES = 3

    def __init__(
        self,
        memory_manager: MemoryManager,
        db_manager: DatabaseManager,
        sync_policy: SyncPolicy = DEFAULT_SYNC_POLICY,
        auto_share_labels: Optional[List[str]] = None
    ):
        """
        Initialize CrossAgentSync

        Args:
            memory_manager: MemoryManager instance
            db_manager: DatabaseManager instance
            sync_policy: Default sync policy
            auto_share_labels: Labels that trigger auto-share
        """
        self.memory_manager = memory_manager
        self.db_manager = db_manager
        self._sync_policy = sync_policy
        self._auto_share_labels = auto_share_labels or self.DEFAULT_AUTO_SHARE_LABELS

        # Sync history
        self._sync_history: Dict[str, SyncHistory] = {}
        self._history_by_memory: Dict[int, List[str]] = defaultdict(list)  # memory_id -> history_ids

        # Tracking
        self._pending_syncs: Set[str] = set()
        self._initialized = False

        # Locks
        self._sync_lock = asyncio.Lock()
        self._history_lock = asyncio.Lock()

    async def initialize(self) -> None:
        """Initialize the sync service"""
        if self._initialized:
            return

        self._initialized = True
        logger.info("CrossAgentSync initialized successfully")

    async def close(self) -> None:
        """Close the sync service"""
        logger.info("Closing CrossAgentSync...")

        # Wait for pending syncs
        while self._pending_syncs:
            logger.info(f"Waiting for {len(self._pending_syncs)} pending syncs...")
            await asyncio.sleep(0.5)

        self._initialized = False
        logger.info("CrossAgentSync closed")

    # Memory Sharing

    async def share_memory(
        self,
        source_memory_id: int,
        source_agent_type: str,
        target_agent_types: Optional[List[str]] = None,
        policy: Optional[SyncPolicy] = None,
        reason: str = "manual_share",
        metadata: Optional[Dict[str, Any]] = None
    ) -> SyncResult:
        """
        Share a memory across agent namespaces

        Args:
            source_memory_id: Source memory ID
            source_agent_type: Source agent type
            target_agent_types: Target agent types (None = all)
            policy: Sync policy to apply
            reason: Reason for sharing
            metadata: Additional metadata

        Returns:
            SyncResult with sync outcome
        """
        await self.ensure_initialized()

        # Create request
        request = MemoryShareRequest(
            source_memory_id=source_memory_id,
            source_agent_type=source_agent_type,
            target_agent_types=target_agent_types or list(AGENT_NAMESPACES.keys()),
            policy=policy or self._sync_policy,
            reason=reason,
            metadata=metadata or {}
        )

        # Track pending
        self._pending_syncs.add(request.request_id)

        try:
            # Get source memory
            source_memory = await self._get_memory(source_memory_id, source_agent_type)
            if not source_memory:
                return SyncResult(
                    request_id=request.request_id,
                    status=SyncStatus.FAILED,
                    failures={"all": "Source memory not found"}
                )

            # Determine targets
            targets = await self._determine_targets(request, source_memory)

            if not targets:
                return SyncResult(
                    request_id=request.request_id,
                    status=SyncStatus.COMPLETED,
                    shared_memory_ids={},
                    metadata={"message": "No eligible targets"}
                )

            # Execute sync
            result = await self._execute_sync(request, source_memory, targets)

            # Record history
            await self._record_history(request, result)

            return result

        finally:
            self._pending_syncs.discard(request.request_id)

    async def share_to_all(
        self,
        source_memory_id: int,
        source_agent_type: str,
        exclude_agent_types: Optional[List[str]] = None
    ) -> SyncResult:
        """
        Share memory to all agent namespaces except specified exclusions

        Args:
            source_memory_id: Source memory ID
            source_agent_type: Source agent type
            exclude_agent_types: Agent types to exclude

        Returns:
            SyncResult with sync outcome
        """
        exclude = set(exclude_agent_types or [])
        exclude.add(source_agent_type)  # Always exclude source

        target_types = [
            agent_type for agent_type in AGENT_NAMESPACES.keys()
            if agent_type not in exclude
        ]

        return await self.share_memory(
            source_memory_id=source_memory_id,
            source_agent_type=source_agent_type,
            target_agent_types=target_types,
            reason="share_to_all"
        )

    async def auto_share_if_eligible(
        self,
        memory_id: int,
        agent_type: str,
        memory_data: Dict[str, Any]
    ) -> Optional[SyncResult]:
        """
        Auto-share memory if it meets criteria

        Checks for cross_agent memory lane type or auto-share labels.

        Args:
            memory_id: Memory ID
            agent_type: Source agent type
            memory_data: Memory data dict

        Returns:
            SyncResult if shared, None otherwise
        """
        if self._sync_policy == SyncPolicy.MANUAL:
            return None

        # Check memory lane type
        memory_lane_type = memory_data.get("memory_lane_type")
        if memory_lane_type == "cross_agent":
            return await self.share_to_all(memory_id, agent_type)

        # Check labels
        labels = memory_data.get("labels", [])
        for label in labels:
            if label.lower() in [l.lower() for l in self._auto_share_labels]:
                return await self.share_to_all(memory_id, agent_type)

        return None

    # Internal Methods

    async def _get_memory(
        self,
        memory_id: int,
        agent_type: str
    ) -> Optional[Dict[str, Any]]:
        """Get memory by ID and agent type"""
        try:
            # Search for the memory
            async with self.db_manager.get_async_session() as session:
                from ..database.models import Memory, AgentNamespace
                from sqlalchemy import select

                # Get namespace
                ns_stmt = select(AgentNamespace).where(
                    AgentNamespace.agent_type == agent_type
                )
                ns_result = await session.execute(ns_stmt)
                namespace = ns_result.scalar_one_or_none()

                if not namespace:
                    return None

                # Get memory
                mem_stmt = select(Memory).where(
                    Memory.id == memory_id,
                    Memory.namespace_id == namespace.id
                )
                mem_result = await session.execute(mem_stmt)
                memory = mem_result.scalar_one_or_none()

                if memory:
                    return memory.to_dict()

                return None

        except Exception as e:
            logger.error(f"Error getting memory {memory_id}: {e}")
            return None

    async def _determine_targets(
        self,
        request: MemoryShareRequest,
        source_memory: Dict[str, Any]
    ) -> List[str]:
        """Determine eligible target agent types"""
        targets = []

        for target_type in request.target_agent_types:
            # Skip if same as source
            if target_type == request.source_agent_type:
                continue

            # Check if namespace exists
            if target_type not in AGENT_NAMESPACES:
                logger.warning(f"Unknown agent type: {target_type}")
                continue

            # Apply policy rules
            if request.policy == SyncPolicy.SELECTIVE:
                # Check labels/categories
                labels = source_memory.get("labels", [])
                category = source_memory.get("category", "")

                # Share if has shareable labels or category
                if not any(l.lower() in [sl.lower() for sl in self._auto_share_labels] for l in labels):
                    if category not in ["general", "shared"]:
                        continue

            targets.append(target_type)

        return targets

    async def _execute_sync(
        self,
        request: MemoryShareRequest,
        source_memory: Dict[str, Any],
        target_types: List[str]
    ) -> SyncResult:
        """Execute sync to target namespaces"""
        result = SyncResult(
            request_id=request.request_id,
            status=SyncStatus.IN_PROGRESS
        )

        async with self._sync_lock:
            for target_type in target_types:
                try:
                    # Create copy in target namespace
                    store_result = await self.memory_manager.store_memory(
                        content=source_memory["content"],
                        agent_type=target_type,
                        category=source_memory.get("category", "general"),
                        memory_lane_type=source_memory.get("memory_lane_type"),
                        labels=source_memory.get("labels", []),
                        metadata={
                            **source_memory.get("metadata", {}),
                            "cross_agent_sync": {
                                "source_memory_id": source_memory["id"],
                                "source_agent_type": request.source_agent_type,
                                "sync_request_id": request.request_id,
                                "synced_at": datetime.now(UTC).isoformat(),
                                "sync_reason": request.reason,
                            }
                        }
                    )

                    if store_result["success"]:
                        new_memory_id = store_result["memory_id"]
                        result.shared_memory_ids[target_type] = new_memory_id
                        logger.info(
                            f"Shared memory {source_memory['id']} to {target_type} "
                            f"-> {new_memory_id}"
                        )
                    else:
                        result.failures[target_type] = store_result.get("error", "Unknown error")

                except Exception as e:
                    logger.error(f"Failed to sync to {target_type}: {e}")
                    result.failures[target_type] = str(e)

            # Determine final status
            if result.failures:
                if result.shared_memory_ids:
                    result.status = SyncStatus.PARTIAL
                else:
                    result.status = SyncStatus.FAILED
            else:
                result.status = SyncStatus.COMPLETED

            result.completed_at = datetime.now(UTC)

        return result

    async def _record_history(
        self,
        request: MemoryShareRequest,
        result: SyncResult
    ) -> None:
        """Record sync operation in history"""
        async with self._history_lock:
            history = SyncHistory(
                history_id=str(uuid.uuid4()),
                request=request,
                result=result
            )

            self._sync_history[history.history_id] = history
            self._history_by_memory[request.source_memory_id].append(history.history_id)

            # Keep history manageable (max 1000 entries)
            if len(self._sync_history) > 1000:
                # Remove oldest entries
                sorted_history = sorted(
                    self._sync_history.items(),
                    key=lambda x: x[1].timestamp
                )
                for hist_id, _ in sorted_history[:100]:
                    del self._sync_history[hist_id]

    # Query Methods

    async def get_sync_history(
        self,
        memory_id: Optional[int] = None,
        limit: int = 50
    ) -> List[Dict[str, Any]]:
        """
        Get sync history

        Args:
            memory_id: Filter by source memory ID
            limit: Maximum results

        Returns:
            List of history entries
        """
        async with self._history_lock:
            if memory_id:
                history_ids = self._history_by_memory.get(memory_id, [])
                histories = [
                    self._sync_history[hid].to_dict()
                    for hid in history_ids[-limit:]
                ]
            else:
                sorted_history = sorted(
                    self._sync_history.values(),
                    key=lambda h: h.timestamp,
                    reverse=True
                )
                histories = [h.to_dict() for h in sorted_history[:limit]]

            return histories

    async def get_sync_stats(self) -> Dict[str, Any]:
        """Get synchronization statistics"""
        async with self._history_lock:
            total_syncs = len(self._sync_history)
            successful = sum(
                1 for h in self._sync_history.values()
                if h.result.status == SyncStatus.COMPLETED
            )
            failed = sum(
                1 for h in self._sync_history.values()
                if h.result.status == SyncStatus.FAILED
            )
            partial = sum(
                1 for h in self._sync_history.values()
                if h.result.status == SyncStatus.PARTIAL
            )

            # Memory shares
            memory_shares = defaultdict(int)
            for history in self._sync_history.values():
                for target_type in history.result.shared_memory_ids:
                    memory_shares[target_type] += 1

            return {
                "total_syncs": total_syncs,
                "successful_syncs": successful,
                "failed_syncs": failed,
                "partial_syncs": partial,
                "pending_syncs": len(self._pending_syncs),
                "shares_by_target": dict(memory_shares),
                "sync_policy": self._sync_policy.name,
            }

    async def ensure_initialized(self) -> None:
        """Ensure the sync service is initialized"""
        if not self._initialized:
            await self.initialize()
