# AutoCLI Daily Microservice — Design

| | |
|---|---|
| **Date** | 2026-05-16 |
| **Branch** | `feat/daily-microservice` (separate worktree, branched from `main`) |
| **Target host** | `100.108.80.9` (Tailscale, Ubuntu 24.04, Docker 29.4) |
| **Public endpoint** | `https://autocli.<your-domain>/{vnc,cdp,api,jobs,grafana}` via Cloudflare Tunnel |
| **Goal** | Convert the manual daily flow (`autocli linkedin recommended … | uv run scripts/sync_autocli_jobs.py`) into an auto-scheduled, externally accessible microservice with monitoring. |

---

## 1. Problem & Motivation

Every day the operator runs by hand:

```bash
autocli linkedin recommended --limit 0 --with_jd true -f json > output/$(date +%Y%m%d).json
uv run scripts/sync_autocli_jobs.py --input output/$(date +%Y%m%d).json
```

This requires a logged-in Chrome on the operator's laptop. Goals of the redesign:

1. **Detach from the laptop** — schedule on a server that's always on (`100.108.80.9`).
2. **Reuse the existing Stagehand-style Chrome setup** that already works locally (VNC + persistent profile + CDP 9222).
3. **Expose status/control over the public internet** with proper auth (operator wants on-the-go VNC re-login, manual run trigger, query proxy, and a Grafana dashboard).
4. **Use the existing pull-based deploy chain** (GHCR + Watchtower) — no new infra wheels.
5. **Stay decoupled**: separate images, no entanglement with the existing `skyvern-*`, `browserless`, `job-*` services on the host (Skyvern will be retired).

---

## 2. Architecture

### 2.1 Container topology (6 services on a dedicated docker-compose stack)

```
┌─ 100.108.80.9 : docker-compose stack "autocli-stack" ────────────────┐
│                                                                       │
│   autocli-chrome          autocli-daily          cloudflared         │
│   (Stagehand image)       (cron + FastAPI)       (Tunnel daemon)     │
│   :6080 :9222 :5900       :8080                  (no exposed port)   │
│        ▲                       ▲                       │             │
│        │CDP (9222)             │HTTP                   │             │
│        └────── docker bridge ──┘                       │             │
│                                                        │             │
│   prometheus :9090   ──▶ scrapes daily:/metrics        │             │
│        ▲                                               │             │
│        │                                               │             │
│   grafana :3000      ──▶ datasource = prometheus       │             │
│        ▲                                               │             │
│        └───────────────────────────────────────────────┘             │
│                                                        │             │
└────────────────────────────────────────────────────────┼─────────────┘
                                                         ▼
                                         Cloudflare Edge (HTTPS + Access)
                                         ▼
                  autocli.<your-domain>/vnc      → chrome:6080
                  autocli.<your-domain>/cdp      → chrome:9222 (strict Access)
                  autocli.<your-domain>/api      → daily:8080
                  autocli.<your-domain>/jobs     → daily:8080/jobs
                  autocli.<your-domain>/grafana  → grafana:3000
```

### 2.2 Component contracts

| Container | Responsibility | Owns | Depends on |
|---|---|---|---|
| `autocli-chrome` | Long-running Chromium with persistent profile and CDP exposure | `chrome-profile` volume | nothing |
| `autocli-daily` | Daily cron, manual `/run`, status & metrics API, Supabase proxy | `data/output/`, `data/logs/`, `run-daily.lock` | `autocli-chrome:9222` |
| `cloudflared` | Cloudflare Tunnel ingress | tunnel credentials env | Cloudflare edge |
| `prometheus` | Scrape `daily:/metrics` every 15 s | `prom-data` volume | `autocli-daily:8080` |
| `grafana` | Visualise metrics; pre-provisioned dashboard | `grafana-data` volume | `prometheus:9090` |

Boundaries:
- `autocli-chrome` does not know it is being used by `autocli-daily`; it only speaks CDP. Replace it with any CDP-speaking Chrome and the rest still works.
- `autocli-daily` discovers Chrome via `curl http://autocli-chrome:9222/json/version` at boot, never hard-codes a page id.

---

## 3. Repository Layout & Worktree

### 3.1 New files inside the existing `AutoCLI` repo

```
AutoCLI/
├── (existing content untouched)
└── deploy/                                ← new top-level directory
    ├── chrome/
    │   ├── Dockerfile                     ← copy of my-stagehand-app/Dockerfile.chrome
    │   └── entrypoint-vnc.sh              ← copy of entrypoint-vnc.sh
    ├── daily/
    │   ├── Dockerfile                     ← multi-stage: python-slim + COPY autocli binary
    │   ├── entrypoint.sh                  ← starts supercronic + uvicorn in parallel
    │   ├── crontab                        ← "0 3 * * * /app/run-daily.sh"
    │   ├── run-daily.sh                   ← orchestrator (flock + retry + log)
    │   └── api/
    │       ├── pyproject.toml             ← uv-managed (fastapi, supabase, prometheus-client)
    │       ├── main.py                    ← FastAPI: /run /status /logs /jobs /metrics
    │       └── trigger.py                 ← shared run-daily executor used by cron + /run
    ├── cloudflared/
    │   └── config.yml                     ← Tunnel ingress map (with <your-domain> placeholder)
    ├── prometheus/
    │   └── prometheus.yml                 ← single scrape job
    ├── grafana/
    │   └── provisioning/
    │       ├── datasources/prometheus.yml
    │       └── dashboards/autocli.json    ← pre-built dashboard JSON
    ├── docker-compose.yml                 ← production stack (6 services)
    ├── docker-compose.local.yml           ← override for laptop e2e testing
    ├── .env.example                       ← every required variable, with empty values
    └── README.md                          ← deploy & runbook

.github/workflows/
└── deploy-microservice.yml                ← CI: build binary + 2 images → push GHCR
```

### 3.2 Worktree strategy

- **Branch**: `feat/daily-microservice` — branched from `main` (not from `feat/indeed-search-adapter`).
- **Worktree path**: `/Users/sanchezrick/Documents/Github/AutoCLI-daily/`
- **Reason**: keep this work isolated from the in-flight Indeed adapter PR; merge order independent.

Created with:
```bash
cd /Users/sanchezrick/Documents/Github/AutoCLI
git worktree add ../AutoCLI-daily -b feat/daily-microservice main
```

### 3.3 Why one repo, not two

- We need the `autocli` binary built from `crates/`, and we ship `scripts/sync_autocli_jobs.py` inside the daily image. Single repo = atomic PRs that change both the code and the deploy config.
- A single GitHub Actions workflow handles both images.

---

## 4. Image Build Pipeline

### 4.1 GitHub Actions (`deploy-microservice.yml`)

```
on:
  push:
    branches: [feat/daily-microservice, main]
    paths:
      - deploy/**
      - crates/**
      - scripts/sync_autocli_jobs.py
      - .github/workflows/deploy-microservice.yml
  workflow_dispatch:

jobs:
  build-autocli-binary:
    runs-on: ubuntu-latest         # x86_64 host = matches prod
    steps:
      - checkout
      - Swatinem/rust-cache@v2
      - cargo build --release -p autocli-cli
      - upload-artifact: target/release/autocli

  build-chrome-image:
    runs-on: ubuntu-latest
    needs: []
    steps:
      - checkout
      - docker/setup-buildx-action
      - docker/login-action: ghcr.io
      - docker/build-push-action:
          context: deploy/chrome
          platforms: linux/amd64
          tags: |
            ghcr.io/ricksanchez88e/autocli-chrome:main
            ghcr.io/ricksanchez88e/autocli-chrome:sha-${{ github.sha }}

  build-daily-image:
    runs-on: ubuntu-latest
    needs: [build-autocli-binary]
    steps:
      - checkout
      - download-artifact: target/release/autocli → deploy/daily/bin/autocli
      - docker/setup-buildx-action
      - docker/login-action: ghcr.io
      - docker/build-push-action:
          context: .                # so it can also COPY scripts/sync_autocli_jobs.py
          file: deploy/daily/Dockerfile
          platforms: linux/amd64
          tags: |
            ghcr.io/ricksanchez88e/autocli-daily:main
            ghcr.io/ricksanchez88e/autocli-daily:sha-${{ github.sha }}
```

### 4.2 Image sizes & decisions

| Image | Base | Approx size | Why this base |
|---|---|---|---|
| `autocli-chrome` | `debian:bookworm-slim` | ~600 MB | Matches local dev image byte-for-byte (Chromium + Xvfb + noVNC + supervisor) |
| `autocli-daily` | `python:3.12-slim-bookworm` | ~200 MB | Need uv + supabase-py + fastapi; autocli binary is a static-ish ELF copied in |
| `cloudflared` | `cloudflare/cloudflared:latest` | ~30 MB | Official |
| `prometheus`, `grafana` | upstream `latest` | upstream | Avoid custom rebuilds |

### 4.3 Watchtower integration

Both Autocli images get:
```yaml
labels:
  com.centurylinklabs.watchtower.enable: "true"
```
The existing `job-watchtower` (5 min poll, `WATCHTOWER_LABEL_ENABLE=true`, `WATCHTOWER_CLEANUP=true`) picks them up.

Cloudflared, Prometheus, Grafana stay pinned to their official `:latest` and **do not** get the Watchtower label — they upgrade manually to avoid surprise breakages of the public surface.

---

## 5. Runtime Flow

### 5.1 Process tree inside `autocli-daily`

```
PID 1 : tini
  ├─ supercronic /etc/cron.d/autocli         (TZ=Europe/London)
  │     └─ "0 3 * * * /app/run-daily.sh"
  │           └─ /app/bin/autocli linkedin recommended --limit 0 --with_jd true -f json
  │              > /data/output/$(date +%Y%m%d).json
  │           └─ uv run /app/scripts/sync_autocli_jobs.py --input /data/output/...
  │           └─ update last_run.json + emit prometheus metrics file
  │
  └─ uvicorn api.main:app --host 0.0.0.0 --port 8080
        FastAPI routes:
        ├─ GET  /api/status      → last_run.json (last_run_unixts, exit_code, row counts, errors[])
        ├─ POST /api/run         → spawn run-daily.sh in background (flock-protected)
        ├─ GET  /api/logs        → tail -n 200 /data/logs/run-<latest>.log
        ├─ GET  /api/metrics     → Prometheus exposition text
        ├─ GET  /jobs?since=…    → proxy: supabase.from("jobs.jobs").select(...)
        └─ GET  /health          → 200 if chrome:9222 reachable
```

### 5.2 Invariants

- **CDP discovery, not hard-coded ws**: daily container reads `http://autocli-chrome:9222/json/version` at startup and at every `/run`; passes the `webSocketDebuggerUrl` field to `AUTOCLI_CDP_ENDPOINT`.
- **Boot ordering**: `autocli-daily` does not start cron or accept POST `/run` until `chrome:9222` returns 200 (retry every 2 s, give up after 60 s with non-zero exit so docker-compose restart kicks in).
- **Mutual exclusion**: `run-daily.sh` wraps the body in `flock -n /var/lock/autocli-daily.lock` — cron and `/run` cannot collide.
- **Retry**: on non-zero exit, sleep 60 s and retry once. After two failures the lock releases and the failure is recorded in `last_run.json`; the next cron tick will retry naturally.
- **Output retention**: JSON files kept 30 days, then auto-removed by a daily 04:00 cron entry (`find /data/output -mtime +30 -delete`).
- **Timezone**: container `TZ=Europe/London`; cron expression `0 3 * * *` therefore means 03:00 BST/GMT automatically.

### 5.3 Cloudflare Tunnel ingress (`cloudflared/config.yml`)

```yaml
tunnel: ${CLOUDFLARE_TUNNEL_ID}                # from token, set in compose
credentials-file: /etc/cloudflared/creds.json  # mounted from secret

ingress:
  - hostname: autocli.<your-domain>
    path: /vnc*
    service: http://autocli-chrome:6080

  - hostname: autocli.<your-domain>
    path: /cdp*
    service: http://autocli-chrome:9222
    # ⚠️ Cloudflare Access: Service Token + operator email — strictest tier

  - hostname: autocli.<your-domain>
    path: /api/*
    service: http://autocli-daily:8080

  - hostname: autocli.<your-domain>
    path: /jobs*
    service: http://autocli-daily:8080

  - hostname: autocli.<your-domain>
    path: /grafana*
    service: http://grafana:3000

  - service: http_status:404
```

Cloudflare Access policies (one Application per path):

| Path | Policy |
|---|---|
| `/cdp*` | Service Token **AND** operator email — strict (CDP equals full browser control) |
| `/vnc*` | Email OTP for the operator |
| `/api/*` | Service Token (for scripts) **OR** operator email (for browser) |
| `/jobs*` | Email OTP |
| `/grafana*` | Email OTP |

### 5.4 Prometheus metrics emitted by `autocli-daily`

```
# HELP autocli_daily_last_run_unixts Unix timestamp of last run start
# TYPE autocli_daily_last_run_unixts gauge
autocli_daily_last_run_unixts 1747958400

autocli_daily_last_duration_seconds 142.3
autocli_daily_last_exit_code 0
autocli_daily_run_in_progress 0
autocli_daily_runs_total{result="success"} 47
autocli_daily_runs_total{result="failure"} 2
autocli_daily_rows_scraped_total 12480
autocli_daily_rows_upserted_total 9213
autocli_daily_rows_skipped_total 3267
autocli_chrome_cdp_up 1
```

### 5.5 Grafana dashboard (`autocli.json`)

Single dashboard, panels:
1. **Stat — Time since last run** (red if > 25 h)
2. **Stat — Last exit code** (green = 0)
3. **Stat — Rows scraped today**
4. **Time series — Daily scraped vs upserted vs skipped (30 d)**
5. **Time series — Run duration (30 d)**
6. **Stat — Chrome CDP up (24 h uptime %)**
7. **Logs panel** (Grafana Infinity datasource hitting `/api/logs`)

Provisioned via files under `grafana/provisioning/`, so a fresh Grafana container reproduces the dashboard automatically.

---

## 6. Secrets & Configuration

### 6.1 Required environment variables

| Variable | Consumer container | Source | Notes |
|---|---|---|---|
| `CLOUDFLARE_TUNNEL_TOKEN` | `cloudflared` | Operator (existing) | Long-lived tunnel JWT |
| `SUPABASE_URL` | `autocli-daily` | Operator's `.env` | |
| `SUPABASE_SERVICE_KEY` | `autocli-daily` | Operator's `.env` | Write — never reaches chrome/cloudflared |
| `SUPABASE_ANON_KEY` | `autocli-daily` | Operator's `.env` | Read-only path for `/jobs` |
| `API_RUN_TOKEN` | `autocli-daily` | Generated at deploy | Defense-in-depth on `/run` if Access ever fails open |
| `VNC_PASSWORD` | `autocli-chrome` | Defaults to `stagehand` | Same as local |
| `GF_SECURITY_ADMIN_PASSWORD` | `grafana` | Generated at deploy | Bootstrap admin |
| `GRAFANA_ROOT_URL` | `grafana` | `https://autocli.<your-domain>/grafana` | Sub-path serving |
| `TZ` | all | `Europe/London` | |
| `CRON_SCHEDULE` | `autocli-daily` | `0 3 * * *` | Override-able |

### 6.2 Server file layout

```
/home/rick/autocli-stack/
├── docker-compose.yml          ← committed in repo, scp'd here at deploy
├── .env                        ← 600 perms, rick-only
├── data/
│   ├── chrome-profile/         ← named-volume backing dir (LinkedIn login lives here)
│   ├── output/                 ← daily JSONs, 30 d retention
│   ├── logs/                   ← run-*.log
│   ├── prom-data/              ← prometheus tsdb
│   └── grafana-data/           ← grafana sqlite + plugins
└── cloudflared/
    └── config.yml              ← rendered with operator's domain
```

### 6.3 Secret transfer mechanism

For each secret the operator owns (`CLOUDFLARE_TUNNEL_TOKEN`, `SUPABASE_*`):

1. Operator writes the value into a local file `~/.autocli-secrets.env` (`chmod 600`).
2. Implementation phase: agent does `scp ~/.autocli-secrets.env rick@100.108.80.9:~/autocli-stack/.env`, then `shred -u ~/.autocli-secrets.env`.
3. Secrets are never echoed to the chat transcript and never committed to git.

### 6.4 Per-service env scoping

`docker-compose.yml` does **not** use a global `env_file:` shortcut. Instead each service gets its own explicit `environment:` block referencing only the keys it needs. Example: `cloudflared` sees `CLOUDFLARE_TUNNEL_TOKEN` only; `autocli-chrome` sees `VNC_PASSWORD` only; Supabase keys live only inside `autocli-daily`.

---

## 7. Acceptance Criteria & Phased Verification

Each phase is a hard gate. Implementation moves to the next only after all checks of the previous pass.

### Phase 0 — Local image build
```bash
cd ../AutoCLI-daily/deploy
docker buildx build --platform linux/amd64 -f chrome/Dockerfile -t test-chrome .
docker buildx build --platform linux/amd64 -f daily/Dockerfile -t test-daily ..
docker run --rm test-daily /app/bin/autocli --version
```
✅ Both images build; `autocli --version` returns a non-empty string from inside `test-daily`.

### Phase 1 — Local e2e (no Cloudflare Tunnel)
```bash
docker compose -f deploy/docker-compose.local.yml up -d
# manual: open http://localhost:6081/vnc.html, log into LinkedIn once
curl http://localhost:8081/api/health     # 200
docker exec autocli-daily-local /app/run-daily.sh
```
✅ JSON written to `data/output/`; Supabase `jobs.jobs` has new rows; `/api/status` shows `last_exit_code:0`.

### Phase 2 — CI green & images on GHCR
Push branch → workflow finishes → both tags visible:
- `ghcr.io/ricksanchez88e/autocli-chrome:main`
- `ghcr.io/ricksanchez88e/autocli-daily:main`

### Phase 3 — Server bring-up (executed by the implementing agent)
```bash
ssh rick@100.108.80.9
docker stop skyvern-skyvern-1 skyvern-skyvern-ui-1
docker rm   skyvern-skyvern-1 skyvern-skyvern-ui-1
mkdir -p ~/autocli-stack
# scp docker-compose.yml, cloudflared/config.yml, .env (with secrets) here
cd ~/autocli-stack
docker compose pull
docker compose up -d
```
✅ `docker ps` shows 6 new containers healthy. Existing `job-*`, `sub2api*` untouched.

### Phase 4 — Tunnel & Access reachable
```bash
# operator's laptop
curl -I https://autocli.<your-domain>/vnc/vnc.html         # 200 or 302→Cloudflare login
curl -I https://autocli.<your-domain>/api/health           # 200
curl -I https://autocli.<your-domain>/cdp/json/version     # 302→Access (unauth) ; 200 after Access
curl -I https://autocli.<your-domain>/grafana/login        # 200
curl -s https://autocli.<your-domain>/api/metrics | grep autocli_daily_   # several lines
```
✅ All five.

### Phase 5 — Forced run via API
```bash
curl -X POST \
  -H "CF-Access-Client-Id: $CF_ID" \
  -H "CF-Access-Client-Secret: $CF_SECRET" \
  https://autocli.<your-domain>/api/run
sleep 180
curl -s https://autocli.<your-domain>/api/status | jq
```
✅ `last_exit_code == 0`, `rows_upserted > 0`; Supabase shows new rows; Grafana dashboard shows the run.

### Phase 6 — Two consecutive scheduled runs
Two days, no manual intervention, `last_run_unixts` advances daily, no failed runs.

### Failure-mode contingencies

| Failure | Detection | Mitigation |
|---|---|---|
| Bad image rolled out | `/api/health` 503; Grafana CDP-up flatlines | Pin previous tag: `docker compose pull` with `:sha-<previous>` in override |
| Chrome profile corruption | `/api/run` fails with "LinkedIn login required" | VNC in, re-login; if volume itself broken, restore from `data/chrome-profile.bak` (a future PR adds the backup cron) |
| Cloudflare Tunnel disconnect | Public 502 | `docker restart cloudflared`; verify token validity |
| Supabase rate limit / 429 | `run-daily.sh` exits non-zero, retries with backoff (3 attempts) | After 3 → next-day retry by cron |
| supercronic drift (>25 h since last run) | Grafana "time since last run" panel red | `docker compose restart autocli-daily` |

---

## 8. Out of Scope (Explicit)

| Item | Reason / Future plan |
|---|---|
| Multiple LinkedIn accounts | One profile per chrome container; future PR can horizontally scale |
| Loki / log aggregation | Grafana Infinity → `/api/logs` is enough at this stage |
| Alertmanager / Slack-Discord webhooks | Grafana panels + email-on-error from a future PR |
| Indeed adapter into the same cron | Land Indeed PR first, then add a single cron line |
| HTTPS certificates on origin | Cloudflare Tunnel egress already terminates HTTPS |
| Backup of `chrome-profile` volume | Documented but not implemented in this phase |
| Multi-region failover | Single-host design; future concern |

---

## 9. Risks & Open Items

1. **CDP public exposure.** Cloudflare Access *must* be configured before bringing Phase 4 traffic up. The implementation will refuse to write the `/cdp*` ingress entry into `cloudflared/config.yml` until the operator confirms the Access Application is created and a Service Token is in `.env`.
2. **LinkedIn cookie lifetime.** Empirically 30-90 days. When it expires, `last_exit_code` becomes non-zero with a recognisable error string. Operator action: open `/vnc/` → re-login. No code change needed.
3. **Skyvern decommission.** The operator authorised stopping `skyvern-skyvern-{1,ui-1}`. Their data volumes are not deleted by this design — only the running containers. Skyvern can be re-enabled later by `docker compose up` from its own compose file if needed.
4. **`<your-domain>`.** Spec leaves the public hostname as a placeholder; the operator must provide it before Phase 3.
5. **`API_RUN_TOKEN` rotation.** Generated at first deploy and stored only on the server. Rotation requires editing `.env` and `docker compose restart autocli-daily`.

---

## 10. Glossary

| Term | Meaning |
|---|---|
| **Stagehand image** | The operator's locally-built `my-stagehand-app-chrome` image — Chromium + Xvfb + x11vnc + noVNC + socat in a single container. Renamed to `autocli-chrome` in this design. |
| **Pull-based deploy** | CI pushes new image tags to GHCR; Watchtower on the server polls every 5 min and recreates containers labelled `com.centurylinklabs.watchtower.enable=true`. |
| **Cloudflare Access** | Identity gate in front of a Cloudflare Tunnel — verifies the caller before passing traffic to the origin. |
| **CDP** | Chrome DevTools Protocol — JSON-over-WebSocket API to control Chromium. |
