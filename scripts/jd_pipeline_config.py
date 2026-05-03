"""Configuration constants for the JD structured extraction pipeline.

Version tracking, LLM server config, concurrency, timeouts, token limits,
schema definitions for validation, and Supabase connection parameters.
"""

import os

# ---------------------------------------------------------------------------
# Version tracking
# ---------------------------------------------------------------------------
EXTRACTOR = "qwen3-jd-parser"
EXTRACTOR_VERSION = "v1"
SCHEMA_VERSION = "v1"
PROMPT_VERSION = "linkedin-v1"
PREPROCESS_VERSION = "linkedin-jd-clean-v1"

# ---------------------------------------------------------------------------
# LLM server config
# ---------------------------------------------------------------------------
LLM_BASE_URL = "http://127.0.0.1:8091"
LLM_MODEL = "qwen3-jd-parser.gguf"

# ---------------------------------------------------------------------------
# Concurrency
# ---------------------------------------------------------------------------
DEFAULT_SEMAPHORE = 6

# ---------------------------------------------------------------------------
# Timeouts (seconds)
# ---------------------------------------------------------------------------
DEFAULT_TIMEOUT = 120.0
MAX_TIMEOUT = 300.0
MIN_TIMEOUT = 60.0

# ---------------------------------------------------------------------------
# Token limits
# ---------------------------------------------------------------------------
DEFAULT_MAX_TOKENS = 1536
EVIDENCE_MAX_TOKENS = 3072
HARD_MAX_TOKENS = 4096

# ---------------------------------------------------------------------------
# Stale processing reaper threshold
# ---------------------------------------------------------------------------
STALE_PROCESSING_MINUTES = 30

# ---------------------------------------------------------------------------
# Context size thresholds for server -c setting
# ---------------------------------------------------------------------------
# Mapping of server context window size -> p95 token threshold.
# If the p95 token count of a batch is below the threshold, the smaller
# context window is sufficient.
CTX_SIZE_TIERS = {
    8192: 6000,          # p95 < 6000 -> -c 8192
    12288: 10000,        # p95 < 10000 -> -c 12288
    16384: float("inf"),  # p95 >= 10000 -> -c 16384
}

# ---------------------------------------------------------------------------
# JD Schema for output validation
# ---------------------------------------------------------------------------
JD_SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "required": [
        "job_title",
        "company_name",
        "skills",
        "summary",
    ],
    "properties": {
        "job_title": {"type": "string", "minLength": 1},
        "company_name": {"type": "string", "minLength": 1},
        "location": {"type": ["string", "null"]},
        "salary_range": {"type": ["string", "null"]},
        "skills": {
            "type": "array",
            "items": {"type": "string", "minLength": 1},
            "maxItems": 50,
        },
        "responsibilities": {
            "type": ["array", "null"],
            "items": {"type": "string", "minLength": 1},
            "maxItems": 12,
        },
        "qualifications": {
            "type": ["array", "null"],
            "items": {"type": "string", "minLength": 1},
            "maxItems": 12,
        },
        "experience_level": {
            "type": ["string", "null"],
            "enum": [
                "intern",
                "junior",
                "mid",
                "senior",
                "lead",
                "principal",
                "unknown",
                None,
            ],
        },
        "employment_type": {
            "type": ["string", "null"],
            "enum": [
                "full_time",
                "part_time",
                "contract",
                "temporary",
                "internship",
                "unknown",
                None,
            ],
        },
        "summary": {"type": "string", "minLength": 10, "maxLength": 800},
        "confidence": {
            "type": ["object", "null"],
            "type": "object",
            "additionalProperties": False,
            "required": ["overall", "missing_fields"],
            "properties": {
                "overall": {"type": "number", "minimum": 0, "maximum": 1},
                "missing_fields": {
                    "type": "array",
                    "items": {"type": "string"},
                },
            },
        },
    },
}

# ---------------------------------------------------------------------------
# Minimal schema for retry attempt 3 (only core fields)
# ---------------------------------------------------------------------------
MINIMAL_SCHEMA = {
    "type": "object",
    "additionalProperties": False,
    "required": ["job_title", "company_name", "skills", "summary"],
    "properties": {
        "job_title": {"type": "string", "minLength": 1},
        "company_name": {"type": "string", "minLength": 1},
        "skills": {
            "type": "array",
            "items": {"type": "string", "minLength": 1},
            "maxItems": 50,
        },
        "summary": {"type": "string", "minLength": 10, "maxLength": 800},
    },
}

# ---------------------------------------------------------------------------
# Supabase config - read from environment variables
# ---------------------------------------------------------------------------
SUPABASE_URL = os.environ.get("SUPABASE_URL", "")
SUPABASE_KEY = os.environ.get("SUPABASE_KEY", "")

# ---------------------------------------------------------------------------
# Input source file
# ---------------------------------------------------------------------------
INPUT_FILE = "output/final.json"
