"""
Nexus Manager - Main business logic for Nexus Memory System
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional, Tuple
from loguru import logger

from ..database.managers import DatabaseManager, MemoryManager, SpecificationManager
from ..services import HooksManager
from ..orchestrator import Orchestrator, OrchestratorConfig
from ..config import config


class NexusManager:
    """
    Main manager class that coordinates all Nexus operations
    """

    def __init__(self):
        self.db_manager: Optional[DatabaseManager] = None
        self.memory_manager: Optional[MemoryManager] = None
        self.specification_manager: Optional[SpecificationManager] = None
        self.hooks_manager: Optional[HooksManager] = None
        self.orchestrator: Optional[Orchestrator] = None
        self._initialized = False

    async def initialize(self):
        """Initialize the manager and database connections"""
        if self._initialized:
            return

        try:
            # Initialize database manager
            self.db_manager = DatabaseManager()
            await self.db_manager.initialize()

            # Initialize specialized managers
            self.memory_manager = MemoryManager(self.db_manager)
            self.specification_manager = SpecificationManager(self.db_manager)

            # Initialize hooks manager
            self.hooks_manager = HooksManager(self)
            await self.hooks_manager.initialize()

            # Initialize orchestrator
            orchestrator_config = OrchestratorConfig(
                session_idle_threshold_seconds=config.orchestrator_session_idle_threshold_seconds,
                session_timeout_seconds=config.orchestrator_session_timeout_seconds,
                session_persistence_enabled=config.orchestrator_session_persistence_enabled,
                session_persistence_dir=None,  # Use default
                event_queue_max_size=config.orchestrator_event_queue_max_size,
                event_max_workers=config.orchestrator_event_max_workers,
                event_persistence_enabled=config.orchestrator_event_persistence_enabled,
                event_persistence_dir=None,  # Use default
                sync_policy=config.orchestrator_sync_policy,
                auto_share_labels=config.orchestrator_auto_share_labels,
            )

            self.orchestrator = Orchestrator(
                memory_manager=self.memory_manager,
                db_manager=self.db_manager,
                hooks_manager=self.hooks_manager,
                config=orchestrator_config
            )
            await self.orchestrator.initialize()

            self._initialized = True
            logger.info("Nexus Manager initialized successfully")

        except Exception as e:
            logger.error(f"Failed to initialize Nexus Manager: {e}")
            raise

    async def ensure_initialized(self):
        """Ensure the manager is initialized"""
        if not self._initialized:
            await self.initialize()

    async def close(self):
        """Close database connections and cleanup"""
        # Close orchestrator first (stops all coordination)
        if self.orchestrator:
            await self.orchestrator.close()

        # Close hooks manager (stops monitoring)
        if self.hooks_manager:
            await self.hooks_manager.close()

        if self.db_manager:
            await self.db_manager.close()
        self._initialized = False

    # Memory Operations
    async def store_memory(
        self,
        content: str,
        agent_type: str = "general",
        category: str = "general",
        labels: List[str] = None,
        metadata: Dict[str, Any] = None
    ) -> Dict[str, Any]:
        """Store a memory with automatic processing"""
        await self.ensure_initialized()

        try:
            # Validate input
            if not content or not content.strip():
                return {
                    "success": False,
                    "error": "Content cannot be empty"
                }

            # Enhance content with metadata
            enhanced_metadata = metadata or {}
            enhanced_metadata.update({
                "stored_at": datetime.now(UTC).isoformat(),
                "agent_type": agent_type,
                "content_length": len(content),
            })

            # Store the memory
            result = await self.memory_manager.store_memory(
                content=content.strip(),
                agent_type=agent_type,
                category=category,
                labels=labels or [],
                metadata=enhanced_metadata
            )

            if result["success"]:
                logger.info(f"Stored memory {result['memory_id']} for {agent_type}")

                # Optionally trigger background processing
                if labels and "auto-index" in labels:
                    await self._trigger_auto_indexing(result["memory_id"])

            return result

        except Exception as e:
            logger.error(f"Failed to store memory: {e}")
            return {
                "success": False,
                "error": str(e)
            }

    async def search_memories(
        self,
        query: str,
        agent_type: str = "general",
        limit: int = 5,
        category: Optional[str] = None
    ) -> Dict[str, Any]:
        """Search memories with intelligent ranking"""
        await self.ensure_initialized()

        try:
            if not query or not query.strip():
                return {
                    "success": False,
                    "error": "Query cannot be empty"
                }

            # Perform search
            result = await self.memory_manager.search_memories(
                query=query.strip(),
                agent_type=agent_type,
                limit=min(limit, 50),  # Cap at 50 for performance
                category=category
            )

            # Enhance results with additional metadata
            if result["success"] and result.get("results"):
                for memory in result["results"]:
                    memory["relevance_summary"] = self._calculate_relevance_summary(
                        query, memory.get("content", "")
                    )

            return result

        except Exception as e:
            logger.error(f"Failed to search memories: {e}")
            return {
                "success": False,
                "error": str(e),
                "query": query,
                "agent_type": agent_type
            }

    # Specification Operations
    async def store_task_specification(
        self,
        task_description: str,
        spec_content: Dict[str, Any],
        agent_type: str = "droid",
        complexity_score: float = 0.5
    ) -> Dict[str, Any]:
        """Store a task specification"""
        await self.ensure_initialized()

        try:
            if not task_description or not task_description.strip():
                return {
                    "success": False,
                    "error": "Task description cannot be empty"
                }

            if not spec_content:
                return {
                    "success": False,
                    "error": "Specification content cannot be empty"
                }

            # Validate complexity score
            complexity_score = max(0.0, min(1.0, complexity_score))

            # Store specification
            result = await self.specification_manager.store_specification(
                task_description=task_description.strip(),
                spec_content=spec_content,
                agent_type=agent_type,
                complexity_score=complexity_score
            )

            if result["success"]:
                logger.info(f"Stored specification {result['spec_id']} for {agent_type}")

            return result

        except Exception as e:
            logger.error(f"Failed to store task specification: {e}")
            return {
                "success": False,
                "error": str(e)
            }

    async def find_reusable_specification(
        self,
        task_description: str,
        agent_type: str = "droid",
        threshold: float = 0.8
    ) -> Tuple[Optional[Dict[str, Any]], List[Dict[str, Any]]]:
        """Find reusable task specifications"""
        await self.ensure_initialized()

        try:
            if not task_description or not task_description.strip():
                return None, []

            # Find specifications
            best_match, alternatives = await self.specification_manager.find_reusable_specification(
                task_description=task_description.strip(),
                agent_type=agent_type,
                threshold=threshold
            )

            if best_match:
                logger.info(f"Found reusable specification {best_match['spec_id']}")
            elif alternatives:
                logger.info(f"Found {len(alternatives)} alternative specifications")

            return best_match, alternatives

        except Exception as e:
            logger.error(f"Failed to find reusable specification: {e}")
            return None, []

    async def update_spec_usage(self, spec_id: str) -> bool:
        """Update specification usage statistics"""
        await self.ensure_initialized()

        try:
            success = await self.specification_manager.update_spec_usage(spec_id)
            if success:
                logger.info(f"Updated usage for specification {spec_id}")
            return success

        except Exception as e:
            logger.error(f"Failed to update spec usage: {e}")
            return False

    # Statistics and Analytics
    async def get_memory_stats(self, agent_type: Optional[str] = None) -> Dict[str, Any]:
        """Get comprehensive memory statistics"""
        await self.ensure_initialized()

        try:
            # Get basic stats
            result = await self.memory_manager.get_memory_stats(agent_type)

            if result["success"]:
                # Add additional analytics
                result.update({
                    "system_info": await self._get_system_info(),
                    "performance_metrics": await self._get_performance_metrics(),
                })

            return result

        except Exception as e:
            logger.error(f"Failed to get memory stats: {e}")
            return {
                "success": False,
                "error": str(e),
                "agent_type": agent_type
            }

    # Helper Methods
    def _calculate_relevance_summary(self, query: str, content: str) -> str:
        """Calculate a relevance summary for search results"""
        if not content:
            return "No content available"

        # Simple relevance calculation (can be enhanced with embeddings)
        query_words = set(query.lower().split())
        content_words = set(content.lower().split())
        overlap = len(query_words.intersection(content_words))

        if overlap > len(query_words) * 0.7:
            return "Highly relevant"
        elif overlap > len(query_words) * 0.3:
            return "Moderately relevant"
        elif overlap > 0:
            return "Somewhat relevant"
        else:
            return "Limited relevance"

    async def _trigger_auto_indexing(self, memory_id: int):
        """Trigger background indexing for a memory"""
        try:
            # This would implement background processing for embeddings, etc.
            logger.debug(f"Triggering auto-indexing for memory {memory_id}")
            # Implementation would depend on specific indexing requirements

        except Exception as e:
            logger.warning(f"Failed to trigger auto-indexing for memory {memory_id}: {e}")

    async def _get_system_info(self) -> Dict[str, Any]:
        """Get system information"""
        try:
            return {
                "server_version": "1.0.0",
                "database_initialized": self._initialized,
                "timestamp": datetime.now(UTC).isoformat(),
            }
        except Exception as e:
            logger.warning(f"Failed to get system info: {e}")
            return {}

    async def _get_performance_metrics(self) -> Dict[str, Any]:
        """Get performance metrics"""
        try:
            # This would implement actual performance monitoring
            return {
                "database_connections": 1,  # Placeholder
                "cache_hit_rate": 0.0,      # Placeholder
                "average_response_time": 0.0, # Placeholder
            }
        except Exception as e:
            logger.warning(f"Failed to get performance metrics: {e}")
            return {}

    # Synchronous methods for compatibility with FastMCP tools
    def store_memory_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of store_memory for FastMCP compatibility"""
        return asyncio.run(self.store_memory(*args, **kwargs))

    def search_memories_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of search_memories for FastMCP compatibility"""
        return asyncio.run(self.search_memories(*args, **kwargs))

    def store_task_specification_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of store_task_specification for FastMCP compatibility"""
        return asyncio.run(self.store_task_specification(*args, **kwargs))

    def find_reusable_specification_sync(self, *args, **kwargs) -> Tuple[Optional[Dict[str, Any]], List[Dict[str, Any]]]:
        """Synchronous version of find_reusable_specification for FastMCP compatibility"""
        return asyncio.run(self.find_reusable_specification(*args, **kwargs))

    def update_spec_usage_sync(self, *args, **kwargs) -> bool:
        """Synchronous version of update_spec_usage for FastMCP compatibility"""
        return asyncio.run(self.update_spec_usage(*args, **kwargs))

    def get_memory_stats_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of get_memory_stats for FastMCP compatibility"""
        return asyncio.run(self.get_memory_stats(*args, **kwargs))

    # Orchestrator Methods

    async def start_session(
        self,
        agent_type: str,
        metadata: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """Start a new session via orchestrator"""
        await self.ensure_initialized()
        if not self.orchestrator:
            return {"success": False, "error": "Orchestrator not initialized"}

        session = await self.orchestrator.start_session(agent_type, metadata)
        return {
            "success": True,
            "session": session.to_dict(),
        }

    async def end_session(
        self,
        session_id: str,
        reason: str = "manual"
    ) -> Dict[str, Any]:
        """End a session via orchestrator"""
        await self.ensure_initialized()
        if not self.orchestrator:
            return {"success": False, "error": "Orchestrator not initialized"}

        session = await self.orchestrator.end_session(session_id, reason)
        return {
            "success": session is not None,
            "session": session.to_dict() if session else None,
        }

    async def share_memory(
        self,
        source_memory_id: int,
        source_agent_type: str,
        target_agent_types: Optional[List[str]] = None
    ) -> Dict[str, Any]:
        """Share memory across agent namespaces via orchestrator"""
        await self.ensure_initialized()
        if not self.orchestrator:
            return {"success": False, "error": "Orchestrator not initialized"}

        result = await self.orchestrator.share_memory(
            source_memory_id=source_memory_id,
            source_agent_type=source_agent_type,
            target_agent_types=target_agent_types
        )
        return {
            "success": result.status.value in ["completed", "partial"],
            "result": result.to_dict(),
        }

    async def get_orchestrator_status(self) -> Dict[str, Any]:
        """Get orchestrator status"""
        await self.ensure_initialized()
        if not self.orchestrator:
            return {"success": False, "error": "Orchestrator not initialized"}

        status = await self.orchestrator.get_status()
        return {
            "success": True,
            "status": status,
        }

    async def get_orchestrator_health(self) -> Dict[str, Any]:
        """Get orchestrator health status"""
        await self.ensure_initialized()
        if not self.orchestrator:
            return {"healthy": False, "error": "Orchestrator not initialized"}

        health = await self.orchestrator.get_health()
        return health

    # Synchronous orchestrator methods for FastMCP compatibility
    def start_session_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of start_session"""
        return asyncio.run(self.start_session(*args, **kwargs))

    def end_session_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of end_session"""
        return asyncio.run(self.end_session(*args, **kwargs))

    def share_memory_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of share_memory"""
        return asyncio.run(self.share_memory(*args, **kwargs))

    def get_orchestrator_status_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of get_orchestrator_status"""
        return asyncio.run(self.get_orchestrator_status(*args, **kwargs))

    def get_orchestrator_health_sync(self, *args, **kwargs) -> Dict[str, Any]:
        """Synchronous version of get_orchestrator_health"""
        return asyncio.run(self.get_orchestrator_health(*args, **kwargs))


# =============================================================================
# Web Server Support
# =============================================================================

def run_web_server(
    host: str = "0.0.0.0",
    port: int = 8768,
    log_level: str = "info"
):
    """
    Run the FastAPI web server

    Args:
        host: Host address to bind to
        port: Port to listen on
        log_level: Logging level
    """
    import uvicorn

    # Import the web app
    from ..web import get_web_app

    app = get_web_app()

    # Run uvicorn server
    uvicorn.run(
        app,
        host=host,
        port=port,
        log_level=log_level.lower(),
        access_log=True
    )


# Global manager instance
_nexus_manager: Optional[NexusManager] = None


def get_memory_manager() -> NexusManager:
    """Get global nexus manager instance"""
    global _nexus_manager
    if _nexus_manager is None:
        _nexus_manager = NexusManager()
        # Note: async initialization will happen on first use
    return _nexus_manager