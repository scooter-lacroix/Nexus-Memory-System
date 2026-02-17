"""
Qwen native hooks using Hooks SubAgent

Resources:
- https://github.com/QwenLM/Qwen-Agent
- Hooks SubAgent (enhanced version available)
- Skills (implemented)
- MCP integration supported
"""

import asyncio
import json
from pathlib import Path
from typing import Callable, Awaitable, Dict, Any

from .base import AgentHook, HookResult


class QwenHook(AgentHook):
    """
    Qwen Native Hook using Hooks SubAgent

    Qwen-Agent framework provides:
    - Hooks SubAgent for lifecycle management
    - Built-in Memory component
    - MCP integration
    """

    def __init__(self):
        super().__init__("qwen")
        self._hook_agent = None
        self._try_initialize_hook_agent()

    def _try_initialize_hook_agent(self):
        """
        Initialize Qwen-Agent Hooks SubAgent

        Requires qwen-agent package to be installed
        """
        try:
            from qwen_agent import Agent

            # Create Hooks SubAgent for memory extraction
            self._hook_agent = Agent(
                role="nexus_memory_extraction_hook",
                hooks=["on_session_end", "on_task_complete", "on_error"],
                description="Extract and store session context to Nexus Memory"
            )

            print("Qwen Hooks SubAgent initialized")
            return True

        except ImportError:
            print("qwen-agent package not installed, using fallback")
            return False
        except Exception as e:
            print(f"Failed to initialize Qwen Hooks SubAgent: {e}")
            return False

    async def install_session_end_hook(self, callback: Callable[[str], Awaitable[None]]) -> bool:
        """
        Install session-end hook using Qwen-Agent's Hooks SubAgent

        The Hooks SubAgent provides lifecycle hooks that auto-trigger.
        """
        if self._hook_agent:
            try:
                # Register lifecycle hook
                self._hook_agent.register_hook("on_session_end", callback)
                self._hook_agent.register_hook("on_task_complete", callback)

                # Register callback locally
                await self.register_callback(callback)

                return True
            except Exception as e:
                print(f"Failed to register Qwen hook: {e}")

        # Fallback: register callback for manual triggering
        await self.register_callback(callback)
        return False

    async def detect_session_activity(self) -> bool:
        """
        Detect if Qwen session is active

        Methods:
        1. Process detection
        2. Qwen-Agent state detection
        3. Agent activity check
        """
        import psutil

        # Method 1: Process detection
        for proc in psutil.process_iter(['name', 'cmdline']):
            try:
                cmdline = proc.info.get('cmdline', [])
                if cmdline and any('qwen' in str(c).lower() for c in cmdline):
                    return True
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass

        # Method 2: Check for Qwen-Agent state
        qwen_state = Path.home() / ".qwen" / "agent_state.json"
        if qwen_state.exists():
            try:
                state = json.loads(qwen_state.read_text())
                if state.get("active"):
                    return True
            except:
                pass

        # Method 3: Check via Hooks SubAgent
        if self._hook_agent:
            try:
                return self._hook_agent.is_active()
            except:
                pass

        return False

    async def extract_session_context(self) -> dict:
        """
        Extract session context using Qwen-Agent's native APIs

        Returns:
            Dictionary with session context
        """
        context = {
            "agent_type": "qwen",
            "conversation": [],
            "decisions": [],
            "context": {},
        }

        # Try to use Qwen-Agent's Memory component
        if self._hook_agent:
            try:
                # Qwen-Agent has built-in memory
                memory_data = self._hook_agent.get_memory()
                context["conversation"] = memory_data.get("history", [])
                context["context"] = memory_data.get("context", {})
            except:
                pass

        # Try to read Qwen session state
        session_file = Path.home() / ".qwen" / "session.json"
        if session_file.exists():
            try:
                session_data = json.loads(session_file.read_text())
                context["decisions"] = session_data.get("decisions", [])
            except:
                pass

        return context


class QwenMCPIntegration:
    """
    Qwen MCP integration

    Qwen-Agent supports MCP for tool access
    """

    @staticmethod
    def configure_mcp_server():
        """
        Configure Nexus as MCP server for Qwen

        Qwen can connect to Nexus via MCP protocol
        """
        mcp_config = {
            "server": "nexus",
            "command": "nexus",
            "args": ["serve", "--transport", "stdio"]
        }

        # This would be added to Qwen's MCP configuration
        return mcp_config
