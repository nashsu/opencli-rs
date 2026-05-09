#!/usr/bin/env python3
"""Sync AutoCLI job JSON into Supabase with optional priority scoring."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import sys
from dataclasses import dataclass
from typing import Any, Iterable
from urllib.parse import urlparse, urlunparse

# Ensure project root is on sys.path for `scripts.*` imports when invoked as
#   python scripts/sync_autocli_jobs.py
_project_root = str(pathlib.Path(__file__).resolve().parent.parent)
if _project_root not in sys.path:
    sys.path.insert(0, _project_root)

from scripts.job_priority_scorer import score_job

def _sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode(
        "utf-8"
    )


def _normalize_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, (int, float)):
        return str(value)
    if not isinstance(value, str):
        return str(value)
    return value.strip()


def _get_first_key(record: dict[str, Any], keys: Iterable[str]) -> str:
    for key in keys:
        if key in record and record[key] is not None:
            v = _normalize_text(record[key])
            if v:
                return v
    return ""


# ── URL canonicalization (for dedup of ATS/external job URLs) ──────────

LINKEDIN_PATTERN = re.compile(
    r"^https?://(?:www\.)?linkedin\.com/",
    re.IGNORECASE,
)

ATS_DOMAINS = frozenset({
    "myworkdayjobs.com",
    "greenhouse.io",
    "lever.co",
    "recruitee.com",
    "applytojob.com",
    "workable.com",
    "breezy.hr",
    "smartrecruiters.com",
    "icims.com",
    "successfactors.eu",
    "successfactors.com",
    "oraclecloud.com",
    "taleo.net",
})

TRACKING_PARAMS = frozenset({
    "source", "share_id", "si", "li_fat_id", "trk", "trackingId", "tracking_id",
    "ref", "referrer",
    "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid",
    "msclkid", "twclid", "sc_campaign", "sc_channel", "sc_content",
    "sc_medium", "sc_outcome", "sc_geo", "sc_country",
    "gh_src", "lever_source", "lever-source",
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
})


def _is_linkedin_url(url: str | None) -> bool:
    """Return True if *url* is a linkedin.com URL."""
    if not url:
        return False
    return bool(LINKEDIN_PATTERN.match(url.strip()))


def _is_ats_url(url: str | None) -> bool:
    """Return True if *url* points to a known ATS / career-portal domain."""
    if not url:
        return False
    try:
        host = urlparse(url.strip()).hostname or ""
    except Exception:
        return False
    host = host.lower()
    # Strip www. prefix for matching
    if host.startswith("www."):
        host = host[4:]
    for domain in ATS_DOMAINS:
        if host == domain or host.endswith("." + domain):
            return True
    # Generic career portals (catch-alls after known ATS domains)
    if host.endswith(".myworkdayjobs.com"):
        return True
    return False


def _canonicalize_url(raw_url: str | None) -> str:
    """Normalize a URL for dedup: lowercase, strip trailing slash, remove tracking params.

    Returns the normalized URL string, or empty string if input is empty/falsy.
    """
    if not raw_url:
        return ""
    try:
        parsed = urlparse(raw_url.strip())
        scheme = parsed.scheme.lower()
        netloc = parsed.netloc.lower()
        # Strip trailing slash from path
        path = parsed.path.rstrip("/")
        if not path:
            path = "/"
        # Filter tracking query params
        cleaned_pairs: list[str] = []
        if parsed.query:
            for pair in parsed.query.split("&"):
                k, _, v = pair.partition("=")
                if k not in TRACKING_PARAMS:
                    cleaned_pairs.append(f"{k}={v}")
        cleaned_query = "&".join(cleaned_pairs)
        result = urlunparse((scheme, netloc, path, parsed.params, cleaned_query, ""))
        return result.rstrip("?")
    except Exception:
        return raw_url.strip()


def _extract_canonical_job_url(
    apply_url: str,
    external_url: str,
) -> str:
    """Determine the canonical job URL to use for identity computation.

    Priority (first non-empty, non-LinkedIn as identity):
      1. external_url if it is an ATS URL
      2. external_url if apply_url is LinkedIn (prefer any external_url over LinkedIn)
      3. apply_url if it is an ATS URL (not LinkedIn)
      4. apply_url as fallback (even if LinkedIn)
      5. empty string
    """
    apply_url_s = apply_url.strip() if apply_url else ""
    external_url_s = external_url.strip() if external_url else ""

    # Rule 1: external_url is ATS → use it
    if _is_ats_url(external_url_s):
        return _canonicalize_url(external_url_s)

    # Rule 2: apply_url is LinkedIn AND external_url exists → use external_url
    if _is_linkedin_url(apply_url_s) and external_url_s:
        return _canonicalize_url(external_url_s)

    # Rule 3: apply_url is ATS (not LinkedIn) → use it
    if _is_ats_url(apply_url_s):
        return _canonicalize_url(apply_url_s)

    # Rule 4: apply_url exists (even LinkedIn) → use it
    if apply_url_s:
        return _canonicalize_url(apply_url_s)

    # Rule 5: external_url exists → use it
    if external_url_s:
        return _canonicalize_url(external_url_s)

    return ""


def _extract_records(doc: Any) -> list[dict[str, Any]]:
    if isinstance(doc, list):
        return [r for r in doc if isinstance(r, dict)]
    if isinstance(doc, dict):
        for key in ("items", "results", "data"):
            val = doc.get(key)
            if isinstance(val, list):
                return [r for r in val if isinstance(r, dict)]
    raise ValueError("Unsupported JSON shape: expected array of objects")


def _load_dotenv(path: str | os.PathLike[str]) -> None:
    """Minimal .env loader (no extra dependencies).

    - Ignores blank lines and comments starting with '#'
    - Supports KEY=VALUE with optional surrounding quotes
    - Does not override already-set environment variables
    """
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
    """Load env vars from `.env` if present.

    Search order:
      1) CWD/.env
      2) Project root (scripts/..)/.env
    """
    _load_dotenv(pathlib.Path.cwd() / ".env")
    _load_dotenv(pathlib.Path(__file__).resolve().parent.parent / ".env")


@dataclass(frozen=True)
class NormalizedJob:
    source: str
    identity_hash: str
    job_title: str
    company_name: str
    location: str
    salary: str
    post_time: str
    apply_url: str
    external_url: str
    job_description: str
    description_hash: str
    url: str
    url_hash: str
    source_channel: str
    apply_type: str
    raw_record: dict[str, Any]
    raw_hash: str


def normalize_job(source: str, raw_record: dict[str, Any]) -> NormalizedJob | None:
    apply_url = _get_first_key(raw_record, ("apply_url", "apply url", "applyUrl"))
    external_url = _get_first_key(raw_record, ("external_url", "externalUrl"))
    job_title = _get_first_key(raw_record, ("job_title", "jobTitle", "title"))
    company_name = _get_first_key(raw_record, ("company_name", "companyName", "company"))
    location = _get_first_key(raw_record, ("location",))
    salary = _get_first_key(raw_record, ("salary", "salary_range", "salaryRange"))
    post_time = _get_first_key(raw_record, ("post_time", "postTime", "posted_date", "postedDate"))
    job_description = _get_first_key(raw_record, ("job_description", "jobDescription", "description"))

    # Use canonical URL for identity, not raw apply_url (which may be a LinkedIn referrer)
    canonical_url = _extract_canonical_job_url(apply_url, external_url)
    if canonical_url:
        identity_source = canonical_url
    else:
        if not job_title or not company_name:
            return None
        identity_source = f"{job_title.lower()}|{company_name.lower()}|{location.lower()}"

    identity_hash = _sha256_hex(identity_source.encode("utf-8"))
    raw_hash = _sha256_hex(_canonical_json_bytes(raw_record))
    description_hash = _sha256_hex(job_description.encode("utf-8")) if job_description else ""

    url = _get_first_key(raw_record, ("url",))
    url_hash = _get_first_key(raw_record, ("url_hash",))
    source_channel = _get_first_key(raw_record, ("source_channel",))
    apply_type = _get_first_key(raw_record, ("apply_type",))
    if not apply_type:
        easy_apply_raw = _get_first_key(raw_record, ("easy_apply",))
        if easy_apply_raw and easy_apply_raw.lower() in ("true", "1", "yes"):
            apply_type = "easy_apply"

    return NormalizedJob(
        source=source,
        identity_hash=identity_hash,
        job_title=job_title,
        company_name=company_name,
        location=location,
        salary=salary,
        post_time=post_time,
        apply_url=apply_url,
        external_url=external_url,
        job_description=job_description,
        description_hash=description_hash,
        url=url,
        url_hash=url_hash,
        source_channel=source_channel,
        apply_type=apply_type,
        raw_record=raw_record,
        raw_hash=raw_hash,
    )


def _create_supabase_client(url: str | None, key: str | None):
    try:
        # Move CWD to end of sys.path so a local `supabase/` dir (migrations
        # folder in the project root) doesn't shadow the `supabase` PyPI package.
        _path_clean = [p for p in sys.path if p not in ("", ".")]
        _path_dirty = [p for p in sys.path if p in ("", ".")]
        sys.path = _path_clean + _path_dirty
        from supabase import create_client  # noqa: F811
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
            "SUPABASE_SERVICE_ROLE_KEY (preferred) or SUPABASE_KEY (can be service-role or anon)."
        )
    return create_client(url, key)


def upsert_job(
    client,
    job: NormalizedJob,
    priority_score: float | None = None,
    priority_tier: str | None = None,
    priority_version: str | None = None,
    priority_signals: dict | None = None,
) -> str:
    params: dict[str, Any] = {
        "p_source": job.source,
        "p_identity_hash": job.identity_hash,
        "p_job_title": job.job_title,
        "p_company_name": job.company_name,
        "p_location": job.location,
        "p_salary": job.salary,
        "p_post_time": job.post_time,
        "p_apply_url": job.apply_url,
        "p_external_url": job.external_url,
        "p_job_description": job.job_description,
        "p_description_hash": job.description_hash,
        "p_raw_record": job.raw_record,
        "p_raw_hash": job.raw_hash,
    }
    if job.url_hash:
        params["p_url"] = job.url
        params["p_url_hash"] = job.url_hash
    if job.source_channel:
        params["p_source_channel"] = job.source_channel
    if job.apply_type:
        params["p_apply_type"] = job.apply_type
    if priority_score is not None:
        params["p_priority_score"] = priority_score
    if priority_tier is not None:
        params["p_priority_tier"] = priority_tier
    if priority_version is not None:
        params["p_priority_version"] = priority_version
    if priority_signals is not None:
        params["p_priority_signals"] = priority_signals
    resp = client.rpc("upsert_job", params).execute()
    # supabase-py returns either scalar or list depending on RPC return shape; normalize.
    data = resp.data
    if isinstance(data, str):
        return data
    if isinstance(data, list) and data:
        # Some PostgREST configs wrap scalar returns.
        if isinstance(data[0], dict) and "upsert_job" in data[0]:
            return str(data[0]["upsert_job"])
        return str(data[0])
    return str(data)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Sync AutoCLI job JSON into Supabase.")
    parser.add_argument("--input", help="Path to JSON file (defaults to stdin).")
    parser.add_argument("--source", default="linkedin", help="Source label stored in DB.")
    parser.add_argument("--dry-run", action="store_true", help="Validate and summarize only.")
    parser.add_argument("--limit", type=int, default=0, help="Cap number of rows processed.")
    parser.add_argument("--supabase-url", dest="supabase_url", help="Override SUPABASE_URL.")
    parser.add_argument("--supabase-key", dest="supabase_key", help="Override Supabase key.")
    parser.add_argument(
        "--env-file",
        help="Optional path to a .env file to load (does not override existing env vars).",
    )
    parser.add_argument(
        "--disable-scoring",
        action="store_true",
        help="Skip priority scoring (useful for testing or backfill via separate script).",
    )
    args = parser.parse_args(argv)

    _auto_load_env()
    if args.env_file:
        _load_dotenv(args.env_file)

    raw_text = ""
    if args.input:
        raw_text = open(args.input, "r", encoding="utf-8").read()
    else:
        raw_text = sys.stdin.read()

    try:
        doc = json.loads(raw_text)
    except Exception as exc:
        print(f"ERROR: invalid JSON input: {exc}", file=sys.stderr)
        return 2

    try:
        records = _extract_records(doc)
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    if args.limit and args.limit > 0:
        records = records[: args.limit]

    normalized: list[NormalizedJob] = []
    scored: list[tuple[NormalizedJob, dict[str, Any] | None]] = []
    skipped = 0
    for idx, rec in enumerate(records):
        job = normalize_job(args.source, rec)
        if job is None:
            skipped += 1
            print(
                f"WARN: skipping row {idx}: missing identity (need apply_url/external_url or job_title+company_name)",
                file=sys.stderr,
            )
            continue
        normalized.append(job)
        score_result = None
        if not args.disable_scoring:
            try:
                score_result = score_job(rec)
            except Exception:
                # Scoring is non-critical -- log and continue without it
                pass
        scored.append((job, score_result))

    if args.dry_run:
        # Group by canonical_job_url to find duplicates
        from collections import defaultdict

        url_groups: dict[str, list[NormalizedJob]] = defaultdict(list)
        for job in normalized:
            url_groups[job.identity_hash].append(job)

        duplicate_groups: list[dict[str, Any]] = []
        for id_hash, jobs in url_groups.items():
            if len(jobs) > 1:
                duplicate_groups.append(
                    {
                        "identity_hash": id_hash,
                        "count": len(jobs),
                        "job_title": jobs[0].job_title,
                        "company_name": jobs[0].company_name,
                        "apply_urls": sorted(set(j.apply_url for j in jobs)),
                        "external_urls": sorted(set(j.external_url for j in jobs)),
                    }
                )

        report: dict[str, Any] = {
            "source": args.source,
            "input_rows": len(records),
            "will_process": len(normalized),
            "skipped": skipped,
            "canonical_distinct_jobs": len(url_groups),
            "duplicate_groups": len(duplicate_groups),
            "scoring": not args.disable_scoring,
        }
        if not args.disable_scoring:
            scored_results = [
                r for _, r in scored if r is not None
            ]
            if scored_results:
                scores = [r.score for r in scored_results]
                tiers = [r.tier for r in scored_results]
                report["priority_scores"] = {
                    "min": round(min(scores), 1),
                    "max": round(max(scores), 1),
                    "avg": round(sum(scores) / len(scores), 1),
                    "total_scored": len(scored_results),
                }
                tier_counts: dict[str, int] = {}
                for t in tiers:
                    tier_counts[t] = tier_counts.get(t, 0) + 1
                report["priority_tier_distribution"] = tier_counts
        if duplicate_groups:
            report["duplicates"] = duplicate_groups

        print(json.dumps(report, indent=2, ensure_ascii=False))
        return 0

    try:
        client = _create_supabase_client(args.supabase_url, args.supabase_key)
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    upserted = 0
    for idx, (job, score_result) in enumerate(scored):
        try:
            if score_result is not None:
                _ = upsert_job(
                    client,
                    job,
                    priority_score=score_result.score,
                    priority_tier=score_result.tier,
                    priority_version=score_result.version,
                    priority_signals=score_result.signals,
                )
            else:
                _ = upsert_job(client, job)
            upserted += 1
        except Exception as exc:
            print(
                f"ERROR: upsert failed for row {idx} identity_hash={job.identity_hash}: {exc}",
                file=sys.stderr,
            )
            return 1

    scored_count = sum(1 for _, r in scored if r is not None)
    print(
        json.dumps(
            {
                "source": args.source,
                "input_rows": len(records),
                "upserted": upserted,
                "scored": scored_count,
                "skipped": skipped,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
