"""
Base classes for native agent hooks
"""

from abc import ABC, abstractmethod
from typing import Optional, Callable, Awaitable


class AgentHook(ABC):
    """
    Base class for agent-specific native hooks

    Each agent type has unique hook mechanisms. This class defines
    the interface that all agent hooks must implement.
    """

    def __init__(self, agent_type: str):
        self.agent_type = agent_type
        self._callbacks = []

    @abstractmethod
    async def install_session_end_hook(self, callback: Callable[[str], Awaitable[None]]) -> bool:
        """
        Install native session-end detection hook

        Args:
            callback: Async function to call when session ends

        Returns:
            True if hook installed successfully
        """
        pass

    @abstractmethod
    async def detect_session_activity(self) -> bool:
        """
        Detect if agent session is currently active

        Returns:
            True if agent process/session is running
        """
        pass

    @abstractmethod
    async def extract_session_context(self) -> dict:
        """
        Extract current session context using native APIs

        Returns:
            Dictionary containing session context (conversation,
            decisions, files, etc.)
        """
        pass

    async def register_callback(self, callback: Callable[[str], Awaitable[None]]):
        """Register a callback for session-end events"""
        self._callbacks.append(callback)

    async def trigger_callbacks(self, source: str):
        """Trigger all registered callbacks"""
        for callback in self._callbacks:
            try:
                await callback(source)
            except Exception as e:
                # Log but don't fail
                print(f"Callback error in {self.agent_type}: {e}")


class HookResult:
    """Result of a hook operation"""

    def __init__(
        self,
        success: bool,
        agent_type: str,
        source: str,
        context: Optional[dict] = None,
        error: Optional[str] = None
    ):
        self.success = success
        self.agent_type = agent_type
        self.source = source  # Where the hook was triggered (e.g., "native_hook", "process_monitor")
        self.context = context
        self.error = error
        self.timestamp = None  # Will be set when result is created

    def to_dict(self) -> dict:
        """Convert to dictionary"""
        return {
            "success": self.success,
            "agent_type": self.agent_type,
            "source": self.source,
            "context": self.context,
            "error": self.error,
            "timestamp": self.timestamp.isoformat() if self.timestamp else None
        }
