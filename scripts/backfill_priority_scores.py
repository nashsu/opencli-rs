#!/usr/bin/env python3
"""Backfill priority scores for existing jobs in Supabase.

Batch-scoring is intentionally *** one-time *** for already-ingested rows.
Ongoing scoring happens in the sync pipeline (--enable-scoring, the default).

Usage:
  python scripts/backfill_priority_scores.py                     # backfill all unscored
  python scripts/backfill_priority_scores.py --force             # re-score *all* (even already scored)
  python scripts/backfill_priority_scores.py --limit 100         # cap rows processed
  python scripts/backfill_priority_scores.py --dry-run           # report only, no writes
  python scripts/backfill_priority_scores.py --env-file .env     # explicit .env path
"""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys
from datetime import date
from typing import Any

# Ensure project root is on sys.path for `scripts.*` imports
_project_root = str(pathlib.Path(__file__).resolve().parent.parent)
if _project_root not in sys.path:
    sys.path.insert(0, _project_root)

from scripts.job_priority_scorer import SCORER_VERSION, score_job


def _load_dotenv(path: str | os.PathLike[str]) -> None:
    p = pathlib.Path(path)
    if not p.is_file():
        return
    for raw_line in p.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip().strip("'").strip('"')
        if not key or key in os.environ:
            continue
        os.environ[key] = value


def _auto_load_env() -> None:
    _load_dotenv(pathlib.Path.cwd() / ".env")
    _load_dotenv(pathlib.Path(__file__).resolve().parent.parent / ".env")


def _create_supabase_client(url: str | None, key: str | None):
    try:
        _path_clean = [p for p in sys.path if p not in ("", ".")]
        _path_dirty = [p for p in sys.path if p in ("", ".")]
        sys.path = _path_clean + _path_dirty
        from supabase import create_client
    except Exception as exc:
        raise RuntimeError(
            "Missing Python dependency 'supabase'. Install deps with:\n"
            "  uv pip install -r scripts/requirements.txt"
        ) from exc

    url = url or os.environ.get("SUPABASE_URL", "")
    key = key or os.environ.get("SUPABASE_SERVICE_ROLE_KEY") or os.environ.get("SUPABASE_KEY", "")
    if not url or not key:
        raise ValueError(
            "Missing Supabase credentials. Set SUPABASE_URL and either "
            "SUPABASE_SERVICE_ROLE_KEY (preferred) or SUPABASE_KEY."
        )
    return create_client(url, key)


def _reconstruct_job_data(row: dict[str, Any]) -> dict[str, Any]:
    """Build a job_data dict suitable for score_job() from a DB row.

    Prefers raw_record fields (which preserve the original shape) and falls
    back to the extracted column values.
    """
    raw = row.get("raw_record")
    if isinstance(raw, dict) and raw:
        # Use raw_record as the primary source, filling in missing fields from columns
        job_data = dict(raw)
        # Ensure critical fields exist
        for col_key, raw_key in [
            ("job_title", "job_title"),
            ("company_name", "company_name"),
            ("location", "location"),
            ("salary", "salary"),
            ("post_time", "post_time"),
            ("apply_url", "apply_url"),
            ("external_url", "external_url"),
            ("job_description", "job_description"),
        ]:
            if col_key not in job_data or not job_data.get(col_key):
                job_data[col_key] = row.get(col_key) or ""
        return job_data

    # No raw_record -- reconstruct from columns
    return {
        "job_title": row.get("job_title") or "",
        "company_name": row.get("company_name") or "",
        "location": row.get("location") or "",
        "salary": row.get("salary") or "",
        "post_time": row.get("post_time") or "",
        "apply_url": row.get("apply_url") or "",
        "external_url": row.get("external_url") or "",
        "job_description": row.get("job_description") or "",
        "apply_type": row.get("apply_type") or "",
        "source_channel": row.get("source_channel") or "",
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Backfill priority scores for existing Supabase jobs."
    )
    parser.add_argument("--limit", type=int, default=0, help="Cap rows processed (0 = no limit).")
    parser.add_argument("--force", action="store_true",
                        help="Re-score even already-scored jobs.")
    parser.add_argument("--dry-run", action="store_true",
                        help="Report only, do not write to DB.")
    parser.add_argument("--supabase-url", dest="supabase_url", help="Override SUPABASE_URL.")
    parser.add_argument("--supabase-key", dest="supabase_key", help="Override Supabase key.")
    parser.add_argument("--env-file", help="Optional path to a .env file.")
    args = parser.parse_args(argv)

    _auto_load_env()
    if args.env_file:
        _load_dotenv(args.env_file)

    # ── Build query ──────────────────────────────────────────────────────
    client = _create_supabase_client(args.supabase_url, args.supabase_key)

    query = client.table("jobs.jobs").select(
        "id, source, raw_record, "
        "job_title, company_name, location, salary, post_time, "
        "apply_url, external_url, job_description, "
        "apply_type, source_channel, "
        "priority_score, priority_version"
    )

    if not args.force:
        # Only rows that have never been scored or whose version is stale
        query = query.or_(
            f"priority_score.is.null,priority_version.neq.{SCORER_VERSION}"
        )
    else:
        # Re-score everything (order so newer-first is optional but nice)
        query = query.order("created_at", desc=True)

    if args.limit and args.limit > 0:
        query = query.limit(args.limit)

    try:
        resp = query.execute()
    except Exception as exc:
        print(f"ERROR: query failed: {exc}", file=sys.stderr)
        return 2

    rows = resp.data if isinstance(resp.data, list) else [resp.data] if resp.data else []

    if not rows:
        print(json.dumps({"rows_fetched": 0, "message": "No unscored jobs found."}))
        return 0

    # ── Score each row ───────────────────────────────────────────────────
    results: list[dict[str, Any]] = []
    for idx, row in enumerate(rows):
        job_id = str(row.get("id", ""))
        if not job_id:
            print(f"WARN: skipping row {idx}: missing id", file=sys.stderr)
            continue

        try:
            job_data = _reconstruct_job_data(row)
            result = score_job(job_data)
        except Exception as exc:
            print(f"WARN: scoring failed for job {job_id}: {exc}", file=sys.stderr)
            results.append({
                "job_id": job_id,
                "status": "error",
                "error": str(exc),
            })
            continue

        results.append({
            "job_id": job_id,
            "score": result.score,
            "tier": result.tier,
            "version": result.version,
        })

    # ── Apply ────────────────────────────────────────────────────────────
    if args.dry_run:
        report: dict[str, Any] = {
            "mode": "dry-run",
            "rows_fetched": len(rows),
            "rows_scored": len([r for r in results if "score" in r]),
            "rows_errored": len([r for r in results if "error" in r]),
            "scorer_version": SCORER_VERSION,
        }
        if results:
            scores = [r["score"] for r in results if "score" in r]
            tiers = [r["tier"] for r in results if "tier" in r]
            if scores:
                report["priority_scores"] = {
                    "min": round(min(scores), 1),
                    "max": round(max(scores), 1),
                    "avg": round(sum(scores) / len(scores), 1),
                }
            if tiers:
                tier_counts: dict[str, int] = {}
                for t in tiers:
                    tier_counts[t] = tier_counts.get(t, 0) + 1
                report["priority_tier_distribution"] = tier_counts
        print(json.dumps(report, indent=2, ensure_ascii=False))
        return 0

    # ── Write back ───────────────────────────────────────────────────────
    updated = 0
    for r in results:
        if "score" not in r:
            continue  # errored row, skip
        try:
            client.rpc("update_job_priority_score", {
                "p_job_id": r["job_id"],
                "p_priority_score": r["score"],
                "p_priority_tier": r["tier"],
                "p_priority_version": r["version"],
                "p_priority_signals": {},
            }).execute()
            updated += 1
        except Exception as exc:
            print(
                f"WARN: update failed for job {r['job_id']}: {exc}",
                file=sys.stderr,
            )

    print(
        json.dumps(
            {
                "mode": "live",
                "rows_fetched": len(rows),
                "rows_scored": len(results),
                "rows_updated": updated,
                "rows_errored": len([r for r in results if "error" in r]),
                "scorer_version": SCORER_VERSION,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
