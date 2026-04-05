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
REINSTALL=0
RESET_DB=0
PROFILE_FILE=""
EXPLICIT_BINARY_PATH=""

# ── Usage ─────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Nexus Memory System installer

Builds all workspace crates from source and installs everything:
  - nexus CLI binary  (installed to ${BIN_DIR}/nexus)
  - environment files  (bash + fish)
  - shell profiles     (auto-detected or --profile)
  - Claude Code hooks  (env vars + lifecycle hook shim)
  - tool wrappers      (codex-nexus, claude-nexus, etc. with auto session lifecycle)

Usage: $0 [options]

Options:
  --skip-build          Skip cargo build; use existing binary
  --binary PATH         Install a specific nexus binary (for example ./target/release/nexus)
  --bin-dir DIR         Executable directory (default: ${BIN_DIR})
  --config-dir DIR      Config directory (default: ${CONFIG_DIR})
  --data-dir DIR        Data directory (default: ${DATA_DIR})
  --state-dir DIR       State/log directory (default: ${STATE_DIR})
  --db-path PATH        Database path (default: ${DB_PATH})
  --profile FILE        Shell profile (.bashrc / .zshrc / config.fish)
  --skip-profile        Do not modify any shell profile
  --reinstall           Clean reinstall: remove installed components, hooks, and wrappers before installing
  --reset-db            With --reinstall: also wipe the database (memories will be lost)
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
        --reinstall)    REINSTALL=1; shift ;;
        --reset-db)     RESET_DB=1; shift ;;
        --binary)       EXPLICIT_BINARY_PATH="$2"; shift 2 ;;
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

    if [[ -n "${EXPLICIT_BINARY_PATH}" ]]; then
        binary="${EXPLICIT_BINARY_PATH}"
    elif [[ -x "${REPO_ROOT}/target/release/nexus" ]]; then
        binary="${REPO_ROOT}/target/release/nexus"
    elif command -v nexus >/dev/null 2>&1; then
        binary="$(command -v nexus)"
    else
        err "Could not find nexus binary. Run without --skip-build or build first:"
        err "  cargo build --release -p nexus-memory"
        exit 1
    fi

    if [[ ! -f "${binary}" ]]; then
        err "Specified binary does not exist: ${binary}"
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

    # nexus-with: sources env and wraps supported CLIs with best-effort session lifecycle
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

nexus_warn() {
    echo "[nexus-with] \$1" >&2
}

run_nexus_lifecycle() {
    local action="\$1"
    if [[ -z "\${NEXUS_WRAPPED_AGENT:-}" || "\${NEXUS_DISABLE_WRAPPER_LIFECYCLE:-0}" == "1" ]]; then
        return 0
    fi

    local args=("${BIN_DIR}/nexus" session "\${action}" --agent "\${NEXUS_WRAPPED_AGENT}")
    if [[ "\${action}" == "start" ]]; then
        args+=(--mode "\${NEXUS_WRAPPED_RUNTIME_MODE:-session}")
    else
        args+=(--reason "\${NEXUS_WRAPPED_EXIT_REASON:-wrapper-exit}")
    fi

    if [[ -n "\${NEXUS_WRAPPED_SESSION_KEY:-}" ]]; then
        args+=(--session-key "\${NEXUS_WRAPPED_SESSION_KEY}")
    fi

    local cwd="\${PWD}"
    if [[ -n "\${NEXUS_WRAPPED_CWD:-}" ]]; then
        cwd="\${NEXUS_WRAPPED_CWD}"
    fi
    args+=(--cwd "\${cwd}")

    if ! "\${args[@]}" >/dev/null 2>&1; then
        nexus_warn "best-effort session \${action} failed for \${NEXUS_WRAPPED_AGENT}"
    fi
}

nexus_cleanup_done=0
child_pid=""
child_group_pid=""

launch_wrapped_tool() {
    if command -v setsid >/dev/null 2>&1; then
        setsid "\$@" &
        child_pid=\$!
        child_group_pid="\${child_pid}"
    else
        "\$@" &
        child_pid=\$!
        child_group_pid=""
    fi
}

wait_for_wrapped_tool() {
    if [[ -z "\${child_pid}" ]]; then
        return 0
    fi

    set +e
    wait "\${child_pid}"
    local status=\$?
    set -e
    return "\${status}"
}

finalize_wrapper() {
    if [[ "\${nexus_cleanup_done}" == "1" ]]; then
        return 0
    fi
    nexus_cleanup_done=1
    run_nexus_lifecycle "end"
}

handle_signal() {
    local signal="\$1"
    local status=1

    if [[ -n "\${child_group_pid}" ]] && kill -0 -- "-\${child_group_pid}" 2>/dev/null; then
        kill -s "\${signal}" -- "-\${child_group_pid}" 2>/dev/null || true
        wait_for_wrapped_tool || true
    elif [[ -n "\${child_pid}" ]] && kill -0 "\${child_pid}" 2>/dev/null; then
        kill -s "\${signal}" "\${child_pid}" 2>/dev/null || true
        wait_for_wrapped_tool || true
    fi

    case "\${signal}" in
        INT) status=130 ;;
        TERM) status=143 ;;
    esac

    finalize_wrapper
    trap - EXIT INT TERM
    exit "\${status}"
}

trap 'finalize_wrapper' EXIT
trap 'handle_signal INT' INT
trap 'handle_signal TERM' TERM

run_nexus_lifecycle "start"
status=0
launch_wrapped_tool "\$@"
wait_for_wrapped_tool || status=\$?
finalize_wrapper
trap - EXIT INT TERM
exit "\${status}"
WITH_EOF
    chmod +x "${BIN_DIR}/nexus-with"

    local installed_version
    installed_version="$("${BIN_DIR}/nexus" --version 2>/dev/null || echo "unknown")"
    ok "Installed nexus to ${BIN_DIR}/nexus (${installed_version})"
}

# ── Tool wrappers ─────────────────────────────────────────────────────
install_tool_wrappers() {
    local tools=(
        "codex:codex"
        "claude:claude-code"
        "claude-code:claude-code"
        "gemini:gemini"
        "qwen:qwen"
        "amp:amp"
        "droid:droid"
        "opencode:opencode"
        "hermes:hermes"
    )
    local installed=0

    for tool_entry in "${tools[@]}"; do
        local tool="${tool_entry%%:*}"
        local agent="${tool_entry##*:}"
        if command -v "${tool}" >/dev/null 2>&1; then
            cat > "${BIN_DIR}/${tool}-nexus" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export NEXUS_WRAPPED_AGENT="${agent}"
export NEXUS_WRAPPED_CWD="\${PWD}"
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
        # Ensure base paths are up to date by patching them in-place (portable)
        local temp_env
        temp_env=$(mktemp)
        sed \
            -e "s|^export NEXUS_DATABASE_PATH=.*|export NEXUS_DATABASE_PATH=\"${DB_PATH}\"|" \
            -e "s|^export NEXUS_AGENT_INBOX_DIR=.*|export NEXUS_AGENT_INBOX_DIR=\"${DATA_DIR}/inbox\"|" \
            "${ENV_FILE}" > "${temp_env}"
        mv "${temp_env}" "${ENV_FILE}"

        # Patch fish env similarly (portable: use temp file instead of -i)
        if [[ -f "${FISH_ENV_FILE}" ]]; then
            local temp_fish
            temp_fish=$(mktemp)
            sed \
                -e "s|^set -gx NEXUS_DATABASE_PATH.*|set -gx NEXUS_DATABASE_PATH \"${DB_PATH}\"|" \
                -e "s|^set -gx NEXUS_AGENT_INBOX_DIR.*|set -gx NEXUS_AGENT_INBOX_DIR \"${DATA_DIR}/inbox\"|" \
                "${FISH_ENV_FILE}" > "${temp_fish}"
            mv "${temp_fish}" "${FISH_ENV_FILE}"
        fi
        ok "Updated paths in existing ${ENV_FILE}"
        return
    fi

    cat > "${ENV_FILE}" <<EOF
# Generated by scripts/install.sh
export NEXUS_DATABASE_PATH="${DB_PATH}"
export NEXUS_SYNC_POLICY="auto"

# Optional semantic embeddings
# Remote provider example:
# export NEXUS_EMBEDDINGS_ENABLED="true"
# export NEXUS_EMBEDDING_BACKEND="openai-compatible"
# export NEXUS_EMBEDDING_PROVIDER="inherit"
# export NEXUS_EMBEDDING_MODEL="text-embedding-004"
# Local ONNX example:
# export NEXUS_EMBEDDING_BACKEND="local"
# export NEXUS_EMBEDDING_MODEL_PATH="${REPO_ROOT}/models/all-MiniLM-L6-v2.onnx"
# export NEXUS_TOKENIZER_PATH="${REPO_ROOT}/models/all-MiniLM-L6-v2-tokenizer"
# Local OpenAI-compatible runtime example (vLLM / LM Studio / llama.cpp):
# export NEXUS_EMBEDDING_BACKEND="openai-compatible"
# export NEXUS_EMBEDDING_PROVIDER="lmstudio"
# export NEXUS_EMBEDDING_BASE_URL="http://127.0.0.1:1234/v1"
# export NEXUS_EMBEDDING_MODEL="text-embedding-3-small"

# Always-on agent (uncomment and configure to enable)
# export NEXUS_LLM_PROVIDER="openai"
# export NEXUS_LLM_MODEL="gpt-4o-mini"
# export NEXUS_LLM_API_KEY_ENV="OPENAI_API_KEY"
# export NEXUS_AGENT_ENABLED="false"
# export NEXUS_AGENT_NAMESPACE="nexus-agent"
# export NEXUS_AGENT_INBOX_DIR="${DATA_DIR}/inbox"
# export NEXUS_AGENT_CONSOLIDATION_INTERVAL_MINS="30"
# export NEXUS_AGENT_SCAN_INTERVAL_SECS="5"
EOF

    cat > "${FISH_ENV_FILE}" <<EOF
# Generated by scripts/install.sh
set -gx NEXUS_DATABASE_PATH "${DB_PATH}"
set -gx NEXUS_SYNC_POLICY "auto"

# Optional semantic embeddings
# Remote provider example:
# set -gx NEXUS_EMBEDDINGS_ENABLED "true"
# set -gx NEXUS_EMBEDDING_BACKEND "openai-compatible"
# set -gx NEXUS_EMBEDDING_PROVIDER "inherit"
# set -gx NEXUS_EMBEDDING_MODEL "text-embedding-004"
# Local ONNX example:
# set -gx NEXUS_EMBEDDING_BACKEND "local"
# set -gx NEXUS_EMBEDDING_MODEL_PATH "${REPO_ROOT}/models/all-MiniLM-L6-v2.onnx"
# set -gx NEXUS_TOKENIZER_PATH "${REPO_ROOT}/models/all-MiniLM-L6-v2-tokenizer"
# Local OpenAI-compatible runtime example (vLLM / LM Studio / llama.cpp):
# set -gx NEXUS_EMBEDDING_BACKEND "openai-compatible"
# set -gx NEXUS_EMBEDDING_PROVIDER "lmstudio"
# set -gx NEXUS_EMBEDDING_BASE_URL "http://127.0.0.1:1234/v1"
# set -gx NEXUS_EMBEDDING_MODEL "text-embedding-3-small"

# Always-on agent (uncomment and configure to enable)
# set -gx NEXUS_LLM_PROVIDER "openai"
# set -gx NEXUS_LLM_MODEL "gpt-4o-mini"
# set -gx NEXUS_LLM_API_KEY_ENV "OPENAI_API_KEY"
# set -gx NEXUS_AGENT_ENABLED "false"
# set -gx NEXUS_AGENT_NAMESPACE "nexus-agent"
# set -gx NEXUS_AGENT_INBOX_DIR "${DATA_DIR}/inbox"
# set -gx NEXUS_AGENT_CONSOLIDATION_INTERVAL_MINS "30"
# set -gx NEXUS_AGENT_SCAN_INTERVAL_SECS "5"
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
    if [[ "${shell_name}" != "bash" && -f "${HOME}/.bashrc" ]]; then
        upsert_profile_block_posix "${HOME}/.bashrc"
    fi
    if [[ "${shell_name}" != "zsh" && -f "${HOME}/.zshrc" ]]; then
        upsert_profile_block_posix "${HOME}/.zshrc"
    fi
    if [[ "${shell_name}" != "fish" && -d "${HOME}/.config/fish" ]]; then
        upsert_profile_block_fish "${HOME}/.config/fish/config.fish"
    fi
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

    python3 << PYTHON_EOF
import json
import os

settings_path = '${settings_file}'
env_path = '${ENV_FILE}'

def parse_env_file(path):
    values = {}
    if not os.path.exists(path):
        return values
    with open(path) as f:
        for raw_line in f:
            line = raw_line.strip()
            if not line or line.startswith('#'):
                continue
            if '=' not in line:
                continue
            key, value = line.split('=', 1)
            key = key.strip()
            value = value.strip().strip('\"')
            values[key] = value
    return values

with open(settings_path) as f:
    s = json.load(f)

if 'env' not in s:
    s['env'] = {}

env_values = parse_env_file(env_path)

desired = {
    'NEXUS_DATABASE_PATH': env_values.get('NEXUS_DATABASE_PATH', '${DB_PATH}'),
    'NEXUS_SYNC_POLICY': env_values.get('NEXUS_SYNC_POLICY', 'auto'),
    'NEXUS_LLM_PROVIDER': env_values.get('NEXUS_LLM_PROVIDER', ''),
    'NEXUS_LLM_MODEL': env_values.get('NEXUS_LLM_MODEL', ''),
    'NEXUS_LLM_API_KEY_ENV': env_values.get('NEXUS_LLM_API_KEY_ENV', ''),
    'NEXUS_EMBEDDINGS_ENABLED': env_values.get('NEXUS_EMBEDDINGS_ENABLED', 'false'),
    'NEXUS_EMBEDDING_BACKEND': env_values.get('NEXUS_EMBEDDING_BACKEND', ''),
    'NEXUS_EMBEDDING_PROVIDER': env_values.get('NEXUS_EMBEDDING_PROVIDER', ''),
    'NEXUS_EMBEDDING_MODEL': env_values.get('NEXUS_EMBEDDING_MODEL', ''),
    'NEXUS_EMBEDDING_API_KEY_ENV': env_values.get('NEXUS_EMBEDDING_API_KEY_ENV', ''),
}

for key in ('NEXUS_EMBEDDING_BASE_URL', 'GEMINI_API_KEY', 'OPENAI_API_KEY', 'OPENROUTER_API_KEY', 'GROQ_API_KEY'):
    value = env_values.get(key, '')
    if value:
        desired[key] = value

for key, value in desired.items():
    if value:
        s['env'][key] = value

with open(settings_path, 'w') as f:
    json.dump(s, f, indent=2)
    f.write('\n')
PYTHON_EOF

    if [[ $? -eq 0 ]]; then
        ok "Updated Claude Code settings.json with Nexus env vars"
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
// Local hook shim — routes lifecycle and tool events into the Nexus CLI.
// Installed by scripts/install.sh. Core behavior stays local; no server is required.

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

function parsePayload(rawInput) {
  if (!rawInput || !rawInput.trim()) {
    return {};
  }
  try {
    return JSON.parse(rawInput);
  } catch (_) {
    return {};
  }
}

function getSessionKey(payload) {
  const explicit = payload.session_id || payload.sessionId || payload.sessionKey || "";
  if (explicit) {
    return explicit;
  }

  const cwd = getCwd(payload) || "unknown-cwd";
  return `derived-${agent}-${process.ppid}-${cwd}`;
}

function getCwd(payload) {
  return payload.cwd || payload.working_directory || payload.workingDirectory || "";
}

function appendArg(args, flag, value) {
  if (value) {
    args.push(flag, String(value));
  }
}

(async () => {
  const rawInput = await readStdin();
  const payload = parsePayload(rawInput);
  const forwardedInput = rawInput && rawInput.trim() ? rawInput : "{}";
  const sessionKey = getSessionKey(payload);
  const cwd = getCwd(payload);

  let args;
  switch (eventName) {
    case "SessionStart":
      args = ["session", "start", "--agent", agent];
      appendArg(args, "--session-key", sessionKey);
      appendArg(args, "--cwd", cwd);
      break;
    case "PreCompact":
    case "SessionCompact":
      args = ["session", "event", "--agent", agent, "--kind", "compact"];
      appendArg(args, "--session-key", sessionKey);
      appendArg(args, "--cwd", cwd);
      break;
    case "Stop":
      args = ["session", "event", "--agent", agent, "--kind", "stop"];
      appendArg(args, "--session-key", sessionKey);
      appendArg(args, "--cwd", cwd);
      break;
    case "SessionEnd":
      args = ["session", "end", "--agent", agent];
      appendArg(args, "--session-key", sessionKey);
      appendArg(args, "--cwd", cwd);
      break;
    default:
      args = ["ingest-hook-event", "--agent", agent, "--event", eventName, "--format", agent];
      appendArg(args, "--session-key", sessionKey);
      appendArg(args, "--cwd", cwd);
      break;
  }

  const result = spawnSync(
    "NEXUS_BIN_PATH",
    args,
    { input: forwardedInput, encoding: "utf8", env: process.env, timeout: 25000, maxBuffer: 50 * 1024 * 1024 },
  );
  if (result.status !== 0) {
    const errorMsg = result.stderr || result.stdout || `exit ${result.status}`;
    logFailure(
      `${agent}/${eventName} failed: ${errorMsg}`,
    );
    console.error(`[nexus-hook] ${agent}/${eventName}: ${errorMsg}`);
  } else if (result.signal === "SIGTERM") {
    logFailure(`${agent}/${eventName} timed out after 25s`);
    console.error(`[nexus-hook] ${agent}/${eventName}: timed out`);
  }
  // Always exit 0 to prevent hook failures from blocking the agent
  process.exit(0);
})().catch((error) => {
  logFailure(`${agent}/${eventName} crashed: ${error.stack || error.message}`);
  console.error(`[nexus-hook] ${agent}/${eventName} crashed: ${error.stack || error.message}`);
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

# ── Session start hook script ─────────────────────────────────────────
install_session_start_hook() {
    step "Installing session start hook"
    local hooks_dir="${CONFIG_DIR}/hooks"
    mkdir -p "${hooks_dir}"

    local hook_path="${hooks_dir}/session-start-delayed.sh"

    cat > "${hook_path}" <<'HOOK_EOF'
#!/bin/bash
# SessionStart hook - initializes nexus runtime for the session
# Executes immediately to prevent Claude Code timeout

# Execute nexus session start - always succeeds
NEXUS_BIN_PATH session start \
  --agent claude-code \
  --mode session \
  "$@" >/dev/null 2>&1 || true

# Always exit 0 to prevent hook failures
exit 0
HOOK_EOF

    # Replace placeholder with the actual binary path (portable across GNU/macOS sed)
    local temp_hook
    temp_hook=$(mktemp)
    sed "s|NEXUS_BIN_PATH|${BIN_DIR}/nexus|g" "${hook_path}" > "${temp_hook}"
    mv "${temp_hook}" "${hook_path}"
    chmod +x "${hook_path}"
    ok "Installed session start hook at ${hook_path}"
}

# ── Claude Code hooks ─────────────────────────────────────────────────
configure_claude_hooks() {
    step "Configuring Claude Code lifecycle hooks"

    if ! command -v python3 >/dev/null 2>&1; then
        warn "python3 not found; skipping Claude Code hook configuration"
        return
    fi

    local settings_file="${HOME}/.claude/settings.json"
    local shim_path="${CONFIG_DIR}/hooks/event-ingest.js"
    local session_start_hook_path="${CONFIG_DIR}/hooks/session-start-delayed.sh"

    if [[ ! -f "${settings_file}" ]]; then
        warn "Claude Code settings not found at ${settings_file}"
        return
    fi

    python3 << PYTHON_EOF
import json

settings_path = '${settings_file}'
shim_path = '${shim_path}'
session_start_hook_path = '${session_start_hook_path}'

with open(settings_path) as f:
    s = json.load(f)

if 'hooks' not in s:
    s['hooks'] = {}

legacy_markers = [
    'NEXUS_SERVER_URL',
    'nexus serve',
]

# Specific legacy commands to remove (exact or partial match)
legacy_commands = [
    "'nexus' session start",
    '"nexus" session start',
    'nexus session start --agent',
    'event-ingest-delayed.js',
    'event-ingest.js claude-code SessionStart',
]

def normalize_entry(entry):
    if not isinstance(entry, dict):
        return None
    matcher = entry.get('matcher', '')
    hooks = entry.get('hooks')
    if isinstance(hooks, list):
        normalized_hooks = []
        for hook in hooks:
            if not isinstance(hook, dict):
                continue
            if hook.get('type') != 'command':
                normalized_hooks.append(hook)
                continue
            command = hook.get('command')
            if not command:
                continue
            normalized = {
                'type': 'command',
                'command': command,
            }
            if 'timeout' in hook:
                normalized['timeout'] = hook['timeout']
            normalized_hooks.append(normalized)
        if normalized_hooks:
            return {'matcher': matcher, 'hooks': normalized_hooks}
        return None
    command = entry.get('command')
    if command:
        hook = {'type': 'command', 'command': command}
        if 'timeout' in entry:
            hook['timeout'] = entry['timeout']
        return {'matcher': matcher, 'hooks': [hook]}
    return None

def entry_commands(entry):
    commands = []
    for hook in entry.get('hooks', []):
        if isinstance(hook, dict):
            command = hook.get('command')
            if command:
                commands.append(command)
    return commands

for hook_name, entries in list(s['hooks'].items()):
    if hook_name == 'SessionCompact':
        del s['hooks'][hook_name]
        continue
    if not isinstance(entries, list):
        s['hooks'][hook_name] = []
        continue
    cleaned = []
    for entry in entries:
        normalized = normalize_entry(entry)
        if normalized is None:
            continue
        commands = entry_commands(normalized)
        is_legacy = any(
            any(marker in command for marker in legacy_markers)
            for command in commands
        ) or any(
            any(legacy_cmd in command for legacy_cmd in legacy_commands)
            for command in commands
        )
        if is_legacy:
            continue
        cleaned.append(normalized)
    s['hooks'][hook_name] = cleaned

required_hooks = {
    'SessionStart': (session_start_hook_path, 10000),
    'PostToolUse': (f'node {shim_path} claude-code PostToolUse', 30000),
    'PreCompact': (f'node {shim_path} claude-code PreCompact', 5000),
    'Stop': (f'node {shim_path} claude-code Stop', 30000),
    'SessionEnd': (f'node {shim_path} claude-code SessionEnd', 30000),
}

for hook_name, (command, timeout) in required_hooks.items():
    if hook_name not in s['hooks']:
        s['hooks'][hook_name] = []
    if not any(command in candidate for entry in s['hooks'][hook_name] for candidate in entry_commands(entry)):
        s['hooks'][hook_name].append({
            'matcher': '',
            'hooks': [{
                'type': 'command',
                'command': command,
                'timeout': timeout,
            }],
        })

with open(settings_path, 'w') as f:
    json.dump(s, f, indent=2)
    f.write('\n')
PYTHON_EOF

    if [[ $? -eq 0 ]]; then
        ok "Configured Claude Code lifecycle hooks"
    else
        warn "Failed to configure Claude Code lifecycle hooks"
    fi
}

# ── Clean uninstall ───────────────────────────────────────────────────
uninstall_components() {
    step "Removing installed components"

    # ── Binary and wrappers ──
    if [[ -f "${BIN_DIR}/nexus" ]]; then
        rm -f "${BIN_DIR}/nexus"
        ok "Removed ${BIN_DIR}/nexus"
    fi
    if [[ -f "${BIN_DIR}/nexus-with" ]]; then
        rm -f "${BIN_DIR}/nexus-with"
        ok "Removed ${BIN_DIR}/nexus-with"
    fi

    # Remove stale copies from older installs
    rm -f "${BIN_DIR}/nexus-bin" "${HOME}/.local/bin/nexus" "${HOME}/.local/bin/nexus-bin"

    # Tool wrappers (only remove ones we installed)
    local tools=("codex" "claude" "claude-code" "gemini" "qwen" "amp" "droid" "opencode" "hermes")
    for tool in "${tools[@]}"; do
        if [[ -f "${BIN_DIR}/${tool}-nexus" ]]; then
            rm -f "${BIN_DIR}/${tool}-nexus"
        fi
    done

    # ── Hook shim and session hook ──
    local hooks_dir="${CONFIG_DIR}/hooks"
    if [[ -d "${hooks_dir}" ]]; then
        rm -f "${hooks_dir}/event-ingest.js"
        rm -f "${hooks_dir}/session-start-delayed.sh"
        # Remove the directory only if it's empty (we don't own it)
        rmdir "${hooks_dir}" 2>/dev/null || true
        ok "Removed hook files"
    fi

    # ── Shell profile blocks ──
    if [[ ${SKIP_PROFILE} -eq 0 ]]; then
        local begin_marker="# >>> nexus-memory-system >>>"
        local end_marker="# <<< nexus-memory-system <<<"

        local profiles=()
        # Always check common profiles
        for prof in "${HOME}/.bashrc" "${HOME}/.zshrc" "${HOME}/.config/fish/config.fish"; do
            if [[ -f "${prof}" ]]; then
                profiles+=("${prof}")
            fi
        done

        for prof in "${profiles[@]}"; do
            if grep -Fq "${begin_marker}" "${prof}" 2>/dev/null; then
                # Use awk to surgically remove only our block
                local temp_prof
                temp_prof=$(mktemp)
                awk -v begin="${begin_marker}" -v end="${end_marker}" '
                    $0 == begin { skip=1; next }
                    $0 == end { skip=0; next }
                    !skip { print }
                ' "${prof}" > "${temp_prof}"
                mv "${temp_prof}" "${prof}"
                ok "Removed profile block from ${prof}"
            fi
        done
    fi

    # ── Claude Code lifecycle hooks in settings.json ──
    if command -v python3 >/dev/null 2>&1; then
        local settings_file="${HOME}/.claude/settings.json"
        if [[ -f "${settings_file}" ]]; then
            python3 << PYTHON_EOF
import json, os

settings_path = '${settings_file}'

with open(settings_path) as f:
    s = json.load(f)

if 'hooks' not in s:
    exit(0)

def entry_commands(entry):
    commands = []
    for hook in entry.get('hooks', []):
        if isinstance(hook, dict):
            command = hook.get('command')
            if command:
                commands.append(command)
    return commands

for hook_name, entries in list(s['hooks'].items()):
    if not isinstance(entries, list):
        continue
    cleaned = []
    for entry in entries:
        commands = entry_commands(entry)
        # Remove entries that reference our shim or nexus commands
        is_ours = any(
            'event-ingest' in c or ('nexus' in c and 'session' in c)
            for c in commands
        )
        if not is_ours:
            cleaned.append(entry)
    if cleaned:
        s['hooks'][hook_name] = cleaned
    else:
        del s['hooks'][hook_name]

# Also remove Nexus env vars from the 'env' section (but keep other user env vars)
if 'env' in s:
    nexus_prefixes = ('NEXUS_',)
    for key in list(s['env'].keys()):
        if any(key.startswith(p) for p in nexus_prefixes):
            del s['env'][key]
    if not s['env']:
        del s['env']

with open(settings_path, 'w') as f:
    json.dump(s, f, indent=2)
    f.write('\n')
PYTHON_EOF
            ok "Cleaned Claude Code settings.json"
        fi
    fi

    # ── Database ──
    if [[ ${RESET_DB} -eq 1 ]]; then
        if [[ -f "${DB_PATH}" ]]; then
            rm -f "${DB_PATH}"
            ok "Removed database at ${DB_PATH}"
        fi
        # Also remove write-ahead log / shared memory files
        rm -f "${DB_PATH}-wal" "${DB_PATH}-shm"
    else
        info "Preserving database at ${DB_PATH} (use --reset-db to wipe)"
    fi

    # ── Config directory ──
    # Only remove the nexus.env and nexus.fish files; preserve any user-added files
    rm -f "${ENV_FILE}" "${FISH_ENV_FILE}"
    # Remove config dir only if empty
    rmdir "${CONFIG_DIR}" 2>/dev/null || true

    # ── Data / state directories ──
    # Remove runtime artifacts but keep the parent dirs if user has other data there
    if [[ -d "${DATA_DIR}/inbox" ]]; then
        rm -rf "${DATA_DIR}/inbox"
    fi
    if [[ -d "${STATE_DIR}/pending-enrichment" ]]; then
        rm -rf "${STATE_DIR}/pending-enrichment"
    fi
    rm -f "${STATE_DIR}/hook-errors.log"
    rmdir "${DATA_DIR}" 2>/dev/null || true
    rmdir "${STATE_DIR}" 2>/dev/null || true

    ok "Uninstall complete"
}

# ── Main ──────────────────────────────────────────────────────────────
main() {
    echo -e "${CYAN}Nexus Memory System Installer${NC}"
    echo

    if [[ ${REINSTALL} -eq 1 ]]; then
        warn "Clean reinstall mode"
        uninstall_components
        echo
    fi

    check_prerequisites
    build_nexus
    write_env_file
    install_binaries
    install_tool_wrappers
    install_hook_shim
    install_session_start_hook
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
