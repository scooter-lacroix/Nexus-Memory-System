#!/usr/bin/env bash
# Nexus Memory System — complete build + install
#
# Builds all workspace crates from source and installs the CLI binary,
# env files, shell profiles, Claude Code hooks, and tool wrappers.
#
# Usage:
#   ./scripts/install.sh              # Full build + install (default)
#   ./scripts/install.sh --skip-build # Install from existing binary
#   ./scripts/install.sh --help

set -euo pipefail
IFS=$'\n\t'

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $1"; }
ok()    { echo -e "${GREEN}[OK]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
err()   { echo -e "${RED}[ERROR]${NC} $1"; }
step()  { echo -e "${CYAN}==>${NC} $1"; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ── Defaults ──────────────────────────────────────────────────────────
CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo}"
BIN_DIR="${NEXUS_INSTALL_BIN_DIR:-${CARGO_HOME}/bin}"
CONFIG_DIR="${NEXUS_INSTALL_CONFIG_DIR:-${XDG_CONFIG_HOME:-${HOME}/.config}/nexus-memory-system}"
DATA_DIR="${NEXUS_INSTALL_DATA_DIR:-${XDG_DATA_HOME:-${HOME}/.local/share}/nexus-memory-system}"
STATE_DIR="${NEXUS_INSTALL_STATE_DIR:-${XDG_STATE_HOME:-${HOME}/.local/state}/nexus-memory-system}"
DB_PATH="${NEXUS_DATABASE_PATH:-${DATA_DIR}/nexus.db}"
ENV_FILE="${CONFIG_DIR}/nexus.env"
FISH_ENV_FILE="${CONFIG_DIR}/nexus.fish"

SKIP_BUILD=0
SKIP_PROFILE=0
PROFILE_FILE=""

# ── Usage ─────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Nexus Memory System installer

Builds all workspace crates from source and installs everything:
  - nexus CLI binary  (installed to ${BIN_DIR}/nexus)
  - environment files  (bash + fish)
  - shell profiles     (auto-detected or --profile)
  - Claude Code hooks  (env vars + PostToolUse hook shim)
  - tool wrappers      (codex-nexus, claude-nexus, etc.)

Usage: $0 [options]

Options:
  --skip-build          Skip cargo build; use existing binary
  --bin-dir DIR         Executable directory (default: ${BIN_DIR})
  --config-dir DIR      Config directory (default: ${CONFIG_DIR})
  --data-dir DIR        Data directory (default: ${DATA_DIR})
  --state-dir DIR       State/log directory (default: ${STATE_DIR})
  --db-path PATH        Database path (default: ${DB_PATH})
  --profile FILE        Shell profile (.bashrc / .zshrc / config.fish)
  --skip-profile        Do not modify any shell profile
  -h, --help            Show this help

Environment variables:
  NEXUS_INSTALL_BIN_DIR     Override --bin-dir
  NEXUS_INSTALL_CONFIG_DIR  Override --config-dir
  NEXUS_INSTALL_DATA_DIR    Override --data-dir
  NEXUS_INSTALL_STATE_DIR   Override --state-dir
  NEXUS_DATABASE_PATH       Override --db-path
  CARGO_HOME                Cargo home (affects default --bin-dir)
EOF
}

# ── Parse arguments ───────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-build)   SKIP_BUILD=1; shift ;;
        --skip-profile) SKIP_PROFILE=1; shift ;;
        --bin-dir)      BIN_DIR="$2"; shift 2 ;;
        --config-dir)
            CONFIG_DIR="$2"
            ENV_FILE="${CONFIG_DIR}/nexus.env"
            FISH_ENV_FILE="${CONFIG_DIR}/nexus.fish"
            shift 2
            ;;
        --data-dir)   DATA_DIR="$2"; DB_PATH="${DATA_DIR}/nexus.db"; shift 2 ;;
        --state-dir)  STATE_DIR="$2"; shift 2 ;;
        --db-path)    DB_PATH="$2"; shift 2 ;;
        --profile)    PROFILE_FILE="$2"; shift 2 ;;
        -h|--help)    usage; exit 0 ;;
        *) err "Unknown argument: $1"; usage; exit 1 ;;
    esac
done

# ── Prerequisites ─────────────────────────────────────────────────────
check_prerequisites() {
    if [[ ${SKIP_BUILD} -eq 0 ]] && ! command -v cargo >/dev/null 2>&1; then
        err "cargo not found. Install Rust first: https://rustup.rs/"
        exit 1
    fi
    if ! command -v python3 >/dev/null 2>&1; then
        warn "python3 not found; Claude Code configuration will be skipped"
    fi
}

# ── Build ─────────────────────────────────────────────────────────────
build_nexus() {
    if [[ ${SKIP_BUILD} -eq 1 ]]; then
        info "Skipping build (--skip-build)"
        return
    fi

    step "Building all workspace crates (release)"
    cargo build --release -p nexus-memory --manifest-path "${REPO_ROOT}/Cargo.toml"
    ok "Build complete"
}

# ── Resolve binary ────────────────────────────────────────────────────
resolve_binary() {
    local binary=""

    if [[ -x "${REPO_ROOT}/target/release/nexus" ]]; then
        binary="${REPO_ROOT}/target/release/nexus"
    elif command -v nexus >/dev/null 2>&1; then
        binary="$(command -v nexus)"
    else
        err "Could not find nexus binary. Run without --skip-build or build first:"
        err "  cargo build --release -p nexus-memory"
        exit 1
    fi

    echo "${binary}"
}

# ── Install binary ────────────────────────────────────────────────────
install_binaries() {
    local binary
    binary="$(resolve_binary)"

    step "Installing nexus to ${BIN_DIR}"
    mkdir -p "${BIN_DIR}"

    # Remove any stale copies from previous installs
    rm -f "${BIN_DIR}/nexus-bin" "${HOME}/.local/bin/nexus" "${HOME}/.local/bin/nexus-bin"

    # Install the real binary directly as "nexus"
    install -m 0755 "${binary}" "${BIN_DIR}/nexus"

    # nexus-with: sources env then runs an arbitrary command (for tool wrappers)
    cat > "${BIN_DIR}/nexus-with" <<WITH_EOF
#!/usr/bin/env bash
set -euo pipefail
if [[ -f "${ENV_FILE}" ]]; then
    . "${ENV_FILE}"
fi
if [[ \$# -eq 0 ]]; then
    echo "Usage: nexus-with <command> [args...]" >&2
    exit 1
fi
exec "\$@"
WITH_EOF
    chmod +x "${BIN_DIR}/nexus-with"

    ok "Installed nexus to ${BIN_DIR}/nexus"
}

# ── Tool wrappers ─────────────────────────────────────────────────────
install_tool_wrappers() {
    local tools=(
        codex
        claude
        claude-code
        gemini
        qwen
        amp
        droid
        opencode
    )
    local installed=0

    for tool in "${tools[@]}"; do
        if command -v "${tool}" >/dev/null 2>&1; then
            cat > "${BIN_DIR}/${tool}-nexus" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec "${BIN_DIR}/nexus-with" "${tool}" "\$@"
EOF
            chmod +x "${BIN_DIR}/${tool}-nexus"
            installed=$((installed + 1))
        fi
    done

    if [[ ${installed} -gt 0 ]]; then
        ok "Installed ${installed} tool wrapper(s)"
    else
        warn "No known AI CLIs found in PATH for wrapper generation"
    fi
}

# ── Environment files ─────────────────────────────────────────────────
write_env_file() {
    step "Writing environment files"
    mkdir -p "${CONFIG_DIR}"

    # Preserve existing user-configured values — only write if file doesn't exist
    if [[ -f "${ENV_FILE}" ]]; then
        # Ensure base paths are up to date by patching them in-place
        sed -i.bak \
            -e "s|^export NEXUS_DATABASE_PATH=.*|export NEXUS_DATABASE_PATH=\"${DB_PATH}\"|" \
            -e "s|^export NEXUS_AGENT_INBOX_DIR=.*|export NEXUS_AGENT_INBOX_DIR=\"${DATA_DIR}/inbox\"|" \
            "${ENV_FILE}"
        rm -f "${ENV_FILE}.bak"

        # Patch fish env similarly
        if [[ -f "${FISH_ENV_FILE}" ]]; then
            sed -i.bak \
                -e "s|^set -gx NEXUS_DATABASE_PATH.*|set -gx NEXUS_DATABASE_PATH \"${DB_PATH}\"|" \
                -e "s|^set -gx NEXUS_AGENT_INBOX_DIR.*|set -gx NEXUS_AGENT_INBOX_DIR \"${DATA_DIR}/inbox\"|" \
                "${FISH_ENV_FILE}"
            rm -f "${FISH_ENV_FILE}.bak"
        fi
        ok "Updated paths in existing ${ENV_FILE}"
        return
    fi

    cat > "${ENV_FILE}" <<EOF
# Generated by scripts/install.sh
export NEXUS_DATABASE_PATH="${DB_PATH}"
export NEXUS_SYNC_POLICY="auto"
export NEXUS_AUTO_INGEST="true"
export NEXUS_EMBEDDINGS_ENABLED="true"

# Always-on agent (uncomment and configure to enable)
# export NEXUS_LLM_PROVIDER="openai"
# export NEXUS_LLM_MODEL="gpt-4o-mini"
# export NEXUS_LLM_API_KEY_ENV="OPENAI_API_KEY"
# export NEXUS_AGENT_ENABLED="false"
# export NEXUS_AGENT_NAMESPACE="nexus-agent"
# export NEXUS_AGENT_INBOX_DIR="${DATA_DIR}/inbox"
# export NEXUS_AGENT_CONSOLIDATION_INTERVAL="30"
# export NEXUS_AGENT_SCAN_INTERVAL="5"
EOF

    cat > "${FISH_ENV_FILE}" <<EOF
# Generated by scripts/install.sh
set -gx NEXUS_DATABASE_PATH "${DB_PATH}"
set -gx NEXUS_SYNC_POLICY "auto"
set -gx NEXUS_AUTO_INGEST "true"
set -gx NEXUS_EMBEDDINGS_ENABLED "true"

# Always-on agent (uncomment and configure to enable)
# set -gx NEXUS_LLM_PROVIDER "openai"
# set -gx NEXUS_LLM_MODEL "gpt-4o-mini"
# set -gx NEXUS_LLM_API_KEY_ENV "OPENAI_API_KEY"
# set -gx NEXUS_AGENT_ENABLED "false"
# set -gx NEXUS_AGENT_NAMESPACE "nexus-agent"
# set -gx NEXUS_AGENT_INBOX_DIR "${DATA_DIR}/inbox"
# set -gx NEXUS_AGENT_CONSOLIDATION_INTERVAL "30"
# set -gx NEXUS_AGENT_SCAN_INTERVAL "5"
if not contains -- "${BIN_DIR}" \$PATH
    set -gx PATH "${BIN_DIR}" \$PATH
end
EOF

    ok "Wrote ${ENV_FILE} and ${FISH_ENV_FILE}"
}

# ── Shell profiles ────────────────────────────────────────────────────
upsert_profile_block_posix() {
    local profile="$1"
    local begin="# >>> nexus-memory-system >>>"
    local end="# <<< nexus-memory-system <<<"

    mkdir -p "$(dirname "${profile}")"
    touch "${profile}"

    if grep -Fq "${begin}" "${profile}"; then
        ok "Profile already configured: ${profile}"
        return
    fi

    cat >> "${profile}" <<EOF

${begin}
if [ -f "${ENV_FILE}" ]; then
  . "${ENV_FILE}"
fi
case ":\$PATH:" in
  *:"${BIN_DIR}":*) ;;
  *) export PATH="${BIN_DIR}:\$PATH" ;;
esac
${end}
EOF

    ok "Updated profile: ${profile}"
}

upsert_profile_block_fish() {
    local profile="$1"
    local begin="# >>> nexus-memory-system >>>"
    local end="# <<< nexus-memory-system <<<"

    mkdir -p "$(dirname "${profile}")"
    touch "${profile}"

    if grep -Fq "${begin}" "${profile}"; then
        ok "Fish profile already configured: ${profile}"
        return
    fi

    cat >> "${profile}" <<EOF

${begin}
if test -f "${FISH_ENV_FILE}"
    source "${FISH_ENV_FILE}"
end
${end}
EOF

    ok "Updated fish profile: ${profile}"
}

configure_profiles() {
    step "Configuring shell profiles"

    if [[ ${SKIP_PROFILE} -eq 1 ]]; then
        warn "Skipping shell profile updates (--skip-profile)"
        return
    fi

    if [[ -n "${PROFILE_FILE}" ]]; then
        case "${PROFILE_FILE}" in
            *.fish) upsert_profile_block_fish "${PROFILE_FILE}" ;;
            *)      upsert_profile_block_posix "${PROFILE_FILE}" ;;
        esac
        return
    fi

    # Detect the user's default shell and update its profile as primary
    local shell_name
    shell_name="$(basename "${SHELL:-/bin/bash}")"

    case "${shell_name}" in
        fish) upsert_profile_block_fish "${HOME}/.config/fish/config.fish" ;;
        zsh)  upsert_profile_block_posix "${HOME}/.zshrc" ;;
        *)    upsert_profile_block_posix "${HOME}/.bashrc" ;;
    esac

    # Also update any other detected shells so PATH works everywhere
    [[ "${shell_name}" != "bash" && -f "${HOME}/.bashrc" ]] && \
        upsert_profile_block_posix "${HOME}/.bashrc"
    [[ "${shell_name}" != "zsh" && -f "${HOME}/.zshrc" ]] && \
        upsert_profile_block_posix "${HOME}/.zshrc"
    [[ "${shell_name}" != "fish" && -d "${HOME}/.config/fish" ]] && \
        upsert_profile_block_fish "${HOME}/.config/fish/config.fish"
}

# ── Database ──────────────────────────────────────────────────────────
initialize_database() {
    step "Initializing database"
    mkdir -p "${DATA_DIR}" "${STATE_DIR}" "${DATA_DIR}/inbox" "${STATE_DIR}/pending-enrichment"
    if [[ -f "${ENV_FILE}" ]]; then
        . "${ENV_FILE}"
    fi
    "${BIN_DIR}/nexus" init
    ok "Initialized database at ${DB_PATH}"
}

# ── Claude Code env vars ──────────────────────────────────────────────
configure_claude_code() {
    step "Configuring Claude Code env vars"

    if ! command -v python3 >/dev/null 2>&1; then
        warn "python3 not found; skipping Claude Code configuration"
        return
    fi

    local settings_file="${HOME}/.claude/settings.json"
    if [[ ! -f "${settings_file}" ]]; then
        warn "Claude Code settings not found at ${settings_file}"
        return
    fi

    # Check if NEXUS_DATABASE_PATH is already present
    if python3 -c "
import json, sys
with open('${settings_file}') as f:
    s = json.load(f)
if s.get('env', {}).get('NEXUS_DATABASE_PATH'):
    sys.exit(0)
sys.exit(1)
" 2>/dev/null; then
        ok "Claude Code settings.json already has Nexus env vars"
        return
    fi

    python3 -c "
import json

settings_path = '${settings_file}'
db_path = '${DB_PATH}'

with open(settings_path) as f:
    s = json.load(f)

if 'env' not in s:
    s['env'] = {}

s['env']['NEXUS_DATABASE_PATH'] = db_path
s['env']['NEXUS_SYNC_POLICY'] = 'auto'
s['env']['NEXUS_AUTO_INGEST'] = 'true'
s['env']['NEXUS_EMBEDDINGS_ENABLED'] = 'true'

with open(settings_path, 'w') as f:
    json.dump(s, f, indent=2)
    f.write('\n')
" 2>/dev/null

    if [[ $? -eq 0 ]]; then
        ok "Added Nexus env vars to Claude Code settings.json"
    else
        warn "Failed to update Claude Code settings.json"
    fi
}

# ── Hook shim ─────────────────────────────────────────────────────────
install_hook_shim() {
    step "Installing Claude Code hook shim"
    local hooks_dir="${CONFIG_DIR}/hooks"
    mkdir -p "${hooks_dir}"

    local shim_path="${hooks_dir}/event-ingest.js"

    cat > "${shim_path}" <<'SHIM_EOF'
#!/usr/bin/env node
// Thin passthrough shim — forwards raw stdin to nexus ingest-hook-event.
// Installed by scripts/install.sh. Intelligence lives in Rust, not here.

const { spawnSync } = require("child_process");
const { mkdirSync, appendFileSync } = require("fs");
const { dirname, join } = require("path");
const os = require("os");

const [, , agent = "generic", eventName = "event"] = process.argv;

function readStdin() {
  return new Promise((resolve) => {
    if (process.stdin.isTTY) {
      resolve("");
      return;
    }
    let data = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => { data += chunk; });
    process.stdin.on("end", () => resolve(data));
    process.stdin.on("error", () => resolve(""));
  });
}

function logFailure(message) {
  try {
    const logPath = join(
      os.homedir(), ".local", "state", "nexus-memory-system", "hook-errors.log",
    );
    mkdirSync(dirname(logPath), { recursive: true });
    appendFileSync(logPath, `[${new Date().toISOString()}] ${message}\n`);
  } catch (_) { /* fail open */ }
}

(async () => {
  const rawInput = await readStdin();
  const result = spawnSync(
    "NEXUS_BIN_PATH",
    ["ingest-hook-event", "--agent", agent, "--event", eventName, "--format", agent],
    { input: rawInput, encoding: "utf8", env: process.env },
  );
  if (result.status !== 0) {
    logFailure(
      `${agent}/${eventName} failed: ${result.stderr || result.stdout || `exit ${result.status}`}`,
    );
  }
  process.exit(0);
})().catch((error) => {
  logFailure(`${agent}/${eventName} crashed: ${error.stack || error.message}`);
  process.exit(0);
});
SHIM_EOF

    # Replace placeholder with the actual binary path (portable across GNU/macOS sed)
    local temp_shim
    temp_shim=$(mktemp)
    sed "s|NEXUS_BIN_PATH|${BIN_DIR}/nexus|g" "${shim_path}" > "${temp_shim}"
    mv "${temp_shim}" "${shim_path}"
    chmod +x "${shim_path}"
    ok "Installed hook shim at ${shim_path}"
}

# ── Claude Code hooks ─────────────────────────────────────────────────
configure_claude_hooks() {
    step "Configuring Claude Code PostToolUse hook"

    if ! command -v python3 >/dev/null 2>&1; then
        warn "python3 not found; skipping Claude Code hook configuration"
        return
    fi

    local settings_file="${HOME}/.claude/settings.json"
    local shim_path="${CONFIG_DIR}/hooks/event-ingest.js"

    if [[ ! -f "${settings_file}" ]]; then
        warn "Claude Code settings not found at ${settings_file}"
        return
    fi

    # Check if nexus hook is already configured
    if python3 -c "
import json, sys
with open('${settings_file}') as f:
    s = json.load(f)
hooks = s.get('hooks', {}).get('PostToolUse', [])
for h in hooks:
    if 'event-ingest.js' in h.get('command', ''):
        sys.exit(0)
sys.exit(1)
" 2>/dev/null; then
        ok "Claude Code hook already configured"
        return
    fi

    python3 -c "
import json

settings_path = '${settings_file}'
shim_path = '${shim_path}'

with open(settings_path) as f:
    s = json.load(f)

if 'hooks' not in s:
    s['hooks'] = {}
if 'PostToolUse' not in s['hooks']:
    s['hooks']['PostToolUse'] = []

s['hooks']['PostToolUse'].append({
    'matcher': '',
    'command': f'node {shim_path} claude-code PostToolUse',
    'timeout': 30000
})

with open(settings_path, 'w') as f:
    json.dump(s, f, indent=2)
    f.write('\n')
" 2>/dev/null

    if [[ $? -eq 0 ]]; then
        ok "Configured Claude Code PostToolUse hook"
    else
        warn "Failed to configure Claude Code hooks"
    fi
}

# ── Main ──────────────────────────────────────────────────────────────
main() {
    echo -e "${CYAN}Nexus Memory System Installer${NC}"
    echo

    check_prerequisites
    build_nexus
    write_env_file
    install_binaries
    install_tool_wrappers
    install_hook_shim
    configure_profiles
    initialize_database
    configure_claude_code
    configure_claude_hooks

    echo
    ok "Installation complete"
    echo
    echo "  Binary:   ${BIN_DIR}/nexus"
    echo "  Database: ${DB_PATH}"
    echo "  Config:   ${ENV_FILE}"
    echo "  Hooks:    ${CONFIG_DIR}/hooks/event-ingest.js"
    echo "  Inbox:    ${DATA_DIR}/inbox/"
    echo
    echo "Restart your shell or run:"
    echo "  source ${ENV_FILE}"
}

main "$@"
