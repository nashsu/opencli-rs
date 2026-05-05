#!/usr/bin/env bash
set -euo pipefail

# Installs Python deps for scripts/ using uv.
# Prereq: uv installed (https://github.com/astral-sh/uv)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_DIR"

if ! command -v uv >/dev/null 2>&1; then
  echo "ERROR: uv not found in PATH."
  echo "Install: https://github.com/astral-sh/uv"
  exit 1
fi

uv venv
source .venv/bin/activate
uv pip install -r scripts/requirements.txt

echo "OK: installed deps into $PROJECT_DIR/.venv"
