#!/bin/bash
# Test suite for autocli-baseline.sh
# Usage: bash scripts/test_baseline.sh
set -euo pipefail

SCRIPT="scripts/autocli-baseline.sh"
PASS=0
FAIL=0

green() { echo "  ✓ $*"; }
red()   { echo "  ✗ $*"; }

# Usage: check "description" command [args...]
# Tests that command exits 0
check_pass() {
    local desc="$1"; shift
    if "$@"; then
        green "$desc"
        PASS=$((PASS + 1))
    else
        red "$desc (expected exit 0, got $?)"
        FAIL=$((FAIL + 1))
    fi
}

# Usage: check_fail "description" command [args...]
# Tests that command exits non-zero
check_fail() {
    local desc="$1"; shift
    if ! "$@"; then
        green "$desc"
        PASS=$((PASS + 1))
    else
        red "$desc (expected non-zero exit)"
        FAIL=$((FAIL + 1))
    fi
}

# Usage: check_contains "description" "pattern" command [args...]
# Tests that command output contains the pattern
check_contains() {
    local desc="$1"; shift
    local pattern="$1"; shift
    if "$@" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -q "$pattern"; then
        green "$desc"
        PASS=$((PASS + 1))
    else
        red "$desc (output missing '$pattern')"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== autocli-baseline.sh Test Suite ==="
echo ""

# ── Test 1: Script exists and is executable ──────────────────────────
echo "[Test 1] Script file check"
check_pass "script exists"        test -f "$SCRIPT"
check_pass "script executable"    test -x "$SCRIPT"

# ── Test 2: Help flag ────────────────────────────────────────────────
echo ""
echo "[Test 2] --help flag"
check_pass "shows usage without error"   bash "$SCRIPT" --help

# ── Test 3: Check-only mode ──────────────────────────────────────────
echo ""
echo "[Test 3] --check-only mode"
check_pass "runs baseline checks"         bash "$SCRIPT" --check-only
check_pass "all checks pass currently"    bash "$SCRIPT" --check-only

# ── Test 4: Log output format ────────────────────────────────────────
echo ""
echo "[Test 4] Log format"
check_contains "has timestamp format"     "[0-9][0-9]:[0-9][0-9]:[0-9][0-9]"     bash "$SCRIPT" --check-only
check_contains "has CHECK markers"        "CHECK"            bash "$SCRIPT" --check-only
check_contains "shows passed count"       "passed"           bash "$SCRIPT" --check-only

# ── Test 5: JSON output ──────────────────────────────────────────────
echo ""
echo "[Test 5] --json output"
JSON_OUT=$(bash "$SCRIPT" --check-only --json 2>/dev/null || true)
if echo "$JSON_OUT" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 'checks' in d; assert 'passed' in d; print('valid')" 2>/dev/null; then
    check_pass "outputs valid JSON with checks"  true
else
    red "JSON output invalid or missing fields"
    FAIL=$((FAIL + 1))
fi

# ── Test 6: Missing binary handled ───────────────────────────────────
echo ""
echo "[Test 6] Missing binary simulation"
check_fail "handles missing autocli"   env PATH=/usr/bin:/bin bash "$SCRIPT" --check-only 2>/dev/null

# ── Test 7: Command passthrough ──────────────────────────────────────
echo ""
echo "[Test 7] Command passthrough"
RESULT=$(bash "$SCRIPT" -- echo "hello-autocli-test" 2>/dev/null || true)
if echo "$RESULT" | grep -q "hello-autocli-test"; then
    check_pass "executes command after checks"   true
else
    red "command not executed after checks"
    FAIL=$((FAIL + 1))
fi

# ── Test 8: Exit codes ───────────────────────────────────────────────
echo ""
echo "[Test 8] Exit codes"
check_pass "--check-only succeeds"      bash "$SCRIPT" --check-only
check_fail "--check-only with bad PATH fails"  env PATH=/usr/bin:/bin bash "$SCRIPT" --check-only 2>/dev/null

# ── Test 9: Extension freshness detection ────────────────────────────
echo ""
echo "[Test 9] Extension freshness"

# Simulate stale dist by touching it and setting an old refresh marker
REFRESH_MARKER="/tmp/.autocli-baseline-refresh-test"
EXT_DIST="extension/dist/background.js"

if [ -f "$EXT_DIST" ]; then
    # Create an old marker (epoch 0)
    touch -t 200001010000 "$REFRESH_MARKER" 2>/dev/null || true

    # Run check — should warn about stale extension
    OUT=$(AUTOCLI_REFRESH_MARKER="$REFRESH_MARKER" bash "$SCRIPT" --check-only 2>&1 || true)
    if echo "$OUT" | grep -qi "refresh\|stale\|outdated\|newer\|behind"; then
        check_pass "detects stale extension"  true
    else
        red "did not detect stale extension"
        FAIL=$((FAIL + 1))
    fi

    # Clean up
    rm -f "$REFRESH_MARKER"
else
    check_pass "dist file exists (skip freshness)"  test -f "$EXT_DIST"
fi

# ── Test 10: --refresh-extension flag exists ─────────────────────────
echo ""
echo "[Test 10] --refresh-extension flag"
check_contains "--refresh-extension in help" "refresh-extension" bash "$SCRIPT" --help

# ── Summary ──────────────────────────────────────────────────────────
echo ""
echo "========================================="
echo "Results: $PASS passed, $FAIL failed"
echo "========================================="

[ "$FAIL" -eq 0 ] || exit 1
