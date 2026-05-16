#!/usr/bin/env bash
# Daily orchestrator. Invoked by:
#   * supercronic (cron tick)
#   * POST /api/run (FastAPI shells out via trigger.py)
# Implements the §5.2 unified retry policy: 3 attempts at 15s/60s/240s.
# Uses flock so cron + /api/run can't collide.

set -euo pipefail

LOCK=/var/lock/autocli-daily.lock
LAST_RUN_JSON=/data/output/last_run.json
LOG_DIR=/data/logs
OUTPUT_DIR=/data/output
DATE_STAMP=$(date +%Y%m%d)
LOG_FILE="${LOG_DIR}/run-${DATE_STAMP}.log"

mkdir -p "${LOG_DIR}" "${OUTPUT_DIR}"

# Single-instance gate. -n = non-blocking; -E 200 = exit 200 if already locked.
exec 9>"${LOCK}"
if ! flock -n -E 200 9; then
    echo "[run-daily] another run is in progress; exit 200" >&2
    exit 200
fi

run_once() {
    local attempt="$1"
    local started_at
    started_at=$(date +%s)
    echo "[run-daily] attempt ${attempt} starting at $(date -Iseconds)" | tee -a "${LOG_FILE}"

    # Refresh CDP endpoint every attempt — Chrome may have restarted.
    if ! /app/cdp-discover.sh >>"${LOG_FILE}" 2>&1; then
        echo "[run-daily] cdp-discover failed" >>"${LOG_FILE}"
        return 1
    fi
    # shellcheck disable=SC1091
    source /run/cdp-endpoint.env

    local out="${OUTPUT_DIR}/${DATE_STAMP}.json"
    if ! /app/bin/autocli linkedin recommended --limit 0 --with_jd true -f json > "${out}" 2>>"${LOG_FILE}"; then
        echo "[run-daily] autocli failed" >>"${LOG_FILE}"
        return 2
    fi

    # Sync to Supabase
    if ! uv --project /app/api run --no-project -- python /app/scripts/sync_autocli_jobs.py --input "${out}" >>"${LOG_FILE}" 2>&1; then
        echo "[run-daily] sync_autocli_jobs.py failed" >>"${LOG_FILE}"
        return 3
    fi

    local ended_at
    ended_at=$(date +%s)
    local duration=$(( ended_at - started_at ))

    # Parse counts from the last JSON line printed by sync_autocli_jobs.py
    local summary
    summary=$(grep -E '^\{' "${LOG_FILE}" | tail -1 || echo "{}")
    local upserted scraped skipped
    upserted=$(jq -r '.upserted // 0' <<<"${summary}")
    scraped=$(jq -r '.input_rows // 0' <<<"${summary}")
    skipped=$(jq -r '.skipped // 0' <<<"${summary}")

    jq -n \
        --argjson last_run_unixts "${started_at}" \
        --argjson last_duration_seconds "${duration}" \
        --argjson last_exit_code 0 \
        --argjson rows_scraped "${scraped}" \
        --argjson rows_upserted "${upserted}" \
        --argjson rows_skipped "${skipped}" \
        --arg last_log "$(basename "${LOG_FILE}")" \
        '{last_run_unixts:$last_run_unixts,last_duration_seconds:$last_duration_seconds,last_exit_code:$last_exit_code,rows_scraped:$rows_scraped,rows_upserted:$rows_upserted,rows_skipped:$rows_skipped,last_log:$last_log,errors:[]}' \
        > "${LAST_RUN_JSON}"
    echo "[run-daily] attempt ${attempt} succeeded in ${duration}s" | tee -a "${LOG_FILE}"
    return 0
}

backoffs=(15 60 240)
attempt=1
final_rc=0
for sleep_for in "${backoffs[@]}" final; do
    if run_once "${attempt}"; then
        final_rc=0
        break
    fi
    final_rc=$?
    if [[ "${sleep_for}" == "final" ]]; then
        break
    fi
    echo "[run-daily] sleeping ${sleep_for}s before retry" | tee -a "${LOG_FILE}"
    sleep "${sleep_for}"
    attempt=$(( attempt + 1 ))
done

if (( final_rc != 0 )); then
    jq -n \
        --argjson last_run_unixts "$(date +%s)" \
        --argjson last_exit_code "${final_rc}" \
        --arg last_log "$(basename "${LOG_FILE}")" \
        '{last_run_unixts:$last_run_unixts,last_exit_code:$last_exit_code,rows_scraped:0,rows_upserted:0,rows_skipped:0,last_log:$last_log,errors:["see log"]}' \
        > "${LAST_RUN_JSON}"
fi
exit "${final_rc}"
