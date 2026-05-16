# AutoCLI Daily Microservice — Design

| | |
|---|---|
| **Date** | 2026-05-16 |
| **Branch** | `feat/daily-microservice` (separate worktree, branched from `main`) |
| **Target host** | `100.108.80.9` (Tailscale, Ubuntu 24.04, Docker 29.4) |
| **Public endpoint** | 5 subdomains under `<your-zone>` (e.g. `vnc.autocli.<your-zone>`, `cdp.autocli.<your-zone>`, `api.autocli.<your-zone>`, `jobs.autocli.<your-zone>`, `grafana.autocli.<your-zone>`) via Cloudflare Tunnel `--token` mode |
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
                  api.autocli.<your-zone>     → daily:8080
                  jobs.autocli.<your-zone>    → daily:8080
                  grafana.autocli.<your-zone> → grafana:3000
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
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/ricksanchez88e/autocli-chrome
          tags: |
            type=raw,value=main,enable=${{ env.IS_MAIN }}
            type=raw,value=branch-${{ github.ref_name }},enable=${{ env.IS_MAIN == 'false' }}
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
          tags: |
            type=raw,value=main,enable=${{ env.IS_MAIN }}
            type=raw,value=branch-${{ github.ref_name }},enable=${{ env.IS_MAIN == 'false' }}
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
  │     reads http://autocli-chrome:9222/json/list  (creates a tab via /json/new if empty)
  │     extracts webSocketDebuggerUrl, rewrites host (localhost → autocli-chrome:9222)
  │     writes the resulting ws:// URL to /run/cdp-endpoint.env  →  AUTOCLI_CDP_ENDPOINT
  │
  ├─ supercronic /etc/cron.d/autocli         (TZ=Europe/London; starts only after cdp-discover.sh exits 0)
  │     └─ "0 3 * * * /app/run-daily.sh"
  │           └─ source /run/cdp-endpoint.env    # rediscover if Chrome restarted
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
        ├─ GET  /api/metrics  [open]     Prometheus exposition; only reachable via docker network
        ├─ GET  /api/health   [open]     200 iff chrome:9222 reachable AND /run/cdp-endpoint.env exists
        └─ GET  /jobs?since=… [Bearer]
              → client = create_client(SUPABASE_URL, SUPABASE_ANON_KEY)
              → client.schema("jobs").table("jobs").select(...).gte("post_time", since).execute()
              (matches scripts/backfill_priority_scores.py — schema-qualified table API)

        [Bearer] = Authorization: Bearer ${API_RUN_TOKEN} required at the FastAPI layer; missing/wrong → 401.
        [open]   = no Bearer at the app layer; external requests still hit Cloudflare Access first.
                   Inside the docker network (Prometheus scrape, docker healthcheck) requests bypass both.
```

### 5.2 Invariants

- **CDP page target, not browser endpoint**: `/json/version` returns a browser-level WebSocket that does not accept page-scoped commands. `cdp-discover.sh` therefore hits `/json/list`, picks the first `type:"page"` target, and if none exists `POST /json/new` to create one. Only `webSocketDebuggerUrl` from that page target is exported as `AUTOCLI_CDP_ENDPOINT`.
- **Host rewrite**: the Stagehand image binds Chromium to `127.0.0.1:9223` (socat exposes 9222 publicly), so `/json/list` returns URLs like `ws://localhost:9223/devtools/page/<id>`. `cdp-discover.sh` rewrites the host:port portion to `autocli-chrome:9222` (the docker-service-name + the externally-mapped port) before exporting. Confirmed against `~/Documents/Github/my-stagehand-app/scripts/entrypoint-vnc.sh`.
- **Boot ordering**: `entrypoint.sh` runs `cdp-discover.sh` synchronously first (retry every 2 s, give up at 60 s and exit non-zero). `restart: unless-stopped` on the `autocli-daily` service then makes docker recreate the container until Chrome is reachable. Only after that does supercronic launch and uvicorn bind `:8080`.
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
| `api.autocli.<your-zone>` | `http://autocli-daily:8080` | FastAPI under `/api/*` |
| `jobs.autocli.<your-zone>` | `http://autocli-daily:8080` | Same backend, `/jobs` route only |
| `grafana.autocli.<your-zone>` | `http://grafana:3000` | Subdomain → no `serve_from_sub_path` needed |

These five hostnames are configured in the dashboard under the same Tunnel. Implementation produces a screenshot/checklist for the operator to apply.

**Cloudflare Access — one Application per subdomain, two policies inside each.** Within a single Application multiple policies are evaluated as **OR** — a request matching any one policy is admitted. This lets us serve both humans and scripts on the same surface:

| Subdomain | Policy A (machines) | Policy B (humans) |
|---|---|---|
| `cdp.autocli` | Service Token *bound to operator's account* — **and additionally** restricted to a Tailscale-CGNAT IP range via Access network selector | Operator email + WARP device posture (browser only) |
| `vnc.autocli` | — (humans only) | Operator email OTP |
| `api.autocli` | Service Token (used by `curl` / scripts) | Operator email OTP |
| `jobs.autocli` | Service Token | Operator email OTP |
| `grafana.autocli` | — (humans only) | Operator email OTP |

`cdp.autocli` is the only surface with the **extra** IP-range constraint inside Policy A — the CDP socket is the equivalent of a remote shell on the browser, so we want even the Service Token to be exercised only from the operator's known networks.

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
| `API_RUN_TOKEN` | `autocli-daily` | Generated at deploy (`openssl rand -hex 32`) | **Enforced** by FastAPI: `POST /api/run` and `GET /api/logs` require `Authorization: Bearer ${API_RUN_TOKEN}`; missing/wrong → 401. Defense-in-depth in case Cloudflare Access ever fails open. |
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
# scp docker-compose.yml + .env (with secrets) here — no cloudflared config file in token mode
cd ~/autocli-stack
docker compose pull
docker compose up -d
```
✅ `docker ps` shows 6 new containers healthy. Existing `job-*`, `sub2api*` untouched.

### Phase 4 — Tunnel & Access reachable (machine-verifiable gate before opening `cdp.*` ingress)

Each subdomain has two probes — one unauthenticated (must hit Access) and one authenticated (must pass through). All probes are runnable from the operator's laptop.

```bash
DOMAIN="<your-zone>"
CF_ID="<service-token-client-id>"          # from Cloudflare Access → Service Tokens
CF_SECRET="<service-token-client-secret>"
TOKEN="<API_RUN_TOKEN from server .env>"

# 1. Unauthenticated → expect 302 to Cloudflare Access login (never 200, never 502)
for sub in vnc cdp api jobs grafana; do
  code=$(curl -s -o /dev/null -w "%{http_code}" "https://${sub}.autocli.${DOMAIN}/")
  echo "${sub} unauth: ${code}"  # MUST be 302
done

# 2. Authenticated with Service Token → expect 200/101 from origin
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://api.autocli.${DOMAIN}/api/health" | head -1     # HTTP/2 200
curl -sI -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://cdp.autocli.${DOMAIN}/json/list" | head -1      # HTTP/2 200
curl -s  -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://api.autocli.${DOMAIN}/api/metrics" | grep -c autocli_daily_   # ≥ 5

# 3. API_RUN_TOKEN enforcement (independent of Cloudflare)
curl -sI -X POST -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     "https://api.autocli.${DOMAIN}/api/run" | head -1        # HTTP/2 401 (no Bearer)
curl -sI -X POST -H "CF-Access-Client-Id: ${CF_ID}" -H "CF-Access-Client-Secret: ${CF_SECRET}" \
     -H "Authorization: Bearer ${TOKEN}" \
     "https://api.autocli.${DOMAIN}/api/run" | head -1        # HTTP/2 202
```

✅ All probes match expected codes. **The `cdp.autocli` ingress is not added to the Cloudflare dashboard until probes 1 and 2 succeed for the other four subdomains** — this is the machine gate required by §9 risk #1.

### Phase 5 — Forced run via API
```bash
curl -X POST \
  -H "CF-Access-Client-Id: $CF_ID" \
  -H "CF-Access-Client-Secret: $CF_SECRET" \
  -H "Authorization: Bearer $API_RUN_TOKEN" \
  https://api.autocli.<your-zone>/api/run
sleep 240   # max single-attempt budget; retries follow §5.2 schedule (15s, 60s, 240s)
curl -s -H "CF-Access-Client-Id: $CF_ID" -H "CF-Access-Client-Secret: $CF_SECRET" \
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

1. **CDP public exposure.** Cloudflare Access *must* be configured before bringing Phase 4 traffic up. The implementation will refuse to add the `cdp.autocli.<your-zone>` hostname to the Cloudflare Tunnel dashboard until all probes in §7 Phase 4 step 1+2 succeed for the other four subdomains AND the operator has confirmed the Access Application with the Service-Token-bound-to-account + IP-range policy is published.
2. **LinkedIn cookie lifetime.** Empirically 30-90 days. When it expires, `last_exit_code` becomes non-zero with a recognisable error string. Operator action: open `/vnc/` → re-login. No code change needed.
3. **Skyvern decommission.** The operator authorised stopping `skyvern-skyvern-{1,ui-1}`. Their data volumes are not deleted by this design — only the running containers. Skyvern can be re-enabled later by `docker compose up` from its own compose file if needed.
4. **`<your-zone>`.** Spec leaves the apex hostname as a placeholder; the operator must provide it (and verify it is a Cloudflare-managed zone) before Phase 3. The 5 subdomains are `{vnc,cdp,api,jobs,grafana}.autocli.<your-zone>`.
5. **`API_RUN_TOKEN` rotation.** Generated at first deploy and stored only on the server. Rotation requires editing `.env` and `docker compose restart autocli-daily`.

---

## 10. Glossary

| Term | Meaning |
|---|---|
| **Stagehand image** | The operator's locally-built `my-stagehand-app-chrome` image — Chromium + Xvfb + x11vnc + noVNC + socat in a single container. Renamed to `autocli-chrome` in this design. |
| **Pull-based deploy** | CI pushes new image tags to GHCR; Watchtower on the server polls every 5 min and recreates containers labelled `com.centurylinklabs.watchtower.enable=true`. |
| **Cloudflare Access** | Identity gate in front of a Cloudflare Tunnel — verifies the caller before passing traffic to the origin. |
| **CDP** | Chrome DevTools Protocol — JSON-over-WebSocket API to control Chromium. |
