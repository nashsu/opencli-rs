#!/usr/bin/env bash
# Discover the CDP page-target WebSocket URL on autocli-chrome and
# write it to /run/cdp-endpoint.env as AUTOCLI_CDP_ENDPOINT=...
# Runs once at boot (gating supercronic + uvicorn) AND once at the
# start of every run-daily.sh (page id can change between cron ticks).
# See deploy/SPEC.md §5.2 "Discovery cadence" + "CDP page target".
#
# Chrome DevTools DNS-rebinding protection: the /json* and /devtools
# endpoints reject any HTTP Host header that is NOT an IP or "localhost"
# (error: "Host header is specified and is not an IP address or
# localhost."). Reaching Chrome by docker service name therefore fails.
# Fix: resolve CHROME_HOST -> container IP and use the IP for BOTH the
# /json probe AND the rewritten ws:// URL, so every Host header Chrome
# sees is an IP (which it accepts). The IP is re-resolved on every run,
# so a recreated chrome container with a new IP is picked up next tick.

set -euo pipefail

CHROME_HOST="${CHROME_HOST:-autocli-chrome}"
CHROME_PORT="${CHROME_PORT:-9222}"
DEADLINE=$(( $(date +%s) + 60 ))   # 60 s budget
INTERVAL=2

resolve_ip() {
    # getent first (glibc NSS, honours docker DNS); fall back to python.
    getent hosts "${CHROME_HOST}" 2>/dev/null | awk '{print $1; exit}' && return 0
    python3 -c "import socket,sys; print(socket.gethostbyname(sys.argv[1]))" "${CHROME_HOST}" 2>/dev/null
}

while (( $(date +%s) < DEADLINE )); do
    chrome_ip="$(resolve_ip || true)"
    if [[ -n "${chrome_ip}" ]]; then
        base="http://${chrome_ip}:${CHROME_PORT}"
        if list_json=$(curl -fsS --max-time 3 "${base}/json/list" 2>/dev/null); then
            ws=$(jq -r '[.[] | select(.type=="page")][0].webSocketDebuggerUrl // empty' <<<"${list_json}")
            if [[ -z "${ws}" || "${ws}" == "null" ]]; then
                # No page target yet — create one. PUT, not POST/GET (Chrome >= M86).
                new_json=$(curl -fsS --max-time 3 -X PUT "${base}/json/new?about:blank" 2>/dev/null || true)
                ws=$(jq -r '.webSocketDebuggerUrl // empty' <<<"${new_json}")
            fi
            if [[ -n "${ws}" && "${ws}" != "null" ]]; then
                # Chrome reports its own bind host (localhost:9223). Rewrite the
                # host:port to the resolved container IP so the WS upgrade's Host
                # header is an IP (passes Chrome's rebind check) and the TCP
                # target is reachable from this container's netns.
                rewritten=$(sed -E "s|ws://[^/]+|ws://${chrome_ip}:${CHROME_PORT}|" <<<"${ws}")
                echo "AUTOCLI_CDP_ENDPOINT=${rewritten}" > /run/cdp-endpoint.env
                chmod 0644 /run/cdp-endpoint.env
                echo "[cdp-discover] ${rewritten}"
                exit 0
            fi
        fi
    fi
    sleep "${INTERVAL}"
done

echo "[cdp-discover] FATAL: chrome unreachable after 60s (host=${CHROME_HOST} ip=${chrome_ip:-unresolved})" >&2
exit 1
