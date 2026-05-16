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
ARG SUPERCRONIC_SHA1SUM=9f27ad28c5c57cd133325b2a66bba69ba2235799
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

# Shipped sync script & priority scorer (sync imports the scorer modules at runtime)
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
