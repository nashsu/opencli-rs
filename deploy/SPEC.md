# AutoCLI Daily Microservice — Design

| | |
|---|---|
| **Date** | 2026-05-16 |
| **Branch** | `feat/daily-microservice` (separate worktree, branched from `main`) |
| **Target host** | `100.108.80.9` (Tailscale, Ubuntu 24.04, Docker 29.4) |
| **Public endpoint** | 4 subdomains under `<your-zone>` — `vnc.autocli.<your-zone>`, `cdp.autocli.<your-zone>`, `api.autocli.<your-zone>` (carries `/api/*` AND `/jobs`), `grafana.autocli.<your-zone>` — via Cloudflare Tunnel `--token` mode |
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

## Prerequisite Patch — autocli CDP wiring (must merge first)

**Problem.** `crates/autocli-browser/src/bridge.rs::BrowserBridge::connect()` currently has only one code path: spawn the daemon, wait for the Chrome extension to connect over WebSocket, return a `DaemonPage`. The `CdpPage` type in `crates/autocli-browser/src/cdp.rs` is defined but never instantiated from the command-execution flow, and `AUTOCLI_CDP_ENDPOINT` is read only by `commands/doctor.rs` for diagnostics. In a containerised deploy the daemon-and-extension path does not work (extension cannot live in the same container as the daemon; `is_chrome_running()` uses `pgrep` which cannot see Chrome in a sibling container).

**Patch.** Add a `AUTOCLI_CDP_ENDPOINT` branch at the top of `BrowserBridge::connect()`:

```rust
pub async fn connect(&mut self) -> Result<Arc<dyn IPage>, CliError> {
    if let Ok(endpoint) = std::env::var("AUTOCLI_CDP_ENDPOINT") {
        let page = CdpPage::connect(&endpoint).await?;
        return Ok(Arc::new(page));
    }
    Ok(self.connect_daemon_page().await?)
}
```

When the env var is set we skip `is_chrome_running()`, `spawn_daemon()`, and `poll_extension()` entirely. The `IPage` trait is the same, so `autocli-pipeline` consumes either page implementation transparently. A small unit test covers the env-var branch with a mock CDP endpoint.

**Scope of the patch.**
- File touched: `crates/autocli-browser/src/bridge.rs` (≈10 LOC) + one test.
- No change to `IPage`, `autocli-pipeline`, or YAML adapter execution.
- Lands on `main` in its own PR **before** the microservice work merges; the daily-image CI build pins to that commit.

**Verification of the patch (locally, before this design's Phase 0).**
```bash
AUTOCLI_CDP_ENDPOINT=ws://localhost:9222/devtools/page/<id> \
  cargo run --release -p autocli -- linkedin recommended --limit 5 -f json
```
Run against the operator's local Stagehand Chrome. Expect a non-empty JSON array. Failure means the patch must be revised before the microservice work proceeds.

---

## 2. Architecture

### 2.1 Container topology (5 services on a dedicated docker-compose stack)

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
│   prometheus :9090   ──▶ scrapes daily:8080/api/metrics│             │
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
                  vnc.autocli.<your-zone>     → chrome:6080
                  cdp.autocli.<your-zone>     → chrome:9222 (strict Access)
                  api.autocli.<your-zone>     → daily:8080  (serves /api/* AND /jobs)
                  grafana.autocli.<your-zone> → grafana:3000
```

### 2.2 Component contracts

| Container | Responsibility | Owns | Depends on |
|---|---|---|---|
| `autocli-chrome` | Long-running Chromium with persistent profile and CDP exposure | `chrome-profile` volume | nothing |
| `autocli-daily` | Daily cron, manual `/run`, status & metrics API, Supabase proxy | `data/output/`, `data/logs/`, `run-daily.lock` | `autocli-chrome:9222` |
| `cloudflared` | Cloudflare Tunnel ingress | tunnel credentials env | Cloudflare edge |
| `prometheus` | Scrape `autocli-daily:8080/api/metrics` every 15 s | `prom-data` volume | `autocli-daily:8080` |
| `grafana` | Visualise metrics; pre-provisioned dashboard | `grafana-data` volume | `prometheus:9090` |

Boundaries:
- `autocli-chrome` does not know it is being used by `autocli-daily`; it only speaks CDP. Replace it with any CDP-speaking Chrome and the rest still works.
- `autocli-daily` discovers Chrome via `curl http://autocli-chrome:9222/json/list` (creating a page with `PUT /json/new?about:blank` if the list is empty) at boot and at every `/api/run`, never hard-codes a page id. See §5.2 for the host-rewrite step.

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
    │       ├── main.py                    ← FastAPI routes: /api/{status,run,logs,metrics,health} + /jobs
    │       └── trigger.py                 ← shared run-daily executor used by cron + /run
    ├── prometheus/
    │   └── prometheus.yml                 ← single scrape job
    ├── grafana/
    │   └── provisioning/
    │       ├── datasources/prometheus.yml
    │       └── dashboards/autocli.json    ← pre-built dashboard JSON
    ├── docker-compose.yml                 ← production stack (5 services)
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

```yaml
on:
  push:
    branches: [feat/daily-microservice, main]
    paths:
      - deploy/**
      - crates/**
      - scripts/sync_autocli_jobs.py
      - .github/workflows/deploy-microservice.yml
  workflow_dispatch:

env:
  # :main only on main; feature branches publish :branch-<slug> + :sha-<short>.
  # Watchtower in prod tracks :main → feature branches NEVER reach prod by accident.
  IS_MAIN: ${{ github.ref == 'refs/heads/main' }}

jobs:
  build-autocli-binary:
    runs-on: ubuntu-latest                       # x86_64 host = matches prod
    steps:
      - uses: actions/checkout@v4
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release -p autocli    # crate name is `autocli` (per crates/autocli-cli/Cargo.toml)
      - uses: actions/upload-artifact@v4
        with: { name: autocli-bin, path: target/release/autocli }

  build-chrome-image:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with: { registry: ghcr.io, username: ${{ github.actor }}, password: ${{ secrets.GITHUB_TOKEN }} }
      # NOTE (slugifier): `type=ref,event=branch` runs metadata-action's slugifier —
      # `feat/daily-microservice` → `branch-feat-daily-microservice` (Docker-tag-safe).
      # Comments live outside `tags: |` because the YAML block-scalar would otherwise
      # treat them as literal text and metadata-action would try to parse them as rules.
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
          context: .                              # unified context = repo root for BOTH images
          file: deploy/chrome/Dockerfile          # COPY paths in Dockerfile are repo-relative
          platforms: linux/amd64
          tags: ${{ steps.meta.outputs.tags }}
          push: true

  build-daily-image:
    runs-on: ubuntu-latest
    needs: [build-autocli-binary]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with: { name: autocli-bin, path: deploy/daily/bin }
      - run: chmod +x deploy/daily/bin/autocli
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with: { registry: ghcr.io, username: ${{ github.actor }}, password: ${{ secrets.GITHUB_TOKEN }} }
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
          context: .                              # same context as chrome image
          file: deploy/daily/Dockerfile
          platforms: linux/amd64
          tags: ${{ steps.meta.outputs.tags }}
          push: true
```

### 4.2 Image sizes & decisions

| Image | Base | Approx size | Why this base |
|---|---|---|---|
| `autocli-chrome` | `debian:bookworm-slim` | ~600 MB | Matches local dev image byte-for-byte (Chromium + Xvfb + noVNC + supervisor) |
| `autocli-daily` | `python:3.12-slim-bookworm` | ~200 MB | Need uv + supabase-py + fastapi; autocli binary is a static-ish ELF copied in |
| `cloudflared` | `cloudflare/cloudflared:2025.4.0` | ~30 MB | Pinned to a specific release — reproducible deploys, no surprise upgrades |
| `prometheus` | `prom/prometheus:v3.5.0` | ~280 MB | Pinned semver |
| `grafana` | `grafana/grafana:11.6.0` | ~400 MB | Pinned semver |

> Watchtower **must not** auto-upgrade these three — they don't carry the `com.centurylinklabs.watchtower.enable` label.

### 4.3 Watchtower integration

Both Autocli images get:
```yaml
labels:
  com.centurylinklabs.watchtower.enable: "true"
```
The existing `job-watchtower` (5 min poll, `WATCHTOWER_LABEL_ENABLE=true`, `WATCHTOWER_CLEANUP=true`) picks them up. **Only the `:main` tag is tracked in prod** — feature branches publish `:branch-*` and `:sha-*` only, so unmerged code can never reach the server.

Cloudflared, Prometheus, Grafana run pinned versions (see §4.2) and **do not** carry the Watchtower label — upgrades are deliberate.

### 4.4 GHCR pull credentials (already configured)

Recon on 2026-05-16 confirmed `100.108.80.9` has `ghcr.io` in `~/.docker/config.json` and `docker pull ghcr.io/ricksanchez88e/job-scraper-api:main` succeeds. No new login step is needed. Phase 3 verifies with:

```bash
docker pull ghcr.io/ricksanchez88e/autocli-chrome:main && \
docker pull ghcr.io/ricksanchez88e/autocli-daily:main
```

If either pull fails with 401/403 (e.g. PAT expired): `echo $GHCR_PAT | docker login ghcr.io -u ricksanchez88e --password-stdin`.

---

## 5. Runtime Flow

### 5.1 Process tree inside `autocli-daily`

```
PID 1 : tini
  ├─ /app/cdp-discover.sh                    (runs once at boot, blocks until chrome ready)
  │     GET  http://autocli-chrome:9222/json/list
  │     if no type:"page" target →  PUT http://autocli-chrome:9222/json/new?about:blank
  │     extract webSocketDebuggerUrl, rewrite host (localhost:9223 → autocli-chrome:9222)
  │     write the resulting ws:// URL to /run/cdp-endpoint.env  →  AUTOCLI_CDP_ENDPOINT
  │     (boot run gates supercronic + uvicorn startup; see §5.2)
  │
  ├─ supercronic /etc/cron.d/autocli         (TZ=Europe/London; starts only after the boot cdp-discover.sh exits 0)
  │     └─ "0 3 * * * /app/run-daily.sh"
  │           └─ /app/cdp-discover.sh                  # re-discover every run — Chrome may have restarted, page id may have changed
  │           └─ source /run/cdp-endpoint.env          # pick up the fresh AUTOCLI_CDP_ENDPOINT
  │           └─ /app/bin/autocli linkedin recommended --limit 0 --with_jd true -f json
  │              > /data/output/$(date +%Y%m%d).json
  │           └─ uv run /app/scripts/sync_autocli_jobs.py --input /data/output/...
  │           └─ update last_run.json + emit prometheus metrics file
  │
  └─ uvicorn api.main:app --host 0.0.0.0 --port 8080
        FastAPI routes (all under /api/* — Prometheus scrape uses /api/metrics):
        ├─ GET  /api/status   [Bearer]   last_run.json {last_run_unixts, exit_code, rows_*, errors[]}
        ├─ POST /api/run      [Bearer]   spawns run-daily.sh (flock-protected)
        ├─ GET  /api/logs     [Bearer]   tail -n 200 /data/logs/run-<latest>.log
        ├─ GET  /api/metrics  [open]     Prometheus exposition; app-layer open. In-cluster scrape hits this directly;
        │                                external callers via api.autocli are still gated by Cloudflare Access.
        ├─ GET  /api/health   [open]     200 iff chrome:9222 reachable AND /run/cdp-endpoint.env exists.
        │                                Same dual-path: docker healthcheck internal, public access via api.autocli + Cloudflare Access.
        └─ GET  /jobs?since=… [Bearer]
              → client = create_client(SUPABASE_URL, SUPABASE_ANON_KEY)
              → client.schema("jobs").table("jobs").select(...).gte("post_time", since).execute()
              (matches scripts/backfill_priority_scores.py — schema-qualified table API)

        [Bearer] = Authorization: Bearer ${API_RUN_TOKEN} required at the FastAPI layer; missing/wrong → 401.
        [open]   = no Bearer at the app layer; external requests still hit Cloudflare Access first.
                   Inside the docker network (Prometheus scrape, docker healthcheck) requests bypass both.
```

### 5.2 Invariants

- **CDP page target, not browser endpoint**: `/json/version` returns a browser-level WebSocket that does not accept page-scoped commands. `cdp-discover.sh` therefore hits `GET /json/list`, picks the first `type:"page"` target, and if none exists `PUT /json/new?about:blank` to create one. (Chrome ≥ M86 rejects `GET` and `POST` on `/json/new` with `405 Method Not Allowed`; `PUT` is the only supported verb.) Only `webSocketDebuggerUrl` from that page target is exported as `AUTOCLI_CDP_ENDPOINT`.
- **Host rewrite**: the Stagehand image binds Chromium to `127.0.0.1:9223` (socat exposes 9222 publicly), so `/json/list` returns URLs like `ws://localhost:9223/devtools/page/<id>`. `cdp-discover.sh` rewrites the host:port portion to `autocli-chrome:9222` (the docker-service-name + the externally-mapped port) before exporting. Confirmed against `~/Documents/Github/my-stagehand-app/scripts/entrypoint-vnc.sh`.
- **Discovery cadence**: `cdp-discover.sh` runs at TWO points — (1) at container boot, gating supercronic and uvicorn startup; (2) at the start of every `run-daily.sh` invocation (cron-driven AND `POST /api/run`-driven), before `AUTOCLI_CDP_ENDPOINT` is sourced. The per-run discovery refreshes the page id in case Chrome restarted or the page was closed between cron ticks. Boot retry: every 2 s, give up at 60 s with exit ≠ 0; `restart: unless-stopped` then recreates the container until Chrome is reachable. Per-run discovery uses the same retry budget but counts as a transient failure under §5.2 unified retry if it gives up.
- **Boot ordering**: `entrypoint.sh` runs the boot-time `cdp-discover.sh` synchronously first; only after it exits 0 does supercronic launch and uvicorn bind `:8080`.
- **Mutual exclusion**: `run-daily.sh` wraps the body in `flock -n /var/lock/autocli-daily.lock` — cron and `/api/run` cannot collide.
- **Retry policy (unified)**: a single backoff schedule applies to every transient failure (autocli exit ≠ 0, Supabase 429/5xx, CDP disconnect). Three attempts at **15 s → 60 s → 240 s**. On the 4th failure: record `last_exit_code` in `last_run.json`, increment `autocli_daily_runs_total{result="failure"}`, release the lock, log to `/data/logs/run-<date>.log`. The next cron tick is the next retry opportunity. This single policy is referenced from runbook, code, metrics, and Phase-7 failure table — all kept in sync.
- **Output retention**: JSON files kept 30 days; a daily 04:00 cron entry runs `find /data/output -mtime +30 -delete`.
- **Timezone**: container `TZ=Europe/London`; cron expression `0 3 * * *` is 03:00 BST/GMT automatically.

### 5.3 Cloudflare Tunnel — token mode + subdomain routing

**Token mode, not credentials-file.** The operator already has a Tunnel token from the Cloudflare dashboard. cloudflared runs as:

```yaml
# docker-compose.yml excerpt
cloudflared:
  image: cloudflare/cloudflared:2025.4.0
  restart: unless-stopped
  command: tunnel --no-autoupdate run --token ${CLOUDFLARE_TUNNEL_TOKEN}
  environment:
    TUNNEL_TOKEN: ${CLOUDFLARE_TUNNEL_TOKEN}
  depends_on: [autocli-chrome, autocli-daily, grafana]
  networks: [autocli-net]
```

In token mode **ingress rules live in the Cloudflare dashboard**, not in a local `config.yml`. There is no `${CLOUDFLARE_TUNNEL_ID}` interpolation problem (cloudflared does not parse a YAML at all) and no `credentials-file` to manage. The two modes are not mixed.

**Subdomains, not path routes.** Cloudflare Tunnel does not strip the matched URL prefix, so `/cdp/json/version` would arrive at the origin as `/cdp/json/version` and Chromium would return 404. Each surface gets its own subdomain — no path rewriting required:

| Public hostname | Origin (docker service) | Notes |
|---|---|---|
| `vnc.autocli.<your-zone>` | `http://autocli-chrome:6080` | noVNC web client |
| `cdp.autocli.<your-zone>` | `http://autocli-chrome:9222` | CDP HTTP + WebSocket upgrade |
| `api.autocli.<your-zone>` | `http://autocli-daily:8080` | FastAPI: `/api/*` (status, run, logs, metrics, health) AND `/jobs` (Supabase read proxy) |
| `grafana.autocli.<your-zone>` | `http://grafana:3000` | Subdomain → no `serve_from_sub_path` needed |

These four hostnames are configured in the dashboard under the same Tunnel. Implementation produces a screenshot/checklist for the operator to apply.

**Cloudflare Access — one Application per subdomain, two policies inside each.** Within a single Application multiple policies are evaluated as **OR** — a request matching any one policy is admitted. This lets us serve both humans and scripts on the same surface:

| Subdomain | Policy A (machines) | Policy B (humans) |
|---|---|---|
| `cdp.autocli` | **Dedicated** Service Token (separate from api.autocli's), short-lived rotation, **plus** mTLS client-certificate validation on the same Access Application | Operator email OTP + **required** WARP device posture (Cloudflare WARP enrolled, healthy device) |
| `vnc.autocli` | — (humans only; scripts have no business here) | Operator email OTP + required WARP device posture |
| `api.autocli` | Service Token (used by `curl` / scripts for `/api/*` and `/jobs`) | Operator email OTP |
| `grafana.autocli` | — (humans only) | Operator email OTP |

> **Why no IP allow-list on `cdp.autocli`.** Cloudflare Access sees the caller's public/WARP-egress IP at the Cloudflare edge — it never sees the Tailscale `100.x` CGNAT address (Tailscale only connects operator devices to the home server, not to Cloudflare). An IP allow-list scoped to "Tailscale range" would never match and would be misleading security theatre. We use **mTLS + dedicated short-lived Service Token + WARP posture** as the strong layers instead.

`cdp.autocli` is the most sensitive surface — the CDP WebSocket is equivalent to a remote shell on the browser. Its Application therefore carries the extra mTLS requirement, a dedicated short-lived Service Token, and WARP-required human policy. (Network-selector / IP allow-lists are deliberately omitted; see callout above.)

### 5.4 Prometheus metrics emitted by `autocli-daily`

Exposed at `GET /api/metrics` (not `/metrics`). Prometheus scrape config must specify the path:

```yaml
# prometheus.yml
scrape_configs:
  - job_name: autocli-daily
    metrics_path: /api/metrics
    static_configs:
      - targets: [autocli-daily:8080]
```

Sample exposition:

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

Single dashboard, six panels (all backed by the bundled Prometheus datasource — no plugins to install):

1. **Stat — Time since last run** (red if > 25 h)
2. **Stat — Last exit code** (green = 0)
3. **Stat — Rows scraped today**
4. **Time series — Daily scraped vs upserted vs skipped (30 d)**
5. **Time series — Run duration (30 d)**
6. **Stat — Chrome CDP up (24 h uptime %)**

Logs are read out of band via `curl https://api.autocli.<your-zone>/api/logs` or `docker logs autocli-daily`. A future PR may add Loki + a Grafana logs panel; that is out of scope for this design.

Dashboard JSON and the datasource pointer are committed under `grafana/provisioning/`, so a fresh Grafana container reproduces the dashboard automatically.

---

## 6. Secrets & Configuration

### 6.1 Required environment variables

| Variable | Consumer container | Source | Notes |
|---|---|---|---|
| `CLOUDFLARE_TUNNEL_TOKEN` | `cloudflared` | Operator (existing) | Long-lived tunnel JWT, passed via `--token` |
| `SUPABASE_URL` | `autocli-daily` | Operator's `.env` | Same name `scripts/sync_autocli_jobs.py` already reads |
| `SUPABASE_SERVICE_ROLE_KEY` | `autocli-daily` | Operator's `.env` | Matches the script's actual env-var name (or `SUPABASE_KEY` fallback). Never reaches chrome/cloudflared. |
| `SUPABASE_ANON_KEY` | `autocli-daily` | Operator's `.env` | Used by `/jobs` read-only path |
| `API_RUN_TOKEN` | `autocli-daily` | Generated at deploy (`openssl rand -hex 32`) | **Enforced** by FastAPI: `GET /api/status`, `POST /api/run`, `GET /api/logs`, AND `GET /jobs` all require `Authorization: Bearer ${API_RUN_TOKEN}`; missing/wrong → 401. (`/api/health` and `/api/metrics` are intentionally open — see §5.1 route table.) Defense-in-depth in case Cloudflare Access ever fails open. |
| `VNC_PASSWORD` | `autocli-chrome` | Generated at deploy (`openssl rand -base64 18`) | **Never** uses the local-dev default `stagehand` in prod; the operator gets the generated value once and stores it (1Password / similar). |
| `GF_SECURITY_ADMIN_PASSWORD` | `grafana` | Generated at deploy | Bootstrap admin |
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
```

> **No `cloudflared/` directory on the server** — token mode (§5.3) keeps ingress definitions in the Cloudflare dashboard, not in a file.

### 6.3 Secret transfer mechanism

For each secret the operator owns (`CLOUDFLARE_TUNNEL_TOKEN`, `SUPABASE_*`):

1. Operator writes the value into a local file `~/.autocli-secrets.env` (`chmod 600`). **This is the operator's source-of-truth file and is never deleted by the agent.**
2. Implementation phase: agent runs `cp ~/.autocli-secrets.env /tmp/autocli-secrets.$$.env` to make a temp copy, then `scp /tmp/autocli-secrets.$$.env rick@100.108.80.9:~/autocli-stack/.env`, then `shred -u /tmp/autocli-secrets.$$.env` to wipe the temp copy only.
3. Generated secrets (`API_RUN_TOKEN`, `VNC_PASSWORD`, `GF_SECURITY_ADMIN_PASSWORD`) are produced *on the server* during deploy and appended directly to `~/autocli-stack/.env`; the values are printed once to the operator's terminal via the SSH session.
4. Secrets are never echoed to the chat transcript and never committed to git.

### 6.4 Per-service env scoping

`docker-compose.yml` does **not** use a global `env_file:` shortcut. Instead each service gets its own explicit `environment:` block referencing only the keys it needs. Example: `cloudflared` sees `CLOUDFLARE_TUNNEL_TOKEN` only; `autocli-chrome` sees `VNC_PASSWORD` only; Supabase keys live only inside `autocli-daily`.

---

## 7. Acceptance Criteria & Phased Verification

Each phase is a hard gate. Implementation moves to the next only after all checks of the previous pass.

### Phase 0 — Local image build (context = repo root, matches CI)

The operator's Mac is arm64-darwin; the production image runs linux/amd64. A `cargo build` on the host would produce a Mach-O binary that can't run inside a Linux container. The binary must therefore be built **inside a Docker rust container** that runs as `linux/amd64`:

```bash
cd /Users/sanchezrick/Documents/Github/AutoCLI-daily    # the worktree, NOT deploy/

# Build autocli for linux/amd64 inside a throwaway Rust container.
# Volume-cache cargo registry so subsequent builds are fast.
mkdir -p deploy/daily/bin
docker run --rm --platform linux/amd64 \
  -v "$PWD":/work -w /work \
  -v autocli-daily-cargo-cache:/usr/local/cargo/registry \
  -v autocli-daily-cargo-target:/work/target \
  rust:1.81-slim-bookworm \
  bash -c "cargo build --release -p autocli && cp target/release/autocli deploy/daily/bin/autocli"
chmod +x deploy/daily/bin/autocli
file deploy/daily/bin/autocli    # MUST say "ELF 64-bit LSB executable, x86-64" — not "Mach-O"

# Now build images using the same context as CI.
docker buildx build --platform linux/amd64 -f deploy/chrome/Dockerfile -t test-chrome .
docker buildx build --platform linux/amd64 -f deploy/daily/Dockerfile  -t test-daily  .
docker run --rm --platform linux/amd64 test-daily /app/bin/autocli --version
```

✅ `file` shows ELF/x86-64; both images build; `autocli --version` returns a non-empty string from inside `test-daily`.

> CI mirrors this: the `build-autocli-binary` GitHub Action runs on `ubuntu-latest` (linux/amd64) directly, so it doesn't need the Docker-rust trick — its host is already the target platform.

### Phase 1 — Local e2e (no Cloudflare Tunnel)
```bash
docker compose -f deploy/docker-compose.local.yml up -d
# manual: open http://localhost:6081/vnc.html, log into LinkedIn once
LOCAL_TOKEN="$(grep ^API_RUN_TOKEN= deploy/.env.local | cut -d= -f2-)"

curl -s http://localhost:8081/api/health                      # 200 (open endpoint)
docker exec autocli-daily-local /app/run-daily.sh             # forced run
curl -s -H "Authorization: Bearer $LOCAL_TOKEN" \
     http://localhost:8081/api/status | jq                    # last_exit_code:0
```
✅ JSON written to `data/output/`; Supabase `jobs.jobs` has new rows; `/api/status` (with Bearer) shows `last_exit_code:0`.

### Phase 2 — CI green & images on GHCR

Tag expectations depend on the branch being pushed (matches §4.1 workflow):

**Pushing `feat/daily-microservice`** (this design's working branch) — expect:
- `ghcr.io/ricksanchez88e/autocli-chrome:branch-feat-daily-microservice`
- `ghcr.io/ricksanchez88e/autocli-chrome:sha-<short>`
- `ghcr.io/ricksanchez88e/autocli-daily:branch-feat-daily-microservice`
- `ghcr.io/ricksanchez88e/autocli-daily:sha-<short>`
- **No `:main` tag** (would be a bug — Watchtower in prod tracks `:main`)

**After merge to `main`** — expect (in addition to the existing sha tags):
- `ghcr.io/ricksanchez88e/autocli-chrome:main`
- `ghcr.io/ricksanchez88e/autocli-daily:main`

Phase 3 server bring-up reads `:main` and therefore only runs after merge (or as a deliberate one-off pull of `:branch-*` for early staging).

### Phase 3 — Server bring-up (executed by the implementing agent)
```bash
ssh rick@100.108.80.9
docker stop skyvern-skyvern-1 skyvern-skyvern-ui-1
docker rm   skyvern-skyvern-1 skyvern-skyvern-ui-1
mkdir -p ~/autocli-stack
# scp docker-compose.yml + .env (with secrets) here — no cloudflared config file in token mode
cd ~/autocli-stack
docker compose pull
docker compose up -d
```
✅ `docker ps` shows 5 new containers healthy (`autocli-chrome`, `autocli-daily`, `cloudflared`, `prometheus`, `grafana`). Existing `job-*`, `sub2api*`, `browser-automation-*`, `browseruse-debug` untouched.

### Phase 4 — Tunnel & Access reachable

Phase 4 is split into three sub-phases because `cdp.autocli` is the high-risk surface and must not be exposed until the rest is proven. Each sub-phase is a hard gate.

```bash
DOMAIN="<your-zone>"
CF_ID="<service-token-client-id>"          # from Cloudflare Access → Service Tokens
CF_SECRET="<service-token-client-secret>"
TOKEN="<API_RUN_TOKEN from server .env>"
```

#### Phase 4a — Pre-CDP gate: add 3 subdomains to the Cloudflare Tunnel dashboard
Add `vnc.autocli`, `api.autocli`, `grafana.autocli` (NOT `cdp.autocli` yet). Then probe:

```bash
# Unauthenticated → 302 to Cloudflare Access login (never 200, never 502)
for sub in vnc api grafana; do
  code=$(curl -s -o /dev/null -w "%{http_code}" "https://${sub}.autocli.${DOMAIN}/")
  echo "${sub} unauth: ${code}"     # MUST be 302
done

# Humans-only negative machine probe — vnc/grafana have NO Service Token policy by design.
# Sending a Service Token must STILL produce 302 (machines are not admitted; only email OTP is).
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://vnc.autocli.${DOMAIN}/"     | head -1   # MUST be 302 — proves Service Token does NOT grant access here
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://grafana.autocli.${DOMAIN}/" | head -1   # MUST be 302 — same negative-probe semantics

# Machine probe — api.autocli is the ONLY subdomain in 4a with a Service Token policy
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://api.autocli.${DOMAIN}/api/health" | head -1              # HTTP/2 200 (open endpoint after Access)
curl -s  -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     -H "Authorization: Bearer ${TOKEN}" \
     "https://api.autocli.${DOMAIN}/api/metrics" | grep -c autocli_daily_   # ≥ 5

# API_RUN_TOKEN enforcement (independent of Cloudflare Access)
curl -sI -X POST -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://api.autocli.${DOMAIN}/api/run" | head -1                 # HTTP/2 401 (no Bearer)
curl -sI -X POST -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     -H "Authorization: Bearer ${TOKEN}" \
     "https://api.autocli.${DOMAIN}/api/run" | head -1                 # HTTP/2 202

# /jobs read proxy
curl -s  -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     -H "Authorization: Bearer ${TOKEN}" \
     "https://api.autocli.${DOMAIN}/jobs?since=2026-05-15" | jq 'length'  # ≥ 0
```

✅ All Phase 4a probes match expected codes. **Phase 4b runs only if every line above passed.**

#### Phase 4b — Add cdp.autocli to the Cloudflare Tunnel dashboard
Only after Phase 4a is green:
1. Confirm Access Application for `cdp.autocli` exists with **two** policies (dedicated short-lived Service Token + mTLS client-cert; AND operator email + required WARP device posture). See §5.3.
2. Add the hostname to the Tunnel dashboard pointing at `http://autocli-chrome:9222`.

#### Phase 4c — Probe cdp.autocli

The `cdp.autocli` Application carries the **dedicated** Service Token (different `CF_ID_CDP` / `CF_SECRET_CDP`) plus mTLS — `api.autocli`'s credentials must be rejected.

```bash
# Unauthenticated → 302
curl -s -o /dev/null -w "%{http_code}\n" "https://cdp.autocli.${DOMAIN}/json/list"   # 302

# Wrong (api-scoped) Service Token + no mTLS → 302/403 (proves CDP token is scoped)
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://cdp.autocli.${DOMAIN}/json/list" | head -1                # MUST be 302/403

# Right CDP Service Token + mTLS client cert → 200
curl -sI \
     -H "CF-Access-Client-Id: ${CF_ID_CDP}" -H "CF-Access-Client-Secret: ${CF_SECRET_CDP}" \
     --cert "$HOME/.cf-access/cdp-client.crt" --key "$HOME/.cf-access/cdp-client.key" \
     "https://cdp.autocli.${DOMAIN}/json/list" | head -1                # HTTP/2 200

# WebSocket upgrade (probes Cloudflare passes Upgrade headers correctly) — should not 4xx/5xx
curl -sI \
     -H "CF-Access-Client-Id: ${CF_ID_CDP}" -H "CF-Access-Client-Secret: ${CF_SECRET_CDP}" \
     --cert "$HOME/.cf-access/cdp-client.crt" --key "$HOME/.cf-access/cdp-client.key" \
     -H "Connection: Upgrade" -H "Upgrade: websocket" -H "Sec-WebSocket-Version: 13" \
     -H "Sec-WebSocket-Key: $(openssl rand -base64 16)" \
     "https://cdp.autocli.${DOMAIN}/devtools/page/<id>" | head -1       # HTTP/2 101 Switching Protocols
```

✅ All Phase 4c probes match. The CDP surface is now live.

### Phase 5 — Forced run via API
```bash
curl -X POST \
  -H "CF-Access-Client-Id: $CF_ID" \
  -H "CF-Access-Client-Secret: $CF_SECRET" \
  -H "Authorization: Bearer $API_RUN_TOKEN" \
  https://api.autocli.<your-zone>/api/run
sleep 240   # max single-attempt budget; retries follow §5.2 schedule (15s, 60s, 240s)

# /api/status is Bearer-protected — same token as /api/run
curl -s -H "CF-Access-Client-Id: $CF_ID" -H "CF-Access-Client-Secret: $CF_SECRET" \
     -H "Authorization: Bearer $API_RUN_TOKEN" \
     https://api.autocli.<your-zone>/api/status | jq
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
| Supabase rate limit / 429 | `run-daily.sh` exits non-zero | Apply the §5.2 unified policy — 3 attempts at 15 s / 60 s / 240 s. On the 4th failure: record in `last_run.json`, increment `autocli_daily_runs_total{result="failure"}`, wait for the next cron tick. |
| supercronic drift (>25 h since last run) | Grafana "time since last run" panel red | `docker compose restart autocli-daily` |

---

## 8. Out of Scope (Explicit)

| Item | Reason / Future plan |
|---|---|
| Multiple LinkedIn accounts | One profile per chrome container; future PR can horizontally scale |
| Loki / log aggregation | Stick to `docker logs` + the `/api/logs` endpoint for now; revisit when a second service joins |
| Alertmanager / Slack-Discord webhooks | Grafana panels + email-on-error from a future PR |
| Indeed adapter into the same cron | Land Indeed PR first, then add a single cron line |
| HTTPS certificates on origin | Cloudflare Tunnel egress already terminates HTTPS |
| Backup of `chrome-profile` volume | Documented but not implemented in this phase |
| Multi-region failover | Single-host design; future concern |

---

## 9. Risks & Open Items

1. **CDP public exposure.** Cloudflare Access *must* be configured before bringing the CDP surface up. The implementation will refuse to add the `cdp.autocli.<your-zone>` hostname to the Cloudflare Tunnel dashboard until every probe in §7 **Phase 4a** passes for the other three subdomains (`vnc`, `api`, `grafana`) AND the operator has confirmed the Access Application for `cdp.autocli` exists with both policies described in §5.3 (dedicated short-lived Service Token + mTLS for machines; operator email + required WARP posture for humans). Only then does **Phase 4b** add the ingress, followed by **Phase 4c** probes.
2. **LinkedIn cookie lifetime.** Empirically 30-90 days. When it expires, `last_exit_code` becomes non-zero with a recognisable error string. Operator action: open `/vnc/` → re-login. No code change needed.
3. **Skyvern decommission.** The operator authorised stopping `skyvern-skyvern-{1,ui-1}`. Their data volumes are not deleted by this design — only the running containers. Skyvern can be re-enabled later by `docker compose up` from its own compose file if needed.
4. **`<your-zone>`.** Spec leaves the apex hostname as a placeholder; the operator must provide it (and verify it is a Cloudflare-managed zone) before Phase 3. The 4 subdomains are `{vnc,cdp,api,grafana}.autocli.<your-zone>`. `/jobs` rides on `api.autocli`, not its own subdomain.
5. **`API_RUN_TOKEN` rotation.** Generated at first deploy and stored only on the server. Rotation requires editing `.env` and `docker compose restart autocli-daily`.

---

## 10. Glossary

| Term | Meaning |
|---|---|
| **Stagehand image** | The operator's locally-built `my-stagehand-app-chrome` image — Chromium + Xvfb + x11vnc + noVNC + socat in a single container. Renamed to `autocli-chrome` in this design. |
| **Pull-based deploy** | CI pushes new image tags to GHCR; Watchtower on the server polls every 5 min and recreates containers labelled `com.centurylinklabs.watchtower.enable=true`. |
| **Cloudflare Access** | Identity gate in front of a Cloudflare Tunnel — verifies the caller before passing traffic to the origin. |
| **CDP** | Chrome DevTools Protocol — JSON-over-WebSocket API to control Chromium. |
