"""
Factory for creating agent-specific native hooks
"""

from typing import Dict

from .base import AgentHook
from .claude import ClaudeCodeHook
from .gemini import GeminiHook
from .qwen import QwenHook
from .cli import CLIHook, OpenCodeHook, CodexHook, AmpHook, DroidHook
from .pi_mono import PiMonoHook
from .oh_my_pi import OhMyPiHook
from .iflow import IFlowHook


# Agent hook registry
HOOK_REGISTRY: Dict[str, type] = {
    # Original hooks
    "claude-code": ClaudeCodeHook,
    "claude": ClaudeCodeHook,  # Alias
    "gemini": GeminiHook,
    "qwen": QwenHook,
    "opencode": OpenCodeHook,
    "codex": CodexHook,
    "amp": AmpHook,
    "droid": DroidHook,
    # New hooks for pi-mono, oh-my-pi, iflow
    "pi-mono": PiMonoHook,
    "pimono": PiMonoHook,  # Alias
    "pi": PiMonoHook,  # Alias
    "oh-my-pi": OhMyPiHook,
    "omp": OhMyPiHook,  # Alias
    "ohmypi": OhMyPiHook,  # Alias
    "iflow": IFlowHook,
    "i-flow": IFlowHook,  # Alias
}


def create_native_hook(agent_type: str) -> AgentHook:
    """
    Create appropriate native hook for agent type

    Args:
        agent_type: Type of agent (claude-code, gemini, qwen, pi-mono, omp, iflow, etc.)

    Returns:
        AgentHook instance appropriate for the agent type

    Raises:
        ValueError: If agent_type is not supported
    """
    agent_type_lower = agent_type.lower()

    if agent_type_lower not in HOOK_REGISTRY:
        # Fall back to generic CLI hook
        return CLIHook(agent_type)

    hook_class = HOOK_REGISTRY[agent_type_lower]
    return hook_class()


def register_hook(agent_type: str, hook_class: type):
    """
    Register a custom hook for an agent type

    Args:
        agent_type: Type of agent
        hook_class: Hook class (must inherit from AgentHook)
    """
    if not issubclass(hook_class, AgentHook):
        raise ValueError(f"Hook class must inherit from AgentHook")

    HOOK_REGISTRY[agent_type.lower()] = hook_class


def list_supported_agents() -> list:
    """
    List all supported agent types

    Returns:
        List of agent type strings
    """
    return list(HOOK_REGISTRY.keys())


def get_hook_info(agent_type: str) -> dict:
    """
    Get information about an agent's hook

    Args:
        agent_type: Type of agent

    Returns:
        Dictionary with hook information
    """
    agent_type_lower = agent_type.lower()

    if agent_type_lower not in HOOK_REGISTRY:
        return {
            "agent_type": agent_type,
            "supported": False,
            "hook_type": "generic_cli",
        }

    hook_class = HOOK_REGISTRY[agent_type_lower]

    return {
        "agent_type": agent_type,
        "supported": True,
        "hook_type": hook_class.__name__,
        "class": hook_class,
    }
