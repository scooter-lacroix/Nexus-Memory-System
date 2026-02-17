"""
Integration tests for memory operations
"""

import pytest
from datetime import datetime, UTC

from tests.conftest import create_test_memory


class TestMemoryOperations:
    """Test memory operations end-to-end"""

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_store_and_search_memory(self, nexus_manager):
        """Test storing and searching memories"""
        # Store a memory
        content = "User prefers Python for data analysis with pandas"
        result = await nexus_manager.store_memory(
            content=content,
            agent_type="claude-code",
            category="preferences",
            labels=["python", "data-analysis"]
        )

        assert result["success"] is True
        memory_id = result["memory_id"]

        # Search for the memory
        search_result = await nexus_manager.search_memories(
            query="python data analysis",
            agent_type="claude-code",
            limit=5
        )

        assert search_result["success"] is True
        assert len(search_result["results"]) >= 1

        # Find our memory in results
        found_memory = None
        for memory in search_result["results"]:
            if memory["id"] == memory_id:
                found_memory = memory
                break

        assert found_memory is not None
        assert found_memory["content"] == content
        assert found_memory["category"] == "preferences"
        assert "python" in found_memory["labels"]
        assert "data-analysis" in found_memory["labels"]

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_cross_agent_memory_isolation(self, nexus_manager):
        """Test that different agents have isolated memory namespaces"""
        # Store memory for agent A
        result_a = await nexus_manager.store_memory(
            content="Secret information for agent A",
            agent_type="claude-code",
            category="secrets"
        )

        # Store memory for agent B
        result_b = await nexus_manager.store_memory(
            content="Secret information for agent B",
            agent_type="gemini",
            category="secrets"
        )

        assert result_a["success"] is True
        assert result_b["success"] is True

        # Search agent A memories
        search_a = await nexus_manager.search_memories(
            query="secret information",
            agent_type="claude-code",
            limit=10
        )

        # Search agent B memories
        search_b = await nexus_manager.search_memories(
            query="secret information",
            agent_type="gemini",
            limit=10
        )

        # Each agent should only find their own memory
        assert search_a["success"] is True
        assert search_b["success"] is True

        a_found = any("agent A" in mem["content"] for mem in search_a["results"])
        b_found = any("agent B" in mem["content"] for mem in search_b["results"])

        assert a_found is True
        assert b_found is True

        # Verify cross-contamination doesn't happen
        a_sees_b = any("agent B" in mem["content"] for mem in search_a["results"])
        b_sees_a = any("agent A" in mem["content"] for mem in search_b["results"])

        assert a_sees_b is False
        assert b_sees_a is False

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_memory_categorization(self, nexus_manager):
        """Test memory categorization and filtering"""
        # Store memories in different categories
        categories = [
            ("User likes Python programming", "preferences"),
            ("Install pandas library", "instructions"),
            ("Error: Module not found", "errors"),
            ("Python 3.9 features", "facts"),
        ]

        for content, category in categories:
            result = await nexus_manager.store_memory(
                content=content,
                agent_type="general",
                category=category
            )
            assert result["success"] is True

        # Search all memories
        all_search = await nexus_manager.search_memories(
            query="Python",
            agent_type="general",
            limit=10
        )

        # Search specific category
        pref_search = await nexus_manager.search_memories(
            query="Python",
            agent_type="general",
            category="preferences",
            limit=10
        )

        assert all_search["success"] is True
        assert pref_search["success"] is True

        # Should find all memories in general search
        assert len(all_search["results"]) >= 4

        # Should only find preferences in category search
        assert len(pref_search["results"]) >= 1
        for memory in pref_search["results"]:
            assert memory["category"] == "preferences"

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_memory_metadata_and_labels(self, nexus_manager):
        """Test memory metadata and labels handling"""
        # Store memory with rich metadata
        content = "Complex memory with metadata"
        labels = ["test", "metadata", "important"]
        metadata = {
            "source": "integration_test",
            "priority": "high",
            "tags": ["production", "critical"],
            "timestamp": datetime.now(UTC).isoformat(),
            "numeric_value": 42,
            "boolean_flag": True,
        }

        result = await nexus_manager.store_memory(
            content=content,
            agent_type="test",
            category="testing",
            labels=labels,
            metadata=metadata
        )

        assert result["success"] is True

        # Retrieve the memory
        search_result = await nexus_manager.search_memories(
            query="complex memory",
            agent_type="test",
            limit=1
        )

        assert search_result["success"] is True
        assert len(search_result["results"]) == 1

        memory = search_result["results"][0]
        assert memory["content"] == content
        assert memory["category"] == "testing"

        # Check labels
        assert set(memory["labels"]) == set(labels)

        # Check metadata
        stored_metadata = memory["metadata"]
        assert stored_metadata["source"] == "integration_test"
        assert stored_metadata["priority"] == "high"
        assert set(stored_metadata["tags"]) == {"production", "critical"}
        assert stored_metadata["numeric_value"] == 42
        assert stored_metadata["boolean_flag"] is True

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_memory_search_relevance(self, nexus_manager):
        """Test memory search relevance and ranking"""
        # Store memories with different relevance to search terms
        memories = [
            ("Python programming guide", "python programming"),
            ("JavaScript tutorial", "javascript tutorial"),
            ("Python vs JavaScript comparison", "python javascript comparison"),
            ("Advanced Python techniques", "python advanced techniques"),
            ("Web development basics", "web development"),
        ]

        for content, category in memories:
            await nexus_manager.store_memory(
                content=content,
                agent_type="general",
                category=category
            )

        # Search for "python"
        result = await nexus_manager.search_memories(
            query="python",
            agent_type="general",
            limit=5
        )

        assert result["success"] is True

        # Should find Python-related memories
        python_memories = [
            mem for mem in result["results"]
            if "python" in mem["content"].lower()
        ]

        assert len(python_memories) >= 3

        # Check that relevance summaries are provided
        for memory in result["results"]:
            assert "relevance_summary" in memory
            assert isinstance(memory["relevance_summary"], str)

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_memory_statistics(self, nexus_manager):
        """Test memory statistics collection"""
        # Store memories for different agents and categories
        test_data = [
            ("Memory 1", "claude-code", "code"),
            ("Memory 2", "claude-code", "preferences"),
            ("Memory 3", "gemini", "general"),
            ("Memory 4", "general", "facts"),
            ("Memory 5", "general", "instructions"),
        ]

        for content, agent, category in test_data:
            await nexus_manager.store_memory(
                content=content,
                agent_type=agent,
                category=category
            )

        # Get stats for all agents
        all_stats = await nexus_manager.get_memory_stats()
        assert all_stats["success"] is True
        assert all_stats["total_memories"] >= 5

        # Get stats for specific agent
        claude_stats = await nexus_manager.get_memory_stats("claude-code")
        assert claude_stats["success"] is True
        assert claude_stats["total_memories"] >= 2
        assert claude_stats["agent_type"] == "claude-code"

        # Check category breakdown
        categories = claude_stats.get("categories", {})
        assert isinstance(categories, dict)
        if categories:  # Only check if categories exist
            for category, count in categories.items():
                assert isinstance(category, str)
                assert isinstance(count, int)
                assert count >= 0

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_empty_search_handling(self, nexus_manager):
        """Test handling of empty and invalid searches"""
        # Test empty query
        result = await nexus_manager.search_memories(
            query="",
            agent_type="general",
            limit=5
        )

        assert result["success"] is False
        assert "error" in result

        # Test whitespace-only query
        result = await nexus_manager.search_memories(
            query="   ",
            agent_type="general",
            limit=5
        )

        assert result["success"] is False
        assert "error" in result

    @pytest.mark.asyncio
    @pytest.mark.integration
    async def test_memory_content_validation(self, nexus_manager):
        """Test memory content validation"""
        # Test empty content
        result = await nexus_manager.store_memory(
            content="",
            agent_type="general"
        )

        assert result["success"] is False
        assert "error" in result

        # Test whitespace-only content
        result = await nexus_manager.store_memory(
            content="   \n\t  ",
            agent_type="general"
        )

        assert result["success"] is False
        assert "error" in result

        # Test very long content
        long_content = "x" * 50000  # Very long content
        result = await nexus_manager.store_memory(
            content=long_content,
            agent_type="general"
        )

        # Should succeed or fail gracefully based on implementation
        assert isinstance(result, dict)
        assert "success" in result