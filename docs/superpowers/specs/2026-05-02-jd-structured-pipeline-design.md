# JD Structured Extraction Pipeline — Design Spec

**Status**: approved
**Date**: 2026-05-02
**Branch**: codex/linkedin-recommended-with-jd

## Overview

Pipeline that reads raw JDs from `output/final.json`, sends them to a local
`qwen3-jd-parser` model for structured JSON extraction, and stores results in
Supabase `jobs.jd_structured`. MVP: manual trigger, single Python script.
Future: message-queue trigger.

## Architecture

```
final.json (200 JDs)
    │
    ▼
┌──────────────────────────────────────────────────────┐
│ jd_pipeline.py                                       │
│                                                      │
│ 1. validate input row schema                         │
│ 2. store jd_raw + compute raw_hash                   │
│ 3. preprocess → jd_cleaned + cleaned_hash            │
│ 4. tokenize stats → adjust server -c                 │
│ 5. skip policy (status=ok & version match & hash ok) │
│ 6. claim (atomic SQL UPDATE RETURNING id)            │
│ 7. async batch → LLM call (temp=0, json_schema)      │
│ 8. parse + jsonschema validate                       │
│ 9. retry: validation-error feedback → minimal extract│
│10. atomic upsert (INSERT ON CONFLICT + run_id guard) │
│11. dead_letter sync (update jobs.status)             │
│12. write extraction_runs summary                     │
└──────────────────────────────────────────────────────┘
    │
    ▼
Supabase jobs.jd_structured (jsonb)
```

## Status Machine

```
            ┌──────────────────────────────────────────┐
            │                                          │
pending ──→ processing ──→ ok                          │
  ↑         │              │                           │
  │         │              └── schema/prompt/extractor  │
  │         │                  version bump → pending   │
  │         │                                          │
  │         ├──→ dead_letter (terminal, log to DLQ)     │
  │         │                                          │
  │         └──→ failed (retryable, will be reclaimed)  │
  │               │                                    │
  │               └──→ pending (on next run)            │
  │                                                   │
  └── stale processing (>30min) ──────────────────────┘
```

## Component Details

### 1. Preprocessing

Rules (script logic, no model involved):

- Remove LinkedIn boilerplate snippets (e.g. "Application Process (Takes 20 Min)...")
- Collapse multiple blank lines
- Unicode normalization (fullwidth → ASCII where applicable)
- Strip control characters

Invariants:

- `jd_raw` is immutable — always stored as-is
- `jd_cleaned` is model input only
- Both hashes stored for traceability: `raw_hash`, `cleaned_hash`
- `preprocess_version` recorded (e.g. `"linkedin-jd-clean-v1"`)

### 2. Tokenize Pass

Before batch processing, run all `jd_cleaned` through llama.cpp `/tokenize` endpoint.
Collect p50, p90, p95, max token counts. Use these to set server `-c`:

- p95 < 6000  → `-c 8192`
- p95 < 10000 → `-c 12288`
- p95 >= 10000 → `-c 16384`

Avoid `-c 40960` unless proven necessary — larger context reduces throughput.

### 3. Skip Policy

Skip a JD when ALL of the following hold:

- `jd_structured_status = 'ok'`
- `jd_structured_extractor_version = current_extractor_version`
- `jd_structured_schema_version = current_schema_version`
- `jd_structured_prompt_version = current_prompt_version`
- `jd_structured_raw_hash = current_raw_hash`
- `jd_structured_cleaned_hash = current_cleaned_hash`

### 4. Claim (Atomic)

```sql
UPDATE jobs SET
  jd_structured_status = 'processing',
  processing_run_id = :run_id,
  processing_started_at = now()
WHERE url_hash = :url_hash
  AND (
    jd_structured_status IS NULL
    OR jd_structured_status IN ('pending', 'failed')
    OR jd_structured_extractor_version IS DISTINCT FROM :extractor_ver
    OR jd_structured_schema_version IS DISTINCT FROM :schema_ver
    OR jd_structured_prompt_version IS DISTINCT FROM :prompt_ver
    OR jd_structured_raw_hash IS DISTINCT FROM :raw_hash
    OR jd_structured_cleaned_hash IS DISTINCT FROM :cleaned_hash
  )
RETURNING id;
```

No RETURNING row → another worker already claimed, skip.

### 5. LLM Call

**Model**: qwen3-jd-parser.gguf via llama.cpp server at `http://127.0.0.1:8091`

**Request**:

```json
{
  "messages": [{"role": "user", "content": "<prompt with jd_cleaned>"}],
  "temperature": 0,
  "max_tokens": 1536,
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "schema": {
        "type": "object",
        "properties": { ... },
        "required": [...],
        "additionalProperties": false
      }
    }
  }
}
```

**max_tokens rules** (output-complexity based, NOT input-length based):

| Condition | max_tokens |
|-----------|-----------|
| Default | 1536 |
| With evidence quotes | 3072 |
| Hard cap | 4096 |

**Timeout**: `min(300, max(60, p95_latency_seconds * 2))`

**Concurrency**: client semaphore ≤ server `-np` slots. Recommended: `-np 8` with semaphore 6 (leave scheduling headroom).

### 6. Server Startup (`run-server.sh`)

```bash
exec llama-server \
  -m qwen3-jd-parser.gguf \
  -ngl 99 \
  --host 127.0.0.1 --port 8091 \
  -c <dynamic_from_tokenize> \
  -np <benchmark_determined> \
  -b 4096 \
  -ub 1024 \
  --cache-type-k f16 --cache-type-v f16 \
  -fa on \
  --jinja \
  --metrics \
  --cont-batching
```

### 7. JSON Schema (output validation)

```python
JD_SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "required": [
        "job_title",
        "company_name",
        "location",
        "skills",
        "responsibilities",
        "qualifications",
        "summary",
        "confidence"
    ],
    "properties": {
        "job_title":       {"type": "string", "minLength": 1},
        "company_name":    {"type": "string", "minLength": 1},
        "location":        {"type": "string"},
        "salary_range":    {"type": ["string", "null"]},

        "skills": {
            "type": "array",
            "items": {"type": "string", "minLength": 1},
            "maxItems": 30
        },
        "responsibilities": {
            "type": "array",
            "items": {"type": "string", "minLength": 1},
            "maxItems": 12
        },
        "qualifications": {
            "type": "array",
            "items": {"type": "string", "minLength": 1},
            "maxItems": 12
        },

        "experience_level": {
            "type": ["string", "null"],
            "enum": ["intern", "junior", "mid", "senior", "lead", "principal", "unknown", None]
        },
        "employment_type": {
            "type": ["string", "null"],
            "enum": ["full_time", "part_time", "contract", "temporary", "internship", "unknown", None]
        },
        "summary": {"type": "string", "minLength": 20, "maxLength": 800},

        "confidence": {
            "type": "object",
            "additionalProperties": False,
            "required": ["overall", "missing_fields"],
            "properties": {
                "overall": {"type": "number", "minimum": 0, "maximum": 1},
                "missing_fields": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            }
        }
    }
}
```

### 8. Retry Strategy

All retries use `temperature = 0`:

| Attempt | Strategy |
|---------|----------|
| 1 | Standard call with json_schema grammar constraint |
| 2 | Feed validation errors back to model for repair |
| 3 | Minimal extraction: only core fields (title, company, skills, summary) |
| Fail | → dead_letter_records + update jobs.status = 'dead_letter' |

### 9. Atomic Upsert

```sql
INSERT INTO jobs (
  url, url_hash, source,
  jd_raw,
  jd_structured,
  jd_structured_status,
  jd_structured_extractor,
  jd_structured_extractor_version,
  jd_structured_schema_version,
  jd_structured_prompt_version,
  jd_structured_raw_hash,
  jd_structured_cleaned_hash,
  jd_structured_processed_at,
  updated_at
) VALUES (...)
ON CONFLICT (url_hash)
DO UPDATE SET
  jd_structured = EXCLUDED.jd_structured,
  jd_structured_status = 'ok',
  jd_structured_extractor = EXCLUDED.jd_structured_extractor,
  jd_structured_extractor_version = EXCLUDED.jd_structured_extractor_version,
  jd_structured_schema_version = EXCLUDED.jd_structured_schema_version,
  jd_structured_prompt_version = EXCLUDED.jd_structured_prompt_version,
  jd_structured_raw_hash = EXCLUDED.jd_structured_raw_hash,
  jd_structured_cleaned_hash = EXCLUDED.jd_structured_cleaned_hash,
  jd_structured_processed_at = now(),
  updated_at = now()
WHERE jobs.processing_run_id = :run_id;
```

The `WHERE processing_run_id = :run_id` guard prevents stale runs from overwriting newer results.

### 10. Dead Letter Sync

```sql
UPDATE jobs SET
  jd_structured_status = 'dead_letter',
  processing_run_id = NULL
WHERE url_hash = :url_hash
  AND processing_run_id = :run_id;

INSERT INTO dead_letter_records (
  url_hash, url, stage, error_class, error_message,
  raw_response, validation_errors, attempt_count,
  model, prompt_version, schema_version
) VALUES (...);
```

### 11. Stale Processing Reaper

Run at pipeline startup:

```sql
UPDATE jobs SET
  jd_structured_status = 'pending',
  processing_run_id = NULL,
  processing_started_at = NULL
WHERE jd_structured_status = 'processing'
  AND processing_started_at < now() - INTERVAL '30 minutes';
```

## Database Changes

### New columns on `jobs`

```sql
ALTER TABLE jobs ADD COLUMN jd_structured_status TEXT
  DEFAULT 'pending'
  CHECK (jd_structured_status IN ('pending','processing','ok','failed','dead_letter'));

ALTER TABLE jobs ADD COLUMN jd_structured_extractor TEXT;
ALTER TABLE jobs ADD COLUMN jd_structured_extractor_version TEXT;
ALTER TABLE jobs ADD COLUMN jd_structured_schema_version TEXT;
ALTER TABLE jobs ADD COLUMN jd_structured_prompt_version TEXT;
ALTER TABLE jobs ADD COLUMN jd_structured_raw_hash TEXT;
ALTER TABLE jobs ADD COLUMN jd_structured_cleaned_hash TEXT;
ALTER TABLE jobs ADD COLUMN jd_structured_processed_at TIMESTAMPTZ;

ALTER TABLE jobs ADD COLUMN processing_run_id TEXT;
ALTER TABLE jobs ADD COLUMN processing_started_at TIMESTAMPTZ;

CREATE INDEX idx_jobs_jd_structured_status ON jobs (jd_structured_status)
  WHERE jd_structured_status IN ('pending','processing');
CREATE INDEX idx_jobs_extractor_version ON jobs (jd_structured_extractor_version);
```

### New table: `extraction_runs`

```sql
CREATE TABLE extraction_runs (
  id BIGSERIAL PRIMARY KEY,
  run_id TEXT UNIQUE NOT NULL,
  started_at TIMESTAMPTZ DEFAULT now(),
  finished_at TIMESTAMPTZ,
  input_file TEXT,
  total_count INT,
  success_count INT DEFAULT 0,
  failed_count INT DEFAULT 0,
  skipped_count INT DEFAULT 0,
  model TEXT,
  model_quant TEXT,
  server_params JSONB,
  prompt_version TEXT,
  schema_version TEXT,
  extractor_version TEXT,
  avg_latency_ms FLOAT,
  p95_latency_ms FLOAT,
  avg_prompt_tokens INT,
  avg_completion_tokens INT
);
```

### New columns on `dead_letter_records` (if not present)

```sql
ALTER TABLE dead_letter_records ADD COLUMN stage TEXT;
ALTER TABLE dead_letter_records ADD COLUMN error_class TEXT;
ALTER TABLE dead_letter_records ADD COLUMN error_message TEXT;
ALTER TABLE dead_letter_records ADD COLUMN raw_response TEXT;
ALTER TABLE dead_letter_records ADD COLUMN validation_errors JSONB;
ALTER TABLE dead_letter_records ADD COLUMN attempt_count INT;
ALTER TABLE dead_letter_records ADD COLUMN model TEXT;
ALTER TABLE dead_letter_records ADD COLUMN prompt_version TEXT;
ALTER TABLE dead_letter_records ADD COLUMN schema_version TEXT;
```

## Files

| File | Purpose |
|------|---------|
| `scripts/jd_pipeline.py` | Main pipeline script |
| `scripts/jd_pipeline_config.py` | Config: versions, timeouts, schema |
| `scripts/requirements.txt` | Python deps: httpx, supabase, jsonschema |
| `home/rick/models/.../run-server.sh` | Updated server startup params |

## Versions

```
extractor:        "qwen3-jd-parser"
extractor_version: "v1"
schema_version:   "v1"
prompt_version:   "linkedin-v1"
preprocess_version: "linkedin-jd-clean-v1"
```

## Error Handling

- Single JD failure does not block the batch
- Network timeout → retry once, then dead_letter
- JSON parse failure → retry with validation feedback, then minimal extract, then dead_letter
- Schema validation failure → same retry ladder
- Stale processing rows → reaped at startup (30min threshold)
- All failures logged to `extraction_runs` summary and `dead_letter_records`

## Testing

- Unit: preprocess rules on sample JD texts
- Unit: JSON schema validation with valid/invalid outputs
- Integration: single JD end-to-end (claim → LLM → upsert)
- Integration: idempotency (run twice, second run skips all)
- Manual: benchmark concurrency (find optimal semaphore for -np)

## Future (out of scope for MVP)

- Message queue trigger (function signature ready: `process_batch(jobs: List[dict])`)
- Evidence quotes in output (`evidence.skills[].quote`)
- Incremental mode (process only new JDs since last run)
- `-c` dynamic adjustment mid-run based on actual token counts
