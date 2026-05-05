#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys
from dataclasses import dataclass
from typing import Any, Iterable

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

    identity_source = ""
    if apply_url:
        identity_source = apply_url
    elif external_url:
        identity_source = external_url
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


def upsert_job(client, job: NormalizedJob) -> str:
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

    if args.dry_run:
        print(
            json.dumps(
                {
                    "source": args.source,
                    "input_rows": len(records),
                    "will_process": len(normalized),
                    "skipped": skipped,
                },
                indent=2,
            )
        )
        return 0

    try:
        client = _create_supabase_client(args.supabase_url, args.supabase_key)
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    upserted = 0
    for idx, job in enumerate(normalized):
        try:
            _ = upsert_job(client, job)
            upserted += 1
        except Exception as exc:
            print(
                f"ERROR: upsert failed for row {idx} identity_hash={job.identity_hash}: {exc}",
                file=sys.stderr,
            )
            return 1

    print(
        json.dumps(
            {
                "source": args.source,
                "input_rows": len(records),
                "upserted": upserted,
                "skipped": skipped,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
