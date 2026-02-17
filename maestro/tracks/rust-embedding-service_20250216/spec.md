# Spec: Rust Embedding Service

**Track ID:** rust-embedding-service_20250216
**Type:** Feature
**Status:** New

---

## Overview

Implement the embedding service in Rust using ONNX Runtime (ort) bindings for the all-MiniLM-L6-v2 model. This provides 384-dimensional vector generation with <5ms latency target.

**Python Mapping:** `nexus/embeddings/service.py`

---

## Functional Requirements

### FR1: EmbeddingService Trait

```rust
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, Error>;
    async fn encode_batch(&self, texts: &[str]) -> Result<Vec<Vec<f32>>, Error>;
    fn dimension(&self) -> usize { 384 }
}
```

### FR2: ONNX Runtime Implementation

- Load all-MiniLM-L6-v2 ONNX model
- Implement async inference using ort
- Support single and batch encoding
- Thread-safe for concurrent access

### FR3: Tokenization

- Integrate with tokenization library (rust-tokenizers or huggingface)
- Handle truncation and padding
- Support special tokens

### FR4: Error Handling

- Model loading failures
- Invalid input handling
- Inference error recovery

---

## Non-Functional Requirements

### NFR1: Performance

| Metric | Target |
|--------|--------|
| Single encode latency | <5ms |
| Batch encode (10) | <20ms |
| Memory footprint | <100MB per instance |

### NFR2: Compatibility

- Output format matches Python sentence-transformers
- 384-dimensional float vectors
- Identical results for same input

### NFR3: Code Quality

- 95%+ test coverage
- All unsafe blocks documented
- Clippy clean

---

## Acceptance Criteria

### AC1: Service Creation and Inference

```rust
let service = OrtEmbeddingService::new("models/all-MiniLM-L6-v2.onnx").await?;
let embedding = service.encode("Hello world").await?;
assert_eq!(embedding.len(), 384);
```

### AC2: Performance Target

```bash
cargo bench --bench embedding
# Result: <5ms p95 latency
```

### AC3: Batch Processing

```rust
let texts = vec!["text1", "text2", "text3"];
let embeddings = service.encode_batch(&texts).await?;
assert_eq!(embeddings.len(), 3);
assert!(embeddings.iter().all(|e| e.len() == 384));
```

### AC4: Python Compatibility

- Identical embeddings for same inputs as Python version
- Within floating-point tolerance (1e-5)

---

## Dependencies

### External Crates

```toml
[dependencies]
ort = "0.1"           # ONNX Runtime
tokenizers = "0.20"   # HuggingFace tokenizers
async-trait = "0.1"
tokio = { version = "1.40", features = ["rt-multi-thread"] }
```

### Local Dependencies

- `nexus-core` - Error types, core traits

---

## Out of Scope

- Model training/exporting (use pre-trained ONNX)
- Alternative embedding models (future extension)
- GPU acceleration (CPU-only is sufficient)

---

## References

- Python implementation: `nexus/embeddings/service.py`
- Model: all-MiniLM-L6-v2 (384 dimensions)
- ONNX Runtime: https://github.com/pykeio/ort
- CLAUDE.md: Rust Port Guide

---

**Version:** 1.0
**Created:** 2025-02-16
