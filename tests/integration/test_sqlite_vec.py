"""
Integration tests for sqlite-vec embedding system.

Run with: pytest tests/integration/test_sqlite_vec.py -v
"""

import pytest
import asyncio
import numpy as np
from pathlib import Path

from nexus.embeddings.service import EmbeddingService, get_embedding_service
from nexus.embeddings.sqlite_vec import SQLiteVecStore, VectorSearchResult
from nexus.database.managers import DatabaseManager, MemoryManager
from nexus.config import config


@pytest.fixture
async def db_manager():
    """Create a test database manager."""
    # Use a test database file
    test_db = Path("/tmp/nexus_test_vec.db")
    if test_db.exists():
        test_db.unlink()

    original_db_path = config.database_path
    config.database_path = str(test_db)

    db_manager = DatabaseManager()
    await db_manager.initialize()

    yield db_manager

    await db_manager.close()
    if test_db.exists():
        test_db.unlink()
    config.database_path = original_db_path


@pytest.fixture
def embedding_service():
    """Get embedding service singleton."""
    return get_embedding_service()


@pytest.fixture
def vec_store():
    """Create vector store for testing."""
    test_db = Path("/tmp/nexus_test_vec_store.db")
    if test_db.exists():
        test_db.unlink()

    store = SQLiteVecStore(str(test_db))
    yield store

    if test_db.exists():
        test_db.unlink()


class TestEmbeddingService:
    """Test embedding service functionality."""

    @pytest.mark.asyncio
    async def test_encode_single_text(self, embedding_service):
        """Test encoding a single text string."""
        text = "Hello, world!"
        embedding = await embedding_service.encode(text)

        assert isinstance(embedding, np.ndarray)
        assert embedding.shape == (384,)
        assert embedding.dtype == np.float32

    @pytest.mark.asyncio
    async def test_encode_batch(self, embedding_service):
        """Test encoding multiple texts."""
        texts = ["Hello world", "Goodbye world", "Testing embeddings"]
        embeddings = await embedding_service.encode(texts)

        assert isinstance(embeddings, np.ndarray)
        assert embeddings.shape == (3, 384)
        assert embeddings.dtype == np.float32

    @pytest.mark.asyncio
    async def test_encode_normalize(self, embedding_service):
        """Test that embeddings are normalized."""
        text = "Normalization test"
        embedding = await embedding_service.encode(text, normalize=True)

        # Check that L2 norm is approximately 1
        norm = np.linalg.norm(embedding)
        assert abs(norm - 1.0) < 1e-5

    def test_compute_similarity(self, embedding_service):
        """Test cosine similarity computation."""
        vec1 = np.random.rand(384).astype(np.float32)
        vec2 = np.random.rand(384).astype(np.float32)

        similarity = embedding_service.compute_similarity(vec1, vec2)

        assert isinstance(similarity, float)
        assert -1.0 <= similarity <= 1.0

    @pytest.mark.asyncio
    async def test_similar_texts_have_high_similarity(self, embedding_service):
        """Test that similar texts have higher similarity scores."""
        embeddings = await embedding_service.encode([
            "machine learning",
            "deep learning",
            "banana sandwich",
        ], normalize=True)

        sim_ml_dl = embedding_service.compute_similarity(embeddings[0], embeddings[1])
        sim_ml_bs = embedding_service.compute_similarity(embeddings[0], embeddings[2])

        # "machine learning" and "deep learning" should be more similar
        assert sim_ml_dl > sim_ml_bs


class TestSQLiteVecStore:
    """Test sqlite-vec wrapper functionality."""

    @pytest.mark.asyncio
    async def test_initialize(self, vec_store):
        """Test vector store initialization."""
        await vec_store.initialize()
        assert vec_store._initialized is True

    @pytest.mark.asyncio
    async def test_insert_embedding(self, vec_store):
        """Test inserting a single embedding."""
        await vec_store.initialize()

        embedding = np.random.rand(384).astype(np.float32)
        result = await vec_store.insert_embedding(1, embedding)

        assert result is True

    @pytest.mark.asyncio
    async def test_insert_batch(self, vec_store):
        """Test batch insertion of embeddings."""
        await vec_store.initialize()

        embeddings = [
            (1, np.random.rand(384).astype(np.float32)),
            (2, np.random.rand(384).astype(np.float32)),
            (3, np.random.rand(384).astype(np.float32)),
        ]

        count = await vec_store.insert_batch(embeddings)
        assert count == 3

    @pytest.mark.asyncio
    async def test_search(self, vec_store):
        """Test vector similarity search."""
        await vec_store.initialize()

        # Insert some test embeddings
        # Create embeddings with different patterns
        base_vec = np.random.rand(384).astype(np.float32)
        similar_vec = base_vec + np.random.randn(384).astype(np.float32) * 0.1
        different_vec = np.random.rand(384).astype(np.float32)

        await vec_store.insert_embedding(1, base_vec)
        await vec_store.insert_embedding(2, similar_vec)
        await vec_store.insert_embedding(3, different_vec)

        # Search for similar vectors
        query = base_vec + np.random.randn(384).astype(np.float32) * 0.05
        results = await vec_store.search(query, k=3)

        assert len(results) <= 3
        # Most similar should be memory 1 or 2 (not 3)
        assert results[0].memory_id in [1, 2]
        # Results should be sorted by similarity
        assert results[0].similarity >= results[-1].similarity

    @pytest.mark.asyncio
    async def test_search_with_threshold(self, vec_store):
        """Test search with similarity threshold."""
        await vec_store.initialize()

        # Insert embeddings
        vec1 = np.random.rand(384).astype(np.float32)
        vec2 = np.random.rand(384).astype(np.float32)

        await vec_store.insert_embedding(1, vec1)
        await vec_store.insert_embedding(2, vec2)

        # Search with high threshold
        results = await vec_store.search(vec1, k=10, threshold=0.9)

        # Should only return memory 1 (exact match)
        assert len(results) >= 1
        assert results[0].memory_id == 1
        assert results[0].similarity >= 0.9

    @pytest.mark.asyncio
    async def test_delete_embedding(self, vec_store):
        """Test deleting an embedding."""
        await vec_store.initialize()

        embedding = np.random.rand(384).astype(np.float32)
        await vec_store.insert_embedding(1, embedding)

        # Verify it exists by searching
        results = await vec_store.search(embedding, k=5)
        assert len(results) > 0

        # Delete
        result = await vec_store.delete_embedding(1)
        assert result is True

        # Verify it's gone
        results = await vec_store.search(embedding, k=5)
        assert len(results) == 0

    @pytest.mark.asyncio
    async def test_get_stats(self, vec_store):
        """Test getting vector store statistics."""
        await vec_store.initialize()

        stats = await vec_store.get_stats()

        assert "vector_count" in stats
        assert "vector_dim" in stats
        assert stats["vector_dim"] == 384


class TestMemoryManagerWithEmbeddings:
    """Test MemoryManager with embedding integration."""

    @pytest.mark.asyncio
    async def test_store_memory_creates_embedding(self, db_manager):
        """Test that storing a memory automatically creates an embedding."""
        memory_manager = MemoryManager(db_manager)

        result = await memory_manager.store_memory(
            content="Test memory content about machine learning",
            agent_type="test-agent",
            category="general",
        )

        assert result["success"] is True
        memory_id = result["memory_id"]

        # Verify embedding was created
        await memory_manager.initialize_embeddings()
        stats = await memory_manager.vec_store.get_stats()
        assert stats["vector_count"] >= 1

    @pytest.mark.asyncio
    async def test_search_by_embedding(self, db_manager):
        """Test semantic search using embeddings."""
        memory_manager = MemoryManager(db_manager)

        # Store some memories
        await memory_manager.store_memory(
            content="Python is a programming language",
            agent_type="test-agent",
            category="facts",
        )
        await memory_manager.store_memory(
            content="JavaScript is used for web development",
            agent_type="test-agent",
            category="facts",
        )
        await memory_manager.store_memory(
            content="I like pizza and pasta",
            agent_type="test-agent",
            category="preferences",
        )

        # Wait for embeddings to be processed
        await asyncio.sleep(0.5)

        # Search for programming-related content
        result = await memory_manager.search_memories_by_embedding(
            query="programming languages",
            agent_type="test-agent",
            k=5,
        )

        assert result["success"] is True
        assert len(result["results"]) >= 1

        # First result should be about programming
        top_result = result["results"][0]
        assert "similarity" in top_result
        assert top_result["similarity"] > 0

    @pytest.mark.asyncio
    async def test_search_with_filters(self, db_manager):
        """Test search with category and memory lane filters."""
        memory_manager = MemoryManager(db_manager)

        # Store memories with different categories
        await memory_manager.store_memory(
            content="Python programming tip",
            agent_type="test-agent",
            category="facts",
        )
        await memory_manager.store_memory(
            content="My coding preference",
            agent_type="test-agent",
            category="preferences",
        )

        # Wait for embeddings
        await asyncio.sleep(0.5)

        # Search with category filter
        result = await memory_manager.search_memories_by_embedding(
            query="programming code",
            agent_type="test-agent",
            k=10,
            category="facts",
        )

        assert result["success"] is True
        # All results should be from facts category
        for r in result["results"]:
            assert r["category"] == "facts"


@pytest.mark.integration
class TestEndToEndEmbeddingWorkflow:
    """End-to-end tests for embedding workflow."""

    @pytest.mark.asyncio
    async def test_full_workflow(self, db_manager):
        """Test complete embedding workflow."""
        memory_manager = MemoryManager(db_manager)

        # 1. Store memories
        memories = [
            "The quick brown fox jumps over the lazy dog",
            "Machine learning models process data",
            "Python lists are mutable sequences",
        ]

        memory_ids = []
        for content in memories:
            result = await memory_manager.store_memory(
                content=content,
                agent_type="test-agent",
                category="test",
            )
            assert result["success"]
            memory_ids.append(result["memory_id"])

        # Wait for embeddings
        await asyncio.sleep(1)

        # 2. Search by semantic similarity
        result = await memory_manager.search_memories_by_embedding(
            query="data processing and machine learning",
            agent_type="test-agent",
            k=3,
        )

        assert result["success"]
        assert len(result["results"]) >= 1

        # The machine learning memory should rank highly
        top_ids = [r["id"] for r in result["results"]]
        assert memory_ids[1] in top_ids  # ML memory

        # 3. Verify vector store stats
        stats = await memory_manager.vec_store.get_stats()
        assert stats["vector_count"] >= len(memories)
