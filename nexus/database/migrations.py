"""
Database migrations for Nexus Memory System
"""

import asyncio
from datetime import datetime, UTC
from pathlib import Path
from typing import Optional
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, text, and_
from loguru import logger
import numpy as np

from ..config import config
from .models import Base, AgentNamespace, Memory
from .managers import DatabaseManager
from .enums import VALID_MEMORY_LANE_TYPES

# Import for embedding migration
from ..embeddings.service import get_embedding_service

try:
    from ..embeddings.sqlite_vec import SQLiteVecStore
    _sqlite_vec_available = True
except ImportError:
    _sqlite_vec_available = False
    SQLiteVecStore = None

try:
    import sqlite_vec
except ImportError:
    sqlite_vec = None


async def setup_database() -> bool:
    """Setup database and create tables"""
    try:
        # Ensure database directory exists
        db_path = Path(config.database_path)
        db_path.parent.mkdir(parents=True, exist_ok=True)

        # Initialize database
        db_manager = DatabaseManager()
        await db_manager.initialize()

        # Create tables
        await create_tables(db_manager)

        # Initialize default namespaces
        await initialize_namespaces(db_manager)

        logger.info(f"Database setup complete: {config.database_path}")
        return True

    except Exception as e:
        logger.error(f"Failed to setup database: {e}")
        return False


async def create_tables(db_manager: DatabaseManager) -> None:
    """Create all database tables"""
    try:
        async with db_manager.async_engine.begin() as conn:
            await conn.run_sync(Base.metadata.create_all)
        logger.info("Database tables created successfully")

    except Exception as e:
        logger.error(f"Failed to create tables: {e}")
        raise


async def initialize_namespaces(db_manager: DatabaseManager) -> None:
    """Initialize default agent namespaces"""
    try:
        from ..config.agent_namespaces import AGENT_NAMESPACES, get_agent_description

        async with db_manager.get_async_session() as session:
            for agent_type, namespace_name in AGENT_NAMESPACES.items():
                # Check if namespace already exists
                stmt = select(AgentNamespace).where(
                    AgentNamespace.agent_type == agent_type
                )
                result = await session.execute(stmt)
                existing = result.scalar_one_or_none()

                if not existing:
                    namespace = AgentNamespace(
                        name=namespace_name,
                        agent_type=agent_type,
                        description=get_agent_description(agent_type),
                        created_at=datetime.now(UTC),
                    )
                    session.add(namespace)

            await session.commit()
        logger.info("Agent namespaces initialized successfully")

    except Exception as e:
        logger.error(f"Failed to initialize namespaces: {e}")
        raise


async def get_database_info() -> dict:
    """Get database information and statistics"""
    try:
        db_manager = DatabaseManager()
        await db_manager.initialize()

        async with db_manager.get_async_session() as session:
            # Get table counts
            tables_info = {}

            # Memory count
            memory_result = await session.execute(text("SELECT COUNT(*) FROM memories"))
            tables_info["memories"] = memory_result.scalar()

            # Specifications count
            spec_result = await session.execute(text("SELECT COUNT(*) FROM task_specifications"))
            tables_info["task_specifications"] = spec_result.scalar()

            # Namespaces count
            ns_result = await session.execute(text("SELECT COUNT(*) FROM agent_namespaces"))
            tables_info["agent_namespaces"] = ns_result.scalar()

            # Database size
            if config.database_path.endswith('.db'):
                db_file = Path(config.database_path)
                db_size = db_file.stat().st_size if db_file.exists() else 0
                tables_info["database_size_bytes"] = db_size
                tables_info["database_size_mb"] = round(db_size / (1024 * 1024), 2)

        await db_manager.close()

        return {
            "success": True,
            "database_path": config.database_path,
            "database_url": config.database_connection_url,
            "tables": tables_info,
            "initialized_at": datetime.now(UTC).isoformat(),
        }

    except Exception as e:
        logger.error(f"Failed to get database info: {e}")
        return {
            "success": False,
            "error": str(e),
            "database_path": config.database_path,
        }


async def run_migrations() -> bool:
    """Run database migrations"""
    try:
        # Check if we need to migrate from old memori database
        old_db_path = Path.home() / ".memori-mcp-server" / "memory.db"
        new_db_path = Path(config.database_path)

        if old_db_path.exists() and not new_db_path.exists():
            logger.info("Migrating data from old memori database...")
            await migrate_from_memori(old_db_path, new_db_path)

        # Run any version migrations here
        await migrate_database_version()

        logger.info("Database migrations completed successfully")
        return True

    except Exception as e:
        logger.error(f"Failed to run migrations: {e}")
        return False


async def migrate_from_memori(old_db_path: Path, new_db_path: Path) -> None:
    """Migrate data from old memori database"""
    try:
        # Ensure new database directory exists
        new_db_path.parent.mkdir(parents=True, exist_ok=True)

        # Copy the old database file
        import shutil
        shutil.copy2(old_db_path, new_db_path)

        # Update schema if needed
        db_manager = DatabaseManager()
        await db_manager.initialize()

        async with db_manager.async_engine.begin() as conn:
            # Add any new columns or tables
            await conn.run_sync(Base.metadata.create_all)

        await db_manager.close()
        logger.info(f"Successfully migrated database from {old_db_path} to {new_db_path}")

    except Exception as e:
        logger.error(f"Failed to migrate from memori database: {e}")
        raise


async def migrate_database_version() -> None:
    """Run version-specific database migrations"""
    try:
        db_manager = DatabaseManager()
        await db_manager.initialize()

        async with db_manager.get_async_session() as session:
            # Check current version
            try:
                version_result = await session.execute(
                    text("SELECT value FROM system_settings WHERE key = 'database_version'")
                )
                current_version = version_result.scalar() or "1.0.0"
            except Exception:
                # system_settings table doesn't exist, create it
                await session.execute(text("""
                    CREATE TABLE IF NOT EXISTS system_settings (
                        key TEXT PRIMARY KEY,
                        value TEXT NOT NULL,
                        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                    )
                """))
                current_version = "1.0.0"

            # Run migrations based on version
            if current_version == "1.0.0":
                # Migration to 1.1.0
                await migrate_to_v1_1_0(session)
                current_version = "1.1.0"

            if current_version == "1.1.0":
                # Migration to 1.2.0 - Add memory_lane_type field
                await migrate_to_v1_2_0(session)
                current_version = "1.2.0"

            if current_version == "1.2.0":
                # Migration to 1.3.0 - Add sqlite-vec support
                await migrate_to_v1_3_0(session)
                current_version = "1.3.0"

            # Update version
            await session.execute(
                text("""
                    INSERT OR REPLACE INTO system_settings (key, value, updated_at)
                    VALUES ('database_version', :version, :updated_at)
                """),
                {"version": current_version, "updated_at": datetime.now(UTC)}
            )

            await session.commit()

        await db_manager.close()

    except Exception as e:
        logger.error(f"Failed to migrate database version: {e}")
        raise


async def migrate_to_v1_1_0(session: AsyncSession) -> None:
    """Migrate database to version 1.1.0"""
    try:
        # Add new columns to memories table
        await session.execute(text("""
            ALTER TABLE memories ADD COLUMN relevance_score REAL
        """))
        await session.execute(text("""
            ALTER TABLE memories ADD COLUMN content_embedding TEXT
        """))
        await session.execute(text("""
            ALTER TABLE memories ADD COLUMN embedding_model TEXT
        """))
        await session.execute(text("""
            ALTER TABLE memories ADD COLUMN last_accessed TIMESTAMP
        """))
        await session.execute(text("""
            ALTER TABLE memories ADD COLUMN access_count INTEGER DEFAULT 0
        """))

        # Add new tables
        await session.execute(text("""
            CREATE TABLE IF NOT EXISTS memory_relations (
                id INTEGER PRIMARY KEY,
                source_memory_id INTEGER NOT NULL,
                target_memory_id INTEGER NOT NULL,
                relation_type TEXT NOT NULL,
                strength REAL DEFAULT 1.0,
                metadata TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (source_memory_id) REFERENCES memories (id),
                FOREIGN KEY (target_memory_id) REFERENCES memories (id),
                UNIQUE(source_memory_id, target_memory_id, relation_type)
            )
        """))

        await session.execute(text("""
            CREATE TABLE IF NOT EXISTS system_metrics (
                id INTEGER PRIMARY KEY,
                metric_name TEXT NOT NULL,
                metric_value REAL NOT NULL,
                metric_unit TEXT,
                metadata TEXT,
                recorded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        """))

        logger.info("Migrated database to version 1.1.0")

    except Exception as e:
        logger.error(f"Failed to migrate to v1.1.0: {e}")
        raise


async def migrate_to_v1_2_0(session: AsyncSession) -> None:
    """
    Migrate database to version 1.2.0 - Add memory_lane_type field

    This migration adds the hybrid memory type system support:
    - Adds memory_lane_type column to memories table (nullable)
    - Adds CHECK constraint for valid memory_lane_type values
    - Adds indexes for memory_lane_type queries
    - Preserves backward compatibility with existing data
    """
    try:
        # Check if memory_lane_type column already exists
        check_column = await session.execute(text("""
            SELECT COUNT(*) FROM pragma_table_info('memories') WHERE name = 'memory_lane_type'
        """))
        column_exists = check_column.scalar() > 0

        if not column_exists:
            # Add memory_lane_type column (nullable for backward compatibility)
            await session.execute(text("""
                ALTER TABLE memories ADD COLUMN memory_lane_type TEXT
            """))
            logger.info("Added memory_lane_type column to memories table")

            # Build the CHECK constraint with all valid Memory Lane types
            valid_types = "', '".join(VALID_MEMORY_LANE_TYPES)
            check_constraint_sql = f"""
                ALTER TABLE memories ADD CONSTRAINT ck_memory_lane_type_valid
                CHECK (memory_lane_type IS NULL OR memory_lane_type IN ('{valid_types}'))
            """
            await session.execute(text(check_constraint_sql))
            logger.info("Added CHECK constraint for memory_lane_type")

            # Create index for memory_lane_type queries
            await session.execute(text("""
                CREATE INDEX IF NOT EXISTS idx_memory_namespace_lane_type
                ON memories(namespace_id, memory_lane_type)
            """))
            await session.execute(text("""
                CREATE INDEX IF NOT EXISTS idx_memory_category_lane_type
                ON memories(category, memory_lane_type)
            """))
            logger.info("Created indexes for memory_lane_type queries")

        else:
            logger.info("memory_lane_type column already exists, skipping migration")

        logger.info("Migrated database to version 1.2.0 (Hybrid Memory Type System)")

    except Exception as e:
        logger.error(f"Failed to migrate to v1.2.0: {e}")
        raise


async def migrate_to_v1_3_0(session: AsyncSession) -> None:
    """
    Migrate database to version 1.3.0 - Add sqlite-vec support.

    This migration:
    1. Creates the vec0 virtual table for vector embeddings
    2. Populates embeddings for existing memories
    3. Enables semantic search with sqlite-vec

    Requires sqlite-vec to be installed.
    """
    try:
        if sqlite_vec is None:
            logger.warning(
                "sqlite-vec not installed, skipping v1.3.0 migration. "
                "Install with: pip install sqlite-vec>=0.1.1"
            )
            return

        # Get database path from config
        db_path = Path(config.database_path)

        # Use synchronous connection for vec0 table creation
        import sqlite3
        conn = sqlite3.connect(str(db_path))
        conn.enable_load_extension(True)
        sqlite_vec.load(conn)

        # Create vec0 virtual table
        try:
            conn.execute("""
                CREATE VIRTUAL TABLE IF NOT EXISTS memory_embeddings
                USING vec0(
                    embedding_float(384),
                    memory_id INTEGER PRIMARY KEY
                )
            """)
            conn.commit()
            logger.info("Created memory_embeddings vec0 table")
        except Exception as e:
            logger.error(f"Failed to create vec0 table: {e}")
            conn.close()
            raise
        finally:
            conn.close()

        # Populate embeddings for existing memories
        await populate_embeddings_for_existing_memories()

        logger.info("Migrated database to version 1.3.0 (sqlite-vec support)")

    except Exception as e:
        logger.error(f"Failed to migrate to v1.3.0: {e}")
        raise


async def populate_embeddings_for_existing_memories() -> None:
    """
    Populate vector embeddings for existing memories that don't have them.

    This function:
    1. Finds memories without embeddings
    2. Generates embeddings using sentence-transformers
    3. Inserts them into the sqlite-vec table

    This is useful as a migration or for backfilling embeddings.
    """
    try:
        if not _sqlite_vec_available or sqlite_vec is None:
            logger.warning("sqlite-vec not installed, skipping embedding population")
            return

        db_manager = DatabaseManager()
        await db_manager.initialize()

        # Initialize vector store
        vec_store = SQLiteVecStore(config.database_path)
        await vec_store.initialize()

        # Get embedding service
        embedding_service = get_embedding_service()

        async with db_manager.get_async_session() as session:
            # Find memories without content_embedding
            stmt = select(Memory).where(
                and_(
                    Memory.is_active == True,
                    Memory.content_embedding.is_(None),
                )
            ).limit(1000)  # Process in batches

            result = await session.execute(stmt)
            memories = result.scalars().all()

            if not memories:
                logger.info("No memories without embeddings found")
                return

            logger.info(f"Generating embeddings for {len(memories)} memories...")

            # Process in batches to avoid overwhelming the model
            batch_size = 32
            for i in range(0, len(memories), batch_size):
                batch = memories[i:i + batch_size]

                # Generate embeddings for batch
                texts = [m.content for m in batch]
                embeddings = await embedding_service.encode(
                    texts,
                    normalize=True,
                    batch_size=batch_size,
                )

                # Prepare data for batch insert
                embedding_tuples = [
                    (m.id, embeddings[j])
                    for j, m in enumerate(batch)
                ]

                # Batch insert into vector store
                count = await vec_store.insert_batch(embedding_tuples)

                # Update memory records with embedding metadata
                for memory in batch:
                    memory.embedding_model = embedding_service.model_name
                    # content_embedding will be updated with the array

                await session.commit()

                logger.info(f"Processed batch {i // batch_size + 1}: {count} embeddings")

            logger.info(f"Successfully populated embeddings for {len(memories)} memories")

        await db_manager.close()

    except Exception as e:
        logger.error(f"Failed to populate embeddings: {e}")
        raise


async def backup_database(backup_path: Optional[str] = None) -> bool:
    """Create a backup of the database"""
    try:
        if not backup_path:
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            backup_path = f"nexus_backup_{timestamp}.db"

        db_file = Path(config.database_path)
        backup_file = Path(backup_path)

        if db_file.exists():
            import shutil
            shutil.copy2(db_file, backup_file)
            logger.info(f"Database backed up to: {backup_file}")
            return True
        else:
            logger.warning("Database file does not exist, nothing to backup")
            return False

    except Exception as e:
        logger.error(f"Failed to backup database: {e}")
        return False


async def restore_database(backup_path: str) -> bool:
    """Restore database from backup"""
    try:
        backup_file = Path(backup_path)
        db_file = Path(config.database_path)

        if not backup_file.exists():
            logger.error(f"Backup file does not exist: {backup_path}")
            return False

        # Create backup of current database before restore
        if db_file.exists():
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            current_backup = f"nexus_before_restore_{timestamp}.db"
            import shutil
            shutil.copy2(db_file, current_backup)
            logger.info(f"Current database backed up to: {current_backup}")

        # Restore from backup
        import shutil
        shutil.copy2(backup_file, db_file)
        logger.info(f"Database restored from: {backup_path}")
        return True

    except Exception as e:
        logger.error(f"Failed to restore database: {e}")
        return False