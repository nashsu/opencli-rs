# AutoCLI Daily Microservice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the manual daily `autocli linkedin recommended … | uv run scripts/sync_autocli_jobs.py` flow into an auto-scheduled, externally accessible microservice (5 containers, daily cron, HTTP API, Cloudflare Tunnel, Prometheus+Grafana), deployed to `100.108.80.9`.

**Architecture:** 5-container docker-compose stack on a dedicated Docker host. `autocli-chrome` (Stagehand-style VNC Chromium with persistent profile + CDP 9222). `autocli-daily` (Python+uv+supercronic+FastAPI; pre-built `autocli` linux/amd64 binary copied in). `cloudflared` (Tunnel token mode, ingress managed in Cloudflare dashboard). `prometheus` + `grafana` (monitoring). Pull-based deploy via GHCR + existing Watchtower. Cloudflare Access enforces auth on all 4 subdomains (`vnc/cdp/api/grafana`.autocli.<your-zone>). Reference: [SPEC.md](./SPEC.md).

**Tech Stack:** Rust 1.94 (autocli binary), Debian Bookworm (base), Chromium + Xvfb + x11vnc + noVNC (chrome image), Python 3.12 + uv + FastAPI + supercronic (daily image), `cloudflare/cloudflared:2025.4.0`, `prom/prometheus:v3.5.0`, `grafana/grafana:11.6.0`, GitHub Actions (CI), `docker/metadata-action@v5`, `docker/build-push-action@v6`.

**Worktree:** `/Users/sanchezrick/Documents/Github/AutoCLI-daily/` on branch `feat/daily-microservice` (already created from `origin/main`).

---

## File map

### Created in this PR

| Path | Purpose |
|---|---|
| `rust-toolchain.toml` | Workspace toolchain pin (1.94) — keeps local / CI / Phase 0 builder in sync |
| `crates/autocli-browser/src/bridge.rs` | **MODIFY**: add `AUTOCLI_CDP_ENDPOINT` branch returning `CdpPage` |
| `crates/autocli-browser/src/bridge.rs` tests | **MODIFY**: add unit test for the env-var branch |
| `deploy/chrome/Dockerfile` | Stagehand-style Chromium + Xvfb + noVNC + socat (copy of my-stagehand-app/Dockerfile.chrome) |
| `deploy/chrome/entrypoint-vnc.sh` | Xvfb → x11vnc → noVNC → socat → Chromium (copy of my-stagehand-app/scripts/entrypoint-vnc.sh) |
| `deploy/daily/Dockerfile` | Python 3.12 + uv + supercronic + pre-built `autocli` binary |
| `deploy/daily/entrypoint.sh` | Boot-time `cdp-discover.sh` → supercronic + uvicorn under tini |
| `deploy/daily/cdp-discover.sh` | `GET /json/list` → if empty `PUT /json/new?about:blank` → rewrite host → write `/run/cdp-endpoint.env` |
| `deploy/daily/run-daily.sh` | `flock` + re-run `cdp-discover.sh` + `source env` + autocli + sync + retry policy |
| `deploy/daily/crontab` | `0 3 * * * /app/run-daily.sh` (TZ=Europe/London) + 04:00 retention sweep |
| `deploy/daily/api/pyproject.toml` | uv project: fastapi, uvicorn, supabase, prometheus-client, httpx |
| `deploy/daily/api/main.py` | FastAPI: `/api/{status,run,logs,metrics,health}` + `/jobs` |
| `deploy/daily/api/trigger.py` | Shared subprocess executor (cron + `/api/run` call into same code) |
| `deploy/daily/api/tests/test_main.py` | FastAPI auth/route tests with httpx TestClient |
| `deploy/prometheus/prometheus.yml` | Single scrape job for `autocli-daily:8080` with `metrics_path: /api/metrics` |
| `deploy/grafana/provisioning/datasources/prometheus.yml` | Pre-provisioned Prometheus datasource |
| `deploy/grafana/provisioning/dashboards/dashboards.yml` | Dashboard provider config |
| `deploy/grafana/provisioning/dashboards/autocli.json` | The 6-panel dashboard JSON |
| `deploy/docker-compose.yml` | Production stack (5 services + named volumes + watchtower labels) |
| `deploy/docker-compose.local.yml` | Local override: bind localhost ports, disable cloudflared |
| `deploy/.env.example` | Empty template — every required var listed |
| `deploy/README.md` | Deploy runbook + secret transfer + Cloudflare dashboard checklist |
| `.github/workflows/deploy-microservice.yml` | CI: rust build → 2 docker images → GHCR push with conditional tags |

### NOT modified

`crates/autocli-pipeline`, `autocli-discovery`, `autocli-core`, `autocli-cli`, every YAML adapter, every script under `scripts/` — these stay untouched because the IPage trait is the only contract the Rust patch changes.

---

## Phase A — Repo hygiene + Rust prerequisite patch

### Task 1: Pin Rust toolchain workspace-wide

**Files:**
- Create: `rust-toolchain.toml`

- [ ] **Step 1: Create the toolchain file**

Write `rust-toolchain.toml` at repo root:
```toml
[toolchain]
channel = "1.94"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 2: Verify cargo picks it up**

Run from worktree root:
```bash
cargo --version
# Expected: cargo 1.94.x (anything)
rustup show active-toolchain
# Expected: 1.94-<host-triple> (from 'rust-toolchain.toml')
```

- [ ] **Step 3: Commit**

```bash
git add rust-toolchain.toml
git commit -m "chore: pin workspace Rust toolchain to 1.94

Aligns local dev (operator was on rustc 1.94.1), CI (was using
ubuntu-latest default), and the Phase 0 Docker builder
(deploy/SPEC.md). Single source of truth; future bumps touch only
this file."
```

---

### Task 2: BrowserBridge CDP-wiring patch — write failing test

**Files:**
- Modify: `crates/autocli-browser/src/bridge.rs` (test module at bottom)

- [ ] **Step 1: Append a new failing test to the existing `#[cfg(test)] mod tests` block**

Open `crates/autocli-browser/src/bridge.rs`. Find the `#[cfg(test)] mod tests` at the bottom. Add this test below `test_bridge_default_port`:

```rust
    #[tokio::test]
    async fn test_connect_uses_cdp_endpoint_when_env_var_set() {
        use std::env;

        // Set AUTOCLI_CDP_ENDPOINT to an unreachable address.
        // We expect a BrowserConnect error (not a "Chrome not running" error from the daemon path).
        // SAFETY: tests in this module are single-threaded by default; if more env-touching tests
        // are added later, switch to `serial_test`.
        env::set_var("AUTOCLI_CDP_ENDPOINT", "ws://127.0.0.1:1/devtools/page/never");
        let mut bridge = BrowserBridge::default_port();
        let result = bridge.connect().await;
        env::remove_var("AUTOCLI_CDP_ENDPOINT");

        // The CDP path took over (not the daemon path) — error must come from CdpPage::connect.
        // CdpPage::connect on unreachable target produces CliError::BrowserConnect with
        // message starting "Failed to connect to CDP endpoint".
        let err = result.expect_err("connect() should fail against an unreachable CDP endpoint");
        let msg = format!("{err}");
        assert!(
            msg.contains("Failed to connect to CDP endpoint") || msg.contains("CDP"),
            "expected CDP-path error, got: {msg}"
        );
        assert!(
            !msg.contains("Chrome is not running"),
            "got daemon-path error — CDP env-var branch was not taken: {msg}"
        );
    }
```

- [ ] **Step 2: Run the test and confirm it fails**

```bash
cargo test -p autocli-browser bridge::tests::test_connect_uses_cdp_endpoint_when_env_var_set -- --nocapture
```

Expected: test FAILS — either because the daemon path took over and reports "Chrome is not running" (the bug we are about to fix), or because `connect()` doesn't yet check the env var at all.

---

### Task 3: BrowserBridge CDP-wiring patch — implement

**Files:**
- Modify: `crates/autocli-browser/src/bridge.rs:33-35`

- [ ] **Step 1: Inspect current `connect()`**

```bash
grep -n "pub async fn connect" crates/autocli-browser/src/bridge.rs
```

Expected: line 33 starts `pub async fn connect`.

- [ ] **Step 2: Replace `connect()` body with the env-var branch**

In `bridge.rs`, find:
```rust
    pub async fn connect(&mut self) -> Result<Arc<dyn IPage>, CliError> {
        Ok(self.connect_daemon_page().await?)
    }
```

Replace with:
```rust
    pub async fn connect(&mut self) -> Result<Arc<dyn IPage>, CliError> {
        // CDP-direct path: bypass daemon + extension when AUTOCLI_CDP_ENDPOINT is set.
        // Used by the autocli-daily microservice (deploy/SPEC.md §5.1).
        if let Ok(endpoint) = std::env::var("AUTOCLI_CDP_ENDPOINT") {
            if !endpoint.is_empty() {
                let page = crate::CdpPage::connect(&endpoint).await?;
                return Ok(Arc::new(page));
            }
        }
        Ok(self.connect_daemon_page().await?)
    }
```

- [ ] **Step 3: Run the test and confirm it passes**

```bash
cargo test -p autocli-browser bridge::tests::test_connect_uses_cdp_endpoint_when_env_var_set -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Run the whole crate's tests to confirm no regression**

```bash
cargo test -p autocli-browser
```

Expected: all tests pass. The two existing `test_bridge_construction` / `test_bridge_default_port` still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/autocli-browser/src/bridge.rs
git commit -m "feat(browser): wire CdpPage into BrowserBridge::connect

Add AUTOCLI_CDP_ENDPOINT env-var branch at the top of
BrowserBridge::connect. When set, skip daemon spawn + extension
polling and return Arc<CdpPage> directly. The IPage trait contract
is unchanged so pipeline executors and YAML adapters consume either
implementation transparently.

Required prerequisite for the autocli-daily microservice
(deploy/SPEC.md §1.A) which runs autocli in a container with no
Chrome extension or daemon, connecting to a sibling Chrome container
via CDP."
```

---

### Task 4: Manual smoke test of the Rust patch against local Stagehand Chrome

**Files:** none modified

- [ ] **Step 1: Build release binary**

```bash
cargo build --release -p autocli
```

Expected: builds; binary at `target/release/autocli`.

- [ ] **Step 2: Confirm local Stagehand Chrome is up + logged into LinkedIn**

```bash
docker ps --filter "name=stagehand-chrome" --format "{{.Status}} {{.Ports}}"
# Expected: a "Up …" line with 9222 and 6080 ports
curl -s http://localhost:9222/json/version | jq -r '.Browser'
# Expected: non-empty Chrome version string
```

If Chrome isn't running locally, start it from `~/Documents/Github/my-stagehand-app/` (the operator's existing setup).

- [ ] **Step 3: Extract page WS URL**

```bash
WS_URL=$(curl -s http://localhost:9222/json/list \
  | jq -r '[.[] | select(.type == "page")][0].webSocketDebuggerUrl')
echo "WS_URL=${WS_URL}"
# Expected: ws://localhost:9223/devtools/page/<id> or ws://127.0.0.1:9223/...
```

If the list is empty:
```bash
WS_URL=$(curl -s -X PUT "http://localhost:9222/json/new?about:blank" | jq -r '.webSocketDebuggerUrl')
echo "WS_URL=${WS_URL}"
```

- [ ] **Step 4: Run autocli LinkedIn recommended through CDP**

```bash
AUTOCLI_CDP_ENDPOINT="${WS_URL}" \
  ./target/release/autocli linkedin recommended --limit 5 --with_jd false -f json \
  > /tmp/cdp-smoketest.json
jq 'length' /tmp/cdp-smoketest.json
# Expected: an integer ≥ 1
jq '.[0] | keys' /tmp/cdp-smoketest.json
# Expected: array including "title", "company", "url" etc.
```

- [ ] **Step 5: Record success**

If the JSON has real job rows, the patch is verified. If empty or error, halt the plan and debug — the rest depends on this working.

No commit needed (no source changes).

---

## Phase B — `deploy/` scaffold

### Task 5: deploy/chrome — Dockerfile

**Files:**
- Create: `deploy/chrome/Dockerfile`

- [ ] **Step 1: Create the file**

Copy verbatim from `~/Documents/Github/my-stagehand-app/Dockerfile.chrome`:
```dockerfile
FROM debian:bookworm-slim

# Install Chromium and dependencies
RUN apt-get update && apt-get install -y \
    chromium \
    curl \
    wget \
    ca-certificates \
    fonts-liberation \
    fonts-noto-cjk \
    fonts-noto-color-emoji \
    libnss3 libgtk-3-0 libdrm2 libgbm1 libasound2 \
    pulseaudio \
    xdg-utils \
    xvfb \
    x11-utils \
    x11-xserver-utils \
    xterm \
    x11vnc \
    novnc \
    websockify \
    autocutsel \
    xclip \
    x11-apps \
    supervisor \
    socat \
    tini \
    --no-install-recommends && \
    rm -rf /var/lib/apt/lists/*

# Create user data directory
RUN mkdir -p /root/.config/chromium && \
    chmod -R 755 /root/.config/chromium && \
    mkdir -p /tmp/vnc

COPY deploy/chrome/entrypoint-vnc.sh /usr/local/bin/entrypoint-vnc.sh
RUN chmod +x /usr/local/bin/entrypoint-vnc.sh

EXPOSE 9222 5900 6080

ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint-vnc.sh"]
```

Note the **single change** from my-stagehand-app: `COPY deploy/chrome/entrypoint-vnc.sh ...` (was `COPY scripts/entrypoint-vnc.sh ...`), because Phase 0 / CI both use repo-root context per SPEC §4.1.

- [ ] **Step 2: Commit**

```bash
git add deploy/chrome/Dockerfile
git commit -m "feat(deploy): chrome image Dockerfile

Copy of my-stagehand-app/Dockerfile.chrome with the COPY path
rewritten for repo-root build context (deploy/SPEC.md §4.1)."
```

---

### Task 6: deploy/chrome — entrypoint-vnc.sh

**Files:**
- Create: `deploy/chrome/entrypoint-vnc.sh`

- [ ] **Step 1: Copy the file**

```bash
cp ~/Documents/Github/my-stagehand-app/scripts/entrypoint-vnc.sh deploy/chrome/entrypoint-vnc.sh
chmod +x deploy/chrome/entrypoint-vnc.sh
```

- [ ] **Step 2: Verify content**

```bash
head -5 deploy/chrome/entrypoint-vnc.sh
# Expected: starts with "#!/bin/bash" and "# Docker Chrome (VNC 可视化模式) 启动脚本"
grep -c "exec chromium" deploy/chrome/entrypoint-vnc.sh
# Expected: 1
```

- [ ] **Step 3: Commit**

```bash
git add deploy/chrome/entrypoint-vnc.sh
git commit -m "feat(deploy): chrome image entrypoint-vnc.sh

Verbatim from my-stagehand-app/scripts/entrypoint-vnc.sh:
Xvfb -> x11vnc -> noVNC -> socat 9222->9223 -> Chromium with
--remote-debugging-port=9223 --user-data-dir=/root/.config/chromium.
Extension loading via /opt/extensions/*/manifest.json is preserved
even though this design ships with no extensions."
```

---

### Task 7: deploy/daily — Dockerfile

**Files:**
- Create: `deploy/daily/Dockerfile`

- [ ] **Step 1: Create the file**

```dockerfile
# syntax=docker/dockerfile:1.7
FROM python:3.12-slim-bookworm

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PROJECT_ENVIRONMENT=/opt/venv \
    PATH=/opt/venv/bin:/usr/local/bin:/usr/bin:/bin

# OS deps: tini for PID-1, jq for cdp-discover, curl for healthcheck, util-linux for flock
RUN apt-get update && apt-get install -y --no-install-recommends \
        tini curl jq ca-certificates util-linux tzdata \
    && rm -rf /var/lib/apt/lists/*

# supercronic: container-friendly cron
ARG SUPERCRONIC_VERSION=v0.2.30
ARG SUPERCRONIC_SHA1SUM=9aeb41e00cc7b71d30d33c57a2333f2c2581a201
RUN curl -fsSLO "https://github.com/aptible/supercronic/releases/download/${SUPERCRONIC_VERSION}/supercronic-linux-amd64" \
    && echo "${SUPERCRONIC_SHA1SUM}  supercronic-linux-amd64" | sha1sum -c - \
    && mv supercronic-linux-amd64 /usr/local/bin/supercronic \
    && chmod +x /usr/local/bin/supercronic

# uv (Astral) — single static binary
RUN curl -LsSf https://astral.sh/uv/install.sh | env INSTALLER_NO_MODIFY_PATH=1 sh \
    && mv /root/.local/bin/uv /usr/local/bin/uv

WORKDIR /app

# Python deps first (cache-friendly)
COPY deploy/daily/api/pyproject.toml deploy/daily/api/uv.lock* /app/api/
RUN cd /app/api && uv sync --frozen --no-dev || uv sync --no-dev

# Shipped sync script & priority scorer
COPY scripts/sync_autocli_jobs.py scripts/job_priority_scorer.py scripts/job_priority_config.py /app/scripts/

# FastAPI app
COPY deploy/daily/api /app/api

# Shell glue
COPY deploy/daily/cdp-discover.sh deploy/daily/run-daily.sh deploy/daily/entrypoint.sh /app/
RUN chmod +x /app/cdp-discover.sh /app/run-daily.sh /app/entrypoint.sh

COPY deploy/daily/crontab /etc/cron.d/autocli

# Pre-built autocli binary (produced by Phase 0 docker-rust step OR CI build-autocli-binary job)
COPY deploy/daily/bin/autocli /app/bin/autocli
RUN chmod +x /app/bin/autocli

# Writable runtime dirs
RUN mkdir -p /data/output /data/logs /run && \
    install -m 0644 /dev/null /data/logs/.keep && \
    install -m 0644 /dev/null /data/output/.keep

ENV TZ=Europe/London \
    CRON_SCHEDULE="0 3 * * *" \
    OUTPUT_RETENTION_DAYS=30

EXPOSE 8080

ENTRYPOINT ["tini", "--", "/app/entrypoint.sh"]
```

- [ ] **Step 2: Commit**

```bash
git add deploy/daily/Dockerfile
git commit -m "feat(deploy): daily image Dockerfile

Multi-arch-aware single-stage image:
- python:3.12-slim-bookworm base
- tini (PID 1), util-linux (flock), jq (CDP discovery), curl (probes)
- supercronic (container cron) pinned to v0.2.30 with sha1 verify
- uv (Astral) for Python deps
- Pre-built autocli binary copied from deploy/daily/bin/
- FastAPI app + scripts/sync_autocli_jobs.py + scorer modules
- Boot via tini -> entrypoint.sh
- TZ=Europe/London, CRON_SCHEDULE default 03:00."
```

---

### Task 8: deploy/daily — cdp-discover.sh

**Files:**
- Create: `deploy/daily/cdp-discover.sh`

- [ ] **Step 1: Create the file**

```bash
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
```

- [ ] **Step 2: Verify shell parses**

```bash
bash -n deploy/daily/cdp-discover.sh
# Expected: no output (syntax OK)
```

- [ ] **Step 3: Commit**

```bash
chmod +x deploy/daily/cdp-discover.sh
git add deploy/daily/cdp-discover.sh
git commit -m "feat(deploy): cdp-discover.sh

Find or create a CDP page target on autocli-chrome:9222.
- GET /json/list, pick first type:page
- if list is empty, PUT /json/new?about:blank (Chrome >= M86)
- rewrite host (localhost:9223 -> autocli-chrome:9222) so the WS URL
  is reachable from the daily container's network namespace
- write to /run/cdp-endpoint.env (sourced by run-daily.sh)
- 60s retry budget; exit 1 on timeout (entrypoint exits non-zero,
  restart: unless-stopped recreates container until chrome ready)."
```

---

### Task 9: deploy/daily — run-daily.sh

**Files:**
- Create: `deploy/daily/run-daily.sh`

- [ ] **Step 1: Create the file**

```bash
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
```

- [ ] **Step 2: Verify shell parses**

```bash
bash -n deploy/daily/run-daily.sh
# Expected: no output
chmod +x deploy/daily/run-daily.sh
```

- [ ] **Step 3: Commit**

```bash
git add deploy/daily/run-daily.sh
git commit -m "feat(deploy): run-daily.sh orchestrator

- flock -n to prevent cron + /api/run from colliding
- per-attempt cdp-discover refresh (page id may have rotated)
- runs autocli linkedin recommended -> JSON -> sync_autocli_jobs.py
- unified retry: 3 attempts at 15s/60s/240s (SPEC §5.2)
- writes /data/output/last_run.json consumed by /api/status."
```

---

### Task 10: deploy/daily — entrypoint.sh

**Files:**
- Create: `deploy/daily/entrypoint.sh`

- [ ] **Step 1: Create the file**

```bash
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
```

- [ ] **Step 2: Verify + commit**

```bash
bash -n deploy/daily/entrypoint.sh
chmod +x deploy/daily/entrypoint.sh
git add deploy/daily/entrypoint.sh
git commit -m "feat(deploy): daily entrypoint.sh

Boot-time cdp-discover gate, then runs supercronic + uvicorn in
parallel under tini. wait -n exits as soon as either child dies, so
compose's restart policy can pick up failure modes (e.g. uvicorn
panic, supercronic crash)."
```

---

### Task 11: deploy/daily — crontab

**Files:**
- Create: `deploy/daily/crontab`

- [ ] **Step 1: Create the file**

```
# supercronic crontab — runs in container TZ=Europe/London.
# Daily LinkedIn pull
0 3 * * * /app/run-daily.sh

# Output retention: delete files older than ${OUTPUT_RETENTION_DAYS:-30} days
0 4 * * * find /data/output -name "*.json" -type f -mtime +30 -delete
```

- [ ] **Step 2: Commit**

```bash
git add deploy/daily/crontab
git commit -m "feat(deploy): supercronic crontab

03:00 daily LinkedIn pull + 04:00 30-day output retention sweep
(SPEC §5.2). TZ resolved by the container's TZ=Europe/London."
```

---

### Task 12: deploy/daily/api — pyproject.toml

**Files:**
- Create: `deploy/daily/api/pyproject.toml`

- [ ] **Step 1: Create the file**

```toml
[project]
name = "autocli-daily-api"
version = "0.1.0"
description = "FastAPI control plane for the autocli-daily microservice"
requires-python = ">=3.12"
dependencies = [
    "fastapi>=0.115,<0.116",
    "uvicorn[standard]>=0.32,<0.33",
    "httpx>=0.28,<0.29",
    "supabase>=2.8,<3.0",
    "prometheus-client>=0.21,<0.22",
    "python-multipart>=0.0.12",
]

[dependency-groups]
dev = [
    "pytest>=8.3,<9",
    "pytest-asyncio>=0.24,<1",
    "respx>=0.21,<1",
]

[tool.uv]
package = false
```

- [ ] **Step 2: Resolve lockfile**

```bash
cd deploy/daily/api && uv lock && cd -
ls deploy/daily/api/uv.lock
# Expected: file exists
```

- [ ] **Step 3: Commit**

```bash
git add deploy/daily/api/pyproject.toml deploy/daily/api/uv.lock
git commit -m "feat(deploy): FastAPI project metadata + lockfile

uv-managed; pins fastapi/uvicorn/supabase/prometheus-client/httpx
to compatible ranges. Lockfile checked in so the Dockerfile's
'uv sync --frozen' is reproducible."
```

---

### Task 13: deploy/daily/api — trigger.py

**Files:**
- Create: `deploy/daily/api/trigger.py`

- [ ] **Step 1: Create the file**

```python
"""Subprocess wrapper for /app/run-daily.sh.

Used by both supercronic (via crontab) and FastAPI /api/run.
Provides a synchronous "is it running?" check via flock probe
and a fire-and-forget spawn for the API path.
"""
from __future__ import annotations

import asyncio
import fcntl
import os
import subprocess
from pathlib import Path

LOCK_PATH = Path("/var/lock/autocli-daily.lock")
RUN_DAILY = "/app/run-daily.sh"


def is_running() -> bool:
    """Non-destructive flock probe: returns True if another process holds the lock."""
    if not LOCK_PATH.exists():
        return False
    fd = os.open(LOCK_PATH, os.O_RDWR | os.O_CREAT, 0o644)
    try:
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.flock(fd, fcntl.LOCK_UN)
            return False
        except BlockingIOError:
            return True
    finally:
        os.close(fd)


async def spawn_run_daily() -> int:
    """Spawn run-daily.sh in the background. Returns PID. Does NOT wait."""
    proc = await asyncio.create_subprocess_exec(
        RUN_DAILY,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    return proc.pid
```

- [ ] **Step 2: Commit**

```bash
git add deploy/daily/api/trigger.py
git commit -m "feat(deploy): trigger.py — shared run-daily executor

Used by POST /api/run to spawn run-daily.sh non-blockingly.
is_running() is a non-destructive flock probe so /api/status can
report in_progress without affecting the actual run."
```

---

### Task 14: deploy/daily/api — main.py (FastAPI)

**Files:**
- Create: `deploy/daily/api/main.py`

- [ ] **Step 1: Create the file**

```python
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

from . import trigger

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
        r = httpx.get(f"http://{CHROME_HOST}:{CHROME_PORT}/json/version", timeout=2.0)
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
         since: str = Query(..., description="ISO date, e.g. 2026-05-15")):
    # Lazy import — supabase client takes ~100ms to construct
    from supabase import create_client
    client = create_client(SUPABASE_URL, SUPABASE_ANON_KEY)
    res = (
        client.schema("jobs")
              .table("jobs")
              .select("id, job_title, company_name, location, salary, post_time, apply_url, priority_score")
              .gte("post_time", since)
              .order("post_time", desc=True)
              .limit(500)
              .execute()
    )
    return {"count": len(res.data or []), "since": since, "rows": res.data or []}
```

- [ ] **Step 2: Verify Python imports**

```bash
cd deploy/daily/api && uv run python -c "import main"
```

Expected: no import errors. (Will fail without env vars set — that's fine; we just want syntax/import-time errors to surface.) Actually the module-level `os.environ["API_RUN_TOKEN"]` will KeyError if unset:

```bash
cd deploy/daily/api && API_RUN_TOKEN=t SUPABASE_URL=http://x SUPABASE_ANON_KEY=x \
  uv run python -c "import main; print('ok')"
```

Expected: `ok`.

- [ ] **Step 3: Commit**

```bash
git add deploy/daily/api/main.py
git commit -m "feat(deploy): FastAPI app — /api/* + /jobs

Routes per SPEC §5.1:
  GET  /api/health   [open]    chrome reachability + cdp file probe
  GET  /api/metrics  [open]    Prometheus exposition (delta-aware counters)
  GET  /api/status   [Bearer]  last_run.json + in_progress
  POST /api/run      [Bearer]  spawn run-daily.sh, 409 if already running
  GET  /api/logs     [Bearer]  tail of latest log (default 200 lines)
  GET  /jobs         [Bearer]  Supabase 'jobs.jobs' read proxy via
                               client.schema('jobs').table('jobs')."
```

---

### Task 15: deploy/daily/api — tests

**Files:**
- Create: `deploy/daily/api/tests/__init__.py` (empty)
- Create: `deploy/daily/api/tests/test_main.py`

- [ ] **Step 1: Empty __init__.py**

```bash
mkdir -p deploy/daily/api/tests
: > deploy/daily/api/tests/__init__.py
```

- [ ] **Step 2: Write tests**

```python
# deploy/daily/api/tests/test_main.py
"""Auth + route shape tests. Run via:
    cd deploy/daily/api && uv run pytest -v
"""
from __future__ import annotations

import json
import os
from pathlib import Path

import pytest
from fastapi.testclient import TestClient


@pytest.fixture
def client(tmp_path, monkeypatch):
    # Required env BEFORE main is imported
    monkeypatch.setenv("API_RUN_TOKEN", "test-token-abc")
    monkeypatch.setenv("SUPABASE_URL", "https://example.supabase.co")
    monkeypatch.setenv("SUPABASE_ANON_KEY", "test-anon")
    monkeypatch.setenv("CHROME_HOST", "example-chrome")
    # Redirect runtime paths
    data_dir = tmp_path / "data" / "output"
    data_dir.mkdir(parents=True)
    logs_dir = tmp_path / "data" / "logs"
    logs_dir.mkdir(parents=True)
    monkeypatch.setattr("pathlib.Path.exists", Path.exists)  # noop placeholder
    # Force module reload to pick up env
    import importlib
    import sys
    sys.modules.pop("main", None)
    import main as m
    importlib.reload(m)
    m.LAST_RUN_PATH = data_dir / "last_run.json"
    m.LOGS_DIR = logs_dir
    m.CDP_ENDPOINT_FILE = tmp_path / "run" / "cdp-endpoint.env"
    return TestClient(m.app)


def test_status_requires_bearer(client):
    r = client.get("/api/status")
    assert r.status_code == 401


def test_status_wrong_bearer(client):
    r = client.get("/api/status", headers={"Authorization": "Bearer wrong"})
    assert r.status_code == 401


def test_status_returns_default_when_no_last_run(client):
    r = client.get("/api/status", headers={"Authorization": "Bearer test-token-abc"})
    assert r.status_code == 200
    body = r.json()
    assert body["last_run_unixts"] == 0
    assert body["rows_scraped"] == 0
    assert body["run_in_progress"] is False


def test_status_reflects_last_run_file(client, tmp_path):
    import main as m
    m.LAST_RUN_PATH.write_text(json.dumps({
        "last_run_unixts": 1747958400,
        "last_duration_seconds": 142.3,
        "last_exit_code": 0,
        "rows_scraped": 100,
        "rows_upserted": 75,
        "rows_skipped": 25,
        "errors": [],
    }))
    r = client.get("/api/status", headers={"Authorization": "Bearer test-token-abc"})
    assert r.status_code == 200
    body = r.json()
    assert body["last_run_unixts"] == 1747958400
    assert body["rows_upserted"] == 75


def test_run_requires_bearer(client):
    r = client.post("/api/run")
    assert r.status_code == 401


def test_logs_requires_bearer(client):
    r = client.get("/api/logs")
    assert r.status_code == 401


def test_jobs_requires_bearer(client):
    r = client.get("/jobs?since=2026-05-15")
    assert r.status_code == 401


def test_metrics_is_open(client):
    r = client.get("/api/metrics")
    assert r.status_code == 200
    assert "autocli_daily" in r.text


def test_health_unreachable_chrome_returns_503(client, monkeypatch):
    import httpx
    def bad_get(*args, **kwargs):
        raise httpx.ConnectError("boom")
    monkeypatch.setattr("httpx.get", bad_get)
    r = client.get("/api/health")
    assert r.status_code == 503
    body = r.json()
    assert body["chrome"] is False
```

- [ ] **Step 3: Run tests**

```bash
cd deploy/daily/api && uv run --group dev pytest -v
```

Expected: 9 passed.

- [ ] **Step 4: Commit**

```bash
git add deploy/daily/api/tests/__init__.py deploy/daily/api/tests/test_main.py
git commit -m "test(deploy): FastAPI auth + route shape tests

9 tests covering:
- /api/status, /api/run, /api/logs, /jobs all return 401 without Bearer
  and 401 with wrong Bearer
- /api/status default-shape + reflects last_run.json
- /api/metrics is open and contains the autocli_daily_ family
- /api/health returns 503 when chrome:9222 unreachable."
```

---

### Task 16: deploy/prometheus — scrape config

**Files:**
- Create: `deploy/prometheus/prometheus.yml`

- [ ] **Step 1: Create the file**

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: autocli-daily
    metrics_path: /api/metrics
    static_configs:
      - targets:
          - autocli-daily:8080
```

- [ ] **Step 2: Commit**

```bash
git add deploy/prometheus/prometheus.yml
git commit -m "feat(deploy): prometheus scrape config

Single job scraping autocli-daily:8080/api/metrics every 15s.
metrics_path is required because FastAPI mounts under /api/*."
```

---

### Task 17: deploy/grafana — provisioning

**Files:**
- Create: `deploy/grafana/provisioning/datasources/prometheus.yml`
- Create: `deploy/grafana/provisioning/dashboards/dashboards.yml`
- Create: `deploy/grafana/provisioning/dashboards/autocli.json`

- [ ] **Step 1: Datasource provisioning**

`deploy/grafana/provisioning/datasources/prometheus.yml`:
```yaml
apiVersion: 1
datasources:
  - name: Prometheus
    type: prometheus
    uid: prom-autocli
    url: http://prometheus:9090
    access: proxy
    isDefault: true
    editable: false
```

- [ ] **Step 2: Dashboard provider config**

`deploy/grafana/provisioning/dashboards/dashboards.yml`:
```yaml
apiVersion: 1
providers:
  - name: autocli
    orgId: 1
    folder: AutoCLI
    type: file
    disableDeletion: true
    updateIntervalSeconds: 30
    allowUiUpdates: false
    options:
      path: /etc/grafana/provisioning/dashboards
```

- [ ] **Step 3: Dashboard JSON**

`deploy/grafana/provisioning/dashboards/autocli.json` — six panels per SPEC §5.5:

```json
{
  "schemaVersion": 39,
  "title": "AutoCLI Daily",
  "uid": "autocli-daily",
  "tags": ["autocli"],
  "timezone": "Europe/London",
  "refresh": "30s",
  "time": {"from": "now-30d", "to": "now"},
  "panels": [
    {
      "id": 1, "type": "stat",
      "title": "Time since last run",
      "gridPos": {"x": 0, "y": 0, "w": 6, "h": 4},
      "targets": [{"datasource": {"uid": "prom-autocli"}, "expr": "time() - autocli_daily_last_run_unixts"}],
      "fieldConfig": {"defaults": {"unit": "s", "thresholds": {"mode": "absolute", "steps": [{"color": "green"}, {"color": "red", "value": 90000}]}}}
    },
    {
      "id": 2, "type": "stat",
      "title": "Last exit code",
      "gridPos": {"x": 6, "y": 0, "w": 6, "h": 4},
      "targets": [{"datasource": {"uid": "prom-autocli"}, "expr": "autocli_daily_last_exit_code"}],
      "fieldConfig": {"defaults": {"thresholds": {"mode": "absolute", "steps": [{"color": "green"}, {"color": "red", "value": 1}]}}}
    },
    {
      "id": 3, "type": "stat",
      "title": "Rows upserted today",
      "gridPos": {"x": 12, "y": 0, "w": 6, "h": 4},
      "targets": [{"datasource": {"uid": "prom-autocli"}, "expr": "increase(autocli_daily_rows_upserted_total[24h])"}]
    },
    {
      "id": 4, "type": "stat",
      "title": "Chrome CDP up (24h avg)",
      "gridPos": {"x": 18, "y": 0, "w": 6, "h": 4},
      "targets": [{"datasource": {"uid": "prom-autocli"}, "expr": "avg_over_time(autocli_chrome_cdp_up[24h])"}],
      "fieldConfig": {"defaults": {"unit": "percentunit", "thresholds": {"mode": "absolute", "steps": [{"color": "red"}, {"color": "yellow", "value": 0.9}, {"color": "green", "value": 0.99}]}}}
    },
    {
      "id": 5, "type": "timeseries",
      "title": "Daily rows (scraped / upserted / skipped)",
      "gridPos": {"x": 0, "y": 4, "w": 24, "h": 8},
      "targets": [
        {"datasource": {"uid": "prom-autocli"}, "expr": "increase(autocli_daily_rows_scraped_total[1d])", "legendFormat": "scraped"},
        {"datasource": {"uid": "prom-autocli"}, "expr": "increase(autocli_daily_rows_upserted_total[1d])", "legendFormat": "upserted"},
        {"datasource": {"uid": "prom-autocli"}, "expr": "increase(autocli_daily_rows_skipped_total[1d])", "legendFormat": "skipped"}
      ]
    },
    {
      "id": 6, "type": "timeseries",
      "title": "Run duration",
      "gridPos": {"x": 0, "y": 12, "w": 24, "h": 8},
      "targets": [{"datasource": {"uid": "prom-autocli"}, "expr": "autocli_daily_last_duration_seconds", "legendFormat": "duration (s)"}],
      "fieldConfig": {"defaults": {"unit": "s"}}
    }
  ]
}
```

- [ ] **Step 4: Commit**

```bash
git add deploy/grafana/
git commit -m "feat(deploy): grafana provisioning + 6-panel dashboard

- Datasource: Prometheus at prometheus:9090 (uid prom-autocli)
- Dashboard provider points at /etc/grafana/provisioning/dashboards
- autocli.json: time-since-last-run, last exit code, rows-upserted-today,
  CDP-up %, daily scraped/upserted/skipped time series, duration
- No plugin dependencies (Infinity dropped per L313 review)."
```

---

### Task 18: deploy/docker-compose.yml

**Files:**
- Create: `deploy/docker-compose.yml`

- [ ] **Step 1: Create the file**

```yaml
name: autocli-stack

x-watchtower-label: &watchtower-enable
  com.centurylinklabs.watchtower.enable: "true"

services:
  autocli-chrome:
    image: ghcr.io/ricksanchez88e/autocli-chrome:main
    container_name: autocli-chrome
    restart: unless-stopped
    shm_size: "2gb"
    environment:
      VNC_PASSWORD: ${VNC_PASSWORD}
      TZ: ${TZ:-Europe/London}
    ports:
      - "6080:6080"   # noVNC web (also proxied via Cloudflare vnc subdomain)
      - "5900:5900"   # native VNC (local-only convenience; not in Cloudflare ingress)
      - "9222:9222"   # CDP (also proxied via Cloudflare cdp subdomain — strict Access)
    volumes:
      - chrome-profile:/root/.config/chromium
      - chrome-tmp:/tmp
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:9222/json/version"]
      interval: 10s
      timeout: 3s
      retries: 10
      start_period: 20s
    networks: [autocli-net]
    labels: *watchtower-enable

  autocli-daily:
    image: ghcr.io/ricksanchez88e/autocli-daily:main
    container_name: autocli-daily
    restart: unless-stopped
    depends_on:
      autocli-chrome:
        condition: service_healthy
    environment:
      TZ: ${TZ:-Europe/London}
      CRON_SCHEDULE: ${CRON_SCHEDULE:-0 3 * * *}
      CHROME_HOST: autocli-chrome
      CHROME_PORT: "9222"
      API_RUN_TOKEN: ${API_RUN_TOKEN}
      SUPABASE_URL: ${SUPABASE_URL}
      SUPABASE_SERVICE_ROLE_KEY: ${SUPABASE_SERVICE_ROLE_KEY}
      SUPABASE_ANON_KEY: ${SUPABASE_ANON_KEY}
    volumes:
      - daily-data:/data
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8080/api/health"]
      interval: 15s
      timeout: 5s
      retries: 6
      start_period: 60s
    networks: [autocli-net]
    labels: *watchtower-enable

  cloudflared:
    image: cloudflare/cloudflared:2025.4.0
    container_name: autocli-cloudflared
    restart: unless-stopped
    command: tunnel --no-autoupdate run --token ${CLOUDFLARE_TUNNEL_TOKEN}
    environment:
      TUNNEL_TOKEN: ${CLOUDFLARE_TUNNEL_TOKEN}
    depends_on:
      autocli-daily:
        condition: service_healthy
    networks: [autocli-net]

  prometheus:
    image: prom/prometheus:v3.5.0
    container_name: autocli-prometheus
    restart: unless-stopped
    command:
      - --config.file=/etc/prometheus/prometheus.yml
      - --storage.tsdb.path=/prometheus
      - --storage.tsdb.retention.time=90d
    volumes:
      - ./prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prom-data:/prometheus
    networks: [autocli-net]

  grafana:
    image: grafana/grafana:11.6.0
    container_name: autocli-grafana
    restart: unless-stopped
    environment:
      GF_SECURITY_ADMIN_PASSWORD: ${GF_SECURITY_ADMIN_PASSWORD}
      GF_USERS_ALLOW_SIGN_UP: "false"
      GF_AUTH_ANONYMOUS_ENABLED: "false"
    volumes:
      - ./grafana/provisioning:/etc/grafana/provisioning:ro
      - grafana-data:/var/lib/grafana
    depends_on:
      - prometheus
    networks: [autocli-net]

networks:
  autocli-net:
    driver: bridge

volumes:
  chrome-profile:
  chrome-tmp:
  daily-data:
  prom-data:
  grafana-data:
```

- [ ] **Step 2: Validate compose syntax**

```bash
docker compose -f deploy/docker-compose.yml config > /dev/null
```

Expected: command exits 0; no warnings about missing env vars (those resolve via .env at runtime).

- [ ] **Step 3: Commit**

```bash
git add deploy/docker-compose.yml
git commit -m "feat(deploy): production docker-compose.yml

5 services on shared autocli-net bridge:
- autocli-chrome (Stagehand, watchtower-tracked, healthcheck on 9222)
- autocli-daily (cron+FastAPI, watchtower-tracked, depends_on chrome
  healthy, env scoped to Supabase creds only)
- cloudflared (Tunnel token mode, depends_on daily healthy)
- prometheus (pinned, 90-day retention)
- grafana (pinned, anon disabled, signup disabled, admin from env)
Named volumes for profile / output / tsdb / grafana state."
```

---

### Task 19: deploy/docker-compose.local.yml

**Files:**
- Create: `deploy/docker-compose.local.yml`

- [ ] **Step 1: Create the file**

```yaml
# Local override for Phase 1 testing.
# Run:
#   docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml --env-file deploy/.env.local up -d

name: autocli-stack-local

services:
  autocli-chrome:
    container_name: autocli-chrome-local
    image: test-chrome:latest        # built locally in Phase 0
    ports:
      - "6081:6080"
      - "5902:5900"
      - "9223:9222"

  autocli-daily:
    container_name: autocli-daily-local
    image: test-daily:latest         # built locally in Phase 0
    ports:
      - "8081:8080"

  # No Cloudflare in local mode
  cloudflared:
    profiles: ["disabled"]

  prometheus:
    ports:
      - "9091:9090"

  grafana:
    ports:
      - "3001:3000"
```

- [ ] **Step 2: Commit**

```bash
git add deploy/docker-compose.local.yml
git commit -m "feat(deploy): local-only override

Binds host ports under non-conflicting numbers (6081/5902/9223/8081/
9091/3001) so the operator can keep their existing local Chrome and
Grafana running alongside. cloudflared moved to a 'disabled' profile."
```

---

### Task 20: deploy/.env.example

**Files:**
- Create: `deploy/.env.example`

- [ ] **Step 1: Create the file**

```
# Cloudflare Tunnel (token mode — credentials NOT used)
CLOUDFLARE_TUNNEL_TOKEN=

# Supabase
SUPABASE_URL=
SUPABASE_SERVICE_ROLE_KEY=
SUPABASE_ANON_KEY=

# API auth (defense-in-depth on top of Cloudflare Access).
# Generate: openssl rand -hex 32
API_RUN_TOKEN=

# VNC password (generate: openssl rand -base64 18). NEVER use the dev value 'stagehand' in prod.
VNC_PASSWORD=

# Grafana admin (generate: openssl rand -hex 16)
GF_SECURITY_ADMIN_PASSWORD=

# Scheduling
TZ=Europe/London
CRON_SCHEDULE=0 3 * * *
```

- [ ] **Step 2: Commit**

```bash
git add deploy/.env.example
git commit -m "feat(deploy): .env.example template

All required environment variables with empty values + inline
generator hints. Real .env never committed (.gitignore already
covers it under '.env')."
```

---

### Task 21: deploy/README.md

**Files:**
- Create: `deploy/README.md`

- [ ] **Step 1: Create the file**

```markdown
# AutoCLI Daily Microservice — Deploy

See [`SPEC.md`](./SPEC.md) for design, [`PLAN.md`](./PLAN.md) for the implementation walkthrough.
This file is the operator-facing runbook.

## Quickstart on a fresh host

```bash
ssh rick@100.108.80.9
mkdir -p ~/autocli-stack && cd ~/autocli-stack

# 1. Copy compose files + .env (scp from your laptop)
#    (See SPEC §6.3 for the secret-transfer mechanism.)
cp deploy/docker-compose.yml .
cp deploy/.env.example .env
$EDITOR .env   # fill every blank

# 2. Bring up the stack
docker compose pull
docker compose up -d
docker compose ps     # all 5 should be healthy

# 3. One-time LinkedIn login via VNC
#    Browse to https://autocli-vnc.<your-zone>/vnc.html, password from .env
#    Log into linkedin.com once, profile cookies persist in the
#    `chrome-profile` named volume.

# 4. Probe the surface (see SPEC §7 Phase 4a)
```

## Cloudflare dashboard checklist

For each subdomain (`vnc`, `cdp`, `api`, `grafana`):
1. Tunnel → Public Hostnames → Add → set service URL to
   `http://autocli-chrome:6080` / `http://autocli-chrome:9222` /
   `http://autocli-daily:8080` / `http://grafana:3000`.
2. Access → Applications → Add Application → Self-Hosted →
   `<sub>.autocli.<your-zone>` → policies per SPEC §5.3 table.
3. **Defer adding `autocli-cdp` until Phase 4a is green for the
   other three subdomains** (SPEC §9 risk 1).

## Forced run

```bash
curl -X POST \
  -H "CF-Access-Client-Id: $CF_ID" \
  -H "CF-Access-Client-Secret: $CF_SECRET" \
  -H "Authorization: Bearer $API_RUN_TOKEN" \
  https://autocli-api.<your-zone>/api/run
```

## Troubleshooting

| Symptom | Where to look |
|---|---|
| `/api/health` 503 | `docker logs autocli-chrome` — usually profile lock or socat |
| LinkedIn login expired | VNC in, re-login. Cookies persist in `chrome-profile` volume |
| Tunnel 502 | `docker logs autocli-cloudflared`; check token |
| Watchtower didn't pull new image | Check it's running (`docker ps \| grep watchtower`); 5-min poll |
```

- [ ] **Step 2: Commit**

```bash
git add deploy/README.md
git commit -m "docs(deploy): operator-facing README + runbook

Quickstart, Cloudflare dashboard checklist, forced-run snippet,
common-failure table. Points back at SPEC + PLAN for the why."
```

---

## Phase C — CI workflow

### Task 22: GitHub Actions workflow

**Files:**
- Create: `.github/workflows/deploy-microservice.yml`

- [ ] **Step 1: Create the file**

```yaml
name: deploy-microservice

on:
  push:
    branches: [feat/daily-microservice, main]
    paths:
      - deploy/**
      - crates/**
      - scripts/sync_autocli_jobs.py
      - scripts/job_priority_scorer.py
      - scripts/job_priority_config.py
      - rust-toolchain.toml
      - .github/workflows/deploy-microservice.yml
  workflow_dispatch:

env:
  IS_MAIN: ${{ github.ref == 'refs/heads/main' }}

jobs:
  build-autocli-binary:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release -p autocli
      - uses: actions/upload-artifact@v4
        with:
          name: autocli-bin
          path: target/release/autocli
          retention-days: 7

  build-chrome-image:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      # NOTE: slugifier — `type=ref,event=branch` runs metadata-action's
      # slugifier, so `feat/daily-microservice` becomes
      # `branch-feat-daily-microservice` (Docker-tag-safe).
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/ricksanchez88e/autocli-chrome
          flavor: latest=false
          tags: |
            type=raw,value=main,enable=${{ env.IS_MAIN }}
            type=ref,event=branch,prefix=branch-,enable=${{ env.IS_MAIN == 'false' }}
            type=sha,prefix=sha-,format=short
      - uses: docker/build-push-action@v6
        with:
          context: .
          file: deploy/chrome/Dockerfile
          platforms: linux/amd64
          tags: ${{ steps.meta.outputs.tags }}
          push: true

  build-daily-image:
    runs-on: ubuntu-latest
    needs: [build-autocli-binary]
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with:
          name: autocli-bin
          path: deploy/daily/bin
      - run: chmod +x deploy/daily/bin/autocli
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/ricksanchez88e/autocli-daily
          flavor: latest=false
          tags: |
            type=raw,value=main,enable=${{ env.IS_MAIN }}
            type=ref,event=branch,prefix=branch-,enable=${{ env.IS_MAIN == 'false' }}
            type=sha,prefix=sha-,format=short
      - uses: docker/build-push-action@v6
        with:
          context: .
          file: deploy/daily/Dockerfile
          platforms: linux/amd64
          tags: ${{ steps.meta.outputs.tags }}
          push: true
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/deploy-microservice.yml
git commit -m "feat(ci): GitHub Actions workflow for the daily microservice

3 jobs:
1. build-autocli-binary: cargo build --release -p autocli on
   ubuntu-latest (linux/amd64) with Swatinem cache; uploads artifact
2. build-chrome-image: builds deploy/chrome from repo-root context;
   docker/metadata-action generates :main on main, :branch-<slug> on
   feature branches, :sha-<short> always
3. build-daily-image: downloads the autocli artifact, builds
   deploy/daily from repo-root context, same tag policy

Path filters include rust-toolchain.toml so a toolchain bump triggers
a rebuild."
```

---

## Phase D — Local Phase 0 + Phase 1 verification

### Task 23: Phase 0 — build images locally

**Files:** none modified

- [ ] **Step 1: Build the autocli binary inside Docker rust 1.94**

```bash
cd /Users/sanchezrick/Documents/Github/AutoCLI-daily
mkdir -p deploy/daily/bin
docker run --rm --platform linux/amd64 \
  -v "$PWD":/work -w /work \
  -v autocli-daily-cargo-cache:/usr/local/cargo/registry \
  -v autocli-daily-cargo-target:/work/target \
  rust:1.94-slim-bookworm \
  bash -c "apt-get update -qq && apt-get install -y -qq pkg-config libssl-dev && cargo build --release -p autocli && cp target/release/autocli deploy/daily/bin/autocli"
chmod +x deploy/daily/bin/autocli
```

- [ ] **Step 2: Verify binary architecture**

```bash
file deploy/daily/bin/autocli
# Expected: ELF 64-bit LSB executable, x86-64
```

If output mentions Mach-O, halt — the build ran on the host, not in the linux/amd64 container.

- [ ] **Step 3: Build both Docker images**

```bash
docker buildx build --platform linux/amd64 -f deploy/chrome/Dockerfile -t test-chrome .
docker buildx build --platform linux/amd64 -f deploy/daily/Dockerfile  -t test-daily  .
```

Both should succeed.

- [ ] **Step 4: Smoke-test the binary inside the daily image**

```bash
docker run --rm --platform linux/amd64 test-daily /app/bin/autocli --version
# Expected: a non-empty version string
```

- [ ] **Step 5: Commit (optional — keeps deploy/daily/bin/.gitkeep)**

```bash
# deploy/daily/bin/autocli is large (~50MB), don't commit it. Add ignore:
echo "deploy/daily/bin/autocli" >> .gitignore
git add .gitignore
git commit -m "chore: gitignore the local Phase 0 autocli binary

CI downloads the artifact at build time; locally Phase 0 produces
this via the docker-rust step."
```

---

### Task 24: Phase 1 — local e2e + LinkedIn login

**Files:**
- Create: `deploy/.env.local`

- [ ] **Step 1: Generate local secrets**

```bash
cat > deploy/.env.local <<EOF
CLOUDFLARE_TUNNEL_TOKEN=
VNC_PASSWORD=stagehand
SUPABASE_URL=$(grep ^SUPABASE_URL= ~/.autocli-secrets.env | cut -d= -f2-)
SUPABASE_SERVICE_ROLE_KEY=$(grep ^SUPABASE_SERVICE_ROLE_KEY= ~/.autocli-secrets.env | cut -d= -f2-)
SUPABASE_ANON_KEY=$(grep ^SUPABASE_ANON_KEY= ~/.autocli-secrets.env | cut -d= -f2-)
API_RUN_TOKEN=$(openssl rand -hex 32)
GF_SECURITY_ADMIN_PASSWORD=$(openssl rand -hex 16)
TZ=Europe/London
CRON_SCHEDULE=0 3 * * *
EOF
chmod 600 deploy/.env.local
```

(`~/.autocli-secrets.env` is the operator's source-of-truth file from SPEC §6.3.)

- [ ] **Step 2: Stop your existing local stagehand-chrome (port conflict)**

```bash
docker stop stagehand-chrome 2>/dev/null || true
```

(You'll restart it later if you still want the original.)

- [ ] **Step 3: Bring up the local stack**

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml --env-file deploy/.env.local up -d
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml ps
# Expected: 4 services Up (cloudflared is disabled in local override)
```

- [ ] **Step 4: One-time LinkedIn login via VNC**

Open `http://localhost:6081/vnc.html?password=stagehand` in a browser. In the VNC viewer:
1. Open `linkedin.com`
2. Sign in with your account
3. Close the tab when done

Cookies persist in the `chrome-profile` named volume.

- [ ] **Step 5: Force a daily run**

```bash
LOCAL_TOKEN=$(grep ^API_RUN_TOKEN= deploy/.env.local | cut -d= -f2-)

# health first
curl -s http://localhost:8081/api/health | jq

# trigger
curl -X POST -H "Authorization: Bearer $LOCAL_TOKEN" http://localhost:8081/api/run

# wait + poll
sleep 240
curl -s -H "Authorization: Bearer $LOCAL_TOKEN" http://localhost:8081/api/status | jq
```

Expected: `last_exit_code: 0`, `rows_upserted > 0`. Supabase `jobs.jobs` should also show today's rows.

- [ ] **Step 6: Inspect Grafana**

Open `http://localhost:3001` (admin / value of `GF_SECURITY_ADMIN_PASSWORD` in `.env.local`). Dashboard "AutoCLI Daily" should already be provisioned and show today's run.

- [ ] **Step 7: Tear down**

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.local.yml down
```

(Volumes are retained; subsequent runs resume from logged-in profile.)

No commit (config only).

---

## Phase E — Push branch + CI green (Phase 2)

### Task 25: Push and watch CI

**Files:** none modified

- [ ] **Step 1: Push branch**

```bash
cd /Users/sanchezrick/Documents/Github/AutoCLI-daily
git push -u origin feat/daily-microservice
```

- [ ] **Step 2: Watch the workflow**

```bash
gh run watch --repo RickSanchez88E/AutoCLI
# Or: gh run list --branch feat/daily-microservice --limit 1
```

Expected: 3 jobs (`build-autocli-binary`, `build-chrome-image`, `build-daily-image`) all green.

- [ ] **Step 3: Verify GHCR tags**

```bash
gh api /users/RickSanchez88E/packages/container/autocli-chrome/versions --jq '.[].metadata.container.tags' | head
gh api /users/RickSanchez88E/packages/container/autocli-daily/versions --jq '.[].metadata.container.tags' | head
```

Expected to see:
- `branch-feat-daily-microservice`
- `sha-<short>`
- **NO `main` tag** (will appear only after merge to main)

If `main` accidentally appears on a feature branch push, halt — the workflow's `enable=${{ env.IS_MAIN }}` is misconfigured.

No commit.

---

## Phase F — Server bring-up (Phase 3)

### Task 26: Pre-flight on 100.108.80.9

**Files:** none modified

- [ ] **Step 1: SSH in**

```bash
sshpass -p '1234' ssh -o StrictHostKeyChecking=no rick@100.108.80.9
```

- [ ] **Step 2: Stop and remove Skyvern (SPEC §1 goal 5)**

```bash
docker stop skyvern-skyvern-1 skyvern-skyvern-ui-1 skyvern-postgres-1
docker rm   skyvern-skyvern-1 skyvern-skyvern-ui-1
# Keep the postgres volume in case the operator wants to bring Skyvern back;
# only kill its container so the postgres port doesn't conflict.
docker rm skyvern-postgres-1
docker volume ls | grep skyvern   # leave volumes; just removed containers
```

- [ ] **Step 3: Verify 6080/9222 are free**

```bash
ss -tlnp | grep -E ':(6080|9222) ' || echo "ports free"
# Expected: "ports free"
```

- [ ] **Step 4: Create stack dir**

```bash
mkdir -p ~/autocli-stack/{prometheus,grafana/provisioning/datasources,grafana/provisioning/dashboards}
```

- [ ] **Step 5: Verify GHCR pull works on this host**

```bash
docker pull ghcr.io/ricksanchez88e/autocli-chrome:branch-feat-daily-microservice
docker pull ghcr.io/ricksanchez88e/autocli-daily:branch-feat-daily-microservice
```

Both should succeed. If 401/403 → run `echo $GHCR_PAT | docker login ghcr.io -u ricksanchez88e --password-stdin` first.

No commit.

---

### Task 27: scp compose + provisioning + .env to server

**Files:** none modified (on the server side, files arrive via scp)

- [ ] **Step 1: From the worktree, scp config**

```bash
cd /Users/sanchezrick/Documents/Github/AutoCLI-daily
sshpass -p '1234' scp deploy/docker-compose.yml rick@100.108.80.9:~/autocli-stack/
sshpass -p '1234' scp deploy/prometheus/prometheus.yml rick@100.108.80.9:~/autocli-stack/prometheus/
sshpass -p '1234' scp deploy/grafana/provisioning/datasources/prometheus.yml \
  rick@100.108.80.9:~/autocli-stack/grafana/provisioning/datasources/
sshpass -p '1234' scp deploy/grafana/provisioning/dashboards/dashboards.yml \
  deploy/grafana/provisioning/dashboards/autocli.json \
  rick@100.108.80.9:~/autocli-stack/grafana/provisioning/dashboards/
```

- [ ] **Step 2: Prepare .env on server (token + secrets)**

```bash
# Operator: ensure ~/.autocli-secrets.env on your laptop has:
#   CLOUDFLARE_TUNNEL_TOKEN=eyJh...
#   SUPABASE_URL=https://...
#   SUPABASE_SERVICE_ROLE_KEY=...
#   SUPABASE_ANON_KEY=...
cp ~/.autocli-secrets.env /tmp/autocli-secrets.$$.env

# Append generated values
cat >> /tmp/autocli-secrets.$$.env <<EOF
API_RUN_TOKEN=$(openssl rand -hex 32)
VNC_PASSWORD=$(openssl rand -base64 18)
GF_SECURITY_ADMIN_PASSWORD=$(openssl rand -hex 16)
TZ=Europe/London
CRON_SCHEDULE=0 3 * * *
EOF
chmod 600 /tmp/autocli-secrets.$$.env

# Push to server
sshpass -p '1234' scp /tmp/autocli-secrets.$$.env rick@100.108.80.9:~/autocli-stack/.env
sshpass -p '1234' ssh rick@100.108.80.9 'chmod 600 ~/autocli-stack/.env'

# Print the generated secrets ONCE for operator to save in 1Password/etc
grep -E '^(API_RUN_TOKEN|VNC_PASSWORD|GF_SECURITY_ADMIN_PASSWORD)=' /tmp/autocli-secrets.$$.env

# Wipe local temp copy ONLY
shred -u /tmp/autocli-secrets.$$.env
```

- [ ] **Step 3: For first deploy, pin to branch tag (not :main)**

On the server:
```bash
sshpass -p '1234' ssh rick@100.108.80.9 'sed -i "s|:main$|:branch-feat-daily-microservice|" ~/autocli-stack/docker-compose.yml'
```

(After PR merges and `:main` tag appears, revert via `sed -i "s|:branch-feat-daily-microservice$|:main|"`.)

No commit on the spec side.

---

### Task 28: docker compose up on server

**Files:** none modified

- [ ] **Step 1: Pull + up**

```bash
sshpass -p '1234' ssh rick@100.108.80.9 'cd ~/autocli-stack && docker compose pull && docker compose up -d'
```

- [ ] **Step 2: Wait + verify**

```bash
sleep 60
sshpass -p '1234' ssh rick@100.108.80.9 'cd ~/autocli-stack && docker compose ps'
```

Expected: 5 services `Up` and `healthy` (autocli-chrome, autocli-daily, autocli-cloudflared, autocli-prometheus, autocli-grafana).

- [ ] **Step 3: One-time LinkedIn login via VNC (over Tailscale)**

From the operator's laptop on Tailscale: open `http://100.108.80.9:6080/vnc.html?password=<VNC_PASSWORD-from-step-2>`. Log into linkedin.com once. Close the tab.

After Cloudflare ingress (Phase 4) is up the operator will use `autocli-vnc.<your-zone>/vnc.html` instead; Tailscale path is the bootstrap.

No commit.

---

## Phase G — Cloudflare Tunnel + Access (Phase 4)

### Task 29: Phase 4a — 3 subdomains in Cloudflare dashboard

**Files:** none modified

This is **operator UI work** (Cloudflare Zero Trust dashboard).

- [ ] **Step 1: Tunnel → Public Hostnames**

In the existing Tunnel (the one whose token is in `.env`), add:
1. `autocli-vnc.<your-zone>` → `http://autocli-chrome:6080`
2. `autocli-api.<your-zone>` → `http://autocli-daily:8080`
3. `autocli-grafana.<your-zone>` → `http://grafana:3000`

**Do NOT add `autocli-cdp` yet.**

- [ ] **Step 2: Access → Applications**

Create one Self-Hosted Application per subdomain. Policies per SPEC §5.3:
- `autocli-vnc`: Policy "operator email + WARP device posture" only.
- `autocli-api`: Policy "Service Token" AND Policy "operator email" (OR semantics within Application).
- `autocli-grafana`: Policy "operator email OTP" only.

Create a **Service Token** under Access → Service Auth. Save the `Client ID` and `Client Secret` — these are the `CF_ID` / `CF_SECRET` used by Phase 4 probes.

- [ ] **Step 3: Phase 4a probes (run from operator's laptop)**

```bash
DOMAIN="<your-zone>"
CF_ID="<service-token-client-id>"
CF_SECRET="<service-token-client-secret>"
TOKEN="$(sshpass -p '1234' ssh rick@100.108.80.9 'grep ^API_RUN_TOKEN= ~/autocli-stack/.env | cut -d= -f2-')"

# 1. Unauthenticated → all three should 302
for sub in vnc api grafana; do
  code=$(curl -s -o /dev/null -w "%{http_code}" "https://autocli-${sub}.${DOMAIN}/")
  echo "${sub} unauth: ${code}"
done
# Expected: each "302"

# 2. Service Token on humans-only subdomains → still 302
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://autocli-vnc.${DOMAIN}/"     | head -1
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://autocli-grafana.${DOMAIN}/" | head -1
# Expected: both HTTP/2 302

# 3. api.autocli — Service Token grants access
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://autocli-api.${DOMAIN}/api/health" | head -1
# Expected: HTTP/2 200

# 4. Bearer enforcement
curl -sI -X POST -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://autocli-api.${DOMAIN}/api/run" | head -1
# Expected: HTTP/2 401

curl -sI -X POST -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     -H "Authorization: Bearer ${TOKEN}" \
     "https://autocli-api.${DOMAIN}/api/run" | head -1
# Expected: HTTP/2 202 (or 409 if a run is already in flight — re-run after a couple of min)

# 5. /jobs (Bearer required)
curl -s -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     -H "Authorization: Bearer ${TOKEN}" \
     "https://autocli-api.${DOMAIN}/jobs?since=2026-05-15" | jq '.count'
# Expected: ≥ 0
```

All 6 probes must match. Halt the plan if any fails — Phase 4b is gated on this.

No commit.

---

### Task 30: Phase 4b — Add cdp.autocli ingress + Access Application

**Files:** none modified

- [ ] **Step 1: Verify Phase 4a was clean**

If any probe in Task 29 failed, **stop** and fix before adding cdp ingress.

- [ ] **Step 2: Generate a dedicated CDP Service Token**

Cloudflare dashboard → Access → Service Auth → Create Service Token "autocli-cdp" (separate from the api.autocli one). Save `CF_ID_CDP` / `CF_SECRET_CDP`.

- [ ] **Step 3: Generate operator mTLS client cert**

Cloudflare dashboard → Access → Service Auth → mTLS → Create CA, then issue a client cert. Download as PEM:
- `~/.cf-access/cdp-client.crt`
- `~/.cf-access/cdp-client.key`
Set permissions `chmod 600 ~/.cf-access/cdp-client.*`.

- [ ] **Step 4: Create cdp.autocli Access Application**

Application → Self-Hosted → hostname `autocli-cdp.<your-zone>`. Add:
- Policy A (machines): require Service Token = `autocli-cdp` **AND** mTLS client cert valid for the CA above.
- Policy B (humans): require operator email + **required** WARP device posture.

- [ ] **Step 5: Add Tunnel ingress for cdp.autocli**

Tunnel → Public Hostnames → Add `autocli-cdp.<your-zone>` → `http://autocli-chrome:9222`.

No commit.

---

### Task 31: Phase 4c — cdp.autocli probes

**Files:** none modified

Run from operator's laptop:

- [ ] **Step 1: HTTP probes 4c-1 through 4c-3**

```bash
DOMAIN="<your-zone>"
CF_ID="<api-service-token-client-id>"
CF_SECRET="<api-service-token-client-secret>"
CF_ID_CDP="<cdp-service-token-client-id>"
CF_SECRET_CDP="<cdp-service-token-client-secret>"

# 4c-1. Unauthenticated → 302
curl -s -o /dev/null -w "%{http_code}\n" "https://autocli-cdp.${DOMAIN}/json/list"
# Expected: 302

# 4c-2. api-scoped token (wrong scope, no mTLS) → 302 or 403
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://autocli-cdp.${DOMAIN}/json/list" | head -1
# Expected: HTTP/2 302 or HTTP/2 403

# 4c-3. Correct cdp token + mTLS → 200
curl -sI \
     -H "CF-Access-Client-Id: ${CF_ID_CDP}" -H "CF-Access-Client-Secret: ${CF_SECRET_CDP}" \
     --cert "$HOME/.cf-access/cdp-client.crt" --key "$HOME/.cf-access/cdp-client.key" \
     "https://autocli-cdp.${DOMAIN}/json/list" | head -1
# Expected: HTTP/2 200
```

- [ ] **Step 2: WebSocket probe 4c-4**

Install websocat first (`brew install websocat` or `apt install websocat`).

```bash
WS_URL=$(curl -s \
  -H "CF-Access-Client-Id: ${CF_ID_CDP}" -H "CF-Access-Client-Secret: ${CF_SECRET_CDP}" \
  --cert "$HOME/.cf-access/cdp-client.crt" --key "$HOME/.cf-access/cdp-client.key" \
  "https://autocli-cdp.${DOMAIN}/json/list" \
  | jq -r '[.[] | select(.type == "page")][0].webSocketDebuggerUrl' \
  | sed -E "s|ws://[^/]+|wss://autocli-cdp.${DOMAIN}|")
echo "WS_URL=${WS_URL}"

echo '{"id":1,"method":"Target.getTargets"}' \
  | websocat -1 -t \
      --header="CF-Access-Client-Id: ${CF_ID_CDP}" \
      --header="CF-Access-Client-Secret: ${CF_SECRET_CDP}" \
      --client-pkcs12-der "$HOME/.cf-access/cdp-client.p12" \
      "${WS_URL}" \
  | jq '.result.targetInfos | length'
# Expected: ≥ 1
```

(If websocat isn't installable, use the curl --http1.1 fallback in SPEC §7 Phase 4c-4 step 2 fallback block.)

All probes match → the CDP surface is live. No commit.

---

## Phase H — Production forced run + monitoring (Phase 5)

### Task 32: Forced run via API + verification

**Files:** none modified

- [ ] **Step 1: Trigger**

```bash
curl -X POST \
  -H "CF-Access-Client-Id: $CF_ID" \
  -H "CF-Access-Client-Secret: $CF_SECRET" \
  -H "Authorization: Bearer $API_RUN_TOKEN" \
  https://autocli-api.<your-zone>/api/run
# Expected: {"started_at": ..., "pid": ...} with HTTP 202
```

- [ ] **Step 2: Poll status**

```bash
sleep 300   # generous: gives all 3 retry attempts time to complete
curl -s \
  -H "CF-Access-Client-Id: $CF_ID" -H "CF-Access-Client-Secret: $CF_SECRET" \
  -H "Authorization: Bearer $API_RUN_TOKEN" \
  https://autocli-api.<your-zone>/api/status | jq
```

Expected: `last_exit_code: 0`, `rows_upserted > 0`, `run_in_progress: false`.

- [ ] **Step 3: Verify Supabase rows**

In Supabase SQL editor:
```sql
SELECT count(*) FROM jobs.jobs WHERE created_at::date = current_date;
```

Expected: matches the `rows_upserted` from `/api/status`.

- [ ] **Step 4: Verify Grafana dashboard**

Browser to `https://autocli-grafana.<your-zone>` → login → Dashboards → "AutoCLI Daily":
- "Time since last run" should be small (single-digit minutes)
- "Last exit code" = 0 (green)
- "Rows upserted today" = same number as Step 3
- CDP-up gauge near 100%

No commit.

---

### Task 33: Phase 6 — schedule rollover observation

**Files:** none modified

- [ ] **Step 1: Wait until tomorrow 03:00 BST + 30 min**

(Calendar event, not a step you can execute right now.)

- [ ] **Step 2: Verify the scheduled run happened**

```bash
curl -s \
  -H "CF-Access-Client-Id: $CF_ID" -H "CF-Access-Client-Secret: $CF_SECRET" \
  -H "Authorization: Bearer $API_RUN_TOKEN" \
  https://autocli-api.<your-zone>/api/status | jq '.last_run_unixts | strftime("%Y-%m-%d %H:%M:%S")'
```

Expected: timestamp within 5 minutes of 03:00 (today's date).

- [ ] **Step 3: Repeat on day 3**

If two consecutive scheduled runs pass without intervention, declare Phase 6 done.

No commit.

---

### Task 34: Open PR

**Files:** none modified

- [ ] **Step 1: Push final state**

```bash
cd /Users/sanchezrick/Documents/Github/AutoCLI-daily
git push
```

- [ ] **Step 2: Open PR**

```bash
gh pr create \
  --base main \
  --head feat/daily-microservice \
  --title "feat: daily LinkedIn microservice + autocli CDP wiring" \
  --body "$(cat <<'EOF'
## Summary

Implements the auto-scheduled daily LinkedIn-recommended pipeline as a
microservice on `100.108.80.9` (Tailscale). See `deploy/SPEC.md` for design
and `deploy/PLAN.md` for the build walkthrough.

- **Prereq Rust patch** (crates/autocli-browser): wires `CdpPage` into
  `BrowserBridge::connect` behind `AUTOCLI_CDP_ENDPOINT`. Required so the
  daily container can drive a sibling Chrome container without the
  extension+daemon path.
- **`rust-toolchain.toml`**: pins workspace to 1.94 so local / CI / Phase 0
  builder all agree.
- **`deploy/`**: chrome image (Stagehand-style VNC Chromium), daily image
  (Python+supercronic+FastAPI+pre-built autocli), docker-compose for prod
  and local, prometheus + grafana provisioning, README runbook.
- **`.github/workflows/deploy-microservice.yml`**: GHCR builds with
  branch-safe slugified tags; only `:main` reaches Watchtower in prod.
- Verified live on `100.108.80.9` via Cloudflare Tunnel (`vnc/cdp/api/grafana`.autocli.<zone>), Phase 4a-4c probes all green, one forced run wrote
  N rows to Supabase, Grafana dashboard provisions automatically.

## Test plan

- [x] Rust unit test: `bridge::tests::test_connect_uses_cdp_endpoint_when_env_var_set`
- [x] FastAPI tests: 9 cases covering auth + route shape
- [x] Phase 0: ELF/x86-64 verified; both images build
- [x] Phase 1 local e2e: forced run, Supabase rows landed
- [x] Phase 2 CI: green on feature branch, correct tag set
- [x] Phase 3: 5 containers healthy on `100.108.80.9`
- [x] Phase 4a/4b/4c: all probes match expected codes
- [x] Phase 5: production forced run, Grafana populated
- [ ] Phase 6: two consecutive scheduled runs (calendar-dependent; will
      tick off after the second 03:00 BST tick post-merge)
EOF
)"
```

- [ ] **Step 3: Flip server to `:main` tag after merge**

(After PR is merged.)
```bash
sshpass -p '1234' ssh rick@100.108.80.9 'cd ~/autocli-stack && \
  sed -i "s|:branch-feat-daily-microservice|:main|" docker-compose.yml && \
  docker compose pull && docker compose up -d'
```

Watchtower will keep `:main` fresh thereafter.

No commit.

---

## Self-Review

**Spec coverage check** (against `deploy/SPEC.md` sections):

- Prerequisite Patch: ✅ Tasks 1–4
- §2 Architecture (5 services + topology): ✅ Tasks 18 (compose) + 16 (prometheus) + 17 (grafana)
- §3 Repo Layout + Worktree: ✅ matches PLAN file-map; worktree already created
- §4 Image Build Pipeline: ✅ Tasks 22 (workflow) + 23 (local mirror)
- §5.1 Process tree (cdp-discover + supercronic + uvicorn): ✅ Tasks 8/9/10/11
- §5.2 Invariants (discovery cadence, retry policy): ✅ Tasks 8/9
- §5.3 Cloudflare Tunnel + Access: ✅ Tasks 29/30/31
- §5.4 Prometheus metrics: ✅ Task 14 (FastAPI exposes them) + Task 16 (scrape config)
- §5.5 Grafana dashboard: ✅ Task 17
- §6 Secrets: ✅ Task 27 (server-side transfer)
- §7 Phase 0–6 acceptance: ✅ Tasks 23/24/25/28/29/30/31/32/33
- §9 Risks: ✅ Task 30 explicitly gated on Task 29 being clean (CDP-public-exposure mitigation)

**Placeholder scan:** No "TBD" / "implement later" / "fill in details". Every step has the actual code or command.

**Type / name consistency:** Path `deploy/daily/api/main.py` referenced as `main:app` in Dockerfile, entrypoint, and tests. `AUTOCLI_CDP_ENDPOINT` referenced identically in Rust patch, cdp-discover.sh, run-daily.sh. `API_RUN_TOKEN` referenced identically in env, FastAPI module, tests, and curl probes.

**Scope check:** Single PR delivers a working, testable system. The Rust patch is a hard prereq embedded in this plan rather than a parallel PR (acceptable per SPEC §1.A wording — "before microservice work merges"; combining into one PR satisfies that).

---

## Execution Handoff

Plan complete and saved to `deploy/PLAN.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration. Best for a 34-task plan because each subagent gets clean context.

2. **Inline Execution** — Execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints for review.

**Which approach?**
