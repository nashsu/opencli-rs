"""Pytest configuration: ensure deploy/daily/api is on sys.path so
'import main' and 'import trigger' work without package context,
matching the uvicorn invocation in entrypoint.sh (cd /app/api)."""
from __future__ import annotations

import sys
from pathlib import Path

# Add the api directory so 'import main' / 'import trigger' work flat.
_api_dir = str(Path(__file__).parent.parent)
if _api_dir not in sys.path:
    sys.path.insert(0, _api_dir)
