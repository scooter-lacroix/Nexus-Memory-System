"""
Agent namespace mapping for Nexus Memory System
"""

from typing import Dict

# Agent namespace mapping
# Each agent type gets its own isolated namespace for memory storage
AGENT_NAMESPACES: Dict[str, str] = {
    # Development agents
    'claude-code': 'claude_code_memory',
    'gemini': 'gemini_agent_memory',
    'qwen': 'qwen_memory',

    # Specialized agents
    'amp': 'amp_memory',
    'droid': 'droid_specs_memory',
    'opencode': 'opencode_memory',
    'codex': 'codex_agent_memory',

    # General purpose
    'general': 'general_agent_memory',
    'default': 'general_agent_memory',
}

def get_agent_namespace(agent_type: str) -> str:
    """
    Get the memory namespace for an agent type

    Args:
        agent_type: Type of agent (claude-code, gemini, qwen, etc.)

    Returns:
        Namespace string for the agent type
    """
    return AGENT_NAMESPACES.get(agent_type.lower(), AGENT_NAMESPACES['general'])

def is_supported_agent(agent_type: str) -> bool:
    """
    Check if an agent type is supported

    Args:
        agent_type: Type of agent to check

    Returns:
        True if agent type is supported, False otherwise
    """
    return agent_type.lower() in AGENT_NAMESPACES

def list_supported_agents() -> list[str]:
    """
    Get list of all supported agent types

    Returns:
        List of supported agent type names
    """
    return list(AGENT_NAMESPACES.keys())

def get_agent_description(agent_type: str) -> str:
    """
    Get a human-readable description for an agent type

    Args:
        agent_type: Type of agent

    Returns:
        Description string for the agent
    """
    descriptions = {
        'claude-code': 'Claude Code - Advanced coding and development assistant',
        'gemini': 'Gemini - Google\'s multimodal AI assistant',
        'qwen': 'Qwen - Alibaba\'s large language model',
        'amp': 'AMP - ETL/ELT data pipeline specialist',
        'droid': 'Droid - Universal task automation agent',
        'opencode': 'OpenCode - High-concurrency API specialist',
        'codex': 'Codex - Code review and modularity expert',
        'general': 'General purpose AI assistant',
        'default': 'Default/general AI assistant',
    }

    return descriptions.get(agent_type.lower(), 'Unknown agent type')