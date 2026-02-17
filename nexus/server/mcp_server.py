"""
Main FastMCP server for Nexus Memory System
"""

from typing import Dict, List, Any, Optional
from loguru import logger
from fastmcp import FastMCP

from ..config import config
from ..database import setup_database, get_database_info
from .nexus_manager import NexusManager

# Initialize FastMCP server
mcp = FastMCP("nexus-memory-system")

# Global memory manager instance
_memory_manager: Optional[NexusManager] = None


def get_memory_manager() -> NexusManager:
    """Get or create the global memory manager instance"""
    global _memory_manager
    if _memory_manager is None:
        _memory_manager = NexusManager()
        logger.info("Nexus memory manager initialized")
    return _memory_manager


@mcp.tool()
def initialize_nexus_system() -> Dict[str, Any]:
    """
    Initialize the Nexus memory system

    Returns initialization status and database information
    """
    try:
        # Setup database if needed
        db_setup_success = setup_database()

        if not db_setup_success:
            return {
                "success": False,
                "error": "Failed to setup database"
            }

        # Get database information
        db_info = get_database_info()

        # Initialize memory manager
        manager = get_memory_manager()

        return {
            "success": True,
            "message": "Nexus memory system initialized successfully",
            "database_info": db_info,
            "server_config": {
                "conscious_ingest": config.conscious_ingest,
                "auto_ingest": config.auto_ingest,
                "supported_agents": list(config.AGENT_NAMESPACES.keys()),
                "spec_similarity_threshold": config.spec_similarity_threshold,
                "embeddings_enabled": config.embeddings_enabled,
                "web_enabled": config.is_web_enabled(),
            }
        }

    except Exception as e:
        logger.error(f"Failed to initialize Nexus system: {e}")
        return {
            "success": False,
            "error": str(e)
        }


@mcp.tool()
def search_agent_memory(query: str, agent_type: str = "general", limit: int = 5) -> Dict[str, Any]:
    """
    Search memory for specific agent type with intelligent retrieval

    Args:
        query: Search query to find relevant memories
        agent_type: Type of agent (claude-code, gemini, qwen, amp, droid, opencode, codex, general)
        limit: Maximum number of results to return

    Returns:
        Search results with relevant memories
    """
    try:
        if not query.strip():
            return {
                "success": False,
                "error": "Query cannot be empty"
            }

        manager = get_memory_manager()
        results = manager.search_memories(query, agent_type, limit)

        return results

    except Exception as e:
        logger.error(f"Failed to search agent memory: {e}")
        return {
            "success": False,
            "error": str(e),
            "query": query,
            "agent_type": agent_type
        }


@mcp.tool()
def store_agent_memory(content: str, agent_type: str = "general", category: str = "general",
                      labels: List[str] = None, metadata: Dict[str, Any] = None) -> Dict[str, Any]:
    """
    Store memory in agent-specific namespace with categorization

    Args:
        content: Content to store in memory
        agent_type: Type of agent (claude-code, gemini, qwen, amp, droid, opencode, codex, general)
        category: Memory category (facts, preferences, context, specifications, etc.)
        labels: Optional labels for categorization
        metadata: Additional metadata

    Returns:
        Storage operation result
    """
    try:
        if not content.strip():
            return {
                "success": False,
                "error": "Content cannot be empty"
            }

        manager = get_memory_manager()
        result = manager.store_memory(content, agent_type, category, labels, metadata)

        return result

    except Exception as e:
        logger.error(f"Failed to store agent memory: {e}")
        return {
            "success": False,
            "error": str(e),
            "agent_type": agent_type,
            "category": category
        }


@mcp.tool()
def get_task_specification(task_description: str, agent_type: str = "droid",
                          reuse_existing: bool = True) -> Dict[str, Any]:
    """
    Get reusable task specification, create new if no match found

    Args:
        task_description: Description of the task requiring specification
        agent_type: Agent type (usually droid for specification creation)
        reuse_existing: Whether to search for existing specifications

    Returns:
        Task specification result
    """
    try:
        if not task_description.strip():
            return {
                "success": False,
                "error": "Task description cannot be empty"
            }

        manager = get_memory_manager()

        if reuse_existing:
            # Try to find existing specification
            best_match, alternatives = manager.find_reusable_specification(task_description, agent_type)

            if best_match:
                # Update usage count
                manager.update_spec_usage(best_match['spec_id'])

                return {
                    "success": True,
                    "specification": best_match,
                    "alternatives": alternatives,
                    "reused": True,
                    "message": f"Found reusable specification {best_match['spec_id']}"
                }

        # No existing specification found or reuse not requested
        return {
            "success": True,
            "specification": None,
            "alternatives": [],
            "reused": False,
            "message": "No existing specification found - create new one with Droid",
            "task_description": task_description,
            "suggested_droid_command": f"droid exec --use-spec -r high \"{task_description}\""
        }

    except Exception as e:
        logger.error(f"Failed to get task specification: {e}")
        return {
            "success": False,
            "error": str(e),
            "task_description": task_description,
            "agent_type": agent_type
        }


@mcp.tool()
def store_droid_specification(task_description: str, spec_content: Dict[str, Any],
                            complexity_score: float = 0.5) -> Dict[str, Any]:
    """
    Store a task specification created by Droid

    Args:
        task_description: Original task description
        spec_content: The specification content from Droid
        complexity_score: Complexity score (0.0-1.0)

    Returns:
        Storage result
    """
    try:
        manager = get_memory_manager()
        result = manager.store_task_specification(
            task_description=task_description,
            spec_content=spec_content,
            agent_type="droid",
            complexity_score=complexity_score
        )

        return result

    except Exception as e:
        logger.error(f"Failed to store Droid specification: {e}")
        return {
            "success": False,
            "error": str(e),
            "task_description": task_description
        }


@mcp.tool()
def enhance_context_with_memory(context: str, agent_type: str = "general") -> Dict[str, Any]:
    """
    Enhance current context with relevant memories

    Args:
        context: Current context to enhance
        agent_type: Agent type for memory filtering

    Returns:
        Enhanced context with relevant memories
    """
    try:
        if not context.strip():
            return {
                "success": False,
                "error": "Context cannot be empty"
            }

        manager = get_memory_manager()

        # Search for relevant memories based on context
        search_results = manager.search_memories(context, agent_type, limit=3)

        if search_results["success"] and search_results.get("results"):
            # Create enhanced context
            memory_summary = "\n".join([
                f"- {mem['content'][:200]}..." if len(mem['content']) > 200 else f"- {mem['content']}"
                for mem in search_results["results"]
            ])

            enhanced_context = f"""RELEVANT MEMORY CONTEXT:
{memory_summary}

CURRENT CONTEXT:
{context}

Please use the relevant memory context above to inform your response to the current context."""

            return {
                "success": True,
                "original_context": context,
                "enhanced_context": enhanced_context,
                "memory_results": search_results["results"],
                "agent_type": agent_type
            }
        else:
            return {
                "success": True,
                "original_context": context,
                "enhanced_context": context,
                "memory_results": [],
                "agent_type": agent_type,
                "message": "No relevant memories found"
            }

    except Exception as e:
        logger.error(f"Failed to enhance context with memory: {e}")
        return {
            "success": False,
            "error": str(e),
            "agent_type": agent_type
        }


@mcp.tool()
def get_agent_memory_stats(agent_type: str = None) -> Dict[str, Any]:
    """
    Get memory statistics for an agent or all agents

    Args:
        agent_type: Optional agent type to get stats for

    Returns:
        Memory statistics
    """
    try:
        manager = get_memory_manager()
        stats = manager.get_memory_stats(agent_type)

        return stats

    except Exception as e:
        logger.error(f"Failed to get memory stats: {e}")
        return {
            "success": False,
            "error": str(e),
            "agent_type": agent_type
        }


@mcp.tool()
def list_supported_agents() -> Dict[str, Any]:
    """
    List all supported agent types and their configurations

    Returns:
        List of supported agents with their configurations
    """
    try:
        from ..config.agent_namespaces import list_supported_agents, get_agent_description

        agents_info = {}
        supported_agents = list_supported_agents()

        for agent_type in supported_agents:
            agents_info[agent_type] = {
                "namespace": config.get_agent_namespace(agent_type),
                "supports_plugins": agent_type in ["claude-code", "gemini", "droid"],
                "mcp_compatible": True,
                "description": get_agent_description(agent_type)
            }

        return {
            "success": True,
            "supported_agents": agents_info,
            "total_agents": len(agents_info),
            "server_info": {
                "version": "1.0.0",
                "host": config.host,
                "port": config.port,
                "web_port": config.web_port if config.is_web_enabled() else None,
                "features": [
                    "conscious_ingest" if config.conscious_ingest else None,
                    "auto_ingest" if config.auto_ingest else None,
                    "embeddings" if config.embeddings_enabled else None,
                    "specification_reuse",
                    "cross_agent_memory",
                    "web_dashboard" if config.is_web_enabled() else None,
                ]
            }
        }

    except Exception as e:
        logger.error(f"Failed to list supported agents: {e}")
        return {
            "success": False,
            "error": str(e)
        }


@mcp.tool()
def get_specification_choices(task_description: str) -> Dict[str, Any]:
    """
    Get multiple specification choices for agent selection (conflict resolution)

    Args:
        task_description: Task description to find specifications for

    Returns:
        Multiple specification options for agent to choose from
    """
    try:
        if not task_description.strip():
            return {
                "success": False,
                "error": "Task description cannot be empty"
            }

        manager = get_memory_manager()
        best_match, alternatives = manager.find_reusable_specification(task_description, "droid")

        # Combine all options
        all_options = []

        if best_match:
            all_options.append({
                "spec_id": best_match["spec_id"],
                "description": best_match["task_description"],
                "complexity_score": best_match["complexity_score"],
                "usage_count": best_match["usage_count"],
                "match_type": "best_match",
                "similarity": best_match.get("similarity", 0.0),
                "spec_content": best_match["spec_content"]
            })

        for i, alt in enumerate(alternatives):
            all_options.append({
                "spec_id": alt["spec_id"],
                "description": alt["task_description"],
                "complexity_score": alt["complexity_score"],
                "usage_count": alt["usage_count"],
                "match_type": f"alternative_{i+1}",
                "similarity": alt.get("similarity", 0.0),
                "spec_content": alt["spec_content"]
            })

        return {
            "success": True,
            "task_description": task_description,
            "options": all_options,
            "total_options": len(all_options),
            "message": f"Found {len(all_options)} specification options"
        }

    except Exception as e:
        logger.error(f"Failed to get specification choices: {e}")
        return {
            "success": False,
            "error": str(e),
            "task_description": task_description
        }