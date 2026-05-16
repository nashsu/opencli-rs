"""Supabase database operations for the JD structured extraction pipeline.

Provides :class:`DatabaseClient` with methods for atomic job claiming,
upserting extraction results, dead-letter recording, stale processing
reaping, and extraction-run bookkeeping.

Requires the following RPC functions (defined in migration
``20260502203205_create_jd_pipeline_rpc_functions.sql`` and
``20260502203206_create_mark_dead_letter_rpc.sql``):

* ``claim_job`` -- atomically claim a pending / version-stale row.
* ``upsert_job_structured`` -- upsert extraction result guarded by run_id.
* ``mark_dead_letter`` -- atomically mark a job as dead_letter and insert a
  dead_letter_records row.
* ``reap_stale_processing`` -- reset rows stuck in ``processing``.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from typing import Any

from supabase import Client, create_client

from jd_pipeline_config import (
    EXTRACTOR,
    EXTRACTOR_VERSION,
    PROMPT_VERSION,
    SCHEMA_VERSION,
    STALE_PROCESSING_MINUTES,
)
import jd_pipeline_config as _cfg

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class DatabaseError(Exception):
    """Base error for database operations."""


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------


class DatabaseClient:
    """Supabase client wrapper for JD pipeline operations.

    Parameters
    ----------
    url:
        Supabase project URL (defaults to ``SUPABASE_URL`` env var).
    key:
        Supabase service-role / anon key (defaults to ``SUPABASE_KEY`` env var).
    """

    def __init__(self, url: str | None = None, key: str | None = None) -> None:
        url = url or _cfg.SUPABASE_URL
        key = key or _cfg.SUPABASE_KEY
        if not url or not key:
            raise ValueError(
                "SUPABASE_URL and SUPABASE_KEY must be set either via "
                "constructor arguments or environment variables."
            )
        self._client: Client = create_client(url, key)

    # ------------------------------------------------------------------
    # claim_job
    # ------------------------------------------------------------------

    def claim_job(
        self,
        url_hash: str,
        run_id: str,
        raw_hash: str,
        cleaned_hash: str,
    ) -> int | None:
        """Atomically claim a job for processing.

        Calls the ``claim_job`` RPC which performs::

            UPDATE jobs SET
              jd_structured_status = 'processing',
              processing_run_id = p_run_id,
              processing_started_at = now()
            WHERE url_hash = p_url_hash
              AND (   jd_structured_status IS NULL
                   OR jd_structured_status IN ('pending', 'failed')
                   OR jd_structured_extractor_version
                      IS DISTINCT FROM p_extractor_ver
                   OR jd_structured_schema_version
                      IS DISTINCT FROM p_schema_ver
                   OR jd_structured_prompt_version
                      IS DISTINCT FROM p_prompt_ver
                   OR jd_structured_raw_hash IS DISTINCT FROM p_raw_hash
                   OR jd_structured_cleaned_hash IS DISTINCT FROM p_cleaned_hash)
            RETURNING id;

        Only rows that match the guard conditions get updated.  If no row
        matched (already claimed or same version already processed), returns
        ``None``.

        Parameters
        ----------
        url_hash:
            SHA-256 of the job URL.
        run_id:
            Unique run identifier for this pipeline invocation.
        raw_hash:
            SHA-256 of the raw JD text (for staleness detection).
        cleaned_hash:
            SHA-256 of the cleaned JD text (for staleness detection).

        Returns
        -------
        int or None
            The ``jobs.id`` of the claimed row, or ``None``.
        """
        try:
            resp = self._client.rpc(
                "claim_job",
                {
                    "p_url_hash": url_hash,
                    "p_run_id": run_id,
                    "p_extractor_ver": EXTRACTOR_VERSION,
                    "p_schema_ver": SCHEMA_VERSION,
                    "p_prompt_ver": PROMPT_VERSION,
                    "p_raw_hash": raw_hash,
                    "p_cleaned_hash": cleaned_hash,
                },
            ).execute()

            rows = resp.data
            if rows and len(rows) > 0:
                return rows[0].get("id")
            return None
        except Exception as exc:
            raise DatabaseError(f"claim_job failed for {url_hash}: {exc}") from exc

    # ------------------------------------------------------------------
    # ensure_job_exists
    # ------------------------------------------------------------------

    def ensure_job_exists(
        self,
        url: str,
        url_hash: str,
        source: str,
        jd_raw: str,
        raw_hash: str,
        cleaned_hash: str,
        company_name: str = "",
        job_title: str = "",
        location: str | None = None,
        salary_text: str | None = None,
        posted_date: str | None = None,
        work_mode: str | None = None,
    ) -> None:
        """Insert a pending job row if it does not already exist.

        Uses INSERT ... ON CONFLICT DO NOTHING so it's idempotent.
        After this, ``claim_job`` can find and lock the row.

        Parameters
        ----------
        url:
            Original job URL.
        url_hash:
            SHA-256 of the job URL.
        source:
            Source platform name (e.g. ``"linkedin"``).
        jd_raw:
            Original raw JD text.
        raw_hash:
            SHA-256 of ``jd_raw``.
        cleaned_hash:
            SHA-256 of the cleaned text.
        company_name:
            Company name from source data.
        job_title:
            Job title from source data.
        location:
            Job location (optional).
        salary_text:
            Salary text from source data (optional).
        posted_date:
            Posted date ISO string (optional).
        work_mode:
            Work mode e.g. Remote, Hybrid (optional).
        """
        try:
            row = {
                    "url": url,
                    "url_hash": url_hash,
                    "source": source,
                    "company_name": company_name,
                    "job_title": job_title,
                    "jd_raw": jd_raw,
                    "jd_structured_status": "pending",
                    "jd_structured_extractor": EXTRACTOR,
                    "jd_structured_extractor_version": EXTRACTOR_VERSION,
                    "jd_structured_schema_version": SCHEMA_VERSION,
                    "jd_structured_prompt_version": PROMPT_VERSION,
                    "jd_structured_raw_hash": raw_hash,
                    "jd_structured_cleaned_hash": cleaned_hash,
                }
            if location is not None:
                row["location"] = location
            if salary_text is not None:
                row["salary_text"] = salary_text
            if posted_date is not None:
                row["posted_date"] = posted_date
            if work_mode is not None:
                row["work_mode"] = work_mode
            self._client.table("jobs").insert(row).execute()
        except Exception as exc:
            # Unique violation (23505) means the row already exists — that's fine.
            if "23505" in str(exc) or "duplicate" in str(exc).lower():
                return
            raise DatabaseError(f"ensure_job_exists failed for {url_hash}: {exc}") from exc

    # ------------------------------------------------------------------
    # upsert_job
    # ------------------------------------------------------------------

    def upsert_job(
        self,
        url: str,
        url_hash: str,
        source: str,
        jd_raw: str,
        jd_structured: dict,
        run_id: str,
        raw_hash: str,
        cleaned_hash: str,
    ) -> None:
        """Upsert a structured extraction result into the ``jobs`` table.

        Uses the ``upsert_job_structured`` RPC which performs::

            INSERT INTO jobs (...)
            VALUES (...)
            ON CONFLICT (url_hash)
            DO UPDATE SET
              jd_structured = EXCLUDED.jd_structured,
              jd_structured_status = 'ok',
              ...
            WHERE jobs.processing_run_id = p_run_id;

        The ``WHERE jobs.processing_run_id = p_run_id`` guard prevents
        overwriting results from a different (newer) run.

        Parameters
        ----------
        url:
            Original job URL.
        url_hash:
            SHA-256 of the job URL.
        source:
            Source platform name (e.g. ``"linkedin"``).
        jd_raw:
            Original raw JD text (immutable).
        jd_structured:
            The extracted JSON object from the LLM.
        run_id:
            Run identifier for the WHERE guard.
        raw_hash:
            SHA-256 of ``jd_raw``.
        cleaned_hash:
            SHA-256 of the cleaned text.
        """
        try:
            self._client.rpc(
                "upsert_job_structured",
                {
                    "p_url": url,
                    "p_url_hash": url_hash,
                    "p_source": source,
                    "p_jd_raw": jd_raw,
                    "p_jd_structured": json.dumps(jd_structured),
                    "p_jd_structured_status": "ok",
                    "p_jd_structured_extractor": EXTRACTOR,
                    "p_jd_structured_extractor_version": EXTRACTOR_VERSION,
                    "p_jd_structured_schema_version": SCHEMA_VERSION,
                    "p_jd_structured_prompt_version": PROMPT_VERSION,
                    "p_jd_structured_raw_hash": raw_hash,
                    "p_jd_structured_cleaned_hash": cleaned_hash,
                    "p_run_id": run_id,
                },
            ).execute()
        except Exception as exc:
            raise DatabaseError(
                f"upsert_job failed for {url_hash}: {exc}"
            ) from exc

    # ------------------------------------------------------------------
    # mark_dead_letter
    # ------------------------------------------------------------------

    def mark_dead_letter(
        self,
        url_hash: str,
        run_id: str,
        url: str,
        stage: str,
        error_class: str,
        error_message: str,
        raw_response: str | None = None,
        validation_errors: list[str] | None = None,
        attempt_count: int = 0,
        model: str | None = None,
    ) -> None:
        """Mark a job as dead-letter and record the failure details.

        Calls the ``mark_dead_letter`` RPC which atomically::

            1. UPDATE jobs SET
                 jd_structured_status = 'dead_letter',
                 processing_run_id = NULL
               WHERE url_hash = p_url_hash
                 AND processing_run_id = p_run_id
               RETURNING id, source;

            2. INSERT INTO dead_letter_records (
                 source_job_id, source, url, stage, error_class,
                 error_message, raw_response, validation_errors,
                 attempt_count, model, prompt_version, schema_version
               )
               SELECT id, source, p_url, ... FROM updated;

        If the UPDATE matches no rows (e.g. the row was already claimed by a
        newer run), no dead-letter record is inserted either.

        Parameters
        ----------
        url_hash:
            SHA-256 of the job URL.
        run_id:
            Run identifier for the WHERE guard on the UPDATE.
        url:
            Original job URL.
        stage:
            Pipeline stage where the error occurred
            (e.g. ``"preprocess"``, ``"llm_extract"``, ``"validate"``).
        error_class:
            Short error class name (e.g. ``"LLMTimeoutError"``).
        error_message:
            Human-readable error description.
        raw_response:
            Raw text returned by the LLM (if any).
        validation_errors:
            List of schema validation error messages (if applicable).
        attempt_count:
            Which attempt number failed (0, 1, 2, or 3).
        model:
            Extractor name (e.g. ``"qwen3-jd-parser"``).
            Defaults to :data:`EXTRACTOR`.
        """
        try:
            self._client.rpc(
                "mark_dead_letter",
                {
                    "p_url_hash": url_hash,
                    "p_run_id": run_id,
                    "p_url": url,
                    "p_stage": stage,
                    "p_error_class": error_class,
                    "p_error_message": error_message,
                    "p_raw_response": raw_response,
                    "p_validation_errors": (
                        json.dumps(validation_errors)
                        if validation_errors is not None
                        else None
                    ),
                    "p_attempt_count": attempt_count,
                    "p_model": model or EXTRACTOR,
                    "p_prompt_version": PROMPT_VERSION,
                    "p_schema_version": SCHEMA_VERSION,
                },
            ).execute()

        except Exception as exc:
            raise DatabaseError(
                f"mark_dead_letter failed for {url_hash}: {exc}"
            ) from exc

    # ------------------------------------------------------------------
    # reap_stale_processing
    # ------------------------------------------------------------------

    def reap_stale_processing(
        self, stale_minutes: int | None = None
    ) -> int:
        """Reap rows stuck in ``processing`` for longer than the threshold.

        Calls the ``reap_stale_processing`` RPC which resets them to
        ``pending`` so a future run will re-process them.

        Parameters
        ----------
        stale_minutes:
            Staleness threshold in minutes.  Defaults to
            ``STALE_PROCESSING_MINUTES`` (30).

        Returns
        -------
        int
            Number of rows reaped.
        """
        try:
            resp = self._client.rpc(
                "reap_stale_processing",
                {
                    "p_stale_minutes": (
                        stale_minutes or STALE_PROCESSING_MINUTES
                    )
                },
            ).execute()

            rows = resp.data
            if rows and len(rows) > 0:
                return int(rows[0].get("reaped_count", 0))
            return 0
        except Exception as exc:
            raise DatabaseError(f"reap_stale_processing failed: {exc}") from exc

    # ------------------------------------------------------------------
    # extraction runs
    # ------------------------------------------------------------------

    def create_extraction_run(
        self,
        run_id: str,
        input_file: str,
        total_count: int,
        model: str,
        server_params: dict | None = None,
    ) -> None:
        """Create a new extraction run record.

        Parameters
        ----------
        run_id:
            Unique run identifier (e.g. UUID).
        input_file:
            Path or name of the input file processed.
        total_count:
            Total number of jobs in the input.
        model:
            Model name (e.g. ``"qwen3-jd-parser.gguf"``).
        server_params:
            Arbitrary JSON-serialisable server parameters
            (e.g. ``{"context_size": 8192, "n_gpu_layers": 35}``).
        """
        try:
            self._client.table("extraction_runs").insert(
                {
                    "run_id": run_id,
                    "input_file": input_file,
                    "total_count": total_count,
                    "model": model,
                    "server_params": (
                        json.dumps(server_params) if server_params else None
                    ),
                    "prompt_version": PROMPT_VERSION,
                    "schema_version": SCHEMA_VERSION,
                    "extractor_version": EXTRACTOR_VERSION,
                }
            ).execute()
        except Exception as exc:
            raise DatabaseError(
                f"create_extraction_run failed for {run_id}: {exc}"
            ) from exc

    def update_extraction_run(
        self,
        run_id: str,
        success: int,
        failed: int,
        skipped: int,
        avg_latency_ms: float | None = None,
        p95_latency_ms: float | None = None,
        avg_prompt_tokens: int | None = None,
        avg_completion_tokens: int | None = None,
    ) -> None:
        """Finalise an extraction run record with completion stats.

        Sets ``finished_at`` to current time.

        Parameters
        ----------
        run_id:
            Run identifier to update.
        success:
            Number of successful extractions.
        failed:
            Number of failed extractions.
        skipped:
            Number of skipped jobs (already processed, up-to-date).
        avg_latency_ms:
            Average per-request latency in milliseconds.
        p95_latency_ms:
            P95 per-request latency in milliseconds.
        avg_prompt_tokens:
            Average prompt token count.
        avg_completion_tokens:
            Average completion token count.
        """
        update: dict[str, Any] = {
            "finished_at": datetime.now(timezone.utc).isoformat(),
            "success_count": success,
            "failed_count": failed,
            "skipped_count": skipped,
        }
        if avg_latency_ms is not None:
            update["avg_latency_ms"] = avg_latency_ms
        if p95_latency_ms is not None:
            update["p95_latency_ms"] = p95_latency_ms
        if avg_prompt_tokens is not None:
            update["avg_prompt_tokens"] = avg_prompt_tokens
        if avg_completion_tokens is not None:
            update["avg_completion_tokens"] = avg_completion_tokens

        try:
            self._client.table("extraction_runs").update(
                update
            ).eq("run_id", run_id).execute()
        except Exception as exc:
            raise DatabaseError(
                f"update_extraction_run failed for {run_id}: {exc}"
            ) from exc

    # ------------------------------------------------------------------
    # get_existing_jobs
    # ------------------------------------------------------------------

    def get_existing_jobs(
        self, url_hashes: list[str]
    ) -> dict[str, dict[str, Any]]:
        """Return existing jobs matching the given URL hashes.

        Selects version and hash columns for staleness comparison::

            SELECT url_hash, jd_structured_status,
                   jd_structured_extractor_version,
                   jd_structured_schema_version,
                   jd_structured_prompt_version,
                   jd_structured_raw_hash, jd_structured_cleaned_hash
            FROM jobs
            WHERE url_hash IN (...)

        Parameters
        ----------
        url_hashes:
            List of hashes to look up.

        Returns
        -------
        dict[str, dict]
            Mapping of ``url_hash`` -> row data dict.  Only hashes that
            exist in the database appear as keys.
        """
        if not url_hashes:
            return {}

        try:
            resp = (
                self._client.table("jobs")
                .select(
                    "url_hash, jd_structured_status, "
                    "jd_structured_extractor_version, "
                    "jd_structured_schema_version, "
                    "jd_structured_prompt_version, "
                    "jd_structured_raw_hash, jd_structured_cleaned_hash"
                )
                .in_("url_hash", url_hashes)
                .execute()
            )

            rows = resp.data
            if not rows:
                return {}

            return {row["url_hash"]: row for row in rows}
        except Exception as exc:
            raise DatabaseError(
                f"get_existing_jobs failed for {len(url_hashes)} hashes: {exc}"
            ) from exc
