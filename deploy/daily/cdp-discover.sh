#!/usr/bin/env bash
# Discover the CDP page-target WebSocket URL on autocli-chrome and
# write it to /run/cdp-endpoint.env as AUTOCLI_CDP_ENDPOINT=...
# Runs once at boot (gating supercronic + uvicorn) AND once at the
# start of every run-daily.sh (page id can change between cron ticks).
# See deploy/SPEC.md §5.2 "Discovery cadence" + "CDP page target".

set -euo pipefail

CHROME_HOST="${CHROME_HOST:-autocli-chrome}"
CHROME_PORT="${CHROME_PORT:-9222}"
EXT_HOST_PORT="${CHROME_HOST}:${CHROME_PORT}"
DEADLINE=$(( $(date +%s) + 60 ))   # 60 s budget
INTERVAL=2

while (( $(date +%s) < DEADLINE )); do
    if list_json=$(curl -fsS --max-time 3 "http://${EXT_HOST_PORT}/json/list" 2>/dev/null); then
        ws=$(jq -r '[.[] | select(.type=="page")][0].webSocketDebuggerUrl // empty' <<<"${list_json}")
        if [[ -z "${ws}" || "${ws}" == "null" ]]; then
            # No page target yet — create one. PUT, not POST/GET (Chrome >= M86).
            new_json=$(curl -fsS --max-time 3 -X PUT "http://${EXT_HOST_PORT}/json/new?about:blank" 2>/dev/null || true)
            ws=$(jq -r '.webSocketDebuggerUrl // empty' <<<"${new_json}")
        fi
        if [[ -n "${ws}" && "${ws}" != "null" ]]; then
            # Chrome reports its internal host (localhost:9223). Rewrite to the docker service name.
            rewritten=$(sed -E "s|ws://[^/]+|ws://${EXT_HOST_PORT}|" <<<"${ws}")
            echo "AUTOCLI_CDP_ENDPOINT=${rewritten}" > /run/cdp-endpoint.env
            chmod 0644 /run/cdp-endpoint.env
            echo "[cdp-discover] ${rewritten}"
            exit 0
        fi
    fi
    sleep "${INTERVAL}"
done

echo "[cdp-discover] FATAL: chrome unreachable after 60s" >&2
exit 1
