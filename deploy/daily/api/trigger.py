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
