#!/usr/bin/env python3
"""
清洗 LinkedIn 职位数据并支持写入 Supabase。

功能:
  - HTML 实体解码、标签剥离、薪资解析、技能提取
  - URL 标准化（移除 tracking 参数）
  - url_hash = sha256(normalized_url) 用于去重
  - 批内去重（按 url_hash）
  - Dead letter queue（不合格数据单独输出）
  - 通过 sync_autocli_jobs.py 写入 Supabase

用法:
    # 仅清洗输出到 stdout
    python3 clean_linkedin_jobs.py input.json > output.json

    # 清洗 + 写入 Supabase
    python3 clean_linkedin_jobs.py input.json --to-db

    # 清洗 + 写入 Supabase + dead letter
    python3 clean_linkedin_jobs.py input.json --to-db --dead-letter-file dead.json
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from typing import Any
from urllib.parse import urlparse, urlunparse


# ---------------------------------------------------------------------------
# HTML / text cleaning (unchanged from original)
# ---------------------------------------------------------------------------

def clean_html_text(text: str) -> str:
    if not text:
        return ""
    html_entities = {
        "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'",
        "&apos;": "'", "&nbsp;": " ", "&mdash;": "—", "&ndash;": "–",
        "&hellip;": "...", "&copy;": "©", "&reg;": "®", "&trade;": "™",
    }
    for entity, char in html_entities.items():
        text = text.replace(entity, char)
    text = re.sub(r"&#(\d+);", lambda m: chr(int(m.group(1))) if 0 <= int(m.group(1)) <= 0x10FFFF else m.group(0), text)
    text = re.sub(r"&#x([0-9a-fA-F]+);", lambda m: chr(int(m.group(1), 16)) if 0 <= int(m.group(1), 16) <= 0x10FFFF else m.group(0), text)
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"[\r\n\t]+", " ", text)
    text = re.sub(r" +", " ", text)
    text = text.replace(" . ", ". ").replace(" , ", ", ")
    text = text.replace(" : ", ": ").replace(" ; ", "; ")
    return text.strip()


def clean_jd(jd: str) -> str:
    if not jd:
        return ""
    jd = clean_html_text(jd)
    jd = re.sub(r"https?://\S+", "", jd)
    jd = re.sub(r"\S+@\S+\.\S+", "", jd)
    jd = re.sub(r"\n{3,}", "\n\n", jd)
    return jd.strip()


def extract_keywords(jd: str) -> list[str]:
    if not jd:
        return []
    skill_patterns = [
        r"\b(Python|Java|JavaScript|TypeScript|C\+\+|Go|Rust|Scala|Kotlin|Swift)\b",
        r"\b(React|Angular|Vue|Node\.?js|Django|Flask|Spring)\b",
        r"\b(AWS|Azure|GCP|Docker|Kubernetes|Terraform)\b",
        r"\b(PostgreSQL|MySQL|MongoDB|Redis|Elasticsearch)\b",
        r"\b(Git|Linux|Agile|Scrum|JIRA)\b",
        r"\b(ML|AI|Machine Learning|Deep Learning|TensorFlow|PyTorch)\b",
        r"\b(REST|GraphQL|gRPC|Microservices)\b",
    ]
    keywords: set[str] = set()
    for pattern in skill_patterns:
        for m in re.findall(pattern, jd, re.IGNORECASE):
            keywords.add(m.lower())
    return sorted(keywords)[:20]


def is_meaningful_value(value: str) -> bool:
    if not value:
        return False
    meaningless = ["n/a", "na", "null", "none", "-", "--", "...", ""]
    if value.lower().strip() in meaningless:
        return False
    if re.match(r"^[\s\.\-\:_]+$", value):
        return False
    return True


def clean_salary(salary: str) -> dict:
    if not salary or not is_meaningful_value(salary):
        return {"raw": "", "min": None, "max": None, "currency": None, "period": None}
    raw = salary.strip()
    currency_match = re.search(r"(£|$|€|¥|USD|GBP|EUR)", salary)
    currency = currency_match.group(1) if currency_match else None
    currency_map = {"£": "GBP", "$": "USD", "€": "EUR", "¥": "CNY"}
    if currency in currency_map:
        currency = currency_map[currency]
    range_match = re.search(r"(\d+(?:,\d{3})*(?:\.\d+)?)\s*[-–—to]+\s*(\d+(?:,\d{3})*(?:\.\d+)?)", salary)
    if range_match:
        try:
            min_sal = float(range_match.group(1).replace(",", ""))
            max_sal = float(range_match.group(2).replace(",", ""))
        except ValueError:
            min_sal = max_sal = None
    else:
        single_match = re.search(r"(\d+(?:,\d{3})*(?:\.\d+)?)", salary)
        if single_match:
            try:
                min_sal = max_sal = float(single_match.group(1).replace(",", ""))
            except ValueError:
                min_sal = max_sal = None
        else:
            min_sal = max_sal = None
    period = "year"
    if "/hr" in salary.lower() or "/hour" in salary.lower():
        period = "hour"
    elif "/month" in salary.lower():
        period = "month"
    return {"raw": raw, "min": min_sal, "max": max_sal, "currency": currency, "period": period}


# ---------------------------------------------------------------------------
# URL normalization
# ---------------------------------------------------------------------------

TRACKING_PARAMS = frozenset({
    "utm_source", "utm_medium", "utm_campaign", "utm_term", "utm_content",
    "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid",
    "msclkid", "twclid", "sc_campaign", "sc_channel", "sc_content",
    "sc_medium", "sc_outcome", "sc_geo", "sc_country",
    "ref", "source", "si", "li_fat_id",
    "trk", "trackingId", "tracking_id",
})


def normalize_url(raw_url: str) -> str:
    """Remove tracking parameters and normalize a URL for dedup.

    Returns the normalized URL string, or empty string if input is empty.
    """
    if not raw_url:
        return ""
    try:
        parsed = urlparse(raw_url)
        scheme = parsed.scheme.lower()
        netloc = parsed.netloc.lower()
        if netloc.startswith("www."):
            netloc = netloc[4:]
        # Filter tracking params, preserving order
        cleaned_pairs: list[str] = []
        if parsed.query:
            for pair in parsed.query.split("&"):
                k, _, v = pair.partition("=")
                if k not in TRACKING_PARAMS:
                    cleaned_pairs.append(f"{k}={v}")
        cleaned_query = "&".join(cleaned_pairs)
        normalized = urlunparse((scheme, netloc, parsed.path, parsed.params, cleaned_query, ""))
        return normalized.rstrip("?")
    except Exception:
        return raw_url


def generate_url_hash(normalized_url: str) -> str:
    """Generate sha256 hex digest of a normalized URL."""
    if not normalized_url:
        return ""
    return hashlib.sha256(normalized_url.encode("utf-8")).hexdigest()


# ---------------------------------------------------------------------------
# Dedup helpers
# ---------------------------------------------------------------------------

def dedup_by_url_hash(records: list[dict]) -> tuple[list[dict], list[dict]]:
    """Dedup a list of records by url_hash.

    Within a batch, the first occurrence wins.
    Returns (deduped, duplicates) where duplicates are the removed items.
    """
    seen: set[str] = set()
    deduped: list[dict] = []
    duplicates: list[dict] = []
    for rec in records:
        h = rec.get("url_hash", "") or ""
        if h and h in seen:
            duplicates.append(rec)
            continue
        if h:
            seen.add(h)
        deduped.append(rec)
    return deduped, duplicates


# ---------------------------------------------------------------------------
# Validation / dead letter
# ---------------------------------------------------------------------------

def validate_record(record: dict) -> tuple[bool, str]:
    """Check if a cleaned record is valid for Supabase upsert.

    Returns (is_valid, reason) where reason explains why invalid.
    """
    if not record.get("title"):
        return False, "empty title"
    if not record.get("company"):
        return False, "empty company"
    url = record.get("url", "") or ""
    external_url = record.get("external_url", "") or ""
    if not url and not external_url:
        return False, "no url and no external_url"
    # LinkedIn records must have easy_apply=true or external_url
    if record.get("source") == "linkedin":
        easy_apply = record.get("easy_apply")
        if not external_url and not (
            easy_apply is True or str(easy_apply).lower().strip() == "true"
        ):
            return False, "linkedin record must have easy_apply=true or external_url"
    return True, ""


# ---------------------------------------------------------------------------
# Cleaning
# ---------------------------------------------------------------------------

def clean_job_record(record: dict, source_prefix: str = "linkedin") -> dict:
    """Clean a single job record. Returns the cleaned dict.

    Args:
        record: Raw input record.
        source_prefix: Source label (default 'linkedin').
                       Sets ``source`` to the label itself and
                       ``source_channel`` = ``'recommended'`` when label is
                       ``'linkedin'``, else ``'unknown'``.
    """
    cleaned: dict[str, Any] = {}
    # Keep the original input for provenance
    cleaned["raw_record"] = record

    # Basic fields
    cleaned["title"] = clean_html_text(record.get("title", ""))
    cleaned["company"] = clean_html_text(record.get("company", ""))
    cleaned["location"] = clean_html_text(record.get("location", ""))
    cleaned["workplace_type"] = record.get("workplace_type", "")

    # Salary
    salary_info = clean_salary(record.get("salary", ""))
    cleaned["salary"] = salary_info if salary_info.get("raw") else {"raw": "", "min": None, "max": None, "currency": None, "period": None}

    # Time & apply
    cleaned["posted_time"] = record.get("posted_time", "")
    cleaned["applicant_count"] = record.get("applicant_count", "")
    cleaned["easy_apply"] = (record.get("easy_apply", "false") == "true")

    # URLs — try multiple field names for the LinkedIn job URL
    raw_url = ""
    for key in ("source_url", "linkedin_url", "job_url", "url"):
        v = str(record.get(key, "") or "")
        if v:
            raw_url = v
            break
    raw_external_url = record.get("external_url", "") or ""
    cleaned["url"] = raw_url
    cleaned["external_url"] = raw_external_url

    # Normalize URL and generate hash
    dedup_target = raw_url or raw_external_url
    normalized = normalize_url(dedup_target)
    cleaned["url_normalized"] = normalized
    cleaned["url_hash"] = generate_url_hash(normalized)

    # JD
    raw_jd = record.get("jd", "")
    cleaned["jd"] = clean_jd(raw_jd)
    cleaned["skills"] = extract_keywords(raw_jd)

    # Work type
    workplace = (record.get("workplace_type") or "").lower()
    if "remote" in workplace:
        cleaned["work_type"] = "Remote"
    elif "hybrid" in workplace:
        cleaned["work_type"] = "Hybrid"
    elif "on-site" in workplace or "onsite" in workplace:
        cleaned["work_type"] = "On-site"
    else:
        cleaned["work_type"] = "Unknown"

    # Source & channel
    cleaned["source"] = source_prefix
    # LinkedIn recommended → source_channel = recommended; other sources → unknown
    if source_prefix == "linkedin":
        cleaned["source_channel"] = "recommended"
    else:
        cleaned["source_channel"] = "unknown"

    # Apply type
    raw_easy_apply = record.get("easy_apply")
    if raw_easy_apply is None:
        cleaned["apply_type"] = "unknown"
    else:
        raw_str = str(raw_easy_apply).lower().strip()
        if raw_str in ("true", "1", "yes"):
            cleaned["apply_type"] = "easy_apply"
        else:
            cleaned["apply_type"] = "external"

    cleaned["scraped_at"] = None

    return cleaned


def clean_jobs(input_data: list[dict], stats: bool = True) -> list[dict]:
    """Clean a list of job records."""
    cleaned = [clean_job_record(job) for job in input_data]

    if stats:
        stats_dict: dict[str, Any] = {
            "total": len(cleaned),
            "with_jd": sum(1 for j in cleaned if j.get("jd")),
            "with_salary": sum(1 for j in cleaned if j.get("salary", {}).get("raw")),
            "with_url_hash": sum(1 for j in cleaned if j.get("url_hash")),
            "easy_apply": sum(1 for j in cleaned if j.get("easy_apply")),
            "work_types": {},
        }
        for job in cleaned:
            wt = job.get("work_type", "Unknown")
            stats_dict["work_types"][wt] = stats_dict["work_types"].get(wt, 0) + 1
        print("=" * 50, file=sys.stderr)
        print("清洗统计:", file=sys.stderr)
        for k, v in stats_dict.items():
            if k == "work_types":
                continue
            print(f"  {k}: {v}", file=sys.stderr)
        print(f"  工作类型: {stats_dict['work_types']}", file=sys.stderr)
        print("=" * 50, file=sys.stderr)

    return cleaned


# ---------------------------------------------------------------------------
# Supabase write via sync_autocli_jobs.py
# ---------------------------------------------------------------------------

def write_to_supabase(cleaned: list[dict], dead_letter_path: str | None = None) -> int:
    """Write cleaned records to Supabase via sync_autocli_jobs.py.

    Args:
        cleaned: List of cleaned job records.
        dead_letter_path: Optional file path for dead letter queue.

    Returns:
        0 on success, 1 on failure.
    """
    # Validate and separate dead letter
    valid: list[dict] = []
    dead_letter: list[dict] = []
    for rec in cleaned:
        ok, reason = validate_record(rec)
        if ok:
            valid.append(rec)
        else:
            rec["_skip_reason"] = reason
            dead_letter.append(rec)

    if dead_letter:
        print(f"Dead letter: {len(dead_letter)} records skipped", file=sys.stderr)
        for dl in dead_letter:
            print(f"  SKIP: [{dl.get('_skip_reason', '?')}] {dl.get('title', '?')} @ {dl.get('company', '?')}", file=sys.stderr)
        if dead_letter_path:
            with open(dead_letter_path, "w", encoding="utf-8") as f:
                json.dump(dead_letter, f, ensure_ascii=False, indent=2)
            print(f"Dead letter 写入: {dead_letter_path}", file=sys.stderr)

    if not valid:
        print("没有有效记录可写入 Supabase", file=sys.stderr)
        return 0

    # Dedup by url_hash within batch
    valid, duplicates = dedup_by_url_hash(valid)
    if duplicates:
        print(f"批内去重移除: {len(duplicates)} 条重复记录", file=sys.stderr)
        if dead_letter_path and duplicates:
            existing_dead = []
            try:
                with open(dead_letter_path, "r", encoding="utf-8") as f:
                    existing_dead = json.load(f)
            except (FileNotFoundError, json.JSONDecodeError):
                pass
            for dl in sorted(duplicates, key=lambda x: x.get("url_hash", "")):
                dl["_skip_reason"] = "batch_dedup"
                existing_dead.append(dl)
            with open(dead_letter_path, "w", encoding="utf-8") as f:
                json.dump(existing_dead, f, ensure_ascii=False, indent=2)

    # Map cleaned fields to sync script format
    rows: list[dict[str, Any]] = [map_row_for_sync(rec) for rec in valid]

    # Write to temp file, pipe through sync script
    fd, tmp_path = tempfile.mkstemp(suffix=".json", prefix="linkedin_cleaned_")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(rows, f, ensure_ascii=False, indent=2)

        sync_script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "sync_autocli_jobs.py")
        # Use uv run so we get the project venv where supabase is installed
        result = subprocess.run(
            ["uv", "run", "--directory", os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
             sync_script, "--input", tmp_path, "--source", "linkedin"],
            capture_output=True, text=True,
        )
        if result.stdout:
            print(result.stdout, file=sys.stderr)
        if result.stderr:
            print(result.stderr, file=sys.stderr)
        if result.returncode != 0:
            print(f"sync_autocli_jobs.py 退出码 {result.returncode}", file=sys.stderr)
            return 1
    finally:
        os.unlink(tmp_path)

    return 0


def map_row_for_sync(cleaned: dict[str, Any]) -> dict[str, Any]:
    """Map a cleaned job record to the format expected by sync_autocli_jobs.py.

    Key mapping invariants:

    - ``url``: the actual LinkedIn job URL (for reference in DB).
    - ``url_normalized``: tracking-stripped version (only used for hash computation).
    - ``url_hash``: sha256 of the normalized URL (for dedup).
    - ``apply_url``: for *external* jobs, the external application URL;
                     for *easy_apply* jobs, empty string (stored as NULL in DB).
    """
    row: dict[str, Any] = {
        "job_title": cleaned.get("title", ""),
        "company_name": cleaned.get("company", ""),
        "location": cleaned.get("location", ""),
        "salary": cleaned.get("salary", {}).get("raw", "") if isinstance(cleaned.get("salary"), dict) else "",
        "post_time": cleaned.get("posted_time", ""),
        "external_url": cleaned.get("external_url", ""),
        "job_description": cleaned.get("jd", ""),
        "url": cleaned.get("url", ""),  # LinkedIn job URL (raw, for reference)
        "url_normalized": cleaned.get("url_normalized", ""),
        "url_hash": cleaned.get("url_hash", ""),
        "source": cleaned.get("source", ""),
        "source_channel": cleaned.get("source_channel", ""),
        "apply_type": cleaned.get("apply_type", ""),
        "raw_record": cleaned.get("raw_record"),
    }
    # apply_url: external jobs get external URL; easy_apply gets empty (NULL in DB)
    row["apply_url"] = (
        cleaned.get("external_url", "") if cleaned.get("apply_type") == "external" else ""
    )
    return row


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description="清洗 LinkedIn 职位数据")
    parser.add_argument("input", help="输入 JSON 文件路径 (或 - 表示 stdin)")
    parser.add_argument("-o", "--output", help="输出 JSON 文件路径（默认 stdout）")
    parser.add_argument("--no-stats", action="store_true", help="不显示统计")
    parser.add_argument("--to-db", action="store_true", help="写入 Supabase（通过 sync_autocli_jobs.py）")
    parser.add_argument("--dead-letter-file", help="Dead letter 输出文件路径")
    args = parser.parse_args()

    # Read input
    if args.input == "-":
        data = json.load(sys.stdin)
    else:
        with open(args.input, "r", encoding="utf-8") as f:
            data = json.load(f)

    if not isinstance(data, list):
        raise ValueError("输入必须是 JSON 数组")

    # Clean
    cleaned = clean_jobs(data, stats=not args.no_stats)

    # Write to DB or output JSON
    if args.to_db:
        rc = write_to_supabase(cleaned, dead_letter_path=args.dead_letter_file)
        raise SystemExit(rc)
    else:
        output = json.dumps(cleaned, ensure_ascii=False, indent=2)
        if args.output:
            with open(args.output, "w", encoding="utf-8") as f:
                f.write(output)
        else:
            print(output)


if __name__ == "__main__":
    main()
