"""Preprocessing module for raw JD text before LLM extraction.

Provides text cleaning (boilerplate removal, unicode normalisation, control
character stripping, whitespace collapsing), hashing, and input row validation.
"""

from __future__ import annotations

import hashlib
import re
import unicodedata
from typing import List

PREPROCESS_VERSION = "linkedin-jd-clean-v1"

# ---------------------------------------------------------------------------
# LinkedIn boilerplate patterns to remove
# ---------------------------------------------------------------------------
LINKEDIN_BOILERPLATE: list[str] = [
    r"Application Process \(Takes \d+ Min\).*",
    r"Easy Apply on LinkedIn.*",
    r"Check email for next steps.*",
    r"Participate in resume evaluation & interview stage.*",
]


def compute_hash(text: str) -> str:
    """Compute SHA-256 hex digest of *text*."""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def preprocess(jd_raw: str) -> tuple[str, str]:
    """Preprocess raw JD text for model consumption.

    Returns
    -------
    tuple[str, str]
        ``(jd_cleaned, cleaned_hash)``

    Cleaning rules
    --------------
    1. Remove LinkedIn boilerplate snippets.
    2. Unicode normalisation (NFKC) -- fullwidth characters -> ASCII equivalents.
    3. Strip control characters (except ``\\n``, ``\\r``, ``\\t``).
    4. Collapse three-or-more consecutive blank lines down to two.
    5. Strip leading/trailing whitespace.

    Invariant
    ---------
    ``jd_raw`` is **never** mutated.  ``jd_cleaned`` is the model-input text
    only and should not be persisted back over the original.
    """
    text = jd_raw

    # 1. Remove LinkedIn boilerplate
    for pattern in LINKEDIN_BOILERPLATE:
        text = re.sub(pattern, "", text, flags=re.IGNORECASE)

    # 2. Unicode normalisation: fullwidth -> ASCII equivalents
    text = unicodedata.normalize("NFKC", text)

    # 3. Strip control characters (keep \n, \r, \t)
    text = "".join(c for c in text if c.isprintable() or c in "\n\r\t")

    # 4. Collapse multiple blank lines to max 2
    text = re.sub(r"\n{3,}", "\n\n", text)

    # 5. Strip leading/trailing whitespace
    text = text.strip()

    return text, compute_hash(text)


def validate_input_row(row: dict) -> List[str]:
    """Validate a row from ``final.json`` has all required fields.

    Parameters
    ----------
    row : dict
        A single job entry from the input JSON file.

    Returns
    -------
    list[str]
        List of error messages.  An empty list means the row is valid.
    """
    errors: list[str] = []
    required = ["url", "title", "company", "jd"]
    for field in required:
        if field not in row or not row[field]:
            errors.append(f"missing required field: {field}")
    return errors
