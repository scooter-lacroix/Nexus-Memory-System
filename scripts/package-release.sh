#!/usr/bin/env bash
# Package a prepared Nexus release binary into dist/ with checksums.

set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DIST_DIR="${REPO_ROOT}/dist"
TARGET_BINARY="${REPO_ROOT}/target/release/nexus"

VERSION="$(awk -F'"' '/^version = "/ { print $2; exit }' "${REPO_ROOT}/Cargo.toml")"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
ARTIFACT_BASENAME="nexus-v${VERSION}-${OS}-${ARCH}"
ARTIFACT_DIR="${DIST_DIR}/${ARTIFACT_BASENAME}"
ARCHIVE_PATH="${DIST_DIR}/${ARTIFACT_BASENAME}.tar.gz"
CHECKSUM_PATH="${DIST_DIR}/${ARTIFACT_BASENAME}.sha256"

if [[ ! -x "${TARGET_BINARY}" ]]; then
    echo "Release binary not found at ${TARGET_BINARY}" >&2
    echo "Build it first with: cargo build --release -p nexus-memory" >&2
    exit 1
fi

rm -rf "${ARTIFACT_DIR}"
mkdir -p "${ARTIFACT_DIR}"

install -m 0755 "${TARGET_BINARY}" "${ARTIFACT_DIR}/nexus"
cp "${REPO_ROOT}/README.md" "${ARTIFACT_DIR}/README.md"
cp "${REPO_ROOT}/LICENSE" "${ARTIFACT_DIR}/LICENSE"

cat > "${ARTIFACT_DIR}/INSTALL.txt" <<EOF
Nexus Memory System ${VERSION}

Quick verify:
  ./nexus --version

Recommended install:
  clone the repository for this release
  build or download the matching \`nexus\` binary
  run ./scripts/install.sh --binary /path/to/nexus

That keeps the installed binary, wrappers, env files, and hook assets in sync.
EOF

mkdir -p "${DIST_DIR}"
tar -C "${DIST_DIR}" -czf "${ARCHIVE_PATH}" "${ARTIFACT_BASENAME}"

if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${ARCHIVE_PATH}" > "${CHECKSUM_PATH}"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${ARCHIVE_PATH}" > "${CHECKSUM_PATH}"
else
    echo "No SHA-256 tool found; skipping checksum generation" >&2
fi

echo "Packaged ${ARCHIVE_PATH}"
if [[ -f "${CHECKSUM_PATH}" ]]; then
    echo "Checksum written to ${CHECKSUM_PATH}"
fi
