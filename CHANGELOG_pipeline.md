# JD Structured Extraction Pipeline — Changelog

## [0.1.0] — 2026-05-03

### Added

- **Pipeline orchestrator** (`jd_pipeline.py`): CLI tool (`--input`, `--dry-run`, `--limit`) that reads `output/final.json`, preprocesses JDs, extracts structured JSON via local LLM, and upserts results into Supabase.
- **LLM client** (`jd_pipeline_llm.py`): Async batch client for llama.cpp `/chat/completions` with grammar-constrained generation (`json_schema`), 3-attempt retry (standard → repair with validation feedback → minimal), dynamic timeout, semaphore-limited concurrency, and latency tracking.
- **Database client** (`jd_pipeline_db.py`): Atomic `claim_job` / `upsert_job_structured` / `mark_dead_letter` / `reap_stale_processing` RPCs, extraction_runs bookkeeping, `.env` auto-loading.
- **Config** (`jd_pipeline_config.py`): Version constants, schema definitions (`JD_SCHEMA` + `MINIMAL_SCHEMA`), LLM/Supabase connection params, token limits, context-size tiers.
- **Preprocessor** (`jd_pipeline_preprocess.py`): LinkedIn boilerplate removal, NFKC normalization, control-char strip, SHA-256 hashing.
- **Supabase migrations** (6 files + RPC grants): `jobs` columns for structured extraction, `extraction_runs` table, `dead_letter_records` with stage/error tracking, atomic RPC functions with run-id guards.
- **Per-run reporting**: Console summary with failed-jobs detail (URL, stage, error class, message) + JSON report file in `output/`.

### Fixed

- `dead_letter_records.reason` and `source_schema`/`source_job_id` made nullable — prevents write failures when fields are absent.
- `skills.maxItems` raised from 30 → 50 to accommodate verbose model output.
- System prompt improved: explicit rules for skills (technical only, max 25), summary (1–3 sentences), experience_level, employment_type.
- Stale processing reaper threshold adjustable; 172 stuck jobs reaped successfully.
- Duplicate counter increments removed — `PipelineStats.record_*()` methods now single source of truth.