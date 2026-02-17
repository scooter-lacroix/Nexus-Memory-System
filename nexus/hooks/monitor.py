"""
Session monitor - SECONDARY LAYER of detection

Monitors agent processes and detects inactivity/termination.
Acts as fallback if native hooks fail.
"""

import asyncio
import psutil
import time
from typing import Callable, Awaitable, Dict, Set
from datetime import datetime, timedelta

from .base import AgentHook


class SessionMonitor:
    """
    SECONDARY LAYER: Session monitor as fallback

    Monitors agent processes and detects:
    - Process termination
    - Inactivity timeout
    - Session state changes

    Runs in background, checking every N seconds.
    """

    def __init__(self, agent_hooks: Dict[str, AgentHook]):
        """
        Initialize session monitor

        Args:
            agent_hooks: Dictionary of agent_type -> AgentHook
        """
        self.agent_hooks = agent_hooks
        self._monitoring = False
        self._previous_states: Dict[str, bool] = {}
        self._inactivity_timers: Dict[str, datetime] = {}
        self._inactivity_threshold = timedelta(minutes=5)  # 5 minutes

    async def start_monitoring(self, callback: Callable[[str, str], Awaitable[None]]):
        """
        Start background session monitoring

        Args:
            callback: Function to call when session ends (agent_type, source)
        """
        self._monitoring = True
        self._callback = callback

        # Initialize previous states
        for agent_type in self.agent_hooks:
            self._previous_states[agent_type] = False

        print("Session monitor started")

        # Monitor loop
        while self._monitoring:
            await self._check_all_agents()
            await asyncio.sleep(5)  # Check every 5 seconds

    async def _check_all_agents(self):
        """Check all agents for state changes"""
        for agent_type, hook in self.agent_hooks.items():
            try:
                is_active = await hook.detect_session_activity()
                was_active = self._previous_states.get(agent_type, False)

                # Detect state change: active -> inactive
                if was_active and not is_active:
                    print(f"SessionMonitor: Detected {agent_type} session end")
                    await self._callback(agent_type, "session_monitor")

                # Update inactivity tracking
                if is_active:
                    self._inactivity_timers[agent_type] = datetime.now()
                elif agent_type in self._inactivity_timers:
                    # Check inactivity timeout
                    inactive_duration = datetime.now() - self._inactivity_timers[agent_type]
                    if inactive_duration > self._inactivity_threshold:
                        print(f"SessionMonitor: {agent_type} inactivity timeout")
                        await self._callback(agent_type, "inactivity_timeout")
                        del self._inactivity_timers[agent_type]

                # Update state
                self._previous_states[agent_type] = is_active

            except Exception as e:
                print(f"SessionMonitor error checking {agent_type}: {e}")

    def stop_monitoring(self):
        """Stop session monitoring"""
        self._monitoring = False
        print("Session monitor stopped")


class ProcessMonitor:
    """
    Detailed process monitoring

    Monitors specific processes with detailed information
    """

    def __init__(self):
        self._tracked_pids: Set[int] = set()

    def track_process(self, pid: int):
        """Start tracking a process"""
        self._tracked_pids.add(pid)

    def untrack_process(self, pid: int):
        """Stop tracking a process"""
        self._tracked_pids.discard(pid)

    def is_process_alive(self, pid: int) -> bool:
        """Check if tracked process is still alive"""
        try:
            proc = psutil.Process(pid)
            return proc.is_running()
        except psutil.NoSuchProcess:
            return False

    def get_process_info(self, pid: int) -> dict:
        """Get detailed process information"""
        try:
            proc = psutil.Process(pid)
            return {
                "pid": pid,
                "name": proc.name(),
                "status": proc.status(),
                "create_time": proc.create_time(),
                "cpu_percent": proc.cpu_percent(),
                "memory_info": proc.memory_info()._asdict(),
                "cmdline": proc.cmdline(),
                "cwd": proc.cwd(),
            }
        except psutil.NoSuchProcess:
            return {"pid": pid, "status": "not_found"}

    def monitor_processes(self, callback: Callable[[int, str], None]):
        """
        Monitor tracked processes and call callback on termination

        Args:
            callback: Function to call when process ends (pid, reason)
        """
        for pid in list(self._tracked_pids):
            if not self.is_process_alive(pid):
                self.untrack_process(pid)
                callback(pid, "process_terminated")


class InactivityDetector:
    """
    TERTIARY LAYER: Inactivity timeout detection

    Detects when an agent session has been inactive for too long.
    """

    def __init__(self, threshold_minutes: int = 5):
        """
        Initialize inactivity detector

        Args:
            threshold_minutes: Minutes of inactivity before triggering
        """
        self.threshold = timedelta(minutes=threshold_minutes)
        self._last_activity: Dict[str, datetime] = {}

    def record_activity(self, agent_type: str):
        """Record activity for an agent"""
        self._last_activity[agent_type] = datetime.now()

    def check_inactive(self, agent_type: str) -> bool:
        """
        Check if agent has been inactive

        Returns:
            True if inactive beyond threshold
        """
        if agent_type not in self._last_activity:
            return False

        inactive_duration = datetime.now() - self._last_activity[agent_type]
        return inactive_duration > self.threshold

    def get_inactive_duration(self, agent_type: str) -> timedelta:
        """Get how long agent has been inactive"""
        if agent_type not in self._last_activity:
            return timedelta(0)

        return datetime.now() - self._last_activity[agent_type]

    async def start_monitoring(
        self,
        agent_hooks: Dict[str, AgentHook],
        callback: Callable[[str, str], Awaitable[None]]
    ):
        """
        Start inactivity monitoring

        Args:
            agent_hooks: Dictionary of agent hooks
            callback: Function to call when inactivity detected (agent_type, "inactivity")
        """
        while True:
            for agent_type, hook in agent_hooks.items():
                # Check if agent is active
                is_active = await hook.detect_session_activity()

                if is_active:
                    # Record activity
                    self.record_activity(agent_type)
                elif self.check_inactive(agent_type):
                    # Inactivity timeout
                    print(f"InactivityDetector: {agent_type} inactive for {self.get_inactive_duration(agent_type)}")
                    await callback(agent_type, "inactivity_timeout")

            await asyncio.sleep(30)  # Check every 30 seconds
