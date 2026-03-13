# Docker Deployment

Docker deployment should package the Rust workspace build output and run the `nexus` binary directly.

## Recommended Pattern

1. Build `nexus-cli` in a Rust builder image.
2. Copy the compiled binary into a slim runtime image.
3. Mount a persistent location for the Nexus database.

## Example Outline

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p nexus-cli

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/nexus /usr/local/bin/nexus
CMD ["nexus", "serve", "--transport", "http", "--port", "8768"]
```
