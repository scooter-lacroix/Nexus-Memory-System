FROM rust:1.75 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p nexus-cli

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/nexus /usr/local/bin/nexus
EXPOSE 8768
CMD ["nexus", "serve", "--transport", "http", "--port", "8768"]
