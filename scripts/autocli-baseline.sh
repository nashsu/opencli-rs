#!/usr/bin/env bash
# =============================================================================
# autocli-baseline.sh — Pre-flight diagnostic checks for autocli browser commands
# =============================================================================
# Usage:
#   scripts/autocli-baseline.sh [--check-only] [--json] [-- <command...>]
#
#   --check-only   Run checks only, don't execute any command
#   --json          Output results as JSON (to stderr: human log, to stdout: JSON)
#   -- <command>    After checks pass, execute this command with logging
#
# Examples:
#   scripts/autocli-baseline.sh --check-only
#   scripts/autocli-baseline.sh -- autocli linkedin recommended --limit 0 -f json
#   scripts/autocli-baseline.sh --json --check-only
# =============================================================================

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────
DAEMON_PORT="${AUTOCLI_DAEMON_PORT:-19925}"
DAEMON_HOST="${AUTOCLI_DAEMON_HOST:-localhost}"
OUTPUT_DIR="output"
TIMEOUT_SHORT=5    # seconds for quick checks
TIMEOUT_LONG=15    # seconds for network checks
SCRIPT_START=$(date +%s)

# ── Flags ──────────────────────────────────────────────────────────────────
CHECK_ONLY=false
JSON_OUT=false
COMMAND=()

# ── Color helpers (auto-detect TTY) ───────────────────────────────────────
if [ -t 2 ]; then
    _BOLD='\033[1m'; _RED='\033[31m'; _GREEN='\033[32m'; _YELLOW='\033[33m'; _CYAN='\033[36m'; _NC='\033[0m'
else
    _BOLD=''; _RED=''; _GREEN=''; _YELLOW=''; _CYAN=''; _NC=''
fi

# ── Logging ────────────────────────────────────────────────────────────────
TS() { date '+%H:%M:%S'; }

log_info()  { echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}INFO${_NC}  $*" >&2; }
log_warn()  { echo -e "[${_YELLOW}$(TS)${_NC}] ${_BOLD}WARN${_NC}  $*" >&2; }
log_error() { echo -e "[${_RED}$(TS)${_NC}] ${_BOLD}ERROR${_NC} $*" >&2; }
log_check() { echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}CHECK${_NC} $* ..." >&2; }
log_pass()  { echo -e "[${_GREEN}$(TS)${_NC}] ${_BOLD}PASS${_NC}  $*" >&2; }
log_fail()  { echo -e "[${_RED}$(TS)${_NC}] ${_BOLD}FAIL${_NC}  $*" >&2; }
log_cmd()   { echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}CMD${_NC}   $*" >&2; }

# ── State ──────────────────────────────────────────────────────────────────
CHECKS_PASS=0
CHECKS_FAIL=0
declare -A CHECK_RESULTS
declare -A CHECK_DETAILS

record_pass() {
    local name="$1"; shift
    CHECK_RESULTS["$name"]="pass"
    CHECK_DETAILS["$name"]="$*"
    log_pass "$name — $*"
    ((CHECKS_PASS++))
}

record_fail() {
    local name="$1"; shift
    CHECK_RESULTS["$name"]="fail"
    CHECK_DETAILS["$name"]="$*"
    log_fail "$name — $*"
    ((CHECKS_FAIL++))
}

# ── Check functions ────────────────────────────────────────────────────────
# Each function: returns 0 on success, calls record_pass/fail internally

check_autocli_binary() {
    log_check "autocli binary"
    local bin
    if bin=$(which autocli 2>/dev/null); then
        local ver
        ver=$(autocli --version 2>/dev/null || echo "unknown")
        record_pass "autocli" "found at $bin, version=$ver"
        return 0
    else
        record_fail "autocli" "not found in PATH — install with: curl -fsSL https://raw.githubusercontent.com/nashsu/AutoCLI/main/scripts/install.sh | sh"
        return 1
    fi
}

check_chrome_running() {
    log_check "Chrome process"
    if pgrep -x "Google Chrome" > /dev/null 2>&1; then
        local count
        count=$(pgrep -c -x "Google Chrome" 2>/dev/null || echo "?")
        record_pass "chrome" "running ($count process(es))"
        return 0
    else
        record_fail "chrome" "Google Chrome is not running — open Chrome with the AutoCLI extension installed"
        return 1
    fi
}

check_daemon_health() {
    log_check "daemon (port $DAEMON_PORT)"
    local resp
    if resp=$(curl -s --max-time "$TIMEOUT_SHORT" "http://${DAEMON_HOST}:${DAEMON_PORT}/ping" 2>/dev/null); then
        local ver
        ver=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('version','unknown'))" 2>/dev/null || echo "parse-error")
        record_pass "daemon" "listening on :${DAEMON_PORT}, version=$ver"
        return 0
    else
        record_fail "daemon" "not responding on http://${DAEMON_HOST}:${DAEMON_PORT}/ping — start with: autocli doctor"
        return 1
    fi
}

check_extension_connected() {
    log_check "Chrome extension"
    local doctor_out
    if doctor_out=$(autocli doctor 2>&1); then
        if echo "$doctor_out" | grep -q '✓ Chrome extension connected'; then
            record_pass "extension" "connected to daemon"
            return 0
        elif echo "$doctor_out" | grep -q '✗ Chrome extension connected'; then
            record_fail "extension" "NOT connected — refresh extension in chrome://extensions, ensure correct Chrome profile"
            return 1
        else
            record_fail "extension" "cannot determine status from autocli doctor"
            return 1
        fi
    else
        record_fail "extension" "autocli doctor command failed"
        return 1
    fi
}

check_linkedin_reachable() {
    log_check "LinkedIn reachability"
    local code
    if code=$(curl -s -o /dev/null -w "%{http_code}" --max-time "$TIMEOUT_LONG" \
        -H "Accept-Language: en-US,en;q=0.9" \
        "https://www.linkedin.com/jobs/" 2>/dev/null); then
        if [ "$code" -lt 400 ]; then
            record_pass "linkedin" "HTTP $code — reachable"
            return 0
        elif [ "$code" -eq 403 ] || [ "$code" -eq 429 ]; then
            record_pass "linkedin" "HTTP $code — rate-limited but reachable"
            return 0
        else
            record_fail "linkedin" "HTTP $code — may be blocked or down"
            return 1
        fi
    else
        record_fail "linkedin" "connection timeout — check network"
        return 1
    fi
}

check_network_dns() {
    log_check "DNS resolution"
    if host linkedin.com > /dev/null 2>&1 || dscacheutil -q host -a name linkedin.com > /dev/null 2>&1 || ping -c 1 -t 3 linkedin.com > /dev/null 2>&1; then
        record_pass "dns" "linkedin.com resolves"
        return 0
    else
        record_warn() {
            echo -e "[${_YELLOW}$(TS)${_NC}] ${_BOLD}WARN${_NC}  $*" >&2
            CHECK_RESULTS["$1"]="warn"
            CHECK_DETAILS["$1"]="$2"
        }
        record_warn "dns" "linkedin.com DNS lookup failed — may still work via cached DNS"
        return 0  # non-critical
    fi
}

check_output_dir() {
    log_check "output directory"
    mkdir -p "$OUTPUT_DIR" 2>/dev/null || true
    if [ -d "$OUTPUT_DIR" ] && [ -w "$OUTPUT_DIR" ]; then
        local files
        files=$(ls "$OUTPUT_DIR"/*.json 2>/dev/null | wc -l | tr -d ' ')
        record_pass "output_dir" "$OUTPUT_DIR is writable ($files existing JSON files)"
        return 0
    else
        record_fail "output_dir" "$OUTPUT_DIR is not writable — check permissions"
        return 1
    fi
}

check_disk_space() {
    log_check "disk space"
    local avail
    if avail=$(df -h . 2>/dev/null | awk 'NR==2 {print $4}'); then
        record_pass "disk" "available: $avail"
        return 0
    else
        record_pass "disk" "could not check (non-critical)"
        return 0
    fi
}

# ── JSON output ────────────────────────────────────────────────────────────
emit_json() {
    local elapsed
    elapsed=$(( $(date +%s) - SCRIPT_START ))
    python3 -c "
import json, sys
results = {
    'timestamp': '$(date -Iseconds)',
    'elapsed_sec': $elapsed,
    'passed': $CHECKS_PASS,
    'failed': $CHECKS_FAIL,
    'checks': {
$(
    for name in "${!CHECK_RESULTS[@]}"; do
        echo "        '$name': {'status': '${CHECK_RESULTS[$name]}', 'detail': '${CHECK_DETAILS[$name]}'},"
    done
)
    }
}
print(json.dumps(results, indent=2))
"
}

# ── Main: Run baseline ────────────────────────────────────────────────────
run_baseline() {
    echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}══════════════════════════════════════════════${_NC}" >&2
    echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}autocli baseline check${_NC}" >&2
    echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}══════════════════════════════════════════════${_NC}" >&2
    echo "" >&2

    # Critical checks — any failure blocks command execution
    check_autocli_binary
    check_chrome_running
    check_daemon_health
    check_extension_connected

    # Advisory checks — failures warn but don't block
    check_linkedin_reachable
    check_network_dns
    check_output_dir
    check_disk_space

    echo "" >&2
    local elapsed
    elapsed=$(( $(date +%s) - SCRIPT_START ))

    if [ "$CHECKS_FAIL" -eq 0 ]; then
        echo -e "[${_GREEN}$(TS)${_NC}] ${_BOLD}══════════════════════════════════════════════${_NC}" >&2
        echo -e "[${_GREEN}$(TS)${_NC}] ${_BOLD}All checks passed (${CHECKS_PASS} checks, ${elapsed}s)${_NC}" >&2
        echo -e "[${_GREEN}$(TS)${_NC}] ${_BOLD}══════════════════════════════════════════════${_NC}" >&2
        return 0
    else
        echo -e "[${_RED}$(TS)${_NC}] ${_BOLD}══════════════════════════════════════════════${_NC}" >&2
        echo -e "[${_RED}$(TS)${_NC}] ${_BOLD}$CHECKS_FAIL check(s) FAILED (${CHECKS_PASS} passed, ${elapsed}s)${_NC}" >&2
        echo -e "[${_RED}$(TS)${_NC}] ${_BOLD}══════════════════════════════════════════════${_NC}" >&2

        # Show remediation hints
        for name in "${!CHECK_RESULTS[@]}"; do
            if [ "${CHECK_RESULTS[$name]}" = "fail" ]; then
                echo -e "[${_YELLOW}$(TS)${_NC}] ${_BOLD}HINT${_NC}  $name: ${CHECK_DETAILS[$name]}" >&2
            fi
        done
        return 1
    fi
}

# ── Execute a command with logging ─────────────────────────────────────────
run_command() {
    local start_ts
    start_ts=$(date +%s)
    log_cmd "Running: $*"
    echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}───── command output ─────${_NC}" >&2

    local cmd_exit=0
    "$@" || cmd_exit=$?

    local elapsed
    elapsed=$(( $(date +%s) - start_ts ))
    echo -e "[${_CYAN}$(TS)${_NC}] ${_BOLD}───── end output ──────────${_NC}" >&2

    if [ "$cmd_exit" -eq 0 ]; then
        log_info "Command completed successfully (${elapsed}s)"
    else
        log_error "Command failed with exit code $cmd_exit (${elapsed}s)"
    fi
    return $cmd_exit
}

# ── Argument parsing ──────────────────────────────────────────────────────
parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --help|-h)
                echo "Usage: $0 [--check-only] [--json] [-- <command...>]"
                echo ""
                echo "Pre-flight diagnostic checks for autocli browser commands."
                echo ""
                echo "Options:"
                echo "  --check-only   Run checks only, don't execute any command"
                echo "  --json         Output final results as JSON to stdout"
                echo "  --help         Show this help"
                echo "  -- <command>   Command to run after checks pass"
                exit 0
                ;;
            --check-only)
                CHECK_ONLY=true
                shift
                ;;
            --json)
                JSON_OUT=true
                shift
                ;;
            --)
                shift
                COMMAND=("$@")
                break
                ;;
            *)
                # Assume everything after is a command
                if [ "$CHECK_ONLY" = false ] && [ ${#COMMAND[@]} -eq 0 ]; then
                    COMMAND=("$@")
                    break
                else
                    log_error "Unknown option: $1"
                    exit 1
                fi
                ;;
        esac
    done
}

# ── Entry point ────────────────────────────────────────────────────────────
main() {
    parse_args "$@"

    local baseline_ok=true
    run_baseline || baseline_ok=false

    if [ "$JSON_OUT" = true ]; then
        emit_json
    fi

    if [ "$CHECK_ONLY" = true ]; then
        if [ "$baseline_ok" = true ]; then
            exit 0
        else
            exit 1
        fi
    fi

    if [ ${#COMMAND[@]} -gt 0 ]; then
        if [ "$baseline_ok" = false ]; then
            CRITICAL_COUNT=0
            for name in autocli chrome daemon extension; do
                if [ "${CHECK_RESULTS[$name]:-fail}" = "fail" ]; then
                    ((CRITICAL_COUNT++))
                fi
            done
            if [ "$CRITICAL_COUNT" -gt 0 ]; then
                log_error "Aborting — $CRITICAL_COUNT critical check(s) failed"
                exit 1
            fi
            log_warn "Continuing despite non-critical warnings..."
        fi
        run_command "${COMMAND[@]}"
        exit $?
    fi

    # No command, not check-only → just ran baseline
    if [ "$baseline_ok" = true ]; then
        exit 0
    else
        exit 1
    fi
}

main "$@"
