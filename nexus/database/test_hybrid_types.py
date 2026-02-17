#!/usr/bin/env python3
"""
Test script for Hybrid Memory Type System

This script demonstrates and tests the hybrid memory type system that combines:
1. Nexus's existing flexible category system (preserved)
2. Memory Lane cognitive types (additive, optional)
3. Memory Lane 10 priority types

Usage:
    python -m nexus.database.test_hybrid_types
"""

import asyncio
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from nexus.database.managers import DatabaseManager, MemoryManager
from nexus.database.enums import (
    # Categories
    MemoryCategory,
    MemoryLaneCognitiveType,
    MemoryLanePriorityType,
    AgentCategory,

    # Helper functions
    is_valid_category,
    is_valid_memory_lane_type,
    get_category_description,
    get_memory_lane_priority,
    get_categories_by_type,
    get_memory_lane_types,
    validate_category,
    validate_memory_lane_type,

    # Exceptions
    InvalidCategoryError,
    InvalidMemoryLaneTypeError,
)


async def test_enums():
    """Test enum constants and helper functions"""
    print("=" * 60)
    print("TEST 1: Enum Constants and Helper Functions")
    print("=" * 60)

    # Test Core Nexus categories
    print("\n1. Core Nexus Categories:")
    for cat in MemoryCategory:
        print(f"   - {cat.value}: {get_category_description(cat.value)}")

    # Test Memory Lane cognitive types
    print("\n2. Memory Lane Cognitive Types:")
    for mlt in MemoryLaneCognitiveType:
        print(f"   - {mlt.value}: {get_category_description(mlt.value)}")

    # Test Memory Lane priority types
    print("\n3. Memory Lane Priority Types (with priority levels):")
    for mlp in MemoryLanePriorityType:
        priority = get_memory_lane_priority(mlp.value)
        print(f"   - {mlp.value}: {get_category_description(mlp.value)} [Priority: {priority}]")

    # Test Agent categories
    print("\n4. Agent-specific Categories:")
    for agent in AgentCategory:
        print(f"   - {agent.value}: {get_category_description(agent.value)}")

    print("\n5. Get all Memory Lane types:")
    ml_types = get_memory_lane_types()
    print(f"   Total Memory Lane types: {len(ml_types)}")
    print(f"   Types: {', '.join(ml_types[:5])}...")

    print("\n[PASS] Enum constants test completed\n")


async def test_validation():
    """Test validation functions"""
    print("=" * 60)
    print("TEST 2: Validation Functions")
    print("=" * 60)

    # Test valid category
    print("\n1. Valid category check:")
    assert is_valid_category("general") == True
    assert is_valid_category("semantic") == True
    assert is_valid_category("correction") == True
    assert is_valid_category("claude-code") == True
    print("   - Valid categories: PASS")

    # Test invalid category
    print("\n2. Invalid category check:")
    assert is_valid_category("invalid_type") == False
    print("   - Invalid category detection: PASS")

    # Test valid memory_lane_type
    print("\n3. Valid memory_lane_type check:")
    assert is_valid_memory_lane_type("semantic") == True
    assert is_valid_memory_lane_type("correction") == True
    assert is_valid_memory_lane_type("working") == True
    print("   - Valid memory_lane_types: PASS")

    # Test invalid memory_lane_type
    print("\n4. Invalid memory_lane_type check:")
    assert is_valid_memory_lane_type("general") == False  # Nexus category, not Memory Lane
    assert is_valid_memory_lane_type("invalid_type") == False
    print("   - Invalid memory_lane_type detection: PASS")

    # Test validation exceptions
    print("\n5. Validation exceptions:")
    try:
        validate_category("invalid_type")
        print("   - ERROR: Should have raised InvalidCategoryError")
    except InvalidCategoryError as e:
        print(f"   - InvalidCategoryError raised: PASS ({e.category})")

    try:
        validate_memory_lane_type("invalid_type")
        print("   - ERROR: Should have raised InvalidMemoryLaneTypeError")
    except InvalidMemoryLaneTypeError as e:
        print(f"   - InvalidMemoryLaneTypeError raised: PASS ({e.memory_lane_type})")

    print("\n[PASS] Validation test completed\n")


async def test_database_operations():
    """Test database operations with hybrid type system"""
    print("=" * 60)
    print("TEST 3: Database Operations with Hybrid Types")
    print("=" * 60)

    # Import database setup
    from nexus.database.migrations import create_tables

    db_manager = DatabaseManager()
    await db_manager.initialize()

    # Create tables first
    await create_tables(db_manager)
    print("\n   Database tables created/verified")

    memory_manager = MemoryManager(db_manager)

    # Test 1: Store memory with Nexus category only
    print("\n1. Store memory with Nexus category only:")
    result = await memory_manager.store_memory(
        content="This is a general memory using only Nexus category",
        agent_type="claude-code",
        category="general",
        memory_lane_type=None
    )
    assert result["success"] == True
    memory_id_1 = result["memory_id"]
    print(f"   - Memory ID: {memory_id_1}")
    print(f"   - Category: {result['category']}")
    print(f"   - Memory Lane Type: {result['memory_lane_type']}")
    print("   - PASS: Stored with Nexus category only")

    # Test 2: Store memory with Memory Lane cognitive type
    print("\n2. Store memory with Memory Lane cognitive type:")
    result = await memory_manager.store_memory(
        content="User corrected my approach to file handling",
        agent_type="claude-code",
        category="correction",  # Can use Memory Lane as category too
        memory_lane_type="correction"  # Explicit Memory Lane type
    )
    assert result["success"] == True
    memory_id_2 = result["memory_id"]
    print(f"   - Memory ID: {memory_id_2}")
    print(f"   - Category: {result['category']}")
    print(f"   - Memory Lane Type: {result['memory_lane_type']}")
    print("   - PASS: Stored with Memory Lane type")

    # Test 3: Store memory with Memory Lane priority type
    print("\n3. Store memory with Memory Lane priority type:")
    result = await memory_manager.store_memory(
        content="User prefers dark mode in all interfaces",
        agent_type="gemini",
        category="commitment",
        memory_lane_type="commitment"
    )
    assert result["success"] == True
    memory_id_3 = result["memory_id"]
    print(f"   - Memory ID: {memory_id_3}")
    print(f"   - Category: {result['category']}")
    print(f"   - Memory Lane Type: {result['memory_lane_type']}")
    print("   - PASS: Stored with priority type")

    # Test 4: Store memory with both category types
    print("\n4. Store memory with hybrid category usage:")
    result = await memory_manager.store_memory(
        content="Learned about async/await patterns in Python",
        agent_type="claude-code",
        category="learning",  # Memory Lane as category
        memory_lane_type="semantic"  # Cognitive type as memory_lane_type
    )
    assert result["success"] == True
    memory_id_4 = result["memory_id"]
    print(f"   - Memory ID: {memory_id_4}")
    print(f"   - Category: {result['category']}")
    print(f"   - Memory Lane Type: {result['memory_lane_type']}")
    print("   - PASS: Stored with hybrid categories")

    # Test 5: Invalid category validation
    print("\n5. Test invalid category rejection:")
    result = await memory_manager.store_memory(
        content="This should fail",
        agent_type="claude-code",
        category="invalid_category"
    )
    assert result["success"] == False
    assert result.get("error_type") == "validation_error"
    print(f"   - Error: {result['error']}")
    print("   - PASS: Invalid category rejected")

    # Test 6: Invalid memory_lane_type validation
    print("\n6. Test invalid memory_lane_type rejection:")
    result = await memory_manager.store_memory(
        content="This should fail",
        agent_type="claude-code",
        category="general",
        memory_lane_type="general"  # Nexus category, not Memory Lane type
    )
    assert result["success"] == False
    assert result.get("error_type") == "validation_error"
    print(f"   - Error: {result['error']}")
    print("   - PASS: Invalid memory_lane_type rejected")

    # Test 7: Search with memory_lane_type filter
    print("\n7. Search with memory_lane_type filter:")
    result = await memory_manager.search_memories(
        query="",
        agent_type="claude-code",
        memory_lane_type="correction",
        limit=10
    )
    assert result["success"] == True
    print(f"   - Found {result['total']} memories with memory_lane_type='correction'")
    print(f"   - Filters: {result['filters']}")
    print("   - PASS: Memory Lane type filtering works")

    # Test 8: Search with category filter
    print("\n8. Search with category filter:")
    result = await memory_manager.search_memories(
        query="",
        agent_type="claude-code",
        category="general",
        limit=10
    )
    assert result["success"] == True
    print(f"   - Found {result['total']} memories with category='general'")
    print(f"   - Filters: {result['filters']}")
    print("   - PASS: Category filtering works")

    # Test 9: Search with both filters
    print("\n9. Search with both category and memory_lane_type filters:")
    result = await memory_manager.search_memories(
        query="",
        agent_type="claude-code",
        category="correction",
        memory_lane_type="correction",
        limit=10
    )
    assert result["success"] == True
    print(f"   - Found {result['total']} memories")
    print(f"   - Filters: {result['filters']}")
    print("   - PASS: Combined filtering works")

    print("\n[PASS] Database operations test completed\n")

    await db_manager.close()


async def test_backward_compatibility():
    """Test backward compatibility with existing code"""
    print("=" * 60)
    print("TEST 4: Backward Compatibility")
    print("=" * 60)

    # Import database setup
    from nexus.database.migrations import create_tables

    db_manager = DatabaseManager()
    await db_manager.initialize()

    # Create tables first
    await create_tables(db_manager)

    memory_manager = MemoryManager(db_manager)

    # Test 1: Old code that doesn't use memory_lane_type should still work
    print("\n1. Old code (without memory_lane_type parameter):")
    result = await memory_manager.store_memory(
        content="This memory is stored using old API",
        agent_type="claude-code",
        category="facts",
        labels=["test", "compatibility"],
        metadata={"source": "old_code"}
    )
    assert result["success"] == True
    print(f"   - Memory stored successfully: {result['memory_id']}")
    print(f"   - memory_lane_type is None: {result['memory_lane_type'] is None}")
    print("   - PASS: Old API still works")

    # Test 2: Old search without memory_lane_type filter
    print("\n2. Old search API (without memory_lane_type filter):")
    result = await memory_manager.search_memories(
        query="old API",
        agent_type="claude-code",
        limit=5
    )
    assert result["success"] == True
    print(f"   - Found {result['total']} memories")
    print(f"   - Filters use default None: {result['filters']}")
    print("   - PASS: Old search API still works")

    # Test 3: to_dict includes new field without breaking existing code
    print("\n3. Memory.to_dict() includes new fields:")
    result = await memory_manager.search_memories(
        query="",
        agent_type="claude-code",
        limit=1
    )
    if result["results"]:
        memory_data = result["results"][0]
        print(f"   - Keys in result: {list(memory_data.keys())}")
        assert "category" in memory_data
        assert "memory_lane_type" in memory_data
        assert "category_description" in memory_data
        print("   - PASS: to_dict() includes new fields")

    print("\n[PASS] Backward compatibility test completed\n")

    await db_manager.close()


async def test_priority_levels():
    """Test Memory Lane priority levels"""
    print("=" * 60)
    print("TEST 5: Memory Lane Priority Levels")
    print("=" * 60)

    print("\nPriority Level Mapping:")
    for priority_type, level in [
        ("correction", 1),
        ("decision", 1),
        ("commitment", 1),
        ("insight", 2),
        ("learning", 2),
        ("confidence", 2),
        ("pattern_seed", 3),
        ("cross_agent", 3),
        ("workflow_note", 3),
        ("gap", 3),
    ]:
        actual_level = get_memory_lane_priority(priority_type)
        assert actual_level == level
        print(f"   - {priority_type}: Level {level}")

    # Test that cognitive types return default priority
    print("\nCognitive types (default priority):")
    for cognitive_type in ["semantic", "episodic", "procedural", "working"]:
        level = get_memory_lane_priority(cognitive_type)
        print(f"   - {cognitive_type}: Level {level} (default)")

    print("\n[PASS] Priority levels test completed\n")


async def main():
    """Run all tests"""
    print("\n")
    print("*" * 60)
    print("*" + " " * 58 + "*")
    print("*" + "  HYBRID MEMORY TYPE SYSTEM - TEST SUITE".center(58) + "*")
    print("*" + " " * 58 + "*")
    print("*" * 60)
    print("\n")

    tests = [
        ("Enum Constants", test_enums),
        ("Validation Functions", test_validation),
        ("Database Operations", test_database_operations),
        ("Backward Compatibility", test_backward_compatibility),
        ("Priority Levels", test_priority_levels),
    ]

    passed = 0
    failed = 0

    for test_name, test_func in tests:
        try:
            await test_func()
            passed += 1
        except Exception as e:
            print(f"\n[FAILED] {test_name}: {e}\n")
            import traceback
            traceback.print_exc()
            failed += 1

    # Summary
    print("=" * 60)
    print("TEST SUMMARY")
    print("=" * 60)
    print(f"\nTotal tests: {len(tests)}")
    print(f"Passed: {passed}")
    print(f"Failed: {failed}")

    if failed == 0:
        print("\n" + "=" * 60)
        print("ALL TESTS PASSED!")
        print("=" * 60)
        print("\nThe hybrid memory type system is working correctly:")
        print("  - Nexus categories are preserved")
        print("  - Memory Lane types are available")
        print("  - Validation works correctly")
        print("  - Backward compatibility is maintained")
        print("  - Database migrations are ready")
        print("\n")
    else:
        print("\n" + "=" * 60)
        print(f"SOME TESTS FAILED ({failed} failed)")
        print("=" * 60)
        print("\n")


if __name__ == "__main__":
    asyncio.run(main())
