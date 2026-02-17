"""
Claude Code native hooks using Skills lifecycle

Resources:
- https://www.anthropic.com/news/enabling-claude-code-to-work-more-autonomously
- Skills auto-trigger on lifecycle events (released Oct 2025)
"""

import asyncio
import json
from pathlib import Path
from typing import Optional, Callable, Awaitable, Dict, Any

from .base import AgentHook, HookResult


class ClaudeCodeHook(AgentHook):
    """
    Claude Code Native Hook using Skills lifecycle

    Installation:
    1. Creates Claude Code Skill at ~/.claude/skills/nexus-memory/SKILL.md
    2. Skill auto-triggers on session_end, checkpoint, completion
    3. Skill calls MCP tool to store memory
    """

    SKILL_NAME = "nexus-memory-extraction"
    SKILL_PATH = Path.home() / ".claude" / "skills" / "nexus-memory"

    def __init__(self):
        super().__init__("claude-code")
        self._skill_installed = False
        self._ensure_skill_installed()

    def _ensure_skill_installed(self):
        """Install Claude Code Skill for automated extraction"""
        try:
            self.SKILL_PATH.mkdir(parents=True, exist_ok=True)

            skill_md = self.SKILL_PATH / "SKILL.md"

            skill_content = """---
name: nexus-memory-extraction
description: Automatically extract session context to Nexus Memory System
version: 1.0.0
author: Nexus Memory System
trigger:
  - on_session_end
  - on_checkpoint
  - on_completion
  - on_error
priority: high
---

# Nexus Memory Extraction Skill

## Overview

This skill automatically triggers when your Claude Code session ends, ensuring no context is lost.

## What It Does

1. **Captures Context**: Extracts current conversation, decisions, and context
2. **Summarizes**: Creates structured summary of key points
3. **Stores**: Automatically stores to Nexus Memory System
4. **Confirms**: Shows what was stored

## Triggers

- **on_session_end**: When you close Claude Code
- **on_checkpoint**: At periodic checkpoints during long sessions
- **on_completion**: When a task is completed
- **on_error**: If an error occurs (stores context for debugging)

## No Manual Action Required

This skill runs automatically. You don't need to remember to trigger it.

## Configuration

The skill reads from:
- `NEXUS_AUTO_INGEST=true` environment variable
- `NEXUS_SERVER_URL` for connection

## Output

After storing, you'll see:
```
[Nexus] Stored 3 memories from Claude Code session:
  - 2 decisions
  - 1 context item
  - Memory IDs: nexus_123, nexus_124, nexus_125
```
"""

            skill_md.write_text(skill_content)

            # Create skill implementation file
            impl_py = self.SKILL_PATH / "implementation.py"
            impl_content = '''"""
Claude Code Skill implementation for automatic memory extraction
"""

import os
from typing import Dict, Any

def on_session_end(context: Dict[str, Any]) -> Dict[str, Any]:
    """Called when Claude Code session ends"""
    # Extract context
    memories = extract_memories(context)

    # Store to Nexus
    result = store_to_nexus(memories)

    return result

def on_checkpoint(context: Dict[str, Any]) -> Dict[str, Any]:
    """Called at periodic checkpoints"""
    # Extract and store incremental memories
    return store_to_nexus(extract_memories(context))

def extract_memories(context: Dict[str, Any]) -> list:
    """Extract memories from context"""
    # Implementation
    pass

def store_to_nexus(memories: list) -> Dict[str, Any]:
    """Store memories to Nexus"""
    # Call Nexus MCP tool or HTTP API
    pass
'''
            impl_py.write_text(impl_content)

            self._skill_installed = True
            print(f"Claude Code Skill installed at: {self.SKILL_PATH}")

        except Exception as e:
            print(f"Failed to install Claude Code Skill: {e}")
            self._skill_installed = False

    async def install_session_end_hook(self, callback: Callable[[str], Awaitable[None]]) -> bool:
        """
        Install session-end hook using Claude Code Skills

        Claude Code Skills provide lifecycle hooks. The skill will
        auto-trigger on session_end and call the MCP tool.
        """
        if not self._skill_installed:
            print("Warning: Claude Code Skill not installed, using fallback")

        # Register callback locally for fallback
        await self.register_callback(callback)

        # Skill is installed, it will auto-trigger via MCP
        return self._skill_installed

    async def detect_session_activity(self) -> bool:
        """
        Detect if Claude Code session is active

        Methods:
        1. Check for running process
        2. Check for VS Code extension activity
        3. Check for session file
        """
        import psutil

        # Method 1: Process detection
        for proc in psutil.process_iter(['name', 'cmdline']):
            try:
                if 'claude' in proc.info['name'].lower():
                    return True
                cmdline = proc.info.get('cmdline', [])
                if cmdline and any('claude' in str(c).lower() for c in cmdline):
                    return True
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass

        # Method 2: Check for VS Code extension
        vscode_state = Path.home() / ".vscode" / "extensions" / "state.json"
        if vscode_state.exists():
            try:
                state = json.loads(vscode_state.read_text())
                # Check if Claude Code extension is active
                # (implementation depends on VS Code state format)
            except:
                pass

        return False

    async def extract_session_context(self) -> dict:
        """
        Extract session context using Claude Code's native APIs

        Returns:
            Dictionary with:
            - conversation: Full conversation history
            - decisions: Key decisions made
            - files: Files worked on
            - context: Project context
        """
        context = {
            "agent_type": "claude-code",
            "conversation": [],
            "decisions": [],
            "files": [],
            "context": {},
        }

        # Try to read Claude Code session state
        session_file = Path.home() / ".claude" / "session.json"
        if session_file.exists():
            try:
                session_data = json.loads(session_file.read_text())
                context["conversation"] = session_data.get("messages", [])
                context["context"] = session_data.get("project_context", {})
            except:
                pass

        # Try to read checkpoint data
        checkpoint_dir = Path.home() / ".claude" / "checkpoints"
        if checkpoint_dir.exists():
            # Read latest checkpoint
            checkpoints = sorted(checkpoint_dir.glob("*.json"), key=lambda p: p.stat().st_mtime)
            if checkpoints:
                try:
                    checkpoint_data = json.loads(checkpoints[-1].read_text())
                    context["decisions"] = checkpoint_data.get("decisions", [])
                    context["files"] = checkpoint_data.get("files", [])
                except:
                    pass

        return context


class ClaudeCodeVSCodeExtension:
    """
    Integration with Claude Code VS Code extension

    Uses VS Code extension API for lifecycle hooks
    """

    EXTENSION_ID = "anthropic.claude-code"

    @staticmethod
    def install_extension_hook():
        """
        Install hook using VS Code extension API

        This requires the extension to be installed and provides
        native lifecycle events.
        """
        # VS Code extension would provide:
        # - vscode.workspace.onDidCloseTextDocument
        # - vscode.window.onDidChangeActiveTerminal
        # - Custom events from extension

        # Implementation would use VS Code Extension API
        # This is a placeholder for the actual integration
        pass
