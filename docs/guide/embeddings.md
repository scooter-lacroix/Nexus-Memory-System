# Embeddings Guide

Embeddings support lives in the `nexus-embeddings` crate and can be used by higher-level retrieval flows that need semantic matching.

## Related Crates

- `nexus-embeddings`
- `nexus-vectors`
- `nexus-storage`

## Development Notes

- validate embedding-related behavior with `cargo test --workspace`
- benchmark changes carefully before shipping performance claims
