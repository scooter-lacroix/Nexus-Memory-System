"""
Native Hooks System for Nexus Memory System

Provides automated memory extraction for all supported agent types
using native agent hooks, with multiple fallback layers for reliability.

Layers:
1. Primary: Native agent hooks (Skills, Functions, SubAgents)
2. Secondary: Process monitoring
3. Tertiary: Inactivity timeout
4. Safety: Persistent buffer for crash recovery

Supported agents:
- claude-code: Claude Code (Anthropic)
- gemini: Google Gemini
- qwen: Qwen (Alibaba)
- opencode, codex, amp, droid: Various coding agents
- pi-mono: Pi-Mono coding agent (TypeScript/Node.js)
- oh-my-pi: Oh-My-Pi (OMP) fork of pi-mono
- iflow: iFlow configuration-based system
"""

from .base import AgentHook
from .claude import ClaudeCodeHook
from .gemini import GeminiHook
from .qwen import QwenHook
from .cli import CLIHook
from .monitor import SessionMonitor
from .buffer import PersistentBuffer
from .detector import SessionDetector
from .factory import create_native_hook, list_supported_agents, register_hook

# New hooks for pi-mono, oh-my-pi, iflow
from .pi_mono import PiMonoHook
from .oh_my_pi import OhMyPiHook
from .iflow import IFlowHook

__all__ = [
    # Base class
    "AgentHook",
    # Original hooks
    "ClaudeCodeHook",
    "GeminiHook",
    "QwenHook",
    "CLIHook",
    # New hooks
    "PiMonoHook",
    "OhMyPiHook",
    "IFlowHook",
    # Utilities
    "SessionMonitor",
    "PersistentBuffer",
    "SessionDetector",
    # Factory functions
    "create_native_hook",
    "list_supported_agents",
    "register_hook",
]
