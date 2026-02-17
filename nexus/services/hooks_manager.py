"""
Hooks Manager - Orchestrates agent hooks for automated memory extraction

This module provides the HooksManager service which:
1. Manages native agent hooks for all agent types
2. Installs automated extraction for session end detection
3. Coordinates between hook layers (native, monitor, inactivity, buffer)
4. Integrates with NexusManager for memory storage

Architecture:
    Session End Detected (any layer)
        ↓
    HooksManager._on_session_end()
        ↓
    Extract context (native → buffer → fallback)
        ↓
    Store to NexusManager
        ↓
    Clear buffer on success
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Optional, Any, Callable, Awaitable
from pathlib import Path
from enum import Enum
from dataclasses import dataclass, field
from loguru import logger

from ..hooks.factory import create_native_hook, list_supported_agents, get_hook_info
from ..hooks.detector import SessionDetector
from ..hooks.monitor import SessionMonitor, InactivityDetector
from ..hooks.base import HookResult


class HookInstallationStatus(Enum):
    """Status of hook installation"""
    SUCCESS = "success"
    FAILED = "failed"
    ALREADY_INSTALLED = "already_installed"
    NOT_SUPPORTED = "not_supported"
    DISABLED = "disabled"


@dataclass
class HookInstallationResult:
    """Result of hook installation operation"""
    agent_type: str
    status: HookInstallationStatus
    hook_type: str = "unknown"
    message: str = ""
    error: Optional[str] = None
    timestamp: datetime = field(default_factory=lambda: datetime.now(UTC))

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary"""
        return {
            "agent_type": self.agent_type,
            "status": self.status.value,
            "hook_type": self.hook_type,
            "message": self.message,
            "error": self.error,
            "timestamp": self.timestamp.isoformat()
        }


@dataclass
class HookStatus:
    """Status of an installed hook"""
    agent_type: str
    installed: bool
    monitoring: bool
    hook_type: str
    last_extraction: Optional[datetime] = None
    extraction_count: int = 0
    error_count: int = 0
    last_error: Optional[str] = None


class HooksManager:
    """
    Orchestrates agent hooks for automated memory extraction

    Manages the complete lifecycle of agent hooks:
    1. Installation of native hooks for specific agent types
    2. Session monitoring as secondary detection layer
    3. Inactivity detection as tertiary fallback
    4. Coordinated extraction and storage to Nexus

    Usage:
        hooks_mgr = HooksManager(nexus_manager)
        await hooks_mgr.initialize()

        # Install hooks for specific agent
        result = await hooks_mgr.install_hooks("claude-code")

        # Or install for all agents
        results = await hooks_mgr.install_all_hooks()

        # Get status
        status = await hooks_mgr.get_hooks_status()
    """

    # Default configuration
    DEFAULT_INACTIVITY_THRESHOLD_MINUTES = 5
    DEFAULT_MONITORING_INTERVAL_SECONDS = 5

    def __init__(self, nexus_manager):
        """
        Initialize HooksManager

        Args:
            nexus_manager: NexusManager instance for memory storage
        """
        self.nexus = nexus_manager
        self._initialized = False

        # Hook registry
        self._hooks: Dict[str, Any] = {}  # agent_type -> AgentHook
        self._detectors: Dict[str, SessionDetector] = {}  # agent_type -> SessionDetector

        # Monitoring layers
        self._session_monitor: Optional[SessionMonitor] = None
        self._inactivity_detector: Optional[InactivityDetector] = None
        self._monitoring_task: Optional[asyncio.Task] = None
        self._monitoring = False

        # Status tracking
        self._hook_status: Dict[str, HookStatus] = {}
        self._extraction_stats: Dict[str, Dict[str, Any]] = {}

        # Configuration
        self._inactivity_threshold_minutes = self.DEFAULT_INACTIVITY_THRESHOLD_MINUTES
        self._buffer_dir: Optional[Path] = None
        self._auto_extraction_enabled = True

    async def initialize(
        self,
        inactivity_threshold_minutes: int = DEFAULT_INACTIVITY_THRESHOLD_MINUTES,
        buffer_dir: Optional[Path] = None,
        auto_extraction_enabled: bool = True
    ):
        """
        Initialize the hooks manager

        Args:
            inactivity_threshold_minutes: Minutes before inactivity triggers extraction
            buffer_dir: Directory for persistent buffers
            auto_extraction_enabled: Enable/disable automated extraction
        """
        if self._initialized:
            return

        try:
            self._inactivity_threshold_minutes = inactivity_threshold_minutes
            self._buffer_dir = buffer_dir or Path.home() / ".nexus-memory-system" / "buffers"
            self._auto_extraction_enabled = auto_extraction_enabled

            # Ensure buffer directory exists
            if self._buffer_dir:
                self._buffer_dir.mkdir(parents=True, exist_ok=True)

            # Initialize monitoring layers
            self._inactivity_detector = InactivityDetector(
                threshold_minutes=self._inactivity_threshold_minutes
            )

            self._initialized = True
            logger.info("HooksManager initialized successfully")

        except Exception as e:
            logger.error(f"Failed to initialize HooksManager: {e}")
            raise

    async def ensure_initialized(self):
        """Ensure the manager is initialized"""
        if not self._initialized:
            await self.initialize()

    async def close(self):
        """Stop all monitoring and cleanup resources"""
        logger.info("Closing HooksManager...")

        # Stop monitoring
        await self.stop_monitoring()

        # Stop all detectors
        for detector in self._detectors.values():
            detector.stop_monitoring()

        self._detectors.clear()
        self._hooks.clear()
        self._initialized = False

        logger.info("HooksManager closed")

    # Hook Installation

    async def install_hooks(
        self,
        agent_type: str,
        enable_monitoring: bool = True
    ) -> HookInstallationResult:
        """
        Install hooks for a specific agent type

        Args:
            agent_type: Type of agent (claude-code, gemini, qwen, etc.)
            enable_monitoring: Enable session monitoring for this agent

        Returns:
            HookInstallationResult with installation status
        """
        await self.ensure_initialized()

        if not self._auto_extraction_enabled:
            return HookInstallationResult(
                agent_type=agent_type,
                status=HookInstallationStatus.DISABLED,
                message="Automated extraction is disabled"
            )

        try:
            # Check if already installed
            if agent_type in self._hooks:
                return HookInstallationResult(
                    agent_type=agent_type,
                    status=HookInstallationStatus.ALREADY_INSTALLED,
                    message=f"Hooks already installed for {agent_type}",
                    hook_type=type(self._hooks[agent_type]).__name__
                )

            # Get hook info
            hook_info = get_hook_info(agent_type)

            # Create native hook
            hook = create_native_hook(agent_type)
            self._hooks[agent_type] = hook

            # Create session detector
            detector = SessionDetector(agent_type, hook, self.nexus)
            await detector.install_automated_extraction()
            self._detectors[agent_type] = detector

            # Initialize status
            self._hook_status[agent_type] = HookStatus(
                agent_type=agent_type,
                installed=True,
                monitoring=False,
                hook_type=hook_info["hook_type"] if hook_info["supported"] else "generic"
            )

            self._extraction_stats[agent_type] = {
                "total_extractions": 0,
                "successful_extractions": 0,
                "failed_extractions": 0,
                "last_extraction_time": None,
                "sources": {}
            }

            logger.info(f"Installed hooks for {agent_type} ({hook_info['hook_type']})")

            # Start monitoring if requested and not already running
            if enable_monitoring and not self._monitoring:
                await self.start_monitoring()

            return HookInstallationResult(
                agent_type=agent_type,
                status=HookInstallationStatus.SUCCESS,
                message=f"Successfully installed hooks for {agent_type}",
                hook_type=hook_info["hook_type"]
            )

        except Exception as e:
            logger.error(f"Failed to install hooks for {agent_type}: {e}")

            # Cleanup on failure
            if agent_type in self._hooks:
                del self._hooks[agent_type]
            if agent_type in self._detectors:
                del self._detectors[agent_type]
            if agent_type in self._hook_status:
                del self._hook_status[agent_type]

            return HookInstallationResult(
                agent_type=agent_type,
                status=HookInstallationStatus.FAILED,
                message=f"Failed to install hooks",
                error=str(e)
            )

    async def install_all_hooks(
        self,
        agent_types: Optional[List[str]] = None,
        enable_monitoring: bool = True
    ) -> Dict[str, HookInstallationResult]:
        """
        Install hooks for multiple or all supported agents

        Args:
            agent_types: List of agent types, or None for all supported
            enable_monitoring: Enable session monitoring

        Returns:
            Dictionary of agent_type -> HookInstallationResult
        """
        await self.ensure_initialized()

        if agent_types is None:
            agent_types = list_supported_agents()

        results = {}
        for agent_type in agent_types:
            result = await self.install_hooks(agent_type, enable_monitoring=False)
            results[agent_type] = result

        # Start monitoring once after all installations
        if enable_monitoring and any(r.status == HookInstallationStatus.SUCCESS for r in results.values()):
            await self.start_monitoring()

        return results

    async def uninstall_hooks(self, agent_type: str) -> bool:
        """
        Uninstall hooks for an agent type

        Args:
            agent_type: Agent type to uninstall

        Returns:
            True if successfully uninstalled
        """
        try:
            # Stop detector
            if agent_type in self._detectors:
                self._detectors[agent_type].stop_monitoring()
                del self._detectors[agent_type]

            # Remove hook
            if agent_type in self._hooks:
                del self._hooks[agent_type]

            # Remove status
            if agent_type in self._hook_status:
                del self._hook_status[agent_type]

            # Remove stats
            if agent_type in self._extraction_stats:
                del self._extraction_stats[agent_type]

            logger.info(f"Uninstalled hooks for {agent_type}")
            return True

        except Exception as e:
            logger.error(f"Failed to uninstall hooks for {agent_type}: {e}")
            return False

    # Monitoring

    async def start_monitoring(self):
        """Start background monitoring for all installed agents"""
        if self._monitoring:
            logger.warning("Monitoring already started")
            return

        if not self._hooks:
            logger.warning("No hooks installed, cannot start monitoring")
            return

        try:
            self._monitoring = True

            # Start all detectors
            for detector in self._detectors.values():
                await detector.start_monitoring()

            # Start session monitor (secondary layer)
            self._session_monitor = SessionMonitor(self._hooks)

            # Create monitoring task
            self._monitoring_task = asyncio.create_task(self._monitoring_loop())

            logger.info(f"Started monitoring for {len(self._hooks)} agents")

        except Exception as e:
            logger.error(f"Failed to start monitoring: {e}")
            self._monitoring = False
            raise

    async def stop_monitoring(self):
        """Stop background monitoring"""
        if not self._monitoring:
            return

        logger.info("Stopping monitoring...")

        self._monitoring = False

        # Cancel monitoring task
        if self._monitoring_task:
            self._monitoring_task.cancel()
            try:
                await self._monitoring_task
            except asyncio.CancelledError:
                pass
            self._monitoring_task = None

        # Stop session monitor
        if self._session_monitor:
            self._session_monitor.stop_monitoring()
            self._session_monitor = None

        # Stop all detectors
        for detector in self._detectors.values():
            detector.stop_monitoring()

        logger.info("Monitoring stopped")

    async def _monitoring_loop(self):
        """Main monitoring loop"""
        try:
            while self._monitoring:
                # Check all agents via session monitor
                if self._session_monitor:
                    await self._session_monitor._check_all_agents()

                await asyncio.sleep(self.DEFAULT_MONITORING_INTERVAL_SECONDS)

        except asyncio.CancelledError:
            logger.debug("Monitoring loop cancelled")
        except Exception as e:
            logger.error(f"Error in monitoring loop: {e}")

    async def _on_session_end(self, agent_type: str, source: str):
        """
        Callback when session end is detected

        Args:
            agent_type: Type of agent
            source: Detection layer that triggered this
        """
        logger.info(f"Session end detected for {agent_type} (source: {source})")

        # Get detector for this agent
        detector = self._detectors.get(agent_type)
        if not detector:
            logger.warning(f"No detector found for {agent_type}")
            return

        try:
            # Extract and store
            result = await detector.extract_and_store(source)

            # Update stats
            if agent_type in self._extraction_stats:
                stats = self._extraction_stats[agent_type]
                stats["total_extractions"] += 1
                stats["last_extraction_time"] = datetime.now(UTC).isoformat()

                if result.success:
                    stats["successful_extractions"] += 1
                else:
                    stats["failed_extractions"] += 1

                # Track source
                source_key = f"source_{source}"
                stats["sources"][source_key] = stats["sources"].get(source_key, 0) + 1

            # Update status
            if agent_type in self._hook_status:
                status = self._hook_status[agent_type]
                status.last_extraction = datetime.now(UTC)
                status.extraction_count += 1
                if not result.success:
                    status.error_count += 1
                    status.last_error = result.error

            if result.success:
                logger.info(f"Successfully extracted and stored memory for {agent_type}")
            else:
                logger.error(f"Failed to extract memory for {agent_type}: {result.error}")

        except Exception as e:
            logger.error(f"Error handling session end for {agent_type}: {e}")

    # Status and Query Methods

    async def get_hooks_status(self) -> Dict[str, Dict[str, Any]]:
        """
        Get status of all installed hooks

        Returns:
            Dictionary of agent_type -> status dict
        """
        status = {}

        for agent_type, hook_status in self._hook_status.items():
            status[agent_type] = {
                "installed": hook_status.installed,
                "monitoring": hook_status.monitoring,
                "hook_type": hook_status.hook_type,
                "last_extraction": hook_status.last_extraction.isoformat() if hook_status.last_extraction else None,
                "extraction_count": hook_status.extraction_count,
                "error_count": hook_status.error_count,
                "last_error": hook_status.last_error
            }

        return status

    async def get_extraction_stats(self, agent_type: Optional[str] = None) -> Dict[str, Any]:
        """
        Get extraction statistics

        Args:
            agent_type: Specific agent type, or None for all

        Returns:
            Statistics dictionary
        """
        if agent_type:
            return self._extraction_stats.get(agent_type, {})
        return self._extraction_stats

    def is_monitoring(self) -> bool:
        """Check if monitoring is active"""
        return self._monitoring

    def get_installed_agents(self) -> List[str]:
        """Get list of agents with installed hooks"""
        return list(self._hooks.keys())

    # Manual Extraction

    async def trigger_extraction(self, agent_type: str) -> HookResult:
        """
        Manually trigger memory extraction for an agent

        Args:
            agent_type: Agent type to extract from

        Returns:
            HookResult with extraction result
        """
        detector = self._detectors.get(agent_type)
        if not detector:
            return HookResult(
                success=False,
                agent_type=agent_type,
                source="manual",
                error=f"No detector installed for {agent_type}"
            )

        logger.info(f"Manual extraction triggered for {agent_type}")
        return await detector.extract_and_store("manual_trigger")

    async def extract_all_active_sessions(self) -> Dict[str, HookResult]:
        """
        Extract from all currently active sessions

        Returns:
            Dictionary of agent_type -> HookResult
        """
        results = {}
        for agent_type in self._hooks.keys():
            try:
                hook = self._hooks[agent_type]
                if await hook.detect_session_activity():
                    result = await self.trigger_extraction(agent_type)
                    results[agent_type] = result
            except Exception as e:
                logger.error(f"Error extracting from {agent_type}: {e}")
                results[agent_type] = HookResult(
                    success=False,
                    agent_type=agent_type,
                    source="extract_all",
                    error=str(e)
                )
        return results

    # Configuration

    def set_inactivity_threshold(self, minutes: int):
        """Set inactivity threshold in minutes"""
        self._inactivity_threshold_minutes = max(1, minutes)
        if self._inactivity_detector:
            self._inactivity_detector.threshold = timedelta(minutes=minutes)
        logger.info(f"Inactivity threshold set to {minutes} minutes")

    def enable_auto_extraction(self, enabled: bool = True):
        """Enable or disable automated extraction"""
        self._auto_extraction_enabled = enabled
        logger.info(f"Auto extraction {'enabled' if enabled else 'disabled'}")

    def is_auto_extraction_enabled(self) -> bool:
        """Check if auto extraction is enabled"""
        return self._auto_extraction_enabled


# Helper imports for type hints
from datetime import timedelta
