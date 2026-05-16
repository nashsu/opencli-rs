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
