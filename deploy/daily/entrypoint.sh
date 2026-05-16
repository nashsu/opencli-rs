#!/usr/bin/env bash
# Container PID 2 (tini is PID 1). Sequence:
#   1. boot-time cdp-discover (gates everything else)
#   2. start supercronic + uvicorn as background children, wait on either.

set -euo pipefail

echo "[entrypoint] boot cdp-discover"
/app/cdp-discover.sh

echo "[entrypoint] starting supercronic + uvicorn"
supercronic -quiet /etc/cron.d/autocli &
CRON_PID=$!

cd /app/api
uv run --no-project -- uvicorn main:app --host 0.0.0.0 --port 8080 &
API_PID=$!

# Forward SIGTERM to children for graceful shutdown via tini.
trap 'kill -TERM "${CRON_PID}" "${API_PID}" 2>/dev/null || true' TERM INT

# Exit when either child exits (compose/Watchtower can then restart cleanly).
wait -n "${CRON_PID}" "${API_PID}"
exit $?
