.PHONY: build release fmt lint test check run-http run-stdio

build:
	cargo build --workspace

release:
	cargo build --release -p nexus-cli

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets

test:
	cargo test --workspace

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets
	cargo test --workspace

run-http:
	cargo run -p nexus-cli -- serve --transport web --port 8768

run-stdio:
	cargo run -p nexus-cli -- serve --transport stdio
