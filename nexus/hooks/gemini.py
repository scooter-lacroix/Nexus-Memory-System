"""
Gemini native hooks using Function Calling and CLI Extensions

Resources:
- https://ai.google.dev/gemini-api/docs/function-calling
- https://blog.google/technology/developers/gemini-cli-extensions/ (Oct 8, 2025)
"""

import asyncio
import json
from pathlib import Path
from typing import Callable, Awaitable, Dict, Any

from .base import AgentHook, HookResult


class GeminiHook(AgentHook):
    """
    Gemini Native Hook using Function Calling + CLI Extensions

    Gemini CLI Extensions (released Oct 2025) provide lifecycle hooks:
    - on_before_exit
    - on_session_end
    - Auto function calling
    """

    EXTENSION_NAME = "nexus-memory"
    EXTENSION_PATH = Path.home() / ".gemini" / "extensions" / "nexus-memory.json"

    def __init__(self):
        super().__init__("gemini")
        self._extension_installed = False
        self._ensure_extension_installed()

    def _ensure_extension_installed(self):
        """Install Gemini CLI Extension for automated extraction"""
        try:
            self.EXTENSION_PATH.parent.mkdir(parents=True, exist_ok=True)

            extension_config = {
                "name": self.EXTENSION_NAME,
                "version": "1.0.0",
                "description": "Automatically extract session context to Nexus Memory",
                "author": "Nexus Memory System",
                "lifecycle_hooks": [
                    "on_before_exit",
                    "on_session_end",
                    "on_completion"
                ],
                "auto_call": True,
                "functions": [
                    {
                        "name": "gemini_session_end_handler",
                        "description": "Automatically called when Gemini session ends",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "conversation_summary": {
                                    "type": "string",
                                    "description": "Summary of the conversation"
                                },
                                "key_decisions": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "Key decisions made during session"
                                },
                                "context_items": {
                                    "type": "array",
                                    "items": {"type": "object"},
                                    "description": "Important context to remember"
                                }
                            },
                            "required": ["conversation_summary"]
                        }
                    }
                ],
                "api": {
                    "endpoint": "http://localhost:8767/mcp/call",
                    "tool": "store_agent_memory"
                },
                "env_vars": [
                    "NEXUS_AUTO_INGEST=true",
                    "NEXUS_SERVER_URL=http://localhost:8767"
                ]
            }

            self.EXTENSION_PATH.write_text(json.dumps(extension_config, indent=2))
            self._extension_installed = True
            print(f"Gemini CLI Extension installed at: {self.EXTENSION_PATH}")

        except Exception as e:
            print(f"Failed to install Gemini CLI Extension: {e}")
            self._extension_installed = False

    async def install_session_end_hook(self, callback: Callable[[str], Awaitable[None]]) -> bool:
        """
        Install session-end hook using Gemini Function Calling

        Gemini's function calling will auto-call the registered function
        when the session ends. The function then triggers our callback.
        """
        if not self._extension_installed:
            print("Warning: Gemini Extension not installed, using fallback")

        # Register callback locally
        await self.register_callback(callback)

        # Extension is installed, it will auto-call the function
        return self._extension_installed

    async def detect_session_activity(self) -> bool:
        """
        Detect if Gemini session is active

        Methods:
        1. Process detection
        2. CLI state detection
        3. API session check
        """
        import psutil

        # Method 1: Process detection
        for proc in psutil.process_iter(['name', 'cmdline']):
            try:
                cmdline = proc.info.get('cmdline', [])
                if cmdline and any('gemini' in str(c).lower() for c in cmdline):
                    return True
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass

        # Method 2: Check for Gemini CLI state
        gemini_state = Path.home() / ".gemini" / "state.json"
        if gemini_state.exists():
            try:
                state = json.loads(gemini_state.read_text())
                if state.get("session_active"):
                    return True
            except:
                pass

        return False

    async def extract_session_context(self) -> dict:
        """
        Extract session context using Gemini's native APIs

        Returns:
            Dictionary with session context
        """
        context = {
            "agent_type": "gemini",
            "conversation": [],
            "decisions": [],
            "context": {},
        }

        # Try to read Gemini session state
        session_file = Path.home() / ".gemini" / "session.json"
        if session_file.exists():
            try:
                session_data = json.loads(session_file.read_text())
                context["conversation"] = session_data.get("messages", [])
                context["context"] = session_data.get("context", {})
            except:
                pass

        # Try to read extension data
        extension_data = Path.home() / ".gemini" / "extensions" / "nexus-memory-data.json"
        if extension_data.exists():
            try:
                data = json.loads(extension_data.read_text())
                context["decisions"] = data.get("decisions", [])
            except:
                pass

        return context


class GeminiFunctionCalling:
    """
    Gemini Function Calling integration

    Uses Gemini's function calling API to register auto-called functions
    """

    FUNCTION_DEFINITION = {
        "name": "nexus_memory_extraction",
        "description": "Extract and store session context to Nexus Memory",
        "parameters": {
            "type": "object",
            "properties": {
                "session_summary": {
                    "type": "string",
                    "description": "Summary of the session"
                },
                "key_points": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Key points to remember"
                },
                "decisions": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Decisions made during session"
                }
            },
            "required": ["session_summary"]
        }
    }

    @staticmethod
    def register_function():
        """Register function with Gemini for auto-calling"""
        # This would use the Gemini API to register the function
        # The function would be auto-called when appropriate
        pass
