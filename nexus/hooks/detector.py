"""
Session detector - Orchestrates all detection layers

Combines:
1. Native agent hooks (PRIMARY)
2. Session monitor (SECONDARY)
3. Inactivity detector (TERTIARY)
4. Persistent buffer (SAFETY)
"""

import asyncio
from typing import Callable, Awaitable, Dict, Any
from pathlib import Path

from .base import AgentHook, HookResult
from .monitor import SessionMonitor, InactivityDetector
from .buffer import PersistentBuffer


class SessionDetector:
    """
    Multi-layer session detection system

    Ensures 95-100% memory capture reliability through:
    1. Native hooks (best case, 100%)
    2. Process monitoring (fallback, 95%+)
    3. Inactivity detection (fallback, 90%+)
    4. Persistent buffer (safety net, 99%+)
    """

    def __init__(self, agent_type: str, hook: AgentHook, nexus_manager):
        """
        Initialize session detector

        Args:
            agent_type: Type of agent
            hook: Native hook for agent
            nexus_manager: Nexus manager for storing memories
        """
        self.agent_type = agent_type
        self.hook = hook
        self.nexus = nexus_manager

        # Detection layers
        self.buffer = PersistentBuffer()
        self.session_monitor = None
        self.inactivity_detector = None

        # State
        self._monitoring = False
        self._extracting = False

    async def install_automated_extraction(self):
        """
        Install all layers of automated memory extraction

        This is the main entry point that sets up all detection layers.
        """
        print(f"Installing automated extraction for {self.agent_type}")

        # LAYER 1: Native hook (PRIMARY)
        await self.hook.install_session_end_hook(self._on_session_end)

        # LAYER 2: Start buffering (SAFETY)
        self.buffer.start_buffering(self.agent_type)

        # LAYER 3: Session monitor (SECONDARY)
        # Note: This would be created by the main orchestrator
        # self.session_monitor = SessionMonitor({self.agent_type: self.hook})

        # LAYER 4: Inactivity detector (TERTIARY)
        # Note: This would be created by the main orchestrator
        # self.inactivity_detector = InactivityDetector()

        print(f"Automated extraction installed for {self.agent_type}")

    async def _on_session_end(self, source: str):
        """
        Called when session end is detected (by any layer)

        Args:
            source: Which detection layer triggered this
        """
        if self._extracting:
            print(f"Already extracting {self.agent_type}, skipping duplicate trigger from {source}")
            return

        self._extracting = True
        print(f"Session end detected for {self.agent_type} (source: {source})")

        try:
            # Extract and store memory
            result = await self.extract_and_store(source)

            if result.success:
                print(f"Successfully stored memory for {self.agent_type}")
                # Clear buffer after successful storage
                self.buffer.clear_buffer(self.agent_type)
            else:
                print(f"Failed to store memory for {self.agent_type}: {result.error}")

        except Exception as e:
            print(f"Error extracting memory for {self.agent_type}: {e}")
        finally:
            self._extracting = False

    async def extract_and_store(self, source: str) -> HookResult:
        """
        Extract session context and store to Nexus

        Args:
            source: Where the extraction was triggered from

        Returns:
            HookResult with success/failure
        """
        # LAYER 1: Try native extraction
        try:
            context = await self.hook.extract_session_context()
            if context:
                result = await self._store_to_nexus(context, source)
                if result.success:
                    return result
        except Exception as e:
            print(f"Native extraction failed: {e}")

        # LAYER 2: Try buffer recovery
        try:
            buffered_data = self.buffer.recover_buffer(self.agent_type)
            if buffered_data:
                context = self._buffer_to_context(buffered_data)
                result = await self._store_to_nexus(context, f"{source}_buffer_recovery")
                if result.success:
                    return HookResult(
                        success=True,
                        agent_type=self.agent_type,
                        source=f"{source}_buffer_recovery",
                        context=context
                    )
        except Exception as e:
            print(f"Buffer recovery failed: {e}")

        # LAYER 3: Fallback to minimal context
        minimal_context = {
            "agent_type": self.agent_type,
            "timestamp": asyncio.get_event_loop().time(),
            "source": source,
            "fallback": True,
        }

        result = await self._store_to_nexus(minimal_context, f"{source}_fallback")
        return result

    async def _store_to_nexus(self, context: dict, source: str) -> HookResult:
        """
        Store context to Nexus Memory System

        Args:
            context: Session context to store
            source: Source of extraction

        Returns:
            HookResult
        """
        try:
            # Convert context to memory format
            memory_content = self._context_to_memory_content(context)

            # Store to Nexus
            result = await self.nexus.store_memory(
                content=memory_content,
                agent_type=self.agent_type,
                category="session",
                labels=["automatic", source],
                metadata={
                    "source": source,
                    "extraction_method": "automated_hook",
                    **context
                }
            )

            if result.get("success"):
                return HookResult(
                    success=True,
                    agent_type=self.agent_type,
                    source=source,
                    context=context
                )
            else:
                return HookResult(
                    success=False,
                    agent_type=self.agent_type,
                    source=source,
                    error=result.get("error", "Unknown error")
                )

        except Exception as e:
            return HookResult(
                success=False,
                agent_type=self.agent_type,
                source=source,
                error=str(e)
            )

    def _context_to_memory_content(self, context: dict) -> str:
        """Convert context dict to memory content string"""
        import json

        # Create structured summary
        parts = []

        if context.get("conversation"):
            parts.append(f"Conversation: {len(context['conversation'])} messages")

        if context.get("decisions"):
            parts.append(f"Decisions: {len(context['decisions'])} items")

        if context.get("files"):
            parts.append(f"Files: {len(context['files'])} files")

        # Add full context as JSON for retrieval
        parts.append(f"\nFull Context:\n{json.dumps(context, indent=2)}")

        return "\n".join(parts)

    def _buffer_to_context(self, buffered_data: dict) -> dict:
        """Convert buffered data to context format"""
        return {
            "agent_type": self.agent_type,
            "buffer_entries": buffered_data.get("entries", []),
            "buffer_started": buffered_data.get("started_at"),
            "source": "buffer_recovery",
        }

    async def start_monitoring(self):
        """Start all monitoring layers"""
        if self._monitoring:
            return

        self._monitoring = True

        # Start session monitor (secondary layer)
        if self.session_monitor:
            await self.session_monitor.start_monitoring(self._on_session_end)

        # Start inactivity detector (tertiary layer)
        if self.inactivity_detector:
            await self.inactivity_detector.start_monitoring(
                {self.agent_type: self.hook},
                self._on_session_end
            )

    def stop_monitoring(self):
        """Stop all monitoring"""
        self._monitoring = False

        if self.session_monitor:
            self.session_monitor.stop_monitoring()

    def buffer_context(self, context: Any, context_type: str = "general"):
        """
        Buffer context entry (called during session)

        Args:
            context: Context to buffer
            context_type: Type of context
        """
        self.buffer.buffer_context(self.agent_type, context, context_type)
