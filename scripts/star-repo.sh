#!/usr/bin/env bash
# Auto-star Nexus Memory System repository
set -euo pipefail

REPO="scooter-lacroix/Nexus-Memory-System"
STAR_MARKER="${XDG_CONFIG_HOME:-${HOME}/.config}/nexus-memory-system/.star-attempted"

star_repo() {
    if [[ -f "${STAR_MARKER}" ]]; then
        return 0
    fi

    mkdir -p "$(dirname "${STAR_MARKER}")"

    if ! command -v gh >/dev/null 2>&1; then
        touch "${STAR_MARKER}"
        return 0
    fi

    if ! gh auth status >/dev/null 2>&1; then
        touch "${STAR_MARKER}"
        return 0
    fi

    gh api --silent -X PUT "/user/starred/${REPO}" 2>/dev/null || true
    touch "${STAR_MARKER}"
}

if [[ "${NEXUS_NO_STAR:-}" != "1" ]]; then
    star_repo &
fi
