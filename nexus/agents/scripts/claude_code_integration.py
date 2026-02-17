#!/usr/bin/env python3
"""
Claude Code integration script for Nexus Memory System

This script provides seamless integration between Claude Code and Nexus Memory System,
enabling Claude Code to store and retrieve memories across sessions.

Usage:
    python claude_code_integration.py --init
    python claude_code_integration.py --store "memory content" --category "category"
    python claude_code_integration.py --search "query" --limit 5
"""

import argparse
import json
import sys
import asyncio
from pathlib import Path
from typing import Optional, List, Dict, Any

# Add the nexus package to Python path
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from nexus.server import get_memory_manager
from nexus.config import config


class ClaudeCodeIntegration:
    """Claude Code integration with Nexus Memory System"""

    def __init__(self):
        self.memory_manager = get_memory_manager()
        self.agent_type = "claude-code"

    async def initialize(self) -> bool:
        """Initialize the integration"""
        try:
            # Initialize memory manager
            await self.memory_manager.initialize()
            print(f"✓ Claude Code integration initialized for agent: {self.agent_type}")
            return True
        except Exception as e:
            print(f"✗ Failed to initialize Claude Code integration: {e}")
            return False

    async def store_memory(
        self,
        content: str,
        category: str = "general",
        labels: Optional[List[str]] = None,
        metadata: Optional[Dict[str, Any]] = None
    ) -> Dict[str, Any]:
        """Store a memory from Claude Code"""
        try:
            # Enhance metadata with Claude Code specific information
            enhanced_metadata = metadata or {}
            enhanced_metadata.update({
                "source": "claude-code",
                "integration_version": "1.0.0",
                "timestamp": asyncio.get_event_loop().time(),
            })

            # Add default labels for Claude Code
            default_labels = ["claude-code", "development"]
            if labels:
                default_labels.extend(labels)

            result = await self.memory_manager.store_memory_sync(
                content=content,
                agent_type=self.agent_type,
                category=category,
                labels=default_labels,
                metadata=enhanced_metadata
            )

            if result["success"]:
                print(f"✓ Memory stored successfully (ID: {result.get('memory_id')})")
                return result
            else:
                print(f"✗ Failed to store memory: {result.get('error')}")
                return result

        except Exception as e:
            error_result = {
                "success": False,
                "error": f"Integration error: {str(e)}"
            }
            print(f"✗ Integration error: {e}")
            return error_result

    async def search_memories(
        self,
        query: str,
        limit: int = 5,
        category: Optional[str] = None
    ) -> Dict[str, Any]:
        """Search memories from Claude Code"""
        try:
            result = await self.memory_manager.search_memories_sync(
                query=query,
                agent_type=self.agent_type,
                limit=limit,
                category=category
            )

            if result["success"]:
                memories = result.get("results", [])
                print(f"✓ Found {len(memories)} memories")

                for i, memory in enumerate(memories, 1):
                    print(f"\n{i}. Memory ID: {memory['id']}")
                    print(f"   Category: {memory['category']}")
                    print(f"   Created: {memory['created_at']}")
                    print(f"   Content: {memory['content'][:200]}{'...' if len(memory['content']) > 200 else ''}")

                return result
            else:
                print(f"✗ Search failed: {result.get('error')}")
                return result

        except Exception as e:
            error_result = {
                "success": False,
                "error": f"Integration error: {str(e)}"
            }
            print(f"✗ Integration error: {e}")
            return error_result

    async def get_context_enhancement(self, context: str) -> Dict[str, Any]:
        """Enhance context with relevant memories"""
        try:
            from nexus.server.mcp_server import enhance_context_with_memory

            result = await enhance_context_with_memory(
                context=context,
                agent_type=self.agent_type
            )

            if result["success"]:
                print(f"✓ Context enhanced with {len(result.get('memory_results', []))} relevant memories")
                return result
            else:
                print(f"✗ Context enhancement failed: {result.get('error')}")
                return result

        except Exception as e:
            error_result = {
                "success": False,
                "error": f"Integration error: {str(e)}"
            }
            print(f"✗ Integration error: {e}")
            return error_result


async def main():
    """Main CLI interface"""
    parser = argparse.ArgumentParser(
        description="Claude Code integration for Nexus Memory System"
    )
    parser.add_argument("--init", action="store_true", help="Initialize the integration")
    parser.add_argument("--store", type=str, help="Store a memory")
    parser.add_argument("--category", type=str, default="general", help="Memory category")
    parser.add_argument("--labels", type=str, help="Comma-separated labels")
    parser.add_argument("--search", type=str, help="Search memories")
    parser.add_argument("--limit", type=int, default=5, help="Search result limit")
    parser.add_argument("--enhance", type=str, help="Enhance context with memories")
    parser.add_argument("--stats", action="store_true", help="Show memory statistics")
    parser.add_argument("--config", action="store_true", help="Show configuration")

    args = parser.parse_args()

    # Initialize integration
    integration = ClaudeCodeIntegration()
    initialized = await integration.initialize()

    if not initialized:
        sys.exit(1)

    # Handle commands
    if args.init:
        print("Claude Code integration initialized successfully")
        print(f"Nexus server: http://{config.host}:{config.port}")
        print(f"Web UI: http://{config.host}:{config.web_port}")

    elif args.store:
        labels = []
        if args.labels:
            labels = [label.strip() for label in args.labels.split(',')]

        await integration.store_memory(
            content=args.store,
            category=args.category,
            labels=labels
        )

    elif args.search:
        await integration.search_memories(
            query=args.search,
            limit=args.limit
        )

    elif args.enhance:
        result = await integration.get_context_enhancement(args.enhance)
        if result["success"]:
            print("\nEnhanced Context:")
            print("-" * 50)
            print(result["enhanced_context"])

    elif args.stats:
        result = await integration.memory_manager.get_memory_stats_sync(integration.agent_type)
        if result["success"]:
            print(f"Claude Code Memory Statistics:")
            print(f"Total memories: {result.get('total_memories', 0)}")
            categories = result.get('categories', {})
            if categories:
                print("By category:")
                for category, count in categories.items():
                    print(f"  {category}: {count}")

    elif args.config:
        print("Claude Code Configuration:")
        print(f"Agent type: {integration.agent_type}")
        print(f"Nexus server: http://{config.host}:{config.port}")
        print(f"Database: {config.database_path}")
        print(f"Conscious ingest: {config.conscious_ingest}")
        print(f"Auto ingest: {config.auto_ingest}")

    else:
        parser.print_help()


if __name__ == "__main__":
    asyncio.run(main())