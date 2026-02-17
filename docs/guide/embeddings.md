# Embeddings and Semantic Search Guide

> **High-Performance Vector Search with sqlite-vec**

**Version:** 1.1.0

---

## Table of Contents

- [Overview](#overview)
- [Setup](#setup)
- [Embedding Model](#embedding-model)
- [Vector Search](#vector-search)
- [Performance Considerations](#performance-considerations)
- [Advanced Usage](#advanced-usage)

---

## Overview

Nexus uses **sqlite-vec** for high-performance vector similarity search combined with **sentence-transformers** for embedding generation.

### Key Features

- **Fast:** ~1000 docs/sec on CPU
- **Compact:** 384-dimensional vectors
- **Multilingual:** Support for 100+ languages
- **SQLite Native:** No separate vector database needed
- **Zero Configuration:** Works out of the box

---

## Setup

### Installation

```bash
# Install with embeddings support
pip install nexus-memory-system[embeddings]
```

This installs:
- `sentence-transformers>=3.3.0`
- `torch>=2.5.0`
- `transformers>=4.47.0`
- `sqlite-vec>=0.1.1`

### Configuration

Enable embeddings in configuration:

```bash
# Environment variables
export NEXUS_EMBEDDINGS_ENABLED=true
export NEXUS_EMBEDDING_MODEL=all-MiniLM-L6-v2
export NEXUS_EMBEDDING_DEVICE=cpu
```

Or in `.env` file:

```bash
NEXUS_EMBEDDINGS_ENABLED=true
NEXUS_EMBEDDING_MODEL=all-MiniLM-L6-v2
NEXUS_EMBEDDING_DEVICE=cpu
```

---

## Embedding Model

### Model: all-MiniLM-L6-v2

| Property | Value |
|----------|-------|
| Model Name | all-MiniLM-L6-v2 |
| Dimensions | 384 |
| Speed | ~1000 docs/sec (CPU) |
| Languages | 100+ |
| Model Size | ~80MB |
| Max Sequence Length | 256 tokens |

### Why This Model?

- **Fast:** Optimized for speed
- **Quality:** Good semantic understanding
- **Size:** Small footprint
- **Languages:** Multilingual support
- **License:** Apache 2.0 (permissive)

### Model Download

First time use automatically downloads the model:

```python
from nexus.embeddings import get_embedding_service

# First call downloads and caches model
service = get_embedding_service()
# Downloading: all-MiniLM-L6-v2
# Cached to: ~/.cache/torch/sentence_transformers/
```

### Model Cache Location

```bash
# Linux/macOS
~/.cache/torch/sentence_transformers/

# Windows
C:\Users\<username>\.cache\torch\sentence_transformers\
```

### Custom Model Directory

```python
from nexus.embeddings import EmbeddingService

# Specify custom cache directory
service = EmbeddingService(
    model_name="all-MiniLM-L6-v2",
    cache_dir="/path/to/cache"
)
```

---

## Vector Search

### How It Works

1. **Text -> Embedding:** Convert query to 384-dimensional vector
2. **Vector Search:** Find similar vectors using cosine similarity
3. **Rank Results:** Return most similar memories

### Semantic Search API

```python
from nexus.server import get_memory_manager

async def search_example():
    manager = get_memory_manager()
    await manager.initialize()

    # Semantic search
    results = await manager.search_memories(
        query="user prefers dark theme",
        agent_type="claude-code",
        limit=10,
        threshold=0.7  # Minimum similarity
    )

    for memory in results["results"]:
        print(f"{memory['similarity_score']:.3f}: {memory['content']}")
```

### REST API

```bash
curl -X POST http://localhost:8000/api/v1/memories/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "user prefers dark theme",
    "agent_type": "claude-code",
    "k": 10,
    "threshold": 0.7
  }'
```

### CLI Search

```bash
nexus search "user prefers dark theme" --agent claude-code --limit 10
```

---

## Performance Considerations

### Benchmarks

| Operation | Performance |
|-----------|-------------|
| Embedding generation | ~1000 docs/sec (CPU) |
| Vector search | <10ms for 100K vectors |
| Storage overhead | ~1.5KB per memory (384 * 4 bytes) |
| Memory usage | ~500MB for 1M vectors |

### Optimization Tips

#### 1. Batch Embedding

```python
from nexus.embeddings import get_embedding_service

service = get_embedding_service()

# Batch encode (faster)
texts = ["text1", "text2", "text3", ...]
embeddings = await service.encode(texts, batch_size=32)
```

#### 2. Use GPU if Available

```python
# Use GPU for faster embedding
service = EmbeddingService(
    model_name="all-MiniLM-L6-v2",
    device="cuda"  # or "mps" for Apple Silicon
)
```

#### 3. Limit Search Results

```python
# Always specify limit
results = await manager.search_memories(
    query="...",
    limit=10  # Don't use 1000+
)
```

#### 4. Use Threshold

```python
# Filter low-similarity results
results = await manager.search_memories(
    query="...",
    threshold=0.7  # Only return 70%+ similarity
)
```

### Memory Management

For large datasets (>1M memories):

1. **Archive old memories:** Reduces active search space
2. **Use PostgreSQL:** Better for large-scale deployments
3. **Deduplicate embeddings:** Share embeddings for identical content
4. **定期清理:** Remove inactive memories

---

## Advanced Usage

### Custom Embedding Model

You can use a different sentence-transformers model:

```python
from nexus.embeddings import EmbeddingService

# Use a different model
service = EmbeddingService(
    model_name="all-mpnet-base-v2",  # Better quality, slower
    dimension=768
)
```

Popular alternatives:

| Model | Dimensions | Speed | Quality |
|-------|------------|-------|---------|
| all-MiniLM-L6-v2 | 384 | Fast | Good |
| all-mpnet-base-v2 | 768 | Medium | Excellent |
| paraphrase-multilingual-MiniLM-L12-v2 | 384 | Fast | Multilingual |

### Embedding Similarity

```python
from nexus.embeddings import get_embedding_service
import numpy as np

service = get_embedding_service()

# Encode two texts
embedding1 = await service.encode("user prefers dark mode")
embedding2 = await service.encode("user likes dark theme")

# Compute similarity
similarity = service.compute_similarity(embedding1, embedding2)
print(f"Similarity: {similarity:.3f}")  # 0.0 to 1.0
```

### Batch Similarity

```python
# Compare query against multiple memories
query = "user preferences"
memories = ["user likes dark mode", "user wants light theme", ...]

query_embedding = await service.encode(query)
memory_embeddings = await service.encode(memories)

similarities = await service.compute_similarity_batch(
    query_embedding,
    memory_embeddings
)

# Find most similar
best_idx = np.argmax(similarities)
print(f"Best match: {memories[best_idx]} (similarity: {similarities[best_idx]:.3f})")
```

### Direct sqlite-vec Queries

For advanced use cases, you can query sqlite-vec directly:

```sql
-- Vector search using sqlite-vec extension
SELECT
    m.id,
    m.content,
    m.category,
    distance
FROM memories m
JOIN vec_search(
    'content_embedding',
    '[0.1, 0.2, ...]'  -- 384-dimensional vector
) ON m.id = rowid
WHERE m.agent_type = 'claude-code'
  AND m.is_active = TRUE
ORDER BY distance
LIMIT 10;
```

### Hybrid Search (Text + Vector)

```python
# Combine keyword and semantic search
async def hybrid_search(query: str, agent_type: str):
    manager = get_memory_manager()

    # Semantic search
    semantic_results = await manager.search_memories(
        query=query,
        agent_type=agent_type,
        limit=20
    )

    # Keyword search (via SQL LIKE)
    keyword_results = await manager.database_manager.search_memories(
        query=query,
        agent_type=agent_type,
        search_field="content",
        limit=20
    )

    # Combine and deduplicate
    seen = set()
    combined = []

    for result in semantic_results["results"] + keyword_results["results"]:
        if result["id"] not in seen:
            seen.add(result["id"])
            combined.append(result)

    return combined[:10]
```

---

## Embedding Service API

### Encode Text

```python
from nexus.embeddings import get_embedding_service

service = get_embedding_service()

# Single text
embedding = await service.encode("Hello world")
print(embedding.shape)  # (384,)

# Multiple texts
embeddings = await service.encode(["Hello", "World"])
print(embeddings.shape)  # (2, 384)

# With normalization
embedding = await service.encode("text", normalize=True)
```

### Compute Similarity

```python
# Between two embeddings
similarity = service.compute_similarity(embedding1, embedding2)

# Batch similarity
similarities = await service.compute_similarity_batch(
    query_embedding,
    corpus_embeddings
)
```

### Model Information

```python
info = service.get_model_info()
print(info)
# {
#     "model_name": "all-MiniLM-L6-v2",
#     "dimension": 384,
#     "device": "cpu",
#     "max_seq_length": 256,
#     "is_loaded": True
# }
```

---

## Troubleshooting

### Issue: Slow embedding generation

**Solution:** Use GPU or batch processing

```python
# Use GPU
service = EmbeddingService(device="cuda")

# Batch processing
embeddings = await service.encode(texts, batch_size=64)
```

### Issue: Out of memory

**Solution:** Reduce batch size or use smaller model

```python
# Smaller batch size
embeddings = await service.encode(texts, batch_size=16)

# Or use smaller model
service = EmbeddingService(model_name="all-MiniLM-L6-v2")
```

### Issue: Poor search results

**Solution:** Try different model or threshold

```python
# Use higher quality model
service = EmbeddingService(model_name="all-mpnet-base-v2")

# Adjust threshold
results = await manager.search_memories(
    query="...",
    threshold=0.8  # Higher threshold
)
```

### Issue: Model download fails

**Solution:** Set cache directory or download manually

```bash
# Set cache directory
export TRANSFORMERS_CACHE=/path/to/cache
export HF_HOME=/path/to/cache

# Or pre-download
python -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('all-MiniLM-L6-v2')"
```

---

## Related Documentation

- [ARCHITECTURE.md](../../ARCHITECTURE.md) - Embedding system architecture
- [Memory Types Guide](memory-types.md) - Memory categorization
- [API Reference](../api/rest-api.md) - REST API for search

---

**Last Updated:** 2025-12-23
