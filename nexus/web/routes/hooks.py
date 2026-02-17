"""
Hooks Routes - API endpoints for agent hooks management

Provides endpoints for:
- Hooks installation status
- Hooks installation/uninstallation
- Triggering manual extraction
- Monitoring status
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional

from fastapi import APIRouter, HTTPException, Query, Path, status
from loguru import logger

from ...server.nexus_manager import get_memory_manager
from ..app import HooksStatusResponse


router = APIRouter()


# =============================================================================
# Hooks Status Endpoints
# =============================================================================


@router.get(
    "/hooks/status",
    summary="Get hooks status",
    description="Retrieve the installation and monitoring status of agent hooks"
)
async def get_hooks_status(
    verbose: bool = Query(
        False,
        description="Include detailed statistics for each agent"
    )
):
    """
    Get the status of all installed hooks.

    Returns information about:
    - Which agents have hooks installed
    - Hook type for each agent
    - Monitoring status
    - Extraction statistics
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            return {
                "success": False,
                "error": "Hooks manager not initialized",
                "installed_agents": [],
                "monitoring": False,
                "auto_extraction": False
            }

        # Get basic status
        installed = manager.hooks_manager.get_installed_agents()
        is_monitoring = manager.hooks_manager.is_monitoring()
        auto_enabled = manager.hooks_manager.is_auto_extraction_enabled()

        # Get detailed status for each agent
        status_dict = await manager.hooks_manager.get_hooks_status()

        hooks_status = []
        for agent_type in installed:
            agent_status = status_dict.get(agent_type, {})
            hooks_status.append({
                "agent_type": agent_type,
                "installed": agent_status.get("installed", False),
                "monitoring": is_monitoring,
                "hook_type": agent_status.get("hook_type", "unknown"),
                "last_extraction": agent_status.get("last_extraction"),
                "extraction_count": agent_status.get("extraction_count", 0),
                "error_count": agent_status.get("error_count", 0),
                "last_error": agent_status.get("last_error")
            })

        response = {
            "success": True,
            "installed_agents": installed,
            "total_installed": len(installed),
            "monitoring": is_monitoring,
            "auto_extraction": auto_enabled,
            "hooks": hooks_status
        }

        # Add detailed statistics if verbose
        if verbose:
            detailed_stats = {}
            for agent_type in installed:
                try:
                    stats = await manager.hooks_manager.get_extraction_stats(agent_type)
                    if stats:
                        detailed_stats[agent_type] = stats
                except Exception as e:
                    logger.warning(f"Failed to get stats for {agent_type}: {e}")
            response["detailed_stats"] = detailed_stats

        return response

    except Exception as e:
        logger.error(f"Error getting hooks status: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get hooks status: {str(e)}"
        )


@router.get(
    "/hooks/status/{agent_type}",
    response_model=HooksStatusResponse,
    summary="Get hooks status for specific agent",
    description="Retrieve hooks status for a specific agent type"
)
async def get_agent_hooks_status(
    agent_type: str = Path(
        ...,
        description="Agent type (e.g., claude-code, gemini, qwen)"
    )
):
    """Get hooks status for a specific agent"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="Hooks manager not initialized"
            )

        installed = manager.hooks_manager.get_installed_agents()
        if agent_type not in installed:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"No hooks installed for agent: {agent_type}"
            )

        status_dict = await manager.hooks_manager.get_hooks_status()
        agent_status = status_dict.get(agent_type, {})

        return HooksStatusResponse(
            agent_type=agent_type,
            installed=agent_status.get("installed", False),
            monitoring=manager.hooks_manager.is_monitoring(),
            hook_type=agent_status.get("hook_type", "unknown"),
            last_extraction=agent_status.get("last_extraction"),
            extraction_count=agent_status.get("extraction_count", 0),
            error_count=agent_status.get("error_count", 0),
            last_error=agent_status.get("last_error")
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting agent hooks status: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get agent hooks status: {str(e)}"
        )


# =============================================================================
# Hooks Management Endpoints
# =============================================================================


@router.post(
    "/hooks/install",
    summary="Install hooks for an agent",
    description="Install agent hooks for automated memory extraction"
)
async def install_hooks(
    agent_type: str = Query(
        ...,
        description="Agent type or 'all' to install for all agents"
    ),
    enable_monitoring: bool = Query(
        True,
        description="Start monitoring after installation"
    )
):
    """
    Install hooks for an agent type.

    Use agent_type='all' to install hooks for all supported agents.
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="Hooks manager not initialized"
            )

        if agent_type.lower() == "all":
            # Install for all agents
            results = await manager.hooks_manager.install_all_hooks(
                enable_monitoring=enable_monitoring
            )

            # Format results
            formatted_results = {}
            for agent, result in results.items():
                formatted_results[agent] = {
                    "status": result.status.value,
                    "message": result.message,
                    "error": result.error,
                    "hook_type": result.hook_type
                }

            return {
                "success": True,
                "installed": formatted_results,
                "monitoring": manager.hooks_manager.is_monitoring()
            }
        else:
            # Install for specific agent
            result = await manager.hooks_manager.install_hooks(
                agent_type,
                enable_monitoring=enable_monitoring
            )

            return {
                "success": result.status.value == "success",
                "agent_type": agent_type,
                "status": result.status.value,
                "message": result.message,
                "error": result.error,
                "hook_type": result.hook_type
            }

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error installing hooks: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to install hooks: {str(e)}"
        )


@router.post(
    "/hooks/uninstall",
    summary="Uninstall hooks for an agent",
    description="Remove agent hooks"
)
async def uninstall_hooks(
    agent_type: str = Query(
        ...,
        description="Agent type to uninstall hooks for"
    )
):
    """Uninstall hooks for an agent"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="Hooks manager not initialized"
            )

        success = await manager.hooks_manager.uninstall_hooks(agent_type)

        return {
            "success": success,
            "agent_type": agent_type,
            "message": "Hooks uninstalled successfully" if success else "Failed to uninstall hooks"
        }

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error uninstalling hooks: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to uninstall hooks: {str(e)}"
        )


@router.post(
    "/hooks/extract",
    summary="Trigger manual extraction",
    description="Manually trigger memory extraction for an agent"
)
async def trigger_extraction(
    agent_type: str = Query(
        ...,
        description="Agent type to extract from, or 'all' for all active agents"
    )
):
    """
    Manually trigger memory extraction.

    Use agent_type='all' to extract from all active agents.
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="Hooks manager not initialized"
            )

        if agent_type.lower() == "all":
            # Extract from all active agents
            results = await manager.hooks_manager.extract_all_active_sessions()

            formatted_results = {}
            for agent, result in results.items():
                formatted_results[agent] = {
                    "success": result.success,
                    "memory_count": result.memory_count,
                    "error": result.error
                }

            return {
                "success": True,
                "results": formatted_results
            }
        else:
            # Extract from specific agent
            result = await manager.hooks_manager.trigger_extraction(agent_type)

            return {
                "success": result.success,
                "agent_type": agent_type,
                "memory_count": result.memory_count,
                "error": result.error
            }

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error triggering extraction: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to trigger extraction: {str(e)}"
        )


# =============================================================================
# Monitoring Control Endpoints
# =============================================================================


@router.post(
    "/hooks/monitoring/start",
    summary="Start hooks monitoring",
    description="Start the hooks monitoring service"
)
async def start_monitoring():
    """Start hooks monitoring"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="Hooks manager not initialized"
            )

        if manager.hooks_manager.is_monitoring():
            return {
                "success": True,
                "message": "Monitoring is already active",
                "monitoring": True
            }

        await manager.hooks_manager.start_monitoring()

        return {
            "success": True,
            "message": "Monitoring started successfully",
            "monitoring": True
        }

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error starting monitoring: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to start monitoring: {str(e)}"
        )


@router.post(
    "/hooks/monitoring/stop",
    summary="Stop hooks monitoring",
    description="Stop the hooks monitoring service"
)
async def stop_monitoring():
    """Stop hooks monitoring"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        if not manager.hooks_manager:
            raise HTTPException(
                status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
                detail="Hooks manager not initialized"
            )

        if not manager.hooks_manager.is_monitoring():
            return {
                "success": True,
                "message": "Monitoring is not active",
                "monitoring": False
            }

        await manager.hooks_manager.stop_monitoring()

        return {
            "success": True,
            "message": "Monitoring stopped successfully",
            "monitoring": False
        }

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error stopping monitoring: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to stop monitoring: {str(e)}"
        )
