# Docker Deployment

Docker deployment should package the Rust workspace build output and run the `nexus` binary directly. Keep the database path on persistent storage if you want memory, digests, and dreaming outputs to survive container restarts.

## Recommended Pattern

1. Build the `nexus-memory` package in a Rust builder image.
2. Copy the compiled binary into a slim runtime image.
3. Mount a persistent location for the Nexus database.

## Example Outline

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p nexus-memory

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/nexus /usr/local/bin/nexus
CMD ["nexus", "serve", "--transport", "http", "--port", "8768"]
```
