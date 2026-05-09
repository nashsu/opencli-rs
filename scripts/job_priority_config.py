"""Configuration constants for job priority scoring.

All configuration is defined here -- no scoring logic.
Imported by the scorer, sync pipeline, backfill scripts, and tests.
"""

import re

# ---------------------------------------------------------------------------
# Version tracking
# ---------------------------------------------------------------------------
SCORER_VERSION = "job-priority-v1"

# ---------------------------------------------------------------------------
# Salary currency conversion (static GBP-based, no live exchange calls)
# ---------------------------------------------------------------------------
GBP_TO_GBP = 1.0
USD_TO_GBP = 0.79
EUR_TO_GBP = 0.86

# ---------------------------------------------------------------------------
# Salary midpoint (GBP) -> raw score mapping
# ---------------------------------------------------------------------------
# The scorer should convert any parsed salary to GBP, take the midpoint of
# ranges, and then pick the score from this table.  Missing/unparseable
# salaries get the default below.
SALARY_SCORE_TABLE: list[tuple[int, int]] = [
    (120_000, 20),
    (90_000, 18),
    (70_000, 15),
    (55_000, 12),
    (40_000, 8),
    (0, 5),
]

# Default compensation score when salary is absent or unparseable
SALARY_MISSING_SCORE = 6

# ---------------------------------------------------------------------------
# Compensation component
# ---------------------------------------------------------------------------
COMPENSATION_WEIGHT = 20  # max score for this component

# ---------------------------------------------------------------------------
# Role fit keywords (positive)
# ---------------------------------------------------------------------------
POSITIVE_ROLE_TERMS = frozenset({
    "software engineer",
    "full stack",
    "fullstack",
    "backend",
    "back end",
    "frontend",
    "front end",
    "platform",
    "developer",
    "typescript",
    "react",
    "node",
    "python",
    "rust",
    "ai",
    "gen ai",
    "genai",
    "llm",
    "agent",
    "cloud",
    "data engineer",
    "devops",
    "sre",
    "site reliability",
})

# ---------------------------------------------------------------------------
# Role fit keywords (negative / lower priority)
# ---------------------------------------------------------------------------
NEGATIVE_ROLE_TERMS = frozenset({
    "recruiter",
    "sales",
    "marketing",
    "data annotation",
    "trainer",
    "teacher",
    "support",
    "intern",
    "apprentice",
    "qa manual",
    "wordpress only",
})

# ---------------------------------------------------------------------------
# Seniority signal groups
# ---------------------------------------------------------------------------
SENIOR_SIGNALS = frozenset({
    "senior",
    "staff",
    "lead",
    "principal engineer",
    "staff engineer",
})

MID_SIGNALS = frozenset({
    "mid",
    "mid-level",
    "ii",
    "iii",
})

JUNIOR_SIGNALS = frozenset({
    "junior",
    "graduate",
    "new grad",
    "associate",
})

PRINCIPAL_DIRECTOR_SIGNALS = frozenset({
    "principal",
    "director",
    "cto",
    "vice president",
    "vp",
    "head of",
})

INTERN_SIGNALS = frozenset({
    "intern",
    "internship",
    "apprentice",
    "trainee",
    "unpaid",
})

# ---------------------------------------------------------------------------
# Work arrangement / location
# ---------------------------------------------------------------------------
UK_LOCATION_TERMS = frozenset({
    "uk",
    "london",
    "england",
    "britain",
    "united kingdom",
    "remote",
    "hybrid",
    "europe",
})

WORKPLACE_TYPES = frozenset({
    "remote",
    "hybrid",
    "on-site",
})

# ---------------------------------------------------------------------------
# Recognised ATS hosts (application-path scoring)
# ---------------------------------------------------------------------------
RECOGNIZED_ATS_HOSTS = frozenset({
    "workday",
    "myworkdayjobs",
    "greenhouse",
    "lever",
    "ashby",
    "smartrecruiters",
    "icims",
    "recruitee",
    "applytojob",
    "workable",
    "breezy",
    "taleo",
    "successfactors",
    "oraclecloud",
})

# ---------------------------------------------------------------------------
# Recruiter / agency company name regex (seed list)
# ---------------------------------------------------------------------------
RECRUITER_COMPANY_RE = re.compile(
    r"(?i)\b(search|recruit|recruitment|staffing|talent|harnham|anson mccade|"
    r"roc search|techohana|la fosse|opus|understanding recruitment|client server|"
    r"xcede|trg|burns sheehan|mcgregor boyall|michael page|develop|hunter bond|"
    r"oliver bernard|gravitas|mason frank|randstad|adecco|manpower|robert half|"
    r"teksystems)\b"
)

# ---------------------------------------------------------------------------
# Recruiter broadcast phrase regex (seed list)
# ---------------------------------------------------------------------------
RECRUITER_PHRASE_RE = re.compile(
    r"(?i)\b(partnered with|on behalf of|our client|my client|representing|"
    r"recruitment agency|talent partner|consultant|shortlisted|send your cv|"
    r"submit your cv|resume to|interviews are currently underway)\b"
)

# ---------------------------------------------------------------------------
# Aggregator / job-board patterns
# ---------------------------------------------------------------------------
AGGREGATOR_RE = re.compile(
    r"(?i)\b(jobs via|efinancialcareers|hackajob|huzzle|fetchjobs|bestjobtool)\b"
)

# ---------------------------------------------------------------------------
# Decorative / noisy text cleanup regexes
# ---------------------------------------------------------------------------
ZERO_WIDTH_RE = re.compile(r"[​-‍﻿]")
CONTROL_RE = re.compile(r"[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]")
DECORATIVE_SYMBOL_RE = re.compile(r"[\U0001F300-\U0001FAFF☀-➿]")
REPEATED_PUNCT_RE = re.compile(r"([!?.•●▪◦])\1{2,}")

# ---------------------------------------------------------------------------
# Tier thresholds
# ---------------------------------------------------------------------------
TIER_THRESHOLDS: dict[str, int] = {
    "high": 75,
    "medium": 50,
    "low": 25,
    "reject": 0,
}

# ---------------------------------------------------------------------------
# Penalty constants
# ---------------------------------------------------------------------------
SCAM_PENALTY = -20
NON_ENGINEERING_PENALTY = -15
UNPAID_COMMISSION_PENALTY = -10
LOW_INFO_RECRUITER_PENALTY = -10
AGGREGATOR_REPOST_PENALTY = -8
SPONSORSHIP_PENALTY = -8
NOISY_TEXT_PENALTY = -5
DUPLICATE_LOW_QUALITY_PENALTY = -5

# ---------------------------------------------------------------------------
# Sponsorship penalty control flag (default disabled for v1)
# ---------------------------------------------------------------------------
SPONSORSHIP_PENALTY_ENABLED = False

# ---------------------------------------------------------------------------
# Minimum JD length thresholds
# ---------------------------------------------------------------------------
MIN_JD_LENGTH_SHORT = 300
MIN_JD_LENGTH_USABLE = 500

# ---------------------------------------------------------------------------
# Freshness scoring (days -> score)
# ---------------------------------------------------------------------------
# Days boundaries are upper-inclusive: e.g. <= 3 days gets 5.
FRESHNESS_DAYS: list[tuple[int, int]] = [
    (3, 5),
    (7, 4),
    (14, 3),
    (30, 1),
]
# Older than the last boundary (or missing): 0
FRESHNESS_DEFAULT_SCORE = 0

# ---------------------------------------------------------------------------
# Rank scoring (rank -> score)
# ---------------------------------------------------------------------------
# Rank boundaries are upper-inclusive: e.g. rank <= 50 gets 5.
RANK_SCORES: list[tuple[int, int]] = [
    (50, 5),
    (150, 4),
    (300, 3),
    (600, 1),
]
# Missing or > 600: 0
RANK_DEFAULT_SCORE = 0

# ---------------------------------------------------------------------------
# Application path scoring levels
# ---------------------------------------------------------------------------
# These are the component scores for the application-path sub-component (0..8).
APP_PATH_ATS_URL = 8
APP_PATH_CLEAN_COMPANY_URL = 7
APP_PATH_EASY_APPLY_USABLE = 5
APP_PATH_EASY_APPLY_WEAK = 1
APP_PATH_AGGREGATOR = 2
APP_PATH_MISSING = 0

# ---------------------------------------------------------------------------
# Work arrangement / location scoring levels
# ---------------------------------------------------------------------------
ARRANGEMENT_REMOTE_UK = 10
ARRANGEMENT_HYBRID_UK = 8
ARRANGEMENT_ONSITE_UK = 5
ARRANGEMENT_ONSITE_OUTSIDE_TARGET = 3
ARRANGEMENT_NOT_UK = 0

# ---------------------------------------------------------------------------
# Source quality sub-penalties (start from 10 and subtract)
# ---------------------------------------------------------------------------
SQ_RECRUITER_COMPANY = -4
SQ_RECRUITER_PHRASE = -3
SQ_MISSING_SALARY = -2
SQ_EASY_APPLY_NO_OWNED_URL = -2
SQ_JD_TOO_SHORT = -2
SQ_WEAK_APPLICANT_COUNT = -1
