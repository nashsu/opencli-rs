"""Async LLM client for batch JD structured extraction.

Provides LLMClient with concurrency-limited batch processing,
grammar-constrained JSON generation via llama.cpp native json_schema format,
and a three-attempt retry strategy (standard -> repair -> minimal).

Designed for llama.cpp server running qwen3-jd-parser.gguf.
"""

from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import Any

import httpx
from jsonschema import ValidationError, validate

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Error classes
# ---------------------------------------------------------------------------


class LLMError(Exception):
    """Base error for LLM operations."""


class LLMTimeoutError(LLMError):
    """Request timed out."""


class LLMJsonParseError(LLMError):
    """Failed to parse model output as JSON."""


class LLMValidationError(LLMError):
    """Model output JSON does not conform to schema."""


# ---------------------------------------------------------------------------
# Minimal core-field schema (Attempt 3 fallback)
# ---------------------------------------------------------------------------

MINIMAL_SCHEMA: dict = {
    "type": "object",
    "properties": {
        "job_title": {"type": "string"},
        "company_name": {"type": "string"},
        "skills": {
            "type": "array",
            "items": {"type": "string"},
        },
        "summary": {"type": "string"},
    },
    "required": ["job_title", "company_name", "skills", "summary"],
    "additionalProperties": False,
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _build_response_format(schema: dict) -> dict:
    """Build llama.cpp native json_schema response_format.

    llama.cpp expects::

        {"type": "json_schema", "json_schema": {"schema": { ... }}}

    This is the *native* format -- NOT the OpenAI format which adds
    ``name`` and ``strict`` wrappers around the schema object.
    """
    return {
        "type": "json_schema",
        "json_schema": {
            "schema": schema,
        },
    }


def _compute_p95(values: list[float]) -> float:
    """Return the 95th percentile of *values*."""
    if not values:
        return 120.0
    sorted_vals = sorted(values)
    idx = max(0, int(len(sorted_vals) * 0.95) - 1)
    return sorted_vals[idx]


# ---------------------------------------------------------------------------
# LLMClient
# ---------------------------------------------------------------------------


class LLMClient:
    """Async LLM client with concurrency-limited batch processing.

    Parameters
    ----------
    base_url:
        Base URL of the llama.cpp server (e.g. ``http://127.0.0.1:8091``).
    model:
        Model name for the ``model`` field in completions requests.
    semaphore:
        Max concurrent in-flight requests.  Defaults to 6 (fewer than
        the 8 server slots for scheduling headroom).
    default_max_tokens:
        Default ``max_tokens`` for generation.  1536 for standard schemas,
        3072 when evidence quotes are requested, hard cap 4096.
    timeout:
        Base request timeout in seconds.  Dynamically adjusted based on
        observed latency via ``_dynamic_timeout()``.
    """

    def __init__(
        self,
        base_url: str,
        model: str,
        semaphore: int = 6,
        default_max_tokens: int = 1536,
        timeout: float = 120.0,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.model = model
        self._semaphore_size = semaphore
        self.default_max_tokens = default_max_tokens
        self._base_timeout = timeout
        self._client: httpx.AsyncClient | None = None
        self._semaphore: asyncio.Semaphore | None = None
        self._latencies: list[float] = []

    # -- context manager -----------------------------------------------------

    async def __aenter__(self) -> LLMClient:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.close()

    # -- internal helpers ----------------------------------------------------

    async def _get_client(self) -> httpx.AsyncClient:
        if self._client is None or self._client.is_closed:
            # Client-level timeout is the 300 s hard cap; per-request
            # timeouts use the dynamic value from _dynamic_timeout().
            self._client = httpx.AsyncClient(
                base_url=self.base_url,
                timeout=httpx.Timeout(300.0),
            )
        return self._client

    def _get_semaphore(self) -> asyncio.Semaphore:
        if self._semaphore is None:
            self._semaphore = asyncio.Semaphore(self._semaphore_size)
        return self._semaphore

    def _dynamic_timeout(self) -> float:
        """``min(300, max(60, p95_latency * 2))`` based on observed latencies."""
        p95 = _compute_p95(self._latencies) if self._latencies else self._base_timeout
        return min(300.0, max(60.0, p95 * 2))

    def _record_latency(self, seconds: float) -> None:
        self._latencies.append(seconds)
        # Keep bounded to avoid unbounded memory growth
        if len(self._latencies) > 500:
            self._latencies = self._latencies[-500:]

    async def close(self) -> None:
        if self._client and not self._client.is_closed:
            await self._client.aclose()

    # -- tokenize ------------------------------------------------------------

    async def tokenize(self, text: str) -> int:
        """Call ``/tokenize`` endpoint to get exact token count."""
        client = await self._get_client()
        payload = {"content": text}
        timeout = self._dynamic_timeout()
        try:
            resp = await client.post("/tokenize", json=payload, timeout=timeout)
            resp.raise_for_status()
            data = resp.json()
            return len(data.get("tokens", []))
        except httpx.TimeoutException as exc:
            raise LLMTimeoutError(f"Tokenize request timed out: {exc}") from exc
        except httpx.ConnectError as exc:
            raise LLMError(f"Cannot connect to LLM server: {exc}") from exc
        except httpx.HTTPStatusError as exc:
            raise LLMError(f"Tokenize request failed: {exc}") from exc

    async def tokenize_stats(self, texts: list[str]) -> dict:
        """Tokenize all *texts* and return ``{p50, p90, p95, max, count}`` stats.

        Uses the same concurrency semaphore as extraction requests so the
        tokenize calls do not starve extraction bandwidth.
        """
        sem = self._get_semaphore()

        async def _safe_tokenize(t: str) -> int:
            async with sem:
                return await self.tokenize(t)

        results = await asyncio.gather(
            *[_safe_tokenize(t) for t in texts],
            return_exceptions=True,
        )

        valid = [c for c in results if isinstance(c, int)]
        if not valid:
            return {"p50": 0, "p90": 0, "p95": 0, "max": 0, "count": 0}
        sorted_counts = sorted(valid)
        n = len(sorted_counts)
        return {
            "p50": sorted_counts[max(0, int(n * 0.50) - 1)],
            "p90": sorted_counts[max(0, int(n * 0.90) - 1)],
            "p95": sorted_counts[max(0, int(n * 0.95) - 1)],
            "max": sorted_counts[-1],
            "count": n,
        }

    # -- extraction ----------------------------------------------------------

    async def _call_model(
        self,
        messages: list[dict],
        schema: dict,
        max_tokens: int | None = None,
    ) -> dict | None:
        """Single call to ``/chat/completions`` with grammar constraint.

        Returns parsed JSON dict on success, or ``None`` if the model
        returned no content.  Raises :class:`LLMJsonParseError` when the
        response body is not valid JSON, and :class:`LLMTimeoutError` /
        :class:`LLMError` on network failures.
        """
        client = await self._get_client()
        mt = min(max_tokens or self.default_max_tokens, 4096)

        payload: dict[str, Any] = {
            "model": self.model,
            "messages": messages,
            "max_tokens": mt,
            "temperature": 0,
            "response_format": _build_response_format(schema),
        }

        timeout = self._dynamic_timeout()
        t0 = time.monotonic()
        try:
            resp = await client.post(
                "/chat/completions", json=payload, timeout=timeout
            )
            resp.raise_for_status()
        except httpx.TimeoutException as exc:
            raise LLMTimeoutError(f"Completion request timed out: {exc}") from exc
        except httpx.ConnectError as exc:
            raise LLMError(f"Cannot connect to LLM server: {exc}") from exc
        except httpx.HTTPStatusError as exc:
            raise LLMError(f"Completion request failed: {exc}") from exc
        finally:
            elapsed = time.monotonic() - t0
            self._record_latency(elapsed)

        data = resp.json()
        content = (
            data.get("choices", [{}])[0].get("message", {}).get("content", "")
        )
        if not content:
            return None

        try:
            parsed = json.loads(content)
        except json.JSONDecodeError as exc:
            raise LLMJsonParseError(
                f"Model output is not valid JSON: {exc}"
            ) from exc

        return parsed

    async def extract(
        self,
        jd_text: str,
        schema: dict,
        max_tokens: int | None = None,
    ) -> dict | None:
        """Extract structured data from *jd_text* using *schema*.

        Three-attempt retry strategy (all at ``temperature=0``):

        1. **Standard** -- call with full schema + grammar constraint.
        2. **Repair** -- feed validation errors back to model for repair.
        3. **Minimal** -- fall back to core fields only
           (``job_title``, ``company_name``, ``skills``, ``summary``).

        Returns the extracted dict on success, or ``None`` if all
        attempts fail (caller should handle dead-letter).
        Network errors (timeout, connect) propagate as :class:`LLMError`
        subclasses.
        """
        # --- Attempt 1: standard -------------------------------------------
        result_1: dict | None = None
        error_1 = ""
        try:
            messages = [
                {
                    "role": "system",
                    "content": (
                        "You are a job description parser. Extract structured data as JSON.\n"
                        "Rules for 'skills':\n"
                        "- Only include technical skills, tools, frameworks, languages, and methodologies.\n"
                        "- Do NOT include: company perks, benefits, culture statements, work "
                        "arrangements, diversity statements, or soft traits like 'problem-solving'.\n"
                        "- Maximum 25 items; prefer the most specific and technical ones.\n"
                        "Rules for 'summary':\n"
                        "- 1-3 sentences capturing the role's purpose and key requirements.\n"
                        "Rules for 'experience_level':\n"
                        "- Choose from: intern, junior, mid, senior, lead, principal, unknown.\n"
                        "Rules for 'employment_type':\n"
                        "- Choose from: full_time, part_time, contract, temporary, internship, unknown.\n"
                        "If a field is unclear, use null rather than guessing."
                    ),
                },
                {"role": "user", "content": jd_text},
            ]
            result_1 = await self._call_model(messages, schema, max_tokens)
            if result_1 is not None:
                validate(instance=result_1, schema=schema)
                return result_1
            error_1 = "Model returned empty content"
        except (LLMJsonParseError, ValidationError) as exc:
            error_1 = str(exc)
            logger.debug("Attempt 1 failed: %s", exc)
        except LLMError:
            raise

        # --- Attempt 2: repair with validation error feedback ---------------
        try:
            repair_prompt = (
                f"The previous extraction had errors:\n{error_1}\n\n"
                "Fix the errors and return valid JSON conforming to the schema. "
                "For 'skills': only technical skills, max 25 items."
            )
            messages: list[dict] = [
                {
                    "role": "system",
                    "content": (
                        "You are a job description parser. Extract structured data as JSON.\n"
                        "Rules for 'skills':\n"
                        "- Only include technical skills, tools, frameworks, languages, and methodologies.\n"
                        "- Do NOT include: company perks, benefits, culture statements, work "
                        "arrangements, diversity statements, or soft traits like 'problem-solving'.\n"
                        "- Maximum 25 items; prefer the most specific and technical ones.\n"
                        "If a field is unclear, use null rather than guessing."
                    ),
                },
                {"role": "user", "content": jd_text},
            ]
            if result_1 is not None:
                # Feed back the parsed-but-invalid output so the model can
                # see what it produced and correct it.
                messages.append(
                    {"role": "assistant", "content": json.dumps(result_1)}
                )
            messages.append({"role": "user", "content": repair_prompt})

            result_2 = await self._call_model(messages, schema, max_tokens)
            if result_2 is not None:
                validate(instance=result_2, schema=schema)
                return result_2
        except (LLMJsonParseError, ValidationError) as exc:
            logger.debug("Attempt 2 (repair) failed: %s", exc)
        except LLMError:
            raise

        # --- Attempt 3: minimal core fields ---------------------------------
        try:
            messages = [
                {
                    "role": "system",
                    "content": (
                        "Extract ONLY the core fields from the job description as JSON: "
                        "job_title, company_name, skills, summary.\n"
                        "For 'skills': only technical skills, max 25 items."
                    ),
                },
                {"role": "user", "content": jd_text},
            ]
            result_3 = await self._call_model(messages, MINIMAL_SCHEMA, max_tokens)
            if result_3 is not None:
                validate(instance=result_3, schema=MINIMAL_SCHEMA)
                return result_3
        except (LLMJsonParseError, ValidationError) as exc:
            logger.warning("Attempt 3 (minimal) failed: %s", exc)
        except LLMError:
            raise

        return None

    async def extract_batch(
        self,
        items: list[tuple[str, dict]],
        max_tokens: int | None = None,
    ) -> list[dict | None]:
        """Process a batch of ``(jd_text, schema)`` tuples concurrently.

        Uses semaphore for throttling.  Returns a list of results in the
        same order as *items*; ``None`` entries mark failures (including
        network errors, which are caught per-item rather than propagated).
        """
        sem = self._get_semaphore()

        async def _extract_one(
            idx: int, jd_text: str, schema: dict
        ) -> tuple[int, dict | None]:
            async with sem:
                try:
                    result = await self.extract(
                        jd_text, schema, max_tokens=max_tokens
                    )
                    return (idx, result)
                except LLMError as exc:
                    logger.warning("Extraction failed for item %d: %s", idx, exc)
                    return (idx, None)

        tasks = [
            _extract_one(i, jd_text, schema) for i, (jd_text, schema) in enumerate(items)
        ]
        results: list[dict | None] = [None] * len(items)

        for coro in asyncio.as_completed(tasks):
            idx, result = await coro
            results[idx] = result

        return results