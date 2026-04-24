#!/usr/bin/env bash
# Publish all workspace crates to crates.io in topological (dependency) order.
#
# Usage:
#   ./scripts/publish-crates.sh              # publish for real
#   ./scripts/publish-crates.sh --dry-run    # package & verify only
#
# The script automatically discovers workspace members and resolves their
# publish order using `cargo metadata`, so it never needs manual updates
# when crates are added, removed, or re-wired.
#
# Requires CARGO_REGISTRY_TOKEN to be set (or `cargo login` already done).

set -euo pipefail

DRY_RUN=""
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN="--dry-run"
  echo "=== DRY RUN MODE ==="
fi

# ---------- extract workspace version ----------
VERSION=$(cargo metadata --format-version 1 --no-deps \
  | python3 -c "
import json, sys
meta = json.load(sys.stdin)
# All workspace members share the same version; take the first.
print(meta['packages'][0]['version'])
")
echo "Publishing workspace version: ${VERSION}"

# ---------- resolve topological publish order ----------
# Uses `cargo metadata` to build a dependency graph of workspace-internal
# crates and emits them in topological order (leaves first).
CRATES=($(cargo metadata --format-version 1 --no-deps \
  | python3 -c "
import json, sys
from collections import defaultdict, deque

meta = json.load(sys.stdin)

# Build set of workspace package ids
ws_ids = set(meta.get('workspace_members', []))
pkgs = {p['id']: p for p in meta['packages'] if p['id'] in ws_ids}
name_by_id = {p['id']: p['name'] for p in pkgs.values()}
id_by_name = {v: k for k, v in name_by_id.items()}

# Build adjacency: pkg -> set of workspace deps it depends on
deps_of = defaultdict(set)       # name -> set of dep names
dependents_of = defaultdict(set) # name -> set of names that depend on it
ws_names = set(name_by_id.values())

for pkg in pkgs.values():
    for dep in pkg.get('dependencies', []):
        # Skip dev-dependencies: they don't constrain publish order
        if dep.get('kind') == 'dev':
            continue
        # dep['name'] is the real package name (e.g. nexus-memory-core);
        # dep['rename'] would be the local alias — not needed here.
        dep_name = dep['name']
        if dep_name in ws_names:
            deps_of[pkg['name']].add(dep_name)
            dependents_of[dep_name].add(pkg['name'])

# Kahn's algorithm for topological sort
in_degree = {n: len(deps_of.get(n, set())) for n in ws_names}
queue = deque(sorted(n for n in ws_names if in_degree[n] == 0))
order = []

while queue:
    node = queue.popleft()
    order.append(node)
    for dependent in sorted(dependents_of.get(node, set())):
        in_degree[dependent] -= 1
        if in_degree[dependent] == 0:
            queue.append(dependent)

if len(order) != len(ws_names):
    print('ERROR: cycle detected in workspace dependencies', file=sys.stderr)
    sys.exit(1)

for name in order:
    print(name)
"))

echo "Publish order (${#CRATES[@]} crates):"
for i in "${!CRATES[@]}"; do
  echo "  $((i+1)). ${CRATES[$i]}"
done
echo ""

# ---------- publish loop ----------
PUBLISHED=0
SKIPPED=0
FAILED=0

for CRATE in "${CRATES[@]}"; do
  echo "━━━ ${CRATE} v${VERSION} ━━━"

  # Skip if already published (only in non-dry-run mode)
  if [[ -z "${DRY_RUN}" ]]; then
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
      "https://crates.io/api/v1/crates/${CRATE}/${VERSION}")

    if [[ "${HTTP_CODE}" == "200" ]]; then
      echo "  Already published — skipping"
      SKIPPED=$((SKIPPED + 1))
      echo ""
      continue
    fi
  fi

  # Publish with retries for index propagation lag
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
  echo ""
done

echo "━━━ Summary ━━━"
echo "  Published: ${PUBLISHED}"
echo "  Skipped:   ${SKIPPED}"
echo "  Failed:    ${FAILED}"
