"""
Generic CLI hooks for OpenCode, Codex, Amp, Droid

Uses Python atexit + signal handlers for exit detection
"""

import atexit
import signal
import asyncio
import psutil
from typing import Callable, Awaitable, Dict, Any, List
from pathlib import Path

from .base import AgentHook, HookResult


class CLIHook(AgentHook):
    """
    Generic CLI Hook for OpenCode, Codex, Amp, Droid

    Uses Python atexit + signal handlers for exit detection.
    Works for any CLI-based agent.
    """

    def __init__(self, agent_name: str):
        super().__init__(agent_name)
        self._callbacks = []
        self._installed = False
        self._previous_state = {}

    async def install_session_end_hook(self, callback: Callable[[str], Awaitable[None]]) -> bool:
        """
        Install atexit + signal handlers for exit detection

        Multiple detection methods:
        1. atexit handler (normal exit)
        2. SIGTERM handler (termination signal)
        3. SIGINT handler (interrupt signal)
        4. Process monitoring (background thread)
        """
        if self._installed:
            await self.register_callback(callback)
            return True

        # Register callback
        self._callbacks.append(callback)

        # Register atexit handler (synchronous wrapper)
        def sync_exit_handler():
            """Synchronous wrapper for async callback"""
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(callback(f"{self.agent_type}_atexit"))
            finally:
                loop.close()

        atexit.register(sync_exit_handler)

        # Register signal handlers
        def signal_handler(signum, frame):
            """Handle signals synchronously"""
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            try:
                loop.run_until_complete(callback(f"{self.agent_type}_signal_{signum}"))
            finally:
                loop.close()
            # Re-raise signal for default handling
            signal.signal(signum, signal.SIG_DFL)
            signal.raise_signal(signum)

        signal.signal(signal.SIGTERM, signal_handler)
        signal.signal(signal.SIGINT, signal_handler)

        # Start process monitoring (background)
        self._start_process_monitoring()

        self._installed = True
        print(f"CLI hooks installed for {self.agent_type}")
        return True

    def _start_process_monitoring(self):
        """
        Start background process monitoring

        Monitors for unexpected process termination
        """
        import threading

        def monitor_process():
            """Monitor process in background thread"""
            import time
            was_active = False

            while True:
                try:
                    is_active = self.detect_session_activity()

                    # Detect state change: active -> inactive
                    if was_active and not is_active:
                        # Process ended unexpectedly
                        print(f"Detected {self.agent_type} process termination")
                        # Trigger callbacks
                        for callback in self._callbacks:
                            try:
                                loop = asyncio.new_event_loop()
                                asyncio.set_event_loop(loop)
                                loop.run_until_complete(
                                    callback(f"{self.agent_type}_process_monitor")
                                )
                                loop.close()
                            except Exception as e:
                                print(f"Process monitor callback error: {e}")

                    was_active = is_active
                    time.sleep(2)  # Check every 2 seconds

                except Exception as e:
                    print(f"Process monitor error: {e}")
                    time.sleep(5)

        monitor_thread = threading.Thread(target=monitor_process, daemon=True)
        monitor_thread.start()

    async def detect_session_activity(self) -> bool:
        """
        Detect if agent process is running

        Methods:
        1. Process name matching
        2. Command line argument matching
        3. Working directory check
        """
        # Method 1: Process name
        for proc in psutil.process_iter(['name', 'cmdline', 'cwd']):
            try:
                # Check process name
                if self.agent_type in proc.info['name'].lower():
                    return True

                # Check command line
                cmdline = proc.info.get('cmdline', [])
                if cmdline and any(self.agent_type in str(c).lower() for c in cmdline):
                    return True

                # Check working directory
                cwd = proc.info.get('cwd')
                if cwd and self.agent_type.lower() in cwd.lower():
                    return True

            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass

        return False

    async def extract_session_context(self) -> dict:
        """
        Extract session context for CLI-based agents

        Methods:
        1. Check for session files in known locations
        2. Read recent command history
        3. Check for temp/output files
        """
        context = {
            "agent_type": self.agent_type,
            "conversation": [],
            "decisions": [],
            "context": {},
        }

        # Check for agent-specific session files
        potential_session_paths = [
            Path.home() / f".{self.agent_type}" / "session.json",
            Path.home() / f".{self.agent_type}" / "state.json",
            Path.cwd() / f".{self.agent_type}" / "session.json",
            Path("/tmp") / f"{self.agent_type}_session.json",
        ]

        for session_path in potential_session_paths:
            if session_path.exists():
                try:
                    session_data = json.loads(session_path.read_text())
                    context.update(session_data)
                    break
                except:
                    pass

        # Check for recent command history
        history_file = Path.home() / f".{self.agent_type}_history"
        if history_file.exists():
            try:
                history = history_file.read_text().split('\n')
                context["recent_commands"] = history[-10:]  # Last 10 commands
            except:
                pass

        return context


class OpenCodeHook(CLIHook):
    """
    OpenCode-specific hook

    OpenCode is a high-concurrency API specialist.
    May have specific API state files.
    """

    def __init__(self):
        super().__init__("opencode")

    async def extract_session_context(self) -> dict:
        """Extract OpenCode-specific context"""
        context = await super().extract_session_context()

        # OpenCode specific: API endpoints, concurrency patterns
        api_state = Path.cwd() / ".opencode" / "api_state.json"
        if api_state.exists():
            try:
                api_data = json.loads(api_state.read_text())
                context["api_endpoints"] = api_data.get("endpoints", [])
                context["concurrency_patterns"] = api_data.get("patterns", [])
            except:
                pass

        return context


class CodexHook(CLIHook):
    """
    Codex-specific hook

    Codex is a code review and modularity expert.
    May have review state files.
    """

    def __init__(self):
        super().__init__("codex")

    async def extract_session_context(self) -> dict:
        """Extract Codex-specific context"""
        context = await super().extract_session_context()

        # Codex specific: Review states, modular patterns
        review_state = Path.cwd() / ".codex" / "reviews.json"
        if review_state.exists():
            try:
                review_data = json.loads(review_state.read_text())
                context["recent_reviews"] = review_data.get("reviews", [])
                context["patterns"] = review_data.get("patterns", [])
            except:
                pass

        return context


class AmpHook(CLIHook):
    """
    Amp-specific hook

    Amp is an ETL/ELT data pipeline specialist.
    May have pipeline state files.
    """

    def __init__(self):
        super().__init__("amp")

    async def extract_session_context(self) -> dict:
        """Extract Amp-specific context"""
        context = await super().extract_session_context()

        # Amp specific: Pipeline states, DAG configs
        pipeline_state = Path.cwd() / ".amp" / "pipelines.json"
        if pipeline_state.exists():
            try:
                pipeline_data = json.loads(pipeline_state.read_text())
                context["pipelines"] = pipeline_data.get("pipelines", [])
                context["dags"] = pipeline_data.get("dags", [])
            except:
                pass

        return context


class DroidHook(CLIHook):
    """
    Droid-specific hook

    Droid is a universal task automation agent.
    May have task/specification files.
    """

    def __init__(self):
        super().__init__("droid")

    async def extract_session_context(self) -> dict:
        """Extract Droid-specific context"""
        context = await super().extract_session_context()

        # Droid specific: Task states, specifications
        task_state = Path.cwd() / ".droid" / "tasks.json"
        if task_state.exists():
            try:
                task_data = json.loads(task_state.read_text())
                context["tasks"] = task_data.get("tasks", [])
                context["specifications"] = task_data.get("specs", [])
            except:
                pass

        # Check for spec files
        spec_dir = Path.cwd() / ".droid" / "specs"
        if spec_dir.exists():
            context["spec_files"] = [str(f) for f in spec_dir.glob("*.json")]

        return context
