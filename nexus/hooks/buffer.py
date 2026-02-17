"""
Persistent buffer for crash recovery

Acts as safety net to prevent memory loss even if all hooks fail.
Continuously buffers session context for recovery after crashes.
"""

import asyncio
import json
import threading
from datetime import datetime
from pathlib import Path
from typing import Dict, Any, Optional


class PersistentBuffer:
    """
    SAFETY LAYER: Persistent buffer for crash recovery

    Continuously buffers session context. If all other detection
    methods fail, we can recover from this buffer.

    Buffer lifecycle:
    1. Start buffering when session starts
    2. Continuously append context
    3. Periodically flush to disk
    4. Recover after crash
    5. Clear buffer after successful storage
    """

    def __init__(self, buffer_dir: Path = None):
        """
        Initialize persistent buffer

        Args:
            buffer_dir: Directory for buffer files (default: ~/.nexus/buffer)
        """
        self.buffer_dir = buffer_dir or Path.home() / ".nexus" / "buffer"
        self.buffer_dir.mkdir(parents=True, exist_ok=True)

        self._buffers: Dict[str, dict] = {}
        self._locks: Dict[str, threading.Lock] = {}
        self._flush_interval = 10  # Flush every 10 seconds

    def start_buffering(self, agent_type: str):
        """
        Start buffering for an agent

        Args:
            agent_type: Type of agent to buffer
        """
        if agent_type not in self._buffers:
            self._buffers[agent_type] = {
                "started_at": datetime.now().isoformat(),
                "entries": [],
                "last_flush": datetime.now().isoformat(),
            }
            self._locks[agent_type] = threading.Lock()

            # Start periodic flush
            asyncio.create_task(self._periodic_flush(agent_type))

    def buffer_context(self, agent_type: str, context: Any, context_type: str = "general"):
        """
        Buffer context entry

        Args:
            agent_type: Type of agent
            context: Context to buffer (any JSON-serializable type)
            context_type: Type of context (decision, conversation, etc.)
        """
        if agent_type not in self._buffers:
            self.start_buffering(agent_type)

        with self._locks[agent_type]:
            entry = {
                "timestamp": datetime.now().isoformat(),
                "type": context_type,
                "context": context,
            }
            self._buffers[agent_type]["entries"].append(entry)

            # Auto-flush if buffer is large
            if len(self._buffers[agent_type]["entries"]) >= 10:
                self._flush_to_disk(agent_type)

    async def _periodic_flush(self, agent_type: str):
        """
        Periodically flush buffer to disk

        Runs in background
        """
        while agent_type in self._buffers:
            await asyncio.sleep(self._flush_interval)
            self._flush_to_disk(agent_type)

    def _flush_to_disk(self, agent_type: str):
        """
        Flush buffer to disk

        Thread-safe flush to disk
        """
        if agent_type not in self._buffers:
            return

        with self._locks[agent_type]:
            try:
                buffer_file = self.buffer_dir / f"{agent_type}.json"
                buffer_file.write_text(json.dumps(self._buffers[agent_type], indent=2))
                self._buffers[agent_type]["last_flush"] = datetime.now().isoformat()
            except Exception as e:
                print(f"Failed to flush buffer for {agent_type}: {e}")

    def recover_buffer(self, agent_type: str) -> Optional[dict]:
        """
        Recover buffered context after crash

        Args:
            agent_type: Type of agent to recover

        Returns:
            Buffered context or None if no buffer exists
        """
        buffer_file = self.buffer_dir / f"{agent_type}.json"

        if not buffer_file.exists():
            return None

        try:
            buffered_data = json.loads(buffer_file.read_text())
            print(f"Recovered buffer for {agent_type}: {len(buffered_data['entries'])} entries")
            return buffered_data
        except Exception as e:
            print(f"Failed to recover buffer for {agent_type}: {e}")
            return None

    def clear_buffer(self, agent_type: str):
        """
        Clear buffer after successful storage

        Args:
            agent_type: Type of agent
        """
        # Clear from memory
        if agent_type in self._buffers:
            del self._buffers[agent_type]

        if agent_type in self._locks:
            del self._locks[agent_type]

        # Clear from disk
        buffer_file = self.buffer_dir / f"{agent_type}.json"
        if buffer_file.exists():
            try:
                buffer_file.unlink()
            except Exception as e:
                print(f"Failed to clear buffer file for {agent_type}: {e}")

    def get_buffer_status(self, agent_type: str) -> Optional[dict]:
        """
        Get buffer status

        Args:
            agent_type: Type of agent

        Returns:
            Buffer status or None
        """
        if agent_type not in self._buffers:
            return None

        with self._locks[agent_type]:
            return {
                "agent_type": agent_type,
                "started_at": self._buffers[agent_type]["started_at"],
                "entries_count": len(self._buffers[agent_type]["entries"]),
                "last_flush": self._buffers[agent_type]["last_flush"],
            }

    def list_buffers(self) -> list:
        """
        List all active buffers

        Returns:
            List of buffer statuses
        """
        return [
            self.get_buffer_status(agent_type)
            for agent_type in self._buffers
        ]


class BufferEntry:
    """Single buffer entry"""

    def __init__(self, context: Any, context_type: str = "general"):
        self.timestamp = datetime.now().isoformat()
        self.type = context_type
        self.context = context

    def to_dict(self) -> dict:
        """Convert to dictionary"""
        return {
            "timestamp": self.timestamp,
            "type": self.type,
            "context": self.context,
        }
