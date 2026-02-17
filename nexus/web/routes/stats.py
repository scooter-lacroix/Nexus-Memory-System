"""
Statistics Routes - API endpoints for system statistics and monitoring

Provides endpoints for:
- System statistics (memory counts, categories, etc.)
- Orchestrator status and health
- Performance metrics
- System information
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional

from fastapi import APIRouter, HTTPException, Query, status
from loguru import logger

from ...server.nexus_manager import get_memory_manager
from ...config import config
from ..app import StatsResponse, OrchestratorStatusResponse


router = APIRouter()


# =============================================================================
# Statistics Endpoints
# =============================================================================


@router.get(
    "/stats",
    response_model=StatsResponse,
    summary="Get system statistics",
    description="Retrieve comprehensive system statistics including memory counts, categories, and system info"
)
async def get_stats(
    agent_type: Optional[str] = Query(
        None,
        description="Filter statistics by agent type (null = all agents)"
    )
):
    """
    Get comprehensive system statistics.

    Returns:
    - Total memory count
    - Memories by category
    - System information
    - Performance metrics
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        result = await manager.get_memory_stats(agent_type=agent_type)

        if not result.get("success"):
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail=result.get("error", "Failed to get statistics")
            )

        return StatsResponse(
            success=True,
            total_memories=result.get("total_memories", 0),
            categories=result.get("categories", {}),
            system_info=result.get("system_info"),
            performance_metrics=result.get("performance_metrics")
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting stats: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get statistics: {str(e)}"
        )


@router.get(
    "/stats/summary",
    summary="Get statistics summary",
    description="Get a quick summary of system statistics"
)
async def get_stats_summary():
    """Get a quick summary of system statistics"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        # Get all stats
        result = await manager.get_memory_stats(agent_type=None)

        # Get orchestrator status if available
        orchestrator_stats = {}
        if manager.orchestrator:
            try:
                health = await manager.orchestrator.get_health()
                orchestrator_stats = {
                    "orchestrator_healthy": health.get("healthy", False),
                    "active_sessions": health.get("active_sessions", 0),
                }
            except Exception as e:
                logger.warning(f"Failed to get orchestrator health: {e}")

        # Get hooks status if available
        hooks_stats = {}
        if manager.hooks_manager:
            try:
                installed = manager.hooks_manager.get_installed_agents()
                monitoring = manager.hooks_manager.is_monitoring()
                hooks_stats = {
                    "hooks_installed": len(installed),
                    "hooks_monitoring": monitoring,
                    "installed_agents": installed,
                }
            except Exception as e:
                logger.warning(f"Failed to get hooks status: {e}")

        return {
            "success": True,
            "timestamp": datetime.now(UTC).isoformat(),
            "total_memories": result.get("total_memories", 0),
            "categories": result.get("categories", {}),
            **orchestrator_stats,
            **hooks_stats,
        }

    except Exception as e:
        logger.error(f"Error getting stats summary: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get statistics summary: {str(e)}"
        )


@router.get(
    "/stats/orchestrator",
    response_model=OrchestratorStatusResponse,
    summary="Get orchestrator status",
    description="Retrieve the current status and health of the Orchestrator service"
)
async def get_orchestrator_stats():
    """
    Get Orchestrator status and health information.

    Returns information about:
    - Active sessions
    - Event processing
    - Sync status
    - Overall health
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.orchestrator:
            return OrchestratorStatusResponse(
                success=False,
                status=None,
                active_sessions=0,
                total_events_processed=0,
                error="Orchestrator not initialized"
            )

        # Get status
        status_result = await manager.get_orchestrator_status()

        # Get health
        health = await manager.orchestrator.get_health()

        return OrchestratorStatusResponse(
            success=status_result.get("success", False),
            status=status_result.get("status"),
            active_sessions=health.get("active_sessions", 0),
            total_events_processed=health.get("total_events_processed", 0),
            error=status_result.get("error") if not status_result.get("success") else None
        )

    except Exception as e:
        logger.error(f"Error getting orchestrator stats: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get orchestrator status: {str(e)}"
        )


@router.get(
    "/stats/database",
    summary="Get database statistics",
    description="Retrieve database-level statistics and information"
)
async def get_database_stats():
    """Get database statistics"""
    try:
        from ...database import get_database_info

        db_info = get_database_info()

        if not db_info.get("success"):
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail=db_info.get("error", "Failed to get database info")
            )

        return {
            "success": True,
            "database_path": db_info.get("database_path"),
            "tables": db_info.get("tables", {}),
            "database_size_bytes": db_info.get("database_size_bytes"),
        }

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting database stats: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get database statistics: {str(e)}"
        )


@router.get(
    "/stats/config",
    summary="Get configuration",
    description="Retrieve current system configuration (sanitized)"
)
async def get_config():
    """Get current configuration (sensitive values hidden)"""
    try:
        config_dict = config.to_dict()

        # Hide sensitive values
        sanitized = {}
        for key, value in config_dict.items():
            if "key" in key.lower() and "api" in key.lower():
                sanitized[key] = "********" if value else None
            else:
                sanitized[key] = value

        return {
            "success": True,
            "config": sanitized
        }

    except Exception as e:
        logger.error(f"Error getting config: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get configuration: {str(e)}"
        )


@router.get(
    "/stats/agents",
    summary="Get supported agents",
    description="List all supported agent types with their configurations"
)
async def get_supported_agents():
    """Get list of supported agent types"""
    try:
        from ...config.agent_namespaces import (
            list_supported_agents,
            get_agent_description,
            get_agent_namespace
        )

        agents = list_supported_agents()
        agent_info = {}

        for agent in agents:
            agent_info[agent] = {
                "description": get_agent_description(agent),
                "namespace": get_agent_namespace(agent)
            }

        return {
            "success": True,
            "agents": agent_info,
            "total": len(agents)
        }

    except Exception as e:
        logger.error(f"Error getting supported agents: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get supported agents: {str(e)}"
        )
