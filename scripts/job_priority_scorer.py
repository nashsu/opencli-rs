#!/usr/bin/env python3
"""Job priority scoring engine.

Contains all 8 scoring components, penalty system, tier mapping,
and the main orchestration function score_job().

All functions are pure and deterministic (same input -> same output).
No LLM calls, no API calls, no external dependencies beyond Python stdlib.
"""

from __future__ import annotations

import re
import unicodedata
from dataclasses import dataclass
from datetime import date, datetime, timezone
from typing import Any
from urllib.parse import urlparse

from scripts.job_priority_config import (
    AGGREGATOR_RE,
    AGGREGATOR_REPOST_PENALTY,
    APP_PATH_AGGREGATOR,
    APP_PATH_ATS_URL,
    APP_PATH_CLEAN_COMPANY_URL,
    APP_PATH_EASY_APPLY_USABLE,
    APP_PATH_EASY_APPLY_WEAK,
    APP_PATH_MISSING,
    ARRANGEMENT_HYBRID_UK,
    ARRANGEMENT_NOT_UK,
    ARRANGEMENT_ONSITE_OUTSIDE_TARGET,
    ARRANGEMENT_ONSITE_UK,
    ARRANGEMENT_REMOTE_UK,
    CONTROL_RE,
    DECORATIVE_SYMBOL_RE,
    DUPLICATE_LOW_QUALITY_PENALTY,
    EUR_TO_GBP,
    FRESHNESS_DAYS,
    FRESHNESS_DEFAULT_SCORE,
    GBP_TO_GBP,
    INTERN_SIGNALS,
    JUNIOR_SIGNALS,
    LOW_INFO_RECRUITER_PENALTY,
    MID_SIGNALS,
    MIN_JD_LENGTH_SHORT,
    MIN_JD_LENGTH_USABLE,
    NEGATIVE_ROLE_TERMS,
    NOISY_TEXT_PENALTY,
    NON_ENGINEERING_PENALTY,
    POSITIVE_ROLE_TERMS,
    PRINCIPAL_DIRECTOR_SIGNALS,
    RANK_DEFAULT_SCORE,
    RANK_SCORES,
    RECOGNIZED_ATS_HOSTS,
    RECRUITER_COMPANY_RE,
    RECRUITER_PHRASE_RE,
    REPEATED_PUNCT_RE,
    SALARY_MISSING_SCORE,
    SALARY_SCORE_TABLE,
    SCAM_PENALTY,
    SCORER_VERSION,
    SENIOR_SIGNALS,
    SPONSORSHIP_PENALTY,
    SPONSORSHIP_PENALTY_ENABLED,
    SQ_EASY_APPLY_NO_OWNED_URL,
    SQ_JD_TOO_SHORT,
    SQ_MISSING_SALARY,
    SQ_RECRUITER_COMPANY,
    SQ_RECRUITER_PHRASE,
    SQ_WEAK_APPLICANT_COUNT,
    TIER_THRESHOLDS,
    UNPAID_COMMISSION_PENALTY,
    USD_TO_GBP,
    ZERO_WIDTH_RE,
)


# ===========================================================================
# ScoreResult
# ===========================================================================


@dataclass(frozen=True)
class ScoreResult:
    score: float
    tier: str
    version: str
    signals: dict
    scoring_text: str


# ===========================================================================
# Internal helpers
# ===========================================================================

# fmt: off
_CURRENCY_TO_GBP = {
    "£": GBP_TO_GBP,  # £
    "$": USD_TO_GBP,
    "€": EUR_TO_GBP,  # €
}
# fmt: on

_YEARS_EXPERIENCE_RE = re.compile(r"\b\d{2}\s*\+\s*(?:years?|yrs?)\b", re.IGNORECASE)

_HANDS_ON_RE = re.compile(
    r"(?i)\b(coding|programming|implement(?:ing|s|ed)?|"
    r"develop(?:ing|s|ed)?|architect(?:ing|s|ed|ure)?|"
    r"design.*system|write.*code|build.*product|mentor|"
    r"code.?review|hands.?on|shipping|deploying)\b"
)

_SCAM_RE = re.compile(
    r"(?i)\b(earn.*money.*(?:from home|online|fast)|"
    r"make \$?\d+[k]?\s*(?:per|a|every)\s*(?:day|week|hour)|"
    r"unlimited earning|start.*today.*(?:no experience|no interview)|"
    r"no interview required|guaranteed.*(?:income|salary|pay)|"
    r"mystery shopper|data entry.*(?:from home|remote).*\d+[kK]|"
    r"bitcoin|crypto.*(?:trading|invest)|"
    r"investment opportunity|"
    r"envelope|"
    r"no experience necessary.*(?:train|earn))\b"
)

_NON_ENGINEERING_SCAM_RE = re.compile(
    r"(?i)\b(?:unpaid|volunteer|commission.?only|commission.?based|"
    r"assessment.?only|assessment.?based|"
    r"1099.*only|equity.?only)\b"
)

_SPONSORSHIP_RE = re.compile(
    r"(?i)\b(no\s+sponsorship|no\s+visa\s+sponsorship|cannot\s+sponsor|"
    r"unable\s+to\s+sponsor|no\s+longer\s+sponsor|does\s+not\s+sponsor|"
    r"sponsorship\s+not\s+available|not\s+able\s+to\s+sponsor|"
    r"no\s+.*\s+visa\s+.*\s+sponsor)\b"
)

# Priority-ordered seniority levels (most specific first).
_SENIORITY_LEVELS: list[tuple[str, frozenset[str], float]] = [
    ("intern", INTERN_SIGNALS, 2.0),
    ("principal", PRINCIPAL_DIRECTOR_SIGNALS, 7.0),
    ("senior", SENIOR_SIGNALS, 11.0),
    ("mid", MID_SIGNALS, 9.0),
    ("junior", JUNIOR_SIGNALS, 5.5),
]

_DATE_FORMATS = [
    "%Y-%m-%d",
    "%Y-%m-%dT%H:%M:%SZ",
    "%Y-%m-%dT%H:%M:%S",
    "%Y/%m/%d",
    "%d/%m/%Y",
    "%B %d, %Y",
    "%d %B %Y",
    "%Y-%m-%d %H:%M:%S",
    "%Y/%m/%d %H:%M:%S",
]


def _term_in(term: str, text: str) -> bool:
    """Check if *term* appears as a whole word in *text* (word boundaries)."""
    return bool(re.search(rf"\b{re.escape(term)}\b", text, re.IGNORECASE))


# ===========================================================================
# Pre-scoring text normalisation
# ===========================================================================


def extract_job_description(job_data: dict, raw_record: dict) -> str:
    """Extract JD text with fallback priority.

    1. job_description (from original normalisation)
    2. jobDescription (raw field)
    3. description (raw field)
    4. jd (raw field – backfill)
    5. raw_record.jd
    6. raw_record.description
    Return empty string if none found.
    """
    normalised = str(job_data.get("job_description", "") or "").strip()
    if normalised:
        return normalised
    for key in ("jobDescription", "description", "jd"):
        val = job_data.get(key)
        if val and isinstance(val, str) and val.strip():
            return val.strip()
    # Try raw_record fallback
    for key in ("jd", "description"):
        val = raw_record.get(key)
        if val and isinstance(val, str) and val.strip():
            return val.strip()
    return ""


def extract_posted_time(job_data: dict, raw_record: dict) -> str:
    """Extract posted time with fallback.

    1. post_time (already in NormalizedJob)
    2. postTime (raw)
    3. posted_date (raw)
    4. postedDate (raw)
    5. posted_time (raw)
    """
    pt = str(job_data.get("post_time", "") or "").strip()
    if pt:
        return pt
    for key in ("postTime", "posted_date", "postedDate", "posted_time"):
        val = raw_record.get(key)
        if val and isinstance(val, str) and val.strip():
            return val.strip()
    return ""


def normalize_scoring_text(raw_text: str) -> tuple[str, dict]:
    """Return (cleaned_text, noise_signals_dict).

    Steps:
    1. NFKC unicode normalize
    2. Remove zero-width chars
    3. Remove control chars
    4. Replace decorative symbols with space
    5. Collapse repeated punctuation to single
    6. Collapse repeated whitespace to single space
    7. Strip
    """
    original_len = len(raw_text)
    text = unicodedata.normalize("NFKC", raw_text)

    n_zw = len(ZERO_WIDTH_RE.findall(text))
    text = ZERO_WIDTH_RE.sub("", text)

    n_ctrl = len(CONTROL_RE.findall(text))
    text = CONTROL_RE.sub("", text)

    n_decor = len(DECORATIVE_SYMBOL_RE.findall(text))
    text = DECORATIVE_SYMBOL_RE.sub(" ", text)

    text = REPEATED_PUNCT_RE.sub(r"\1", text)

    text = re.sub(r"\s+", " ", text)

    text = text.strip()

    total_removed = original_len - len(text)
    removal_ratio = total_removed / max(original_len, 1)

    noise = {
        "original_length": original_len,
        "clean_length": len(text),
        "zero_width_removed": n_zw,
        "control_chars_removed": n_ctrl,
        "decorative_symbols_removed": n_decor,
        "total_chars_removed": total_removed,
        "removal_ratio": round(removal_ratio, 4),
        "was_noisy": removal_ratio > 0.05,
    }
    return text, noise


# ===========================================================================
# Date parsing helper
# ===========================================================================


def _parse_date(date_str: str) -> date | None:
    """Try to parse a date string using known formats.

    Returns None if parsing fails (e.g., relative dates like "2 days ago").
    """
    s = date_str.strip()
    if not s:
        return None

    # Try absolute date formats
    for fmt in _DATE_FORMATS:
        try:
            dt = datetime.strptime(s, fmt)
            return dt.date()
        except ValueError:
            continue

    # Try ISO-8601 with timezone offset (e.g. "2024-01-15T12:00:00+00:00")
    try:
        dt = datetime.fromisoformat(s)
        return dt.date()
    except (ValueError, TypeError):
        pass

    # Try numeric (Unix timestamp in seconds or milliseconds)
    try:
        ts = float(s)
        # If it looks like ms (> year 10000 threshold), divide
        if ts > 100_000_000_000:
            ts /= 1000
        return datetime.fromtimestamp(ts, tz=timezone.utc).date()
    except (ValueError, OverflowError, OSError):
        pass

    # Relative dates (can't parse without NLP; return None)
    return None


# ===========================================================================
# URL helpers
# ===========================================================================


def _is_linkedin_url(url: str) -> bool:
    if not url:
        return False
    try:
        host = urlparse(url.strip()).hostname or ""
    except Exception:
        return False
    return "linkedin.com" in host.lower()


def _is_ats_url(url: str) -> bool:
    if not url:
        return False
    try:
        host = urlparse(url.strip()).hostname or ""
    except Exception:
        return False
    host = host.lower()
    return any(ats in host for ats in RECOGNIZED_ATS_HOSTS)


# ===========================================================================
# Data-completeness helpers
# ===========================================================================


def _has_raw_jd(raw_record: dict) -> bool:
    """Check if any raw JD field exists and is non-empty."""
    for key in ("jd", "description", "jobDescription"):
        val = raw_record.get(key)
        if val and isinstance(val, str) and val.strip():
            return True
    return False


# ===========================================================================
# Compensation (0..20)
# ===========================================================================


def _parse_salary(salary_str: str) -> tuple[float | None, float | None, str]:
    """Parse salary string into (min_gbp, max_gbp, detected_currency).

    Handles £ $ € prefixes, K/k multipliers, ranges and single values.
    Returns (None, None, '') for unparseable input.
    """
    s = salary_str.strip()
    if not s:
        return None, None, ""

    # Detect currency symbol
    currency = ""
    for sym in _CURRENCY_TO_GBP:
        if sym in s:
            currency = sym
            break

    # Normalise text for parsing
    cleaned = re.sub(r"(?i)\b(?:competitive|negotiable|depends|doe"
                     r"|commensurate|up\s+to|from|range|approx)\b",
                     "", s)
    # Normalise K notation
    cleaned = re.sub(r"(?i)(\d[\d,.]*)\s*[kK]", r"\g<1>000", cleaned)
    # Strip commas inside numbers
    cleaned = re.sub(r"(?<=\d),(?=\d)", "", cleaned)

    nums = [float(n) for n in re.findall(r"\d+(?:\.\d+)?", cleaned)]
    if not nums:
        return None, None, currency

    if len(nums) >= 2:
        # Check the two biggest numbers to form a range
        sorted_nums = sorted(nums, reverse=True)
        hi = sorted_nums[0]
        lo = sorted_nums[1]
        if hi - lo < 0.01 * max(abs(hi), 1):
            # Very close – treat as a single value
            return hi, hi, currency
        return lo, hi, currency

    return nums[0], nums[0], currency


def score_compensation(salary_str: str) -> tuple[float, dict]:
    """Score compensation 0..20 from parsed salary.

    Returns (score, signals_dict).
    """
    signals: dict[str, Any] = {"raw_salary": salary_str}

    min_gbp, max_gbp, currency = _parse_salary(salary_str)

    if min_gbp is None or max_gbp is None:
        signals["score"] = SALARY_MISSING_SCORE
        signals["parseable"] = False
        signals["currency"] = currency
        return float(SALARY_MISSING_SCORE), signals

    # Convert to GBP
    rate = _CURRENCY_TO_GBP.get(currency, GBP_TO_GBP)
    min_gbp *= rate
    max_gbp *= rate

    midpoint_gbp = (min_gbp + max_gbp) / 2.0

    # Look up score in SALARY_SCORE_TABLE (first row where midpoint >= threshold)
    table_score = SALARY_MISSING_SCORE
    for threshold, score in SALARY_SCORE_TABLE:
        if midpoint_gbp >= threshold:
            table_score = score
            break

    signals["parseable"] = True
    signals["currency"] = currency
    signals["gbp_midpoint"] = round(midpoint_gbp, 2)
    signals["parsed_min_gbp"] = round(min_gbp, 2)
    signals["parsed_max_gbp"] = round(max_gbp, 2)
    signals["score"] = table_score
    return float(table_score), signals


# ===========================================================================
# Role Fit (0..20)
# ===========================================================================


def score_role_fit(title: str, scoring_text: str) -> tuple[float, dict]:
    """Score role fit 0..20 from job title and JD text.

    Title matches are weighted +3/-3, JD text matches +1/-1.
    """
    title_lower = title.lower()
    text_lower = scoring_text.lower()

    pos_title: list[str] = []
    neg_title: list[str] = []
    pos_text: list[str] = []
    neg_text: list[str] = []

    # Positive terms
    for term in POSITIVE_ROLE_TERMS:
        found_title = _term_in(term, title_lower)
        found_text = _term_in(term, text_lower)
        if found_title:
            pos_title.append(term)
        if found_text:
            pos_text.append(term)

    # Negative terms
    for term in NEGATIVE_ROLE_TERMS:
        found_title = _term_in(term, title_lower)
        found_text = _term_in(term, text_lower)
        if found_title:
            neg_title.append(term)
        if found_text:
            neg_text.append(term)

    score = (
        len(pos_title) * 3
        - len(neg_title) * 3
        + len(pos_text) * 1
        - len(neg_text) * 1
    )
    clamped = max(0.0, min(20.0, float(score)))

    signals = {
        "positive_title_matches": pos_title,
        "negative_title_matches": neg_title,
        "positive_jd_matches": [t for t in pos_text if t not in pos_title],
        "negative_jd_matches": [t for t in neg_text if t not in neg_title],
        "matched_positive": len(pos_title) + len(pos_text),
        "matched_negative": len(neg_title) + len(neg_text),
        "score": clamped,
    }
    return clamped, signals


# ===========================================================================
# Seniority (0..12)
# ===========================================================================


def _find_seniority(text: str) -> tuple[str | None, float | None]:
    """Find the highest-priority seniority level in *text*.

    Priority order: intern -> principal -> senior -> mid -> junior.
    Returns (level_name, base_score) or (None, None).
    """
    for level, signals, score in _SENIORITY_LEVELS:
        if any(_term_in(term, text) for term in signals):
            return level, score
    return None, None


def score_seniority(title: str, scoring_text: str) -> tuple[float, dict]:
    """Score seniority 0..12.

    Checks title first; if no signal, falls back to scoring_text.
    Principal level gets +2 if hands-on signals are found in the JD.
    -1 penalty if "10+ years" or similar mentioned.
    """
    level, base = _find_seniority(title)

    source = "title"
    matched_terms: list[str] = []
    if level is not None:
        # Retrieve actual matched terms for the signal set
        matched_level = level
        for lvl, sig_set, _ in _SENIORITY_LEVELS:
            if lvl == level:
                matched_terms = [t for t in sig_set if _term_in(t, title)]
                break
    else:
        # Fall back to scoring_text
        level, base = _find_seniority(scoring_text)
        if level is not None:
            source = "scoring_text"
            for lvl, sig_set, _ in _SENIORITY_LEVELS:
                if lvl == level:
                    matched_terms = [t for t in sig_set if _term_in(t, scoring_text)]
                    break

    if level is None:
        return 6.0, {
            "matched_level": None,
            "matched_terms": [],
            "score": 6.0,
            "notes": "default: no seniority signal",
        }

    # +2 hands-on bonus for principal/director
    hands_on = False
    if level == "principal":
        hands_on = bool(_HANDS_ON_RE.search(scoring_text))
        if hands_on:
            base += 2.0

    # -1 for 10+ years mentioned in JD
    years_penalty = bool(_YEARS_EXPERIENCE_RE.search(scoring_text))
    if years_penalty:
        base -= 1.0

    final = min(base, 12.0)

    return final, {
        "matched_level": level,
        "matched_terms": matched_terms,
        "source": source,
        "hands_on_bonus": hands_on,
        "years_penalty": years_penalty,
        "score": final,
    }


# ===========================================================================
# Work Arrangement (0..10)
# ===========================================================================


# UK geographical terms (excludes "remote" / "hybrid" which cause false
# positives when the location field is just "Remote" with no actual UK
# indicator).
_UK_GEO_TERMS = frozenset({"uk", "london", "england", "britain", "united kingdom", "europe"})


def _is_uk_location(location: str, scoring_text: str) -> bool:
    """Check if location or scoring_text indicates a UK-based role.

    Uses geographical terms only -- "remote"/"hybrid" in the location
    field alone do _not_ count as a UK signal (they are ambiguous).  The
    caller's workplace_type branch handles those separately.
    """
    combined = f"{location.lower()} {scoring_text.lower()}"
    return any(_term_in(t, combined) for t in _UK_GEO_TERMS)


def score_work_arrangement(
    workplace_type: str, location: str, scoring_text: str
) -> tuple[float, dict]:
    """Score work arrangement 0..10."""
    wt = workplace_type.strip().lower()
    is_uk = _is_uk_location(location, scoring_text)

    signal: dict[str, Any] = {
        "workplace_type": wt or "unknown",
        "location": location,
        "is_uk": is_uk,
    }

    if wt == "remote":
        score = float(ARRANGEMENT_REMOTE_UK) if is_uk else float(ARRANGEMENT_HYBRID_UK - 3)
        # Remote non-UK lands at 5 (hybrid - 3)
        if not is_uk:
            score = 5.0
    elif wt == "hybrid":
        score = float(ARRANGEMENT_HYBRID_UK) if is_uk else 4.0
    elif wt == "on-site":
        score = float(ARRANGEMENT_ONSITE_UK) if is_uk else float(ARRANGEMENT_ONSITE_OUTSIDE_TARGET)
    elif wt == "unknown" or not wt:
        # Empty/unknown: check JD and location for UK signals
        if is_uk:
            score = 6.0
        else:
            score = float(ARRANGEMENT_NOT_UK)
    else:
        score = float(ARRANGEMENT_NOT_UK)

    signal["score"] = score
    return float(score), signal


# ===========================================================================
# Application Path (0..8)
# ===========================================================================


def score_application_path(
    apply_url: str,
    external_url: str,
    apply_type: str,
    scoring_text: str,
    has_salary: bool,
    has_usable_jd: bool,
) -> tuple[float, dict]:
    """Score application path quality 0..8."""
    au = apply_url.strip()
    eu = external_url.strip()
    at = apply_type.strip().lower()
    is_easy_apply = at == "easy_apply"

    has_ats_url = _is_ats_url(au) or _is_ats_url(eu)
    is_linkedin = _is_linkedin_url(au) or _is_linkedin_url(eu)
    # A "clean" URL is non-empty, non-ATS, non-LinkedIn.
    # Each URL is checked independently so a LinkedIn apply_url with a
    # clean external_url (e.g. company career page) counts as clean.
    has_clean_url = (
        (bool(au) and not _is_ats_url(au) and not _is_linkedin_url(au))
        or (bool(eu) and not _is_ats_url(eu) and not _is_linkedin_url(eu))
    )

    is_aggregator = bool(AGGREGATOR_RE.search(scoring_text))

    if has_ats_url:
        score = APP_PATH_ATS_URL
        reason = "ats_url"
    elif has_clean_url:
        score = APP_PATH_CLEAN_COMPANY_URL
        reason = "clean_company_url"
    elif is_easy_apply and (has_usable_jd or has_salary):
        score = APP_PATH_EASY_APPLY_USABLE
        reason = "easy_apply_usable"
    elif is_easy_apply:
        score = APP_PATH_EASY_APPLY_WEAK
        reason = "easy_apply_weak"
    elif is_aggregator:
        score = APP_PATH_AGGREGATOR
        reason = "aggregator"
    else:
        score = APP_PATH_MISSING
        reason = "missing_application_info"

    signals = {
        "score": float(score),
        "reason": reason,
        "has_ats_url": has_ats_url,
        "has_clean_url": has_clean_url,
        "is_linkedin": is_linkedin,
        "is_aggregator": is_aggregator,
        "is_easy_apply": is_easy_apply,
    }
    return float(score), signals


# ===========================================================================
# Freshness (0..10) = Freshness (0..5) + Rank (0..5)
# ===========================================================================


def parse_reference_date(input_path: str | None) -> date | None:
    """Try to parse YYYYMMDD from --input path like output/YYYYMMDD.json."""
    if not input_path:
        return None
    m = re.search(r"(\d{4})(\d{2})(\d{2})", input_path)
    if m:
        try:
            return date(int(m.group(1)), int(m.group(2)), int(m.group(3)))
        except ValueError:
            return None
    return None


def score_freshness(
    posted_time: str,
    raw_record: dict,
    reference_date: date | None = None,
) -> tuple[float, dict]:
    """Score freshness 0..10 (0..5 freshness + 0..5 rank)."""
    signals: dict[str, Any] = {}

    # --- Freshness sub-score (0..5) ---
    parsed = _parse_date(posted_time)
    use_default_date = False
    if parsed is None:
        use_default_date = True
        freshness = float(FRESHNESS_DEFAULT_SCORE)
        days_ago = None
    else:
        ref = reference_date if reference_date is not None else date.today()
        days_ago = (ref - parsed).days if ref >= parsed else 0
        freshness = float(FRESHNESS_DEFAULT_SCORE)
        for max_days, score in FRESHNESS_DAYS:
            if days_ago <= max_days:
                freshness = float(score)
                break

    signals["days_ago"] = days_ago
    signals["freshness_score"] = freshness
    signals["use_default_date"] = use_default_date

    # --- Rank sub-score (0..5) ---
    rank_val: int | None = None
    rank_raw = raw_record.get("rank")
    if rank_raw is not None:
        try:
            rank_val = int(rank_raw)
        except (ValueError, TypeError):
            rank_val = None

    rank_score = float(RANK_DEFAULT_SCORE)
    if rank_val is not None:
        for max_rank, score in RANK_SCORES:
            if rank_val <= max_rank:
                rank_score = float(score)
                break

    signals["rank"] = rank_val
    signals["rank_score"] = rank_score

    total = freshness + rank_score
    signals["score"] = total
    return total, signals


# ===========================================================================
# Data Completeness (0..10)
# ===========================================================================


def score_data_completeness(
    job_title: str,
    company_name: str,
    location: str,
    job_description: str,
    salary: str,
    posted_time: str,
    apply_url: str,
    external_url: str,
    raw_record: dict | None = None,
    apply_type: str = "",
) -> tuple[float, dict]:
    """Score data completeness (1 point each for 10 signals, max 10)."""
    if raw_record is None:
        raw_record = {}

    # Resolve JD text for length check (fallback to raw_record JD fields)
    jd_len_text = job_description.strip()
    if not jd_len_text:
        for key in ("jd", "description", "jobDescription"):
            val = (raw_record or {}).get(key)
            if val and isinstance(val, str) and val.strip():
                jd_len_text = val.strip()
                break

    checks: list[tuple[str, bool]] = [
        ("has_title", bool(job_title.strip())),
        ("has_company", bool(company_name.strip())),
        ("has_location", bool(location.strip())),
        ("has_jd_normalized", bool(job_description.strip())),
        ("has_jd_raw", _has_raw_jd(raw_record)),
        ("has_jd_length_500", len(jd_len_text) >= MIN_JD_LENGTH_USABLE),
        ("has_salary", bool(salary.strip())),
        ("has_posted_date", bool(posted_time.strip())),
        ("has_application_url", bool(apply_url.strip() or external_url.strip())),
        ("has_easy_apply", False),  # determined below
    ]

    # Determine easy-apply signal from apply_type (preferred) or raw_record
    at = apply_type.strip().lower() if apply_type else ""
    if not at:
        at = str(raw_record.get("apply_type", "") or "").lower()
    easy_apply_raw = str(raw_record.get("easy_apply", "") or "").lower()
    is_easy_apply = at == "easy_apply" or easy_apply_raw in ("true", "1", "yes")
    checks[9] = ("has_easy_apply", is_easy_apply)

    # Determine description source
    if job_description.strip():
        desc_source = "normalized"
    elif _has_raw_jd(raw_record):
        desc_source = "raw"
    else:
        desc_source = "none"

    score = float(sum(1 for _, present in checks if present))

    signals: dict[str, Any] = dict(checks)
    signals["description_source"] = desc_source
    signals["score"] = score
    return score, signals


# ===========================================================================
# Source Quality / Recruiter Risk (0..10)
# ===========================================================================


def score_source_quality(
    company_name: str,
    scoring_text: str,
    salary: str,
    apply_type: str,
    apply_url: str,
    external_url: str,
    has_usable_jd: bool,
    applicant_count: str,
    raw_record: dict,
    rank: int | None,
) -> tuple[float, dict]:
    """Score source quality 0..10 (start at 10, subtract penalties)."""
    score = 10.0
    subtractions: dict[str, float] = {}

    # -4: recruiter company
    recruiter_company = bool(RECRUITER_COMPANY_RE.search(company_name))
    if recruiter_company:
        subtractions["recruiter_company"] = SQ_RECRUITER_COMPANY
        score += SQ_RECRUITER_COMPANY

    # -3: recruiter phrase in JD
    recruiter_phrase = bool(RECRUITER_PHRASE_RE.search(scoring_text))
    if recruiter_phrase:
        subtractions["recruiter_phrase"] = SQ_RECRUITER_PHRASE
        score += SQ_RECRUITER_PHRASE

    # -2: salary missing
    missing_salary = not bool(salary.strip())
    if missing_salary:
        subtractions["missing_salary"] = SQ_MISSING_SALARY
        score += SQ_MISSING_SALARY

    # -2: easy apply and no owned ATS/company URL
    at = apply_type.strip().lower()
    is_easy_apply = at == "easy_apply"
    au = apply_url.strip()
    eu = external_url.strip()
    has_owned_url = _is_ats_url(au) or _is_ats_url(eu) or (bool(eu) and not _is_linkedin_url(eu))
    easy_no_url = is_easy_apply and not has_owned_url
    if easy_no_url:
        subtractions["easy_apply_no_owned_url"] = SQ_EASY_APPLY_NO_OWNED_URL
        score += SQ_EASY_APPLY_NO_OWNED_URL

    # -2: JD shorter than MIN_JD_LENGTH_SHORT
    jd_too_short = len(scoring_text) < MIN_JD_LENGTH_SHORT
    if jd_too_short:
        subtractions["jd_too_short"] = SQ_JD_TOO_SHORT
        score += SQ_JD_TOO_SHORT

    # -1: applicant_count is N/A and rank is weak
    ac = applicant_count.strip().lower()
    is_na = ac in ("n/a", "na", "not applicable", "")
    weak_rank = rank is None or rank > 300
    weak_applicant = is_na and weak_rank
    if weak_applicant:
        subtractions["weak_applicant_count"] = SQ_WEAK_APPLICANT_COUNT
        score += SQ_WEAK_APPLICANT_COUNT

    clamped = max(0.0, min(10.0, score))

    signals = {
        "start_score": 10.0,
        "recruiter_company": recruiter_company,
        "recruiter_phrase": recruiter_phrase,
        "missing_salary": missing_salary,
        "easy_apply_no_owned_url": easy_no_url,
        "jd_too_short": jd_too_short,
        "weak_applicant_count": weak_applicant,
        "subtractions": subtractions,
        "score": clamped,
    }
    return clamped, signals


# ===========================================================================
# Penalties
# ===========================================================================


def _has_any_positive_role_term(title: str, scoring_text: str) -> bool:
    return any(
        _term_in(term, title) or _term_in(term, scoring_text)
        for term in POSITIVE_ROLE_TERMS
    )


def _has_any_negative_role_term(title: str, scoring_text: str) -> bool:
    return any(
        _term_in(term, title) or _term_in(term, scoring_text)
        for term in NEGATIVE_ROLE_TERMS
    )


def _is_recruiter_company(company_name: str) -> bool:
    return bool(RECRUITER_COMPANY_RE.search(company_name))


def _is_aggregator_source(scoring_text: str) -> bool:
    return bool(AGGREGATOR_RE.search(scoring_text))


def apply_penalties(
    score: float, signals: dict, job_data: dict, scoring_text: str = ""
) -> tuple[float, list[str]]:
    """Subtract penalties from *score* after component scoring.

    *scoring_text* is the pre-normalised JD text (already resolved via
    extract_job_description).  When empty, falls back to
    job_data["job_description"] for backward compatibility with direct calls.

    Returns (penalized_score, applied_penalties_list).
    """
    penalties: list[str] = []
    penalised = score

    job_title = str(job_data.get("job_title", "") or "")
    company_name = str(job_data.get("company_name", "") or "")
    salary = str(job_data.get("salary", "") or "")
    apply_url = str(job_data.get("apply_url", "") or "")
    external_url = str(job_data.get("external_url", "") or "")
    apply_type = str(job_data.get("apply_type", "") or "").lower()
    if not scoring_text:
        scoring_text = str(job_data.get("job_description", "") or "")

    # 1. SCAM_PENALTY (-20)
    if _SCAM_RE.search(scoring_text):
        penalised += SCAM_PENALTY
        penalties.append(f"scam:{SCAM_PENALTY}")

    # 2. NON_ENGINEERING_PENALTY (-15): negative terms but no positive
    has_pos = _has_any_positive_role_term(job_title, scoring_text)
    has_neg = _has_any_negative_role_term(job_title, scoring_text)
    if not has_pos and has_neg:
        penalised += NON_ENGINEERING_PENALTY
        penalties.append(f"non_engineering:{NON_ENGINEERING_PENALTY}")

    # 3. UNPAID_COMMISSION_PENALTY (-10)
    if _NON_ENGINEERING_SCAM_RE.search(scoring_text):
        penalised += UNPAID_COMMISSION_PENALTY
        penalties.append(f"unpaid_commission:{UNPAID_COMMISSION_PENALTY}")

    # 4. LOW_INFO_RECRUITER_PENALTY (-10)
    is_recruiter = _is_recruiter_company(company_name)
    missing_salary = not bool(salary.strip())
    is_easy_apply = apply_type == "easy_apply"
    jd_text = scoring_text
    usable_jd = len(jd_text.strip()) >= MIN_JD_LENGTH_USABLE
    if is_recruiter and missing_salary and is_easy_apply and not usable_jd:
        penalised += LOW_INFO_RECRUITER_PENALTY
        penalties.append(f"low_info_recruiter:{LOW_INFO_RECRUITER_PENALTY}")

    # 5. AGGREGATOR_REPOST_PENALTY (-8)
    is_aggregator = _is_aggregator_source(scoring_text)
    au = apply_url.strip()
    eu = external_url.strip()
    has_owned_url = bool(au) or bool(eu)
    if is_aggregator and missing_salary and not has_owned_url:
        penalised += AGGREGATOR_REPOST_PENALTY
        penalties.append(f"aggregator_repost:{AGGREGATOR_REPOST_PENALTY}")

    # 6. SPONSORSHIP_PENALTY (-8)
    if SPONSORSHIP_PENALTY_ENABLED and _SPONSORSHIP_RE.search(scoring_text):
        penalised += SPONSORSHIP_PENALTY
        penalties.append(f"sponsorship:{SPONSORSHIP_PENALTY}")

    # 7. NOISY_TEXT_PENALTY (-5)
    noise = signals.get("noise", {})
    removal_ratio = noise.get("removal_ratio", 0)
    clean_len = noise.get("clean_length", 0)
    if removal_ratio > 0.05 and clean_len < MIN_JD_LENGTH_USABLE:
        penalised += NOISY_TEXT_PENALTY
        penalties.append(f"noisy_text:{NOISY_TEXT_PENALTY}")

    # 8. DUPLICATE_LOW_QUALITY_PENALTY (-5)
    missing_title = not bool(job_title.strip())
    missing_company = not bool(company_name.strip())
    extremely_short_jd = len(jd_text.strip()) < 100
    if missing_title or missing_company or extremely_short_jd:
        penalised += DUPLICATE_LOW_QUALITY_PENALTY
        penalties.append(f"low_quality_duplicate:{DUPLICATE_LOW_QUALITY_PENALTY}")

    return penalised, penalties


# ===========================================================================
# Low-value signal counting (hard-reject guard)
# ===========================================================================


def _count_low_value_signals(job_data: dict, signals: dict) -> int:
    """Count how many independent low-value signals are present.

    Hard-reject requires at least 2 independent low-value signals.
    """
    count = 0
    sq = signals.get("source_quality", {})
    ap = signals.get("application_friction", {})
    dc = signals.get("data_quality", {})
    rf = signals.get("role_fit", {})

    # Recruiter-like company
    if sq.get("recruiter_company", False):
        count += 1
    # Aggregator-like source
    if ap.get("is_aggregator", False):
        count += 1
    # Missing salary
    if not str(job_data.get("salary", "") or "").strip():
        count += 1
    # Missing usable JD
    if not dc.get("has_jd_length_500", False):
        count += 1
    # Non-engineering role (no positive terms, has negative)
    if rf.get("matched_positive", 0) == 0 and rf.get("matched_negative", 0) > 0:
        count += 1
    # Easy apply only (no ATS or clean URL)
    at = str(job_data.get("apply_type", "") or "").lower()
    if at == "easy_apply" and not ap.get("has_ats_url", False) and not ap.get("has_clean_url", False):
        count += 1

    return count


# ===========================================================================
# Tier mapping
# ===========================================================================


def map_tier(score: float) -> str:
    """Map score to tier using TIER_THRESHOLDS.

    Thresholds are upper-inclusive (score >= threshold).
    """
    tiers = sorted(TIER_THRESHOLDS.items(), key=lambda x: -x[1])
    for tier, threshold in tiers:
        if score >= threshold:
            return tier
    return "reject"


# ===========================================================================
# Main orchestration
# ===========================================================================


def score_job(
    job_data: dict, reference_date: date | None = None
) -> ScoreResult:
    """Score a single job and return a ScoreResult.

    *job_data* is a dict with keys matching NormalizedJob fields:
      job_title, company_name, location, salary, post_time,
      apply_url, external_url, job_description (may be empty),
      apply_type, source_channel,
      workplace_type (optional),
      raw_record (dict with raw fields).

    Returns ScoreResult with score (0..100), tier, signals, and scoring_text.
    """
    # -- Extract structured fields from job_data --
    job_title = str(job_data.get("job_title", "") or "")
    company_name = str(job_data.get("company_name", "") or "")
    location = str(job_data.get("location", "") or "")
    salary = str(job_data.get("salary", "") or "")
    post_time = str(job_data.get("post_time", "") or "")
    apply_url = str(job_data.get("apply_url", "") or "")
    external_url = str(job_data.get("external_url", "") or "")
    job_description = str(job_data.get("job_description", "") or "")
    apply_type = str(job_data.get("apply_type", "") or "")
    source_channel = str(job_data.get("source_channel", "") or "")
    workplace_type = str(job_data.get("workplace_type", "") or "")

    raw_record = job_data.get("raw_record", {})
    if not isinstance(raw_record, dict):
        raw_record = {}

    # -- Fallback extraction --
    jd_text = extract_job_description({"job_description": job_description}, raw_record)
    posted_time = extract_posted_time({"post_time": post_time}, raw_record)

    # -- Normalise --
    scoring_text, noise_signals = normalize_scoring_text(jd_text)

    # -- Component scoring --
    comp_score, comp_signals = score_compensation(salary)

    role_score, role_signals = score_role_fit(job_title, scoring_text)

    sen_score, sen_signals = score_seniority(job_title, scoring_text)

    # Work arrangement: try job_data first, then raw_record
    if not workplace_type:
        workplace_type = str(raw_record.get("workplace_type", "") or "")
    arr_score, arr_signals = score_work_arrangement(workplace_type, location, scoring_text)

    has_salary = bool(salary.strip())
    has_usable_jd = bool(scoring_text) and len(scoring_text) >= MIN_JD_LENGTH_USABLE

    app_score, app_signals = score_application_path(
        apply_url, external_url, apply_type, scoring_text,
        has_salary, has_usable_jd,
    )

    fresh_score, fresh_signals = score_freshness(
        posted_time, raw_record, reference_date,
    )

    dc_score, dc_signals = score_data_completeness(
        job_title, company_name, location,
        job_description, salary, posted_time,
        apply_url, external_url,
        raw_record,
        apply_type=apply_type,
    )

    rank_val: int | None = None
    rank_raw = raw_record.get("rank")
    if rank_raw is not None:
        try:
            rank_val = int(rank_raw)
        except (ValueError, TypeError):
            pass

    applicant_count = str(raw_record.get("applicant_count", "") or "")

    sq_score, sq_signals = score_source_quality(
        company_name, scoring_text, salary, apply_type,
        apply_url, external_url, has_usable_jd,
        applicant_count, raw_record, rank_val,
    )

    # -- Sum component scores --
    raw_total = (
        comp_score
        + role_score
        + sen_score
        + arr_score
        + app_score
        + fresh_score
        + dc_score
        + sq_score
    )

    # -- Assemble signals --
    combined_signals: dict[str, Any] = {
        "compensation": comp_signals,
        "role_fit": role_signals,
        "seniority": sen_signals,
        "work_arrangement": arr_signals,
        "application_friction": app_signals,
        "freshness": fresh_signals,
        "data_quality": dc_signals,
        "source_quality": sq_signals,
        "noise": noise_signals,
        "penalties": [],
    }

    # -- Penalties --
    penalised_score, penalties = apply_penalties(raw_total, combined_signals, job_data, scoring_text)
    combined_signals["penalties"] = penalties

    # -- Clamp to 0..100 --
    final_score = max(0.0, min(100.0, penalised_score))
    final_score = round(final_score, 1)

    # -- Map to tier (with hard-reject guard) --
    tier = map_tier(final_score)
    if tier == "reject":
        low_count = _count_low_value_signals(job_data, combined_signals)
        if low_count < 2:
            tier = "low"

    return ScoreResult(
        score=final_score,
        tier=tier,
        version=SCORER_VERSION,
        signals=combined_signals,
        scoring_text=scoring_text,
    )
