#!/usr/bin/env python3
"""JD Structured Extraction Pipeline

Reads raw JDs from output/final.json, preprocesses them, sends to local LLM
for structured JSON extraction, and stores results in Supabase.

Usage:
    python scripts/jd_pipeline.py [--input output/final.json] [--dry-run] [--limit N]
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import logging
import os
import signal
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any

from jsonschema import ValidationError, validate

# ---------------------------------------------------------------------------
# Ensure scripts/ is on sys.path so sibling modules are importable
# ---------------------------------------------------------------------------
_scripts_dir = str(Path(__file__).resolve().parent)
if _scripts_dir not in sys.path:
    sys.path.insert(0, _scripts_dir)

from jd_pipeline_config import (  # noqa: E402
    EXTRACTOR,
    EXTRACTOR_VERSION,
    INPUT_FILE,
    JD_SCHEMA,
    LLM_BASE_URL,
    LLM_MODEL,
    PROMPT_VERSION,
    SCHEMA_VERSION,
)
from jd_pipeline_db import DatabaseClient, DatabaseError  # noqa: E402
from jd_pipeline_llm import LLMClient, LLMError  # noqa: E402
from jd_pipeline_preprocess import compute_hash, preprocess, validate_input_row  # noqa: E402

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def url_hash(url: str) -> str:
    """Compute full SHA-256 hex digest of *url*.

    Matches the existing convention in the ``jobs`` table where ``url_hash``
    is the complete 64-character hex digest (NOT truncated).
    """
    return hashlib.sha256(url.encode("utf-8")).hexdigest()


def _should_skip(
    row: dict[str, Any],
    current_raw_hash: str,
    current_cleaned_hash: str,
) -> bool:
    """Return True if *row* (from ``get_existing_jobs``) is already up-to-date.

    A job should be *re-processed* if ANY of these is true:
      - jd_structured_status IS NULL
      - jd_structured_status IN ('pending', 'failed')
      - jd_structured_extractor_version differs from current
      - jd_structured_schema_version differs from current
      - jd_structured_prompt_version differs from current
      - jd_structured_raw_hash differs from current
      - jd_structured_cleaned_hash differs from current

    This mirrors the SQL guard conditions in ``claim_job()``.
    """
    status = row.get("jd_structured_status")

    # Never processed before -> needs processing
    if status is None:
        return False

    # Pending or previously failed -> needs processing
    if status in ("pending", "failed"):
        return False

    # Already ok -> check versions and hashes
    if status == "ok":
        if row.get("jd_structured_extractor_version") != EXTRACTOR_VERSION:
            return False
        if row.get("jd_structured_schema_version") != SCHEMA_VERSION:
            return False
        if row.get("jd_structured_prompt_version") != PROMPT_VERSION:
            return False
        if row.get("jd_structured_raw_hash") != current_raw_hash:
            return False
        if row.get("jd_structured_cleaned_hash") != current_cleaned_hash:
            return False
        return True  # all versions/hashes match -> skip

    # Any other status (processing, dead_letter, etc.) -> needs processing
    return False


# ---------------------------------------------------------------------------
# Pipeline stats
# ---------------------------------------------------------------------------


class _JobResult:
    __slots__ = ("url", "status", "stage", "error_class", "error_message")

    def __init__(
        self,
        url: str,
        status: str,
        stage: str | None = None,
        error_class: str | None = None,
        error_message: str | None = None,
    ) -> None:
        self.url = url
        self.status = status  # "ok" | "failed" | "skipped"
        self.stage = stage
        self.error_class = error_class
        self.error_message = error_message


class PipelineStats:
    """Accumulate counts and per-job results for a pipeline run."""

    __slots__ = ("total", "success", "failed", "skipped", "_jobs")

    def __init__(self) -> None:
        self.total: int = 0
        self.success: int = 0
        self.failed: int = 0
        self.skipped: int = 0
        self._jobs: list[_JobResult] = []

    def record_ok(self, url: str) -> None:
        self.success += 1
        self._jobs.append(_JobResult(url, "ok"))

    def record_failed(
        self,
        url: str,
        stage: str,
        error_class: str,
        error_message: str,
    ) -> None:
        self.failed += 1
        self._jobs.append(
            _JobResult(url, "failed", stage, error_class, error_message)
        )

    def record_skipped(self, url: str) -> None:
        self.skipped += 1
        self._jobs.append(_JobResult(url, "skipped"))

    def run_report(
        self,
        run_id: str,
        avg_latency_ms: float | None = None,
        p95_latency_ms: float | None = None,
        reaped: int = 0,
    ) -> str:
        avg_str = f"{avg_latency_ms:.0f} ms" if avg_latency_ms is not None else "N/A"
        p95_str = f"{p95_latency_ms:.0f} ms" if p95_latency_ms is not None else "N/A"
        success_rate = (
            f"{self.success / (self.success + self.failed) * 100:.1f}%"
            if (self.success + self.failed) > 0
            else "N/A"
        )

        lines = [
            "=" * 60,
            f"  JD Pipeline Run: {run_id}",
            "=" * 60,
            "",
            "  Summary",
            "  -------",
            f"  Total input:     {self.total}",
            f"  Success:         {self.success}  ({success_rate} of processed)",
            f"  Failed:          {self.failed}",
            f"  Skipped:         {self.skipped}",
            f"  Stale reaped:    {reaped}",
            f"  Avg latency:     {avg_str}",
            f"  P95 latency:     {p95_str}",
        ]

        failed_jobs = [j for j in self._jobs if j.status == "failed"]
        if failed_jobs:
            lines += [
                "",
                "  Failed Jobs Detail",
                "  ------------------",
            ]
            for i, j in enumerate(failed_jobs, 1):
                msg = j.error_message or "N/A"
                if len(msg) > 120:
                    msg = msg[:117] + "..."
                lines.append(f"  [{i}] {j.url}")
                lines.append(f"      stage: {j.stage}  error: {j.error_class}")
                lines.append(f"      {msg}")

        lines += ["", "=" * 60]
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# Main pipeline
# ---------------------------------------------------------------------------


async def main() -> None:
    parser = argparse.ArgumentParser(
        description="JD Structured Extraction Pipeline"
    )
    parser.add_argument(
        "--input",
        default=INPUT_FILE,
        help=f"Path to input JSON file (default: {INPUT_FILE})",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Skip all database writes (preprocess + LLM calls still run)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=0,
        help="Process only N items (0 = all)",
    )
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    # Also log to file
    log_dir = Path(__file__).resolve().parent.parent / "output"
    log_dir.mkdir(exist_ok=True)
    fh = logging.FileHandler(log_dir / "jd_pipeline.log")
    fh.setLevel(logging.DEBUG)
    fh.setFormatter(logging.Formatter("%(asctime)s [%(levelname)s] %(name)s: %(message)s"))
    logging.getLogger().addHandler(fh)

    # ------------------------------------------------------------------
    # Generate run_id
    # ------------------------------------------------------------------
    run_id = f"jd-extract-{datetime.now().strftime('%Y%m%d-%H%M%S')}"
    logger.info("Run ID: %s", run_id)

    stats = PipelineStats()
    stats_reaped = 0
    cancel_requested = False

    def _signal_handler(sig: int, frame: Any) -> None:
        nonlocal cancel_requested
        if cancel_requested:
            logger.warning("Second interrupt -- exiting immediately.")
            sys.exit(1)
        cancel_requested = True
        logger.warning(
            "Interrupt received, finishing current batch then exiting..."
        )

    signal.signal(signal.SIGINT, _signal_handler)

    # ------------------------------------------------------------------
    # Initialise components
    # ------------------------------------------------------------------
    db: DatabaseClient | None = None
    llm: LLMClient | None = None

    # Load .env from scripts/ directory if present (before any DB init)
    env_path = Path(__file__).resolve().parent / ".env"
    if env_path.exists():
        for line in env_path.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, _, value = line.partition("=")
                os.environ.setdefault(key.strip(), value.strip())
    # Refresh config from environment (config may have been imported before .env loaded)
    import jd_pipeline_config as _cfg
    _cfg.SUPABASE_URL = os.environ.get("SUPABASE_URL", _cfg.SUPABASE_URL)
    _cfg.SUPABASE_KEY = os.environ.get("SUPABASE_KEY", _cfg.SUPABASE_KEY)

    if not args.dry_run:
        try:
            db = DatabaseClient()
        except ValueError as exc:
            logger.error("Database init failed: %s", exc)
            logger.error("Set SUPABASE_URL and SUPABASE_KEY env vars or create scripts/.env")
            sys.exit(1)

    llm = LLMClient(base_url=LLM_BASE_URL, model=LLM_MODEL)

    try:
        # --------------------------------------------------------------
        # 1. Reap stale processing rows
        # --------------------------------------------------------------
        if db:
            reaped = db.reap_stale_processing()
            stats_reaped = reaped
            logger.info("Reaped %d stale processing row(s).", reaped)

        # --------------------------------------------------------------
        # 2. Load and validate input
        # --------------------------------------------------------------
        input_path = Path(args.input)
        if not input_path.exists():
            logger.error("Input file not found: %s", input_path)
            sys.exit(1)

        with open(input_path, "r", encoding="utf-8") as f:
            raw_items: list[dict] = json.load(f)

        if args.limit > 0:
            raw_items = raw_items[: args.limit]

        logger.info("Loaded %d items from %s", len(raw_items), input_path)

        valid_items: list[dict] = []
        for idx, row in enumerate(raw_items):
            errors = validate_input_row(row)
            if errors:
                logger.warning("Row %d skipped: %s", idx, "; ".join(errors))
                continue
            valid_items.append(row)

        logger.info(
            "Validated: %d / %d items passed input checks.",
            len(valid_items),
            len(raw_items),
        )

        # --------------------------------------------------------------
        # 3. Preprocess: compute hashes and cleaned text
        # --------------------------------------------------------------
        processed: list[dict[str, Any]] = []
        for row in valid_items:
            jd_raw_text: str = row["jd"]
            jd_cleaned, cleaned_hash = preprocess(jd_raw_text)
            raw_hash = compute_hash(jd_raw_text)
            uh = url_hash(row["url"])

            processed.append(
                {
                    "url": row["url"],
                    "url_hash": uh,
                    "source": "linkedin",
                    "title": row.get("title", ""),
                    "company": row.get("company", ""),
                    "jd_raw": jd_raw_text,
                    "jd_cleaned": jd_cleaned,
                    "raw_hash": raw_hash,
                    "cleaned_hash": cleaned_hash,
                }
            )

        stats.total = len(processed)

        # --------------------------------------------------------------
        # 4. Tokenize stats (for context-size tier selection)
        # --------------------------------------------------------------
        try:
            token_stats = await llm.tokenize_stats(
                [p["jd_cleaned"] for p in processed]
            )
            logger.info(
                "Token stats: p50=%d p90=%d p95=%d max=%d count=%d",
                token_stats["p50"],
                token_stats["p90"],
                token_stats["p95"],
                token_stats["max"],
                token_stats["count"],
            )
        except LLMError as exc:
            logger.warning("Tokenize stats failed: %s (continuing)", exc)

        # --------------------------------------------------------------
        # 5. Skip policy: check which jobs are already up-to-date
        # --------------------------------------------------------------
        if db:
            all_url_hashes = [p["url_hash"] for p in processed]
            # get_existing_jobs has Supabase IN clause limits,
            # so we batch in chunks of 500.
            existing: dict[str, dict[str, Any]] = {}
            chunk_size = 500
            for i in range(0, len(all_url_hashes), chunk_size):
                chunk = all_url_hashes[i : i + chunk_size]
                chunk_result = db.get_existing_jobs(chunk)
                existing.update(chunk_result)

            logger.info(
                "Found %d existing job(s) in database.", len(existing)
            )
        else:
            existing = {}

        to_process: list[dict[str, Any]] = []
        for p in processed:
            row = existing.get(p["url_hash"])
            if row and _should_skip(row, p["raw_hash"], p["cleaned_hash"]):
                stats.record_skipped(p["url"])
                logger.debug("Skipping up-to-date job: %s", p["url"][:80])
                continue
            to_process.append(p)

        logger.info(
            "To process: %d  Skipped (up-to-date): %d",
            len(to_process),
            stats.skipped,
        )

        # --------------------------------------------------------------
        # 5b. Ensure job rows exist in DB (insert new ones)
        # --------------------------------------------------------------
        if db:
            for p in to_process:
                if p["url_hash"] not in existing:
                    db.ensure_job_exists(
                        url=p["url"],
                        url_hash=p["url_hash"],
                        source="linkedin",
                        jd_raw=p["jd_raw"],
                        raw_hash=p["raw_hash"],
                        cleaned_hash=p["cleaned_hash"],
                        company_name=p.get("company", ""),
                        job_title=p.get("title", ""),
                        location=p.get("location"),
                        salary_text=p.get("salary"),
                        work_mode=p.get("workplace_type"),
                    )

        # --------------------------------------------------------------
        # 6. Claim jobs (database-level lock)
        # --------------------------------------------------------------
        if db:
            claimed: list[dict[str, Any]] = []
            for p in to_process:
                if cancel_requested:
                    break
                claim_id = db.claim_job(
                    url_hash=p["url_hash"],
                    run_id=run_id,
                    raw_hash=p["raw_hash"],
                    cleaned_hash=p["cleaned_hash"],
                )
                if claim_id is not None:
                    claimed.append(p)
                else:
                    # Another run claimed it, or it's now up-to-date
                    stats.record_skipped(p["url"])
                    logger.debug(
                        "Claim failed (already claimed/up-to-date): %s",
                        p["url"][:80],
                    )
            to_process = claimed
            logger.info("Claimed %d job(s) for processing.", len(to_process))

        # --------------------------------------------------------------
        # 7. Create extraction run record
        # --------------------------------------------------------------
        if db:
            db.create_extraction_run(
                run_id=run_id,
                input_file=str(input_path),
                total_count=stats.total,
                model=LLM_MODEL,
            )

        # --------------------------------------------------------------
        # 8. Extract: send batches to LLM
        # --------------------------------------------------------------
        if not to_process:
            logger.info("No jobs to extract.")
        else:
            extraction_items = [
                (p["jd_cleaned"], JD_SCHEMA) for p in to_process
            ]

            t_start = time.monotonic()
            results = await llm.extract_batch(extraction_items)
            elapsed = time.monotonic() - t_start

            logger.info(
                "Extraction batch completed in %.1f s (%d items).",
                elapsed,
                len(results),
            )

            # ----------------------------------------------------------
            # 9. Validate and upsert / dead-letter
            # ----------------------------------------------------------
            for idx, result in enumerate(results):
                if cancel_requested:
                    break

                job = to_process[idx]

                if result is not None:
                    # Validate against schema
                    try:
                        validate(instance=result, schema=JD_SCHEMA)
                        # Success
                        stats.record_ok(job["url"])
                        if db:
                            db.upsert_job(
                                url=job["url"],
                                url_hash=job["url_hash"],
                                source=job["source"],
                                jd_raw=job["jd_raw"],
                                jd_structured=result,
                                run_id=run_id,
                                raw_hash=job["raw_hash"],
                                cleaned_hash=job["cleaned_hash"],
                            )
                        logger.info(
                            "[%d/%d] OK: %s",
                            idx + 1,
                            len(to_process),
                            job["url"][:80],
                        )
                    except ValidationError as verr:
                        # Schema validation failed -> dead letter
                        stats.record_failed(
                            url=job["url"],
                            stage="validate",
                            error_class="ValidationError",
                            error_message=str(verr.message),
                        )
                        if db:
                            try:
                                db.mark_dead_letter(
                                    url_hash=job["url_hash"],
                                    run_id=run_id,
                                    url=job["url"],
                                    stage="validate",
                                    error_class="ValidationError",
                                    error_message=str(verr.message),
                                    validation_errors=[
                                        str(p) for p in verr.absolute_path
                                    ],
                                    attempt_count=3,
                                    model=EXTRACTOR,
                                )
                            except Exception as dl_exc:
                                logger.error(
                                    "Failed to write dead_letter for %s: %s",
                                    job["url"][:80], dl_exc,
                                )
                        logger.warning(
                            "[%d/%d] VALIDATION FAILED: %s -- %s",
                            idx + 1,
                            len(to_process),
                            job["url"][:80],
                            verr.message,
                        )
                else:
                    # LLM returned None (all 3 attempts failed)
                    stats.record_failed(
                        url=job["url"],
                        stage="llm_extract",
                        error_class="LLMAllAttemptsFailed",
                        error_message="All 3 LLM extraction attempts returned None.",
                    )
                    if db:
                        try:
                            db.mark_dead_letter(
                                url_hash=job["url_hash"],
                                run_id=run_id,
                                url=job["url"],
                                stage="llm_extract",
                                error_class="LLMAllAttemptsFailed",
                                error_message="All 3 LLM extraction attempts returned None.",
                                attempt_count=3,
                                model=EXTRACTOR,
                            )
                        except Exception as dl_exc:
                            logger.error(
                                "Failed to write dead_letter for %s: %s",
                                job["url"][:80], dl_exc,
                            )
                    logger.warning(
                        "[%d/%d] FAILED: %s",
                        idx + 1,
                        len(to_process),
                        job["url"][:80],
                    )

                # Progress log every 10 jobs
                processed_count = stats.success + stats.failed
                if processed_count % 10 == 0 and processed_count > 0:
                    logger.info(
                        "Progress: %d/%d processed (%d ok, %d failed)",
                        processed_count,
                        len(to_process),
                        stats.success,
                        stats.failed,
                    )

    except Exception as exc:
        logger.exception("Pipeline aborted: %s", exc)
        raise
    finally:
        # ----------------------------------------------------------
        # 10. Finalise extraction run record
        # ----------------------------------------------------------
        # Compute latency metrics from the LLM client's per-request
        # latency records.
        avg_latency_ms: float | None = None
        p95_latency_ms: float | None = None
        if llm and llm._latencies:
            lats = sorted(llm._latencies)
            avg_latency_ms = sum(lats) / len(lats) * 1000
            idx = max(0, int(len(lats) * 0.95) - 1)
            p95_latency_ms = lats[idx] * 1000

        if db:
            try:
                db.update_extraction_run(
                    run_id=run_id,
                    success=stats.success,
                    failed=stats.failed,
                    skipped=stats.skipped,
                    avg_latency_ms=avg_latency_ms,
                    p95_latency_ms=p95_latency_ms,
                )
            except Exception as exc:
                logger.error("Failed to update extraction run: %s", exc)

        # Close LLM client
        if llm:
            await llm.close()

    # ------------------------------------------------------------------
    # 11. Print summary
    # ------------------------------------------------------------------
    report = stats.run_report(
        run_id=run_id,
        avg_latency_ms=avg_latency_ms,
        p95_latency_ms=p95_latency_ms,
        reaped=stats_reaped,
    )
    print(report)
    logger.info("Run report:\n%s", report)

    # Write structured JSON report for programmatic consumption
    report_path = log_dir / f"jd_pipeline_{run_id}.json"
    failed_jobs = [
        {
            "url": j.url,
            "stage": j.stage,
            "error_class": j.error_class,
            "error_message": j.error_message,
        }
        for j in stats._jobs
        if j.status == "failed"
    ]
    report_json = {
        "run_id": run_id,
        "total": stats.total,
        "success": stats.success,
        "failed": stats.failed,
        "skipped": stats.skipped,
        "stale_reaped": stats_reaped,
        "success_rate": (
            f"{stats.success / (stats.success + stats.failed) * 100:.1f}%"
            if (stats.success + stats.failed) > 0
            else "N/A"
        ),
        "avg_latency_ms": round(avg_latency_ms) if avg_latency_ms else None,
        "p95_latency_ms": round(p95_latency_ms) if p95_latency_ms else None,
        "failed_jobs": failed_jobs,
    }
    report_path.write_text(json.dumps(report_json, indent=2, ensure_ascii=False))
    logger.info("JSON report written to %s", report_path)

    if args.dry_run:
        print("  (DRY RUN -- no database writes)")

    if cancel_requested:
        logger.warning("Pipeline interrupted by user.")
        sys.exit(130)


if __name__ == "__main__":
    asyncio.run(main())