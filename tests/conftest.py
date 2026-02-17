"""
Pytest configuration and fixtures for Nexus Memory System tests
"""

import pytest
import asyncio
import tempfile
import shutil
from pathlib import Path
from typing import AsyncGenerator, Generator

from nexus.config import ServerConfig
from nexus.database.managers import DatabaseManager, MemoryManager, SpecificationManager
from nexus.server.nexus_manager import NexusManager


@pytest.fixture(scope="session")
def event_loop():
    """Create an instance of the default event loop for the test session."""
    loop = asyncio.get_event_loop_policy().new_event_loop()
    yield loop
    loop.close()


@pytest.fixture
async def temp_db_path() -> AsyncGenerator[str, None]:
    """Create a temporary database path for testing."""
    with tempfile.NamedTemporaryFile(suffix='.db', delete=False) as tmp_file:
        db_path = tmp_file.name

    yield db_path

    # Cleanup
    try:
        Path(db_path).unlink()
    except FileNotFoundError:
        pass


@pytest.fixture
async def test_config(temp_db_path: str) -> ServerConfig:
    """Create test configuration with temporary database."""
    return ServerConfig(
        database_path=temp_db_path,
        host="127.0.0.1",
        port=8767,
        web_port=8768,
        verbose=False,
        debug=False,
        conscious_ingest=True,
        auto_ingest=True,
        memory_search_limit=10,
        spec_similarity_threshold=0.8,
    )


@pytest.fixture
async def db_manager(test_config: ServerConfig) -> AsyncGenerator[DatabaseManager, None]:
    """Create a database manager for testing."""
    manager = DatabaseManager(database_url=test_config.database_connection_url)
    await manager.initialize()

    yield manager

    await manager.close()


@pytest.fixture
async def memory_manager(db_manager: DatabaseManager) -> MemoryManager:
    """Create a memory manager for testing."""
    return MemoryManager(db_manager)


@pytest.fixture
async def spec_manager(db_manager: DatabaseManager) -> SpecificationManager:
    """Create a specification manager for testing."""
    return SpecificationManager(db_manager)


@pytest.fixture
async def nexus_manager(test_config: ServerConfig) -> AsyncGenerator[NexusManager, None]:
    """Create a nexus manager for testing."""
    # Mock the config
    import nexus.config
    original_config = nexus.config.config
    nexus.config.config = test_config

    manager = NexusManager()
    await manager.initialize()

    yield manager

    await manager.close()
    # Restore original config
    nexus.config.config = original_config


@pytest.fixture
def sample_memory_content() -> str:
    """Sample memory content for testing."""
    return "This is a test memory content for Nexus Memory System testing."


@pytest.fixture
def sample_memory_data() -> dict:
    """Sample memory data for testing."""
    return {
        "content": "User prefers Python for data processing tasks and has experience with pandas.",
        "category": "preferences",
        "labels": ["python", "data-processing", "pandas"],
        "metadata": {
            "source": "test",
            "importance": "medium"
        }
    }


@pytest.fixture
def sample_spec_data() -> dict:
    """Sample specification data for testing."""
    return {
        "task_description": "Create a data processing pipeline using Python",
        "spec_content": {
            "requirements": ["Python 3.9+", "pandas", "numpy"],
            "steps": [
                "Load data from CSV",
                "Process data using pandas",
                "Generate summary statistics",
                "Save results"
            ],
            "output_format": "CSV",
            "error_handling": "log errors and continue"
        },
        "complexity_score": 0.7
    }


class AsyncContextManager:
    """Helper class for async context management in tests."""

    def __init__(self, async_func):
        self.async_func = async_func
        self.result = None

    async def __aenter__(self):
        self.result = await self.async_func()
        return self.result

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        if hasattr(self.result, 'close'):
            await self.result.close()


@pytest.fixture
def async_context():
    """Create an async context manager helper."""
    return AsyncContextManager


# Test markers
pytest_plugins = []

# Custom markers
def pytest_configure(config):
    """Configure custom pytest markers."""
    config.addinivalue_line(
        "markers", "unit: mark test as a unit test"
    )
    config.addinivalue_line(
        "markers", "integration: mark test as an integration test"
    )
    config.addinivalue_line(
        "markers", "slow: mark test as slow running"
    )
    config.addinivalue_line(
        "markers", "database: mark test as database test"
    )
    config.addinivalue_line(
        "markers", "api: mark test as API test"
    )


# Helper functions
async def create_test_memory(memory_manager: MemoryManager, **kwargs) -> dict:
    """Create a test memory with default values."""
    default_data = {
        "content": "Test memory content",
        "agent_type": "general",
        "category": "test",
        "labels": ["test"],
        "metadata": {"source": "pytest"}
    }
    default_data.update(kwargs)

    result = await memory_manager.store_memory(**default_data)
    assert result["success"], f"Failed to create test memory: {result.get('error')}"
    return result


async def create_test_specification(spec_manager: SpecificationManager, **kwargs) -> dict:
    """Create a test specification with default values."""
    default_data = {
        "task_description": "Test task",
        "spec_content": {"steps": ["step1", "step2"]},
        "agent_type": "droid",
        "complexity_score": 0.5
    }
    default_data.update(kwargs)

    result = await spec_manager.store_specification(**default_data)
    assert result["success"], f"Failed to create test specification: {result.get('error')}"
    return result