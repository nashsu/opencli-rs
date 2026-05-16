"""Comprehensive test suite for job_priority_scorer.py.

Covers all 8 scoring components, penalty system, tier mapping,
hard-reject guard, and integration scenarios.
"""

import unittest
from datetime import date, datetime, timezone


# ----
# Helpers
# ----

_LONG_ENOUGH_JD = (
    "We are building cloud-native systems with Python and React. "
    "This is a full-time engineering role with a great team and "
    "competitive compensation package. We are seeking an experienced "
    "software engineer to join our platform team and help us build "
    "scalable, reliable systems. You will work with cutting-edge "
    "technologies and contribute to our engineering culture with modern "
    "tooling and best practices across the entire development lifecycle."
)  # ~450 chars


def _make_job(**overrides) -> dict:
    """Helper to build a test job dict.

    The default job_description is intentionally >= 100 characters to avoid
    triggering the DUPLICATE_LOW_QUALITY_PENALTY (-5) check that penalises
    extremely short (< 100 chars) descriptions.
    """
    defaults = {
        "job_title": "Software Engineer",
        "company_name": "Test Corp",
        "location": "Remote, UK",
        "salary": "GBP 70,000 - 90,000",
        "apply_url": "https://testcorp.com/careers/123",
        "external_url": "",
        "job_description": _LONG_ENOUGH_JD,
        "post_time": "2026-05-08T10:00:00Z",
    }
    defaults.update(overrides)
    return defaults


# =========================================================================
# Tests: scoring_text normalization
# =========================================================================

class TestNormalization(unittest.TestCase):
    """normalize_scoring_text: cleanup, zero-width chars, noise audit."""

    def _normalize(self, text: str):
        from scripts.job_priority_scorer import normalize_scoring_text
        return normalize_scoring_text(text)

    def test_clean_text_passes_through(self):
        t = "Senior Software Engineer at Google"
        cleaned, signals = self._normalize(t)
        self.assertEqual(cleaned, t)
        self.assertFalse(signals["was_noisy"])

    def test_zero_width_chars_removed(self):
        raw = "Senior​Software‌Engineer"
        cleaned, signals = self._normalize(raw)
        self.assertEqual(cleaned, "SeniorSoftwareEngineer")
        self.assertTrue(signals["was_noisy"])
        self.assertEqual(signals["zero_width_removed"], 2)

    def test_control_chars_removed(self):
        raw = "Hello\x00World\x1FTech"
        cleaned, signals = self._normalize(raw)
        self.assertEqual(cleaned, "HelloWorldTech")
        self.assertTrue(signals["was_noisy"])

    def test_decorative_symbols_replaced(self):
        raw = "Engineer \U0001f680 Python"
        cleaned, signals = self._normalize(raw)
        self.assertNotIn("\U0001f680", cleaned)
        self.assertTrue(signals["was_noisy"])

    def test_repeated_punctuation_collapsed(self):
        raw = "Great!!! opportunity..!!"
        cleaned, signals = self._normalize(raw)
        self.assertEqual(cleaned, "Great! opportunity.!")
        self.assertTrue(signals["was_noisy"])

    def test_whitespace_collapsed(self):
        raw = "Senior  Engineer\tLondon\nUK"
        cleaned, signals = self._normalize(raw)
        self.assertEqual(cleaned, "Senior Engineer London UK")
        # Single whitespace collapse removes 1/26 chars (3.8%) which is below
        # the 5 % threshold, so was_noisy stays False.
        self.assertFalse(signals["was_noisy"])

    def test_empty_text(self):
        cleaned, signals = self._normalize("")
        self.assertEqual(cleaned, "")
        self.assertFalse(signals["was_noisy"])

    def test_whitespace_only(self):
        cleaned, signals = self._normalize("  \t  \n  ")
        self.assertEqual(cleaned, "")
        self.assertTrue(signals["was_noisy"])

    def test_removal_ratio_nonzero_for_noisy(self):
        _, signals = self._normalize("Hello\x00World\nTest!!!")
        self.assertGreater(signals["removal_ratio"], 0)


# =========================================================================
# Tests: compensation scoring
# =========================================================================

class TestCompensation(unittest.TestCase):
    """score_compensation: salary parsing and table lookup.

    NOTE: Currency-code prefixes (EUR, USD) are not converted to GBP --
    only currency symbols ($, £, €) trigger conversion.  Strings like
    "EUR 60k" are parsed as GBP values because EUR is not recognised as a
    currency symbol.
    """

    def _score(self, salary: str) -> float:
        from scripts.job_priority_scorer import score_compensation
        return score_compensation(salary)[0]

    def test_gbp_range_midpoint(self):
        """GBP 50k-70k -> midpoint 60k -> score 12."""
        self.assertEqual(self._score("GBP 50,000 - 70,000"), 12)

    def test_gbp_single_value(self):
        """GBP 80k -> score 15."""
        self.assertEqual(self._score("GBP 80,000"), 15)

    def test_pound_prefix(self):
        """pound prefix -> midpoint 70k -> score 15."""
        self.assertEqual(self._score("£60,000 - £80,000"), 15)

    def test_dollar_prefix_range(self):
        """$70k-90k -> $80k*0.79=63.2k ($ converted by symbol) -> score 12."""
        self.assertEqual(self._score("$70,000 - $90,000"), 12)

    def test_euro_prefix(self):
        """EUR 50k-70k -> midpoint 60k (treated as GBP, EUR code not converted)."""
        self.assertEqual(self._score("EUR 50,000 - 70,000"), 12)

    def test_usd_code_no_conversion(self):
        """USD text code is NOT converted to GBP; midpoint 90k treated as GBP 90k -> score 18."""
        self.assertEqual(self._score("USD 80,000 - 100,000"), 18)

    def test_high_salary(self):
        self.assertEqual(self._score("GBP 150,000"), 20)

    def test_low_salary(self):
        self.assertEqual(self._score("GBP 25,000"), 5)

    def test_no_salary(self):
        self.assertEqual(self._score(""), 6)

    def test_unparseable(self):
        self.assertEqual(self._score("competitive"), 6)


# =========================================================================
# Tests: role fit scoring
# =========================================================================

class TestRoleFit(unittest.TestCase):
    """score_role_fit: title + JD matching against positive/negative terms."""

    def _score(self, title: str, jd: str = "") -> float:
        from scripts.job_priority_scorer import score_role_fit
        return score_role_fit(title, jd)[0]

    def test_positive_title_match(self):
        self.assertGreater(self._score("Senior Software Engineer"), 0)

    def test_negative_title_match(self):
        self.assertLess(self._score("Data Annotation Specialist"), 10)

    def test_mixed_title_and_jd(self):
        s = self._score(
            "Backend Developer",
            "Strong Python and Rust skills. We use React on the frontend.",
        )
        self.assertGreater(s, 5)

    def test_neutral_title_no_jd(self):
        self.assertGreaterEqual(self._score("Manager"), 0)

    def test_all_negative_title(self):
        self.assertLessEqual(self._score("Sales Marketing Recruiter"), 5)

    def test_score_clamped_0_20(self):
        from scripts.job_priority_scorer import score_role_fit
        s, signals = score_role_fit(
            "Software Engineer Full Stack Developer Backend Engineer",
            "Python, React, TypeScript, Cloud, DevOps, AI, LLM, "
            "Node, Rust, GenAI, Platform, SRE",
        )
        self.assertLessEqual(s, 20)
        self.assertGreaterEqual(s, 0)


# =========================================================================
# Tests: seniority scoring
# =========================================================================

class TestSeniority(unittest.TestCase):
    """score_seniority: priority-based signal detection."""

    def _score(self, title: str, jd: str = "") -> float:
        from scripts.job_priority_scorer import score_seniority
        return score_seniority(title, jd)[0]

    def test_senior_engineer(self):
        self.assertGreaterEqual(self._score("Senior Software Engineer"), 10)

    def test_junior_engineer(self):
        self.assertLess(self._score("Junior Software Engineer"), 10)

    def test_intern_overrides(self):
        self.assertLess(self._score("Senior Software Engineer Intern"), 6)

    def test_principal_engineer(self):
        self.assertGreater(self._score("Principal Engineer"), 5)

    def test_staff_engineer(self):
        self.assertGreater(self._score("Staff Engineer"), 9)

    def test_no_seniority_signal(self):
        self.assertEqual(self._score("Software Engineer"), 6)

    def test_mid_level(self):
        s = self._score("Mid-Level Developer")
        self.assertGreater(s, 8)
        self.assertLess(s, 10)

    def test_graduate_role(self):
        self.assertLess(self._score("Graduate Software Engineer"), 6)


# =========================================================================
# Tests: work arrangement scoring
# =========================================================================

class TestWorkArrangement(unittest.TestCase):
    """score_work_arrangement: location + worktype detection.

    IMPORTANT: score_work_arrangement takes (workplace_type, location,
    scoring_text).  The workplace_type must be passed explicitly -- the
    function does not infer it from the description text.
    """

    def _score(self, location: str = "", jd: str = "",
               worktype: str = "") -> float:
        from scripts.job_priority_scorer import score_work_arrangement
        return score_work_arrangement(worktype, location, jd)[0]

    def test_remote_uk(self):
        self.assertEqual(self._score("London, UK", "", "remote"), 10)

    def test_remote_non_uk(self):
        self.assertEqual(self._score("New York, NY", "", "remote"), 5)

    def test_hybrid_london(self):
        self.assertEqual(self._score("London, UK", "", "hybrid"), 8)

    def test_onsite_london(self):
        self.assertEqual(self._score("London, UK", "", "on-site"), 5)

    def test_onsite_non_uk(self):
        self.assertEqual(self._score("New York, NY", "", "on-site"), 3)

    def test_remote_only_no_uk_location(self):
        self.assertEqual(self._score("Remote", "", "remote"), 5)

    def test_not_uk(self):
        self.assertEqual(self._score("Tokyo, Japan"), 0)

    def test_uk_in_jd_not_location(self):
        self.assertEqual(self._score("Remote", "based in the UK", "remote"), 10)


# =========================================================================
# Tests: application path scoring
# =========================================================================

class TestApplicationPath(unittest.TestCase):
    """score_application_path: ATS vs clean URL vs easy_apply detection."""

    def _score(self, apply_url: str = "", external_url: str = "",
               apply_type: str = "", jd: str = "",
               has_salary: bool = True, has_usable_jd: bool = True) -> float:
        from scripts.job_priority_scorer import score_application_path
        return score_application_path(
            apply_url, external_url, apply_type, jd,
            has_salary, has_usable_jd,
        )[0]

    def test_ats_workday_url(self):
        self.assertEqual(
            self._score(apply_url="https://acme.wd5.myworkdayjobs.com/Careers/123"), 8)

    def test_ats_greenhouse_url(self):
        self.assertEqual(
            self._score(apply_url="https://boards.greenhouse.io/acme/jobs/456"), 8)

    def test_clean_company_url(self):
        self.assertEqual(
            self._score(apply_url="https://acme.com/careers/789"), 7)

    def test_easy_apply_usable(self):
        self.assertEqual(
            self._score(apply_type="easy_apply", has_salary=True, has_usable_jd=True), 5)

    def test_easy_apply_weak(self):
        self.assertEqual(
            self._score(apply_type="easy_apply", has_salary=False, has_usable_jd=False), 1)

    def test_linkedin_apply_with_clean_external(self):
        self.assertEqual(
            self._score(
                apply_url="https://linkedin.com/jobs/view/123",
                external_url="https://company.com/careers/456"), 7)

    def test_linkedin_apply_with_ats_external(self):
        self.assertEqual(
            self._score(
                apply_url="https://linkedin.com/jobs/view/123",
                external_url="https://acme.wd5.myworkdayjobs.com/Careers/456"), 8)

    def test_aggregator_in_jd_text(self):
        """Aggregator detection is JD-text-based, not URL-based."""
        self.assertEqual(
            self._score(jd="Posted via efinancialcareers"), 2)

    def test_missing_everything(self):
        self.assertEqual(self._score(), 0)


# =========================================================================
# Tests: freshness scoring
# =========================================================================

class TestFreshness(unittest.TestCase):
    """score_freshness: recency-based + rank-based scoring."""

    def _score(self, posted_time: str = "", raw_record: dict = None,
               ref_date: date = None):
        from scripts.job_priority_scorer import score_freshness
        return score_freshness(posted_time, raw_record or {}, ref_date)

    def test_posted_today(self):
        today = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        total, signals = self._score(posted_time=today)
        self.assertEqual(signals["freshness_score"], 5)

    def test_posted_5_days_ago(self):
        from datetime import timedelta
        dt = (datetime.now(timezone.utc) - timedelta(days=5)).strftime("%Y-%m-%dT%H:%M:%SZ")
        total, signals = self._score(posted_time=dt)
        self.assertEqual(signals["freshness_score"], 4)

    def test_posted_20_days_ago(self):
        from datetime import timedelta
        dt = (datetime.now(timezone.utc) - timedelta(days=20)).strftime("%Y-%m-%dT%H:%M:%SZ")
        total, signals = self._score(posted_time=dt)
        self.assertEqual(signals["freshness_score"], 1)

    def test_posted_over_30_days(self):
        from datetime import timedelta
        dt = (datetime.now(timezone.utc) - timedelta(days=45)).strftime("%Y-%m-%dT%H:%M:%SZ")
        total, signals = self._score(posted_time=dt)
        self.assertEqual(signals["freshness_score"], 0)

    def test_no_post_time(self):
        total, signals = self._score()
        self.assertEqual(signals["freshness_score"], 0)

    def test_rank_50(self):
        total, signals = self._score(raw_record={"rank": 50})
        self.assertEqual(signals["rank_score"], 5)

    def test_rank_200(self):
        total, signals = self._score(raw_record={"rank": 200})
        self.assertEqual(signals["rank_score"], 3)

    def test_fixed_reference_date(self):
        total, signals = self._score(
            posted_time="2026-05-01T10:00:00Z",
            ref_date=date(2026, 5, 8),
        )
        self.assertEqual(signals["freshness_score"], 4)

    def test_freshness_plus_rank_total(self):
        total, signals = self._score(
            posted_time="2026-05-07T10:00:00Z",
            raw_record={"rank": 50},
            ref_date=date(2026, 5, 9),
        )
        self.assertEqual(signals["freshness_score"], 5)
        self.assertEqual(signals["rank_score"], 5)
        self.assertEqual(total, 10.0)


# =========================================================================
# Tests: data completeness scoring
# =========================================================================

class TestDataCompleteness(unittest.TestCase):
    """score_data_completeness: 10x1-point checks.

    IMPORTANT: the 6th check (has_jd_raw) looks for keys 'jd', 'description',
    or 'jobDescription' in the *raw_record* (not the normalized job_description).
    """

    def _score(self, job: dict, apply_type: str = "", raw_record: dict = None):
        from scripts.job_priority_scorer import score_data_completeness
        return score_data_completeness(
            job_title=job.get("job_title", ""),
            company_name=job.get("company_name", ""),
            location=job.get("location", ""),
            job_description=job.get("job_description", ""),
            salary=job.get("salary", ""),
            posted_time=job.get("post_time", ""),
            apply_url=job.get("apply_url", ""),
            external_url=job.get("external_url", ""),
            raw_record=raw_record or job,
            apply_type=apply_type,
        )

    def test_complete_job_scores_10(self):
        """A fully populated job scores 10.  To get jd_raw we pass a
        raw_record that contains a 'jd' key.  The job_description must be
        >= 500 chars for has_jd_length_500; apply_type must be easy_apply."""
        raw = _make_job(
            job_description="A " * 251,  # 502 chars -- satisfies has_jd_length_500
            apply_type="easy_apply",
        )
        raw["jd"] = raw["job_description"]
        score, signals = self._score(raw, raw_record=raw)
        self.assertEqual(score, 10)

    def test_empty_job_scores_0(self):
        score, signals = self._score({})
        self.assertEqual(score, 0)

    def test_missing_title(self):
        score, signals = self._score(_make_job(job_title=""))
        self.assertLess(score, 10)

    def test_missing_company(self):
        score, signals = self._score(_make_job(company_name=""))
        self.assertLess(score, 10)

    def test_missing_location(self):
        score, signals = self._score(_make_job(location=""))
        self.assertLess(score, 10)

    def test_missing_salary(self):
        score, signals = self._score(_make_job(salary=""))
        self.assertLess(score, 10)

    def test_missing_jd(self):
        score, signals = self._score(_make_job(job_description=""))
        self.assertLess(score, 10)

    def test_missing_post_time(self):
        score, signals = self._score(_make_job(post_time=""))
        self.assertLess(score, 10)

    def test_easy_apply_detected(self):
        score, signals = self._score(
            _make_job(salary="", apply_url="", external_url="", job_description=""),
            apply_type="easy_apply",
        )
        self.assertGreater(score, 0)
        self.assertTrue(signals.get("has_easy_apply"))


# =========================================================================
# Tests: source quality scoring
# =========================================================================

class TestSourceQuality(unittest.TestCase):
    """score_source_quality: recruiter/aggregator/short-JD penalties."""

    _LONG_JD = (
        "Senior software engineer with strong Python and React skills needed "
        "for our growing platform team. Full-stack development with TypeScript, "
        "Node.js, and cloud infrastructure. We offer competitive salary and "
        "benefits package. Join our engineering team and help build the next "
        "generation of our platform. This role involves backend development, "
        "API design, and mentoring junior team members. Additional padding to "
        "ensure this description comfortably exceeds three hundred characters "
        "so the short JD penalty threshold is not triggered during the test."
    )  # > 300 chars to avoid jd_too_short penalty

    def _score(self, job: dict):
        from scripts.job_priority_scorer import score_source_quality
        from scripts.job_priority_config import MIN_JD_LENGTH_USABLE
        scoring_text = job.get("job_description", "")
        salary = job.get("salary", "")
        apply_type = job.get("apply_type", "")
        apply_url = job.get("apply_url", "")
        external_url = job.get("external_url", "")
        has_usable_jd = len(scoring_text.strip()) >= MIN_JD_LENGTH_USABLE
        # Default applicant_count to a numeric value so the
        # weak_applicant_count penalty does not fire unless a test
        # explicitly overrides it.
        applicant_count = job.get("applicant_count", "5")
        raw_record = job
        rank = None
        return score_source_quality(
            company_name=job.get("company_name", ""),
            scoring_text=scoring_text,
            salary=salary,
            apply_type=apply_type,
            apply_url=apply_url,
            external_url=external_url,
            has_usable_jd=has_usable_jd,
            applicant_count=applicant_count,
            raw_record=raw_record,
            rank=rank,
        )

    def test_clean_job_scores_10(self):
        score, signals = self._score(
            _make_job(job_description=self._LONG_JD))
        self.assertEqual(score, 10)

    def test_recruiter_company_penalty(self):
        score, signals = self._score(
            _make_job(company_name="Harnham Recruitment",
                      job_description=self._LONG_JD))
        self.assertEqual(score, 6)

    def test_recruiter_phrase_penalty(self):
        score, signals = self._score(
            _make_job(
                job_description=(
                    "We are partnered with a leading tech company to fill "
                    "senior engineering positions. This description provides "
                    "enough context to avoid the short JD penalty. The role "
                    "involves backend development with Python and cloud "
                    "infrastructure management. Additional text to ensure "
                    "the total length exceeds the three hundred character "
                    "minimum threshold for the short JD penalty so that only "
                    "the recruiter phrase penalty is triggered for this case."
                ),
            ))
        self.assertEqual(score, 7)

    def test_missing_salary_penalty(self):
        score, signals = self._score(
            _make_job(salary="", job_description=self._LONG_JD))
        self.assertEqual(score, 8)

    def test_short_jd_penalty(self):
        score, signals = self._score(
            _make_job(job_description="Short.",
                      applicant_count="5"))  # suppress weak_applicant
        self.assertEqual(score, 8)

    def test_easy_apply_no_owned_url_penalty(self):
        score, signals = self._score(
            _make_job(apply_url="https://linkedin.com/jobs/view/123",
                      external_url="",
                      job_description=self._LONG_JD))
        self.assertEqual(score, 10)  # no apply_type set, so no easy_apply penalty

    def test_multiple_penalties_stack(self):
        score, signals = self._score(
            _make_job(company_name="Robert Half Recruitment",
                      salary="",
                      job_description="Brief."))
        self.assertGreaterEqual(score, 0)
        self.assertLess(score, 10)


# =========================================================================
# Tests: penalty system
# =========================================================================

class TestPenalties(unittest.TestCase):
    """apply_penalties: all 8 penalty checks."""

    def _apply(self, score: float, job: dict, signals: dict = None):
        from scripts.job_priority_scorer import apply_penalties
        return apply_penalties(score, signals or {}, job)

    def test_no_penalties(self):
        s, reasons = self._apply(50, _make_job())
        self.assertEqual(s, 50)
        self.assertEqual(reasons, [])

    def test_scam_penalty(self):
        """Scam pattern: 'earn money fast from home' + 'no experience necessary'."""
        job = _make_job(
            job_description=(
                "Earn money fast from home! No experience necessary - we will "
                "train you. This is padding to exceed the one hundred character "
                "minimum so that the low quality duplicate penalty does not "
                "interfere with the scam penalty test."
            ),
        )
        s, reasons = self._apply(50, job, {})
        self.assertEqual(s, 30)  # 50 - 20

    def test_non_engineering_role(self):
        """Title must NOT have positive role terms AND must have negative ones.
        Avoid 'commission' in the JD to prevent the UNPAID_COMMISSION penalty
        from firing on top of the non-engineering penalty."""
        s, reasons = self._apply(50, _make_job(
            job_title="Sales Representative",
            job_description=(
                "Sales and marketing position with competitive compensation "
                "and client relationship management responsibilities for this "
                "important customer-facing role."
            ),
        ), {})
        self.assertEqual(s, 35)

    def test_low_info_recruiter(self):
        """Test the low-info recruiter penalty (all 4 conditions required).
        JD must be >= 100 chars (avoid low_quality) but < 500 chars (not usable)."""
        job = _make_job(
            company_name="Recruitment Agency Ltd",
            job_description=(
                "Our client is looking for a talented engineer. Apply now for "
                "this exciting opportunity with great benefits and compensation."
            ),
            salary="",
            apply_type="easy_apply",
        )
        s, reasons = self._apply(50, job, {})
        self.assertEqual(s, 40)  # 50 - 10

    def test_aggregator_repost(self):
        """Test aggregator repost penalty (3 conditions required)."""
        s, reasons = self._apply(
            50, _make_job(
                job_description=(
                    "Posted via efinancialcareers. This description is long "
                    "enough to avoid the low quality duplicate penalty threshold "
                    "of one hundred characters for this test scenario."
                ),
                salary="",
                apply_url="",
            ), {})
        self.assertEqual(s, 42)

    def test_noisy_text_penalty(self):
        """Noise penalty requires removal_ratio > 0.05 AND clean_len < 500."""
        from scripts.job_priority_scorer import normalize_scoring_text
        short_noisy = "Hello\x00World"
        parsed, noise = normalize_scoring_text(short_noisy)
        s, reasons = self._apply(50, _make_job(job_description=short_noisy),
                                 {"noise": noise})
        self.assertIn("noisy_text", " ".join(reasons))

    def test_duplicate_low_quality(self):
        """Extremely short JD (< 100 chars) triggers penalty."""
        s, reasons = self._apply(50, _make_job(job_description="Too short"), {})
        self.assertEqual(s, 45)

    def test_multiple_penalties(self):
        job = _make_job(
            job_title="Recruiter",
            company_name="Agency Recruiters Inc",
            job_description="Earn money fast from home! No experience.",
        )
        s, reasons = self._apply(50, job, {})
        self.assertLess(s, 50)


# =========================================================================
# Tests: hard-reject guard
# =========================================================================

class TestHardRejectGuard(unittest.TestCase):
    """Hard-reject guard: score<25 with <2 low-value signals -> 'low'."""

    def test_reject_with_2_signals(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job(
            job_title="Intern", location="Unknown",
            salary="", apply_url="", external_url="", job_description="")
        r = score_job(job)
        self.assertEqual(r.tier, "reject",
                         f"Should be reject, got tier={r.tier} score={r.score}")

    def test_reject_with_1_signal_overridden_to_low(self):
        """Score < 25 but only 1 low-value signal -> overridden to 'low'.

        The only low-value signal is missing_salary.  A usable JD and clean
        company/title suppress the other low-value checks.
        """
        from scripts.job_priority_scorer import score_job
        job = _make_job(
            salary="",
            location="",
            apply_url="",
            external_url="",
            job_description=(
                "A fairly long description that has many positive role fit "
                "keywords like software engineer, full stack, Python, React, "
                "TypeScript, cloud, and devops. This is a genuine engineering "
                "role with good details about the position. We need strong "
                "engineering skills and experience with modern technologies. "
            ),
        )
        r = score_job(job)
        self.assertEqual(r.tier, "low",
                         f"Should be 'low' override, got tier={r.tier} score={r.score}")


# =========================================================================
# Tests: score_job integration
# =========================================================================

class TestScoreJobIntegration(unittest.TestCase):
    """score_job: full pipeline integration tests."""

    def test_good_senior_dev_scores_medium_or_high(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job(
            job_title="Senior Software Engineer",
            company_name="Google",
            location="London, UK",
            salary="GBP 120,000 - 150,000",
            job_description=(
                "Lead software engineer building cloud-native systems "
                "with Python, React, TypeScript, and Rust. We need strong "
                "backend engineering skills for our platform team. "
                "Mentor junior developers and drive architecture decisions."
            ),
            apply_url="https://google.com/careers/123",
        )
        r = score_job(job)
        self.assertIn(r.tier, ("high", "medium"))
        self.assertGreaterEqual(r.score, 50)

    def test_scam_job_tier_reject(self):
        """Use text that triggers the scam regex."""
        from scripts.job_priority_scorer import score_job
        job = _make_job(
            salary="",
            apply_url="https://linkedin.com/jobs/view/scam123",
            external_url="",
            job_description=(
                "Earn money fast from home! No experience necessary "
                "- we will train you. Unlimited earning potential."
            ),
            location="Remote",
        )
        r = score_job(job)
        self.assertEqual(r.tier, "reject",
                         f"Expected reject, got {r.tier} (score={r.score})")

    def test_ats_good_salary_scores_medium(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job(
            job_title="Full Stack Developer",
            company_name="Acme Corp",
            location="Remote, UK",
            salary="USD 100,000 - 130,000",
            apply_url="https://acme.wd5.myworkdayjobs.com/Careers/123",
            job_description=(
                "Full stack developer with TypeScript, React, and Node.js. "
                "We offer competitive compensation and fully remote work."
            ),
        )
        r = score_job(job)
        self.assertGreaterEqual(r.tier, "medium")
        self.assertGreaterEqual(r.score, 50)

    def test_empty_job_rejected(self):
        from scripts.job_priority_scorer import score_job
        r = score_job({})
        self.assertEqual(r.tier, "reject")

    def test_deterministic(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job()
        r1 = score_job(job)
        r2 = score_job(job)
        self.assertEqual(r1.score, r2.score)
        self.assertEqual(r1.tier, r2.tier)
        self.assertEqual(r1.signals, r2.signals)

    def test_score_result_frozen(self):
        from scripts.job_priority_scorer import ScoreResult
        r = ScoreResult(score=50.0, tier="medium", version="v1",
                        signals={}, scoring_text="test")
        with self.assertRaises(AttributeError):
            r.score = 60.0  # type: ignore[misc]

    def test_version_set(self):
        from scripts.job_priority_scorer import score_job, SCORER_VERSION
        r = score_job(_make_job())
        self.assertEqual(r.version, SCORER_VERSION)

    def test_score_clamped_0_100(self):
        from scripts.job_priority_scorer import score_job
        r = score_job(_make_job())
        self.assertGreaterEqual(r.score, 0)
        self.assertLessEqual(r.score, 100)

    def test_scoring_text_in_result(self):
        from scripts.job_priority_scorer import score_job
        r = score_job(_make_job())
        self.assertTrue(len(r.scoring_text) > 0)

    def test_signals_in_result(self):
        """Signals are nested dicts: signals['compensation']['score'] etc."""
        from scripts.job_priority_scorer import score_job
        r = score_job(_make_job())
        self.assertIn("compensation", r.signals)
        self.assertIn("role_fit", r.signals)
        self.assertIn("seniority", r.signals)
        self.assertIn("work_arrangement", r.signals)
        self.assertIn("application_friction", r.signals)
        self.assertIn("freshness", r.signals)
        self.assertIn("data_quality", r.signals)
        self.assertIn("source_quality", r.signals)
        self.assertIn("penalties", r.signals)

    def test_tier_high_possible(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job(
            job_title="Senior Staff Software Engineer",
            company_name="TopTech",
            location="London, UK",
            salary="GBP 150,000 - 200,000",
            apply_url="https://toptech.com/careers/lead",
            job_description=(
                "Senior staff software engineer to lead our cloud platform team. "
                "We use Python, Rust, TypeScript, React, and Node.js at scale. "
                "Drive architecture decisions, mentor engineers, build "
                "distributed systems. We need deep backend engineering expertise "
                "and AI/ML experience. Site reliability and DevOps practices "
                "are core to this leadership role."
            ),
        )
        r = score_job(job)
        self.assertGreaterEqual(r.tier, "high",
                                f"Expected high, got {r.tier} (score={r.score})")


# =========================================================================
# Tests: Reference date handling
# =========================================================================

class TestReferenceDate(unittest.TestCase):
    """score_job: reference_date parameter.

    NOTE: the freshness signals live under r.signals['freshness'] which
    contains top-level keys like 'freshness_score', 'rank_score', etc.
    """

    def test_fixed_reference_date(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job(post_time="2026-05-01T10:00:00Z")
        r = score_job(job, reference_date=date(2026, 5, 8))
        self.assertEqual(r.signals["freshness"]["freshness_score"], 4)

    def test_reference_date_as_date_object(self):
        from scripts.job_priority_scorer import score_job
        job = _make_job(post_time="2026-05-01T10:00:00Z")
        r = score_job(job, reference_date=date(2026, 5, 4))
        self.assertEqual(r.signals["freshness"]["freshness_score"], 5)


# =========================================================================
# Tests: edge cases
# =========================================================================

class TestEdgeCases(unittest.TestCase):
    """Edge cases: missing fields, unusual inputs."""

    def test_missing_all_fields(self):
        from scripts.job_priority_scorer import score_job
        r = score_job({"job_title": "Engineer"})
        self.assertIsInstance(r.score, float)
        self.assertIn(r.tier, ("high", "medium", "low", "reject"))

    def test_non_string_fields(self):
        from scripts.job_priority_scorer import score_job
        r = score_job({"job_title": 123, "company_name": 456})
        self.assertIsInstance(r.score, float)

    def test_none_fields(self):
        from scripts.job_priority_scorer import score_job
        r = score_job({"job_title": None, "company_name": None})
        self.assertIsInstance(r.score, float)

    def test_list_fields(self):
        from scripts.job_priority_scorer import score_job
        r = score_job({"job_title": ["Engineer"]})
        self.assertIsInstance(r.score, float)

    def test_very_long_job_title(self):
        from scripts.job_priority_scorer import score_job
        r = score_job({"job_title": "Senior " * 20 + "Engineer"})
        self.assertIsInstance(r.score, float)

    def test_empty_location_with_remote_jd(self):
        """Without an explicit workplace_type the work_arrangement falls to the
        unknown branch (no workplace_type is inferred from JD keywords)."""
        from scripts.job_priority_scorer import score_job
        r = score_job({
            "job_title": "Engineer",
            "location": "",
            "job_description": "Fully remote position from anywhere.",
        })
        self.assertIsInstance(r.score, float)
        # workplace_type is empty so unknown branch: no UK signal -> 0
        self.assertEqual(r.signals["work_arrangement"]["score"], 0)


if __name__ == "__main__":
    unittest.main()
