#!/usr/bin/env bash
# Publish all workspace crates to crates.io in topological order.
# Usage: ./scripts/publish-crates.sh [--dry-run]
#
# Requires CARGO_REGISTRY_TOKEN to be set (or `cargo login` already done).

set -euo pipefail

DRY_RUN=""
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN="--dry-run"
  echo "=== DRY RUN MODE ==="
fi

VERSION=$(grep -A2 '^\[workspace\.package\]' Cargo.toml | grep '^version' | sed 's/.*"\(.*\)".*/\1/')
echo "Publishing workspace version: ${VERSION}"

# Topological order — dependencies before dependents
CRATES=(
  nexus-memory-core
  nexus-memory-storage
  nexus-memory-llm
  nexus-memory-embeddings
  nexus-memory-lephase
  nexus-memory-vectors
  nexus-memory-hooks
  nexus-memory-orchestrator
  nexus-memory-agent
  nexus-memory-mcp
  nexus-memory-web
  nexus-memory
)

PUBLISHED=0
SKIPPED=0
FAILED=0

for CRATE in "${CRATES[@]}"; do
  echo ""
  echo "━━━ ${CRATE} v${VERSION} ━━━"

  # Skip if already published (only in non-dry-run mode)
  if [[ -z "${DRY_RUN}" ]]; then
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
      "https://crates.io/api/v1/crates/${CRATE}/${VERSION}")

    if [[ "${HTTP_CODE}" == "200" ]]; then
      echo "  Already published — skipping"
      SKIPPED=$((SKIPPED + 1))
      continue
    fi
  fi

  # Publish (with retries for index propagation)
  MAX_RETRIES=3
  RETRY=0
  SUCCESS=false

  while [[ "${RETRY}" -lt "${MAX_RETRIES}" ]]; do
    if cargo publish -p "${CRATE}" --locked ${DRY_RUN} --allow-dirty 2>&1; then
      SUCCESS=true
      break
    fi
    RETRY=$((RETRY + 1))
    if [[ "${RETRY}" -lt "${MAX_RETRIES}" ]]; then
      echo "  Attempt ${RETRY} failed, waiting 60s for index propagation..."
      sleep 60
    fi
  done

  if [[ "${SUCCESS}" == "true" ]]; then
    echo "  ✓ Published"
    PUBLISHED=$((PUBLISHED + 1))
  else
    echo "  ✗ Failed after ${MAX_RETRIES} attempts"
    FAILED=$((FAILED + 1))
    exit 1
  fi

  # Wait for crates.io index to propagate before next crate
  if [[ -z "${DRY_RUN}" ]]; then
    echo "  Waiting 30s for index propagation..."
    sleep 30
  fi
done

echo ""
echo "━━━ Summary ━━━"
echo "  Published: ${PUBLISHED}"
echo "  Skipped:   ${SKIPPED}"
echo "  Failed:    ${FAILED}"
