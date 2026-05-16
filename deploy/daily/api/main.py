"""FastAPI control plane for autocli-daily.

Routes (mounted per SPEC §5.1):
  GET  /api/status   [Bearer]  last_run.json
  POST /api/run      [Bearer]  spawn run-daily.sh (flock-protected)
  GET  /api/logs     [Bearer]  tail -n 200 latest log
  GET  /api/metrics  [open]    Prometheus exposition
  GET  /api/health   [open]    chrome reachability + cdp endpoint sanity
  GET  /jobs         [Bearer]  Supabase read proxy
"""
from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Annotated

import httpx
from fastapi import Depends, FastAPI, HTTPException, Query, Response, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer
from prometheus_client import CONTENT_TYPE_LATEST, Counter, Gauge, generate_latest

import trigger

# ── config ───────────────────────────────────────────────────────────
API_RUN_TOKEN = os.environ["API_RUN_TOKEN"]
SUPABASE_URL = os.environ["SUPABASE_URL"]
SUPABASE_ANON_KEY = os.environ["SUPABASE_ANON_KEY"]
CHROME_HOST = os.environ.get("CHROME_HOST", "autocli-chrome")
CHROME_PORT = int(os.environ.get("CHROME_PORT", "9222"))
LAST_RUN_PATH = Path("/data/output/last_run.json")
LOGS_DIR = Path("/data/logs")
CDP_ENDPOINT_FILE = Path("/run/cdp-endpoint.env")

# ── metrics ──────────────────────────────────────────────────────────
M_RUNS_TOTAL = Counter(
    "autocli_daily_runs_total",
    "Run outcomes",
    labelnames=("result",),
)
M_LAST_RUN_UNIXTS = Gauge("autocli_daily_last_run_unixts", "Unix ts of last run start")
M_LAST_DURATION = Gauge("autocli_daily_last_duration_seconds", "Duration of last run")
M_LAST_EXIT_CODE = Gauge("autocli_daily_last_exit_code", "Exit code of last run")
M_RUN_IN_PROGRESS = Gauge("autocli_daily_run_in_progress", "1 if a run is in flight")
M_ROWS_SCRAPED = Counter("autocli_daily_rows_scraped_total", "Cumulative scraped rows")
M_ROWS_UPSERTED = Counter("autocli_daily_rows_upserted_total", "Cumulative upserted rows")
M_ROWS_SKIPPED = Counter("autocli_daily_rows_skipped_total", "Cumulative skipped rows")
M_CDP_UP = Gauge("autocli_chrome_cdp_up", "1 if chrome:9222 reachable")

# Counter de-dupe key (do not double-count between scrapes)
_last_seen_counters = {"upserted": 0, "scraped": 0, "skipped": 0}


# ── auth ─────────────────────────────────────────────────────────────
bearer = HTTPBearer(auto_error=False)


def require_bearer(creds: Annotated[HTTPAuthorizationCredentials | None, Depends(bearer)]):
    if creds is None or creds.scheme.lower() != "bearer" or creds.credentials != API_RUN_TOKEN:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="missing or invalid bearer")
    return True


# ── app ──────────────────────────────────────────────────────────────
app = FastAPI(title="autocli-daily")


def _read_last_run() -> dict:
    if not LAST_RUN_PATH.exists():
        return {"last_run_unixts": 0, "last_exit_code": None, "rows_scraped": 0, "rows_upserted": 0, "rows_skipped": 0, "errors": []}
    return json.loads(LAST_RUN_PATH.read_text())


def _refresh_metrics():
    """Reflect last_run.json + chrome reachability into Prometheus gauges."""
    lr = _read_last_run()
    if lr.get("last_run_unixts"):
        M_LAST_RUN_UNIXTS.set(lr["last_run_unixts"])
    if lr.get("last_duration_seconds") is not None:
        M_LAST_DURATION.set(lr["last_duration_seconds"])
    if lr.get("last_exit_code") is not None:
        M_LAST_EXIT_CODE.set(lr["last_exit_code"])
    M_RUN_IN_PROGRESS.set(1 if trigger.is_running() else 0)

    # Counter delta — only emit increase, never decrease
    for field, counter in (("rows_upserted", M_ROWS_UPSERTED),
                          ("rows_scraped", M_ROWS_SCRAPED),
                          ("rows_skipped", M_ROWS_SKIPPED)):
        cur = lr.get(field, 0)
        delta = cur - _last_seen_counters[field.split("_", 1)[1]]
        if delta > 0:
            counter.inc(delta)
        _last_seen_counters[field.split("_", 1)[1]] = cur


@app.get("/api/health")
def health():
    try:
        # Chrome DevTools rejects Host headers that aren't an IP or "localhost"
        # (DNS-rebinding protection). We reach it by docker service name, so
        # override the Host header to "localhost" — Chrome accepts that, and
        # this is a yes/no liveness probe (we don't use the response body).
        r = httpx.get(
            f"http://{CHROME_HOST}:{CHROME_PORT}/json/version",
            timeout=2.0,
            headers={"Host": "localhost"},
        )
        chrome_ok = r.status_code == 200
    except Exception:
        chrome_ok = False
    M_CDP_UP.set(1 if chrome_ok else 0)
    cdp_file_ok = CDP_ENDPOINT_FILE.exists()
    body = {"chrome": chrome_ok, "cdp_endpoint_file": cdp_file_ok}
    code = 200 if chrome_ok and cdp_file_ok else 503
    return Response(content=json.dumps(body), status_code=code, media_type="application/json")


@app.get("/api/metrics")
def metrics():
    _refresh_metrics()
    return Response(generate_latest(), media_type=CONTENT_TYPE_LATEST)


@app.get("/api/status")
def api_status(_: Annotated[bool, Depends(require_bearer)]):
    body = _read_last_run()
    body["run_in_progress"] = trigger.is_running()
    return body


@app.post("/api/run", status_code=202)
async def api_run(_: Annotated[bool, Depends(require_bearer)]):
    if trigger.is_running():
        raise HTTPException(status_code=409, detail="run already in progress")
    pid = await trigger.spawn_run_daily()
    return {"started_at": int(time.time()), "pid": pid}


@app.get("/api/logs")
def api_logs(_: Annotated[bool, Depends(require_bearer)], lines: int = Query(200, ge=1, le=10000)):
    files = sorted(LOGS_DIR.glob("run-*.log"))
    if not files:
        return Response(content="", media_type="text/plain")
    latest = files[-1]
    with latest.open("rb") as fh:
        fh.seek(0, 2)
        size = fh.tell()
        # Read up to last ~256KB and then split lines
        read = min(size, 256 * 1024)
        fh.seek(size - read)
        data = fh.read().decode("utf-8", errors="replace")
    tail = "\n".join(data.splitlines()[-lines:])
    return Response(content=tail, media_type="text/plain")


@app.get("/jobs")
def jobs(_: Annotated[bool, Depends(require_bearer)],
         since: str = Query(..., description="ISO date — rows added (created_at) on or after this date")):
    # Lazy import — supabase client takes ~100ms to construct.
    # Uses SUPABASE_ANON_KEY (a real anon JWT, not service-role) so RLS on
    # jobs.jobs is actually enforced. Policy `anon_read_jobs_jobs` (see
    # supabase/migrations/20260516120100_enable_jobs_jobs_rls.sql) grants
    # SELECT-only to anon/authenticated.
    from supabase import create_client
    client = create_client(SUPABASE_URL, SUPABASE_ANON_KEY)
    # Filter on created_at (database insert time), NOT post_time (LinkedIn's
    # original posting date — often days/weeks old for fresh scrapes). Callers
    # asking "jobs added since X" expect ingestion time. Order by created_at
    # newest first so the freshest scrapes surface.
    res = (
        client.schema("jobs")
              .table("jobs")
              .select("id, job_title, company_name, location, salary, post_time, apply_url, priority_score, created_at")
              .gte("created_at", since)
              .order("created_at", desc=True)
              .limit(500)
              .execute()
    )
    return {"count": len(res.data or []), "since": since, "rows": res.data or []}
