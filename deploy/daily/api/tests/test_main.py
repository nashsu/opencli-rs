"""Auth + route shape tests. Run via:
    cd deploy/daily/api && uv run --group dev pytest -v
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
    # Force fresh import to pick up env; unregister prometheus metrics first
    # to avoid "Duplicated timeseries" on repeated test-fixture setup.
    import sys
    from prometheus_client import REGISTRY

    # Unregister all collectors before dropping the module so the fresh
    # import can re-register them without hitting duplicate errors.
    collectors = list(REGISTRY._names_to_collectors.values())
    for c in set(collectors):
        try:
            REGISTRY.unregister(c)
        except Exception:
            pass
    sys.modules.pop("main", None)

    import main as m
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
