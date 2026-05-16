import re
import unittest
from typing import Any

from scripts.sync_autocli_jobs import (
    _canonicalize_url,
    _extract_canonical_job_url,
    _is_linkedin_url,
    _is_ats_url,
    normalize_job,
)


class TestUrlHelpers(unittest.TestCase):
    def test_is_linkedin_url_true(self) -> None:
        self.assertTrue(_is_linkedin_url("https://www.linkedin.com/jobs/view/123"))
        self.assertTrue(_is_linkedin_url("https://linkedin.com/jobs/view/123"))
        self.assertTrue(_is_linkedin_url("http://linkedin.com/jobs/view/123"))

    def test_is_linkedin_url_false(self) -> None:
        self.assertFalse(_is_linkedin_url("https://example.wd12.myworkdayjobs.com/job/123"))
        self.assertFalse(_is_linkedin_url(""))
        self.assertFalse(_is_linkedin_url(None))

    def test_is_ats_url_true(self) -> None:
        self.assertTrue(_is_ats_url("https://example.wd12.myworkdayjobs.com/job/123"))
        self.assertTrue(_is_ats_url("https://jobs.lever.co/company/role"))
        self.assertTrue(_is_ats_url("https://boards.greenhouse.io/company/jobs/123"))
        self.assertTrue(_is_ats_url("https://example.recruitee.com/job/123"))
        self.assertTrue(_is_ats_url("https://example.applytojob.com/apply/123"))

    def test_is_ats_url_false(self) -> None:
        self.assertFalse(_is_ats_url("https://www.linkedin.com/jobs/view/123"))
        self.assertFalse(_is_ats_url("https://linkedin.com/jobs/view/123"))
        self.assertFalse(_is_ats_url("http://example.com/random"))
        self.assertFalse(_is_ats_url(""))
        self.assertFalse(_is_ats_url(None))

    def test_canonicalize_url_lowercases_scheme_and_host(self) -> None:
        result = _canonicalize_url("HTTPS://EXAMPLE.COM/Job/123")
        self.assertEqual(result, "https://example.com/Job/123")

    def test_canonicalize_url_strips_trailing_slash(self) -> None:
        result = _canonicalize_url("https://example.com/job/123/")
        self.assertEqual(result, "https://example.com/job/123")

    def test_canonicalize_url_strips_tracking_params(self) -> None:
        result = _canonicalize_url(
            "https://example.wd12.myworkdayjobs.com/job/123?source=linkedin&share_id=abc123"
        )
        self.assertEqual(result, "https://example.wd12.myworkdayjobs.com/job/123")

    def test_canonicalize_url_strips_utm_params(self) -> None:
        result = _canonicalize_url(
            "https://careers.example.com/job/456?utm_source=linkedin&utm_medium=social&keep=abc"
        )
        self.assertEqual(result, "https://careers.example.com/job/456?keep=abc")

    def test_canonicalize_url_strips_gh_src(self) -> None:
        result = _canonicalize_url(
            "https://boards.greenhouse.io/company/jobs/123?gh_src=abc123"
        )
        self.assertEqual(result, "https://boards.greenhouse.io/company/jobs/123")

    def test_canonicalize_url_strips_lever_source(self) -> None:
        result = _canonicalize_url(
            "https://jobs.lever.co/company/role?lever-source=linkedin"
        )
        self.assertEqual(result, "https://jobs.lever.co/company/role")

    def test_canonicalize_url_keeps_stable_query_params(self) -> None:
        result = _canonicalize_url(
            "https://example.wd12.myworkdayjobs.com/job/123?jobId=456&source=linkedin"
        )
        self.assertEqual(result, "https://example.wd12.myworkdayjobs.com/job/123?jobId=456")

    def test_canonicalize_url_empty(self) -> None:
        self.assertEqual(_canonicalize_url(""), "")
        self.assertEqual(_canonicalize_url(None), "")

    def test_extract_canonical_prefers_ats_external_url(self) -> None:
        """When external_url is an ATS URL and apply_url is LinkedIn, use external_url."""
        result = _extract_canonical_job_url(
            apply_url="https://www.linkedin.com/jobs/view/123",
            external_url="https://example.wd12.myworkdayjobs.com/job/456",
        )
        self.assertEqual(result, "https://example.wd12.myworkdayjobs.com/job/456")

    def test_extract_canonical_uses_ats_apply_url_when_no_external(self) -> None:
        """When no external_url but apply_url is an ATS URL, use apply_url."""
        result = _extract_canonical_job_url(
            apply_url="https://boards.greenhouse.io/company/jobs/123",
            external_url="",
        )
        self.assertEqual(result, "https://boards.greenhouse.io/company/jobs/123")

    def test_extract_canonical_uses_linkedin_as_last_resort(self) -> None:
        """When no external_url and apply_url is LinkedIn, still use apply_url."""
        result = _extract_canonical_job_url(
            apply_url="https://www.linkedin.com/jobs/view/123",
            external_url="",
        )
        self.assertEqual(result, "https://www.linkedin.com/jobs/view/123")

    def test_extract_canonical_returns_empty_when_none(self) -> None:
        result = _extract_canonical_job_url(apply_url="", external_url="")
        self.assertEqual(result, "")

    def test_extract_canonical_canonicalizes_result(self) -> None:
        """Result should be canonicalized (normalized host, stripped trailing slash, etc)."""
        result = _extract_canonical_job_url(
            apply_url="https://www.linkedin.com/jobs/view/123",
            external_url="HTTPS://EXAMPLE.WD12.MYWORKDAYJOBS.COM/Job/456/?source=linkedin",
        )
        self.assertEqual(result, "https://example.wd12.myworkdayjobs.com/Job/456")


class TestNormalizeJobDedup(unittest.TestCase):
    """Regression tests for deduplication of same ATS job arriving via different URLs."""

    def test_same_workday_job_produces_same_identity_hash(self) -> None:
        """Ameresco case: same Workday URL, different apply_url shape → same identity_hash.

        Record A: has LinkedIn apply_url + Workday external_url
        Record B: has same Workday external_url, no apply_url
        Both must produce the same identity_hash.
        """
        workday_url = "https://ameresco.wd1.myworkdayjobs.com/en-US/Ameresco_Careers/job/Ameresco-Senior-Developer"
        linkedin_url = "https://www.linkedin.com/jobs/view/1234567890"

        job_a = normalize_job(
            "linkedin",
            {
                "apply_url": linkedin_url,
                "external_url": workday_url,
                "job_title": "Senior Developer",
                "company_name": "Ameresco",
                "location": "Framingham, MA",
            },
        )
        job_b = normalize_job(
            "linkedin",
            {
                "external_url": workday_url,
                "job_title": "Senior Developer",
                "company_name": "Ameresco",
                "location": "Framingham, MA",
            },
        )

        self.assertIsNotNone(job_a)
        self.assertIsNotNone(job_b)
        assert job_a is not None and job_b is not None
        self.assertEqual(
            job_a.identity_hash,
            job_b.identity_hash,
            "Same Workday URL with different apply_url shapes must produce the same identity_hash",
        )

    def test_different_ats_urls_produce_different_hashes(self) -> None:
        """Different ATS URLs should still produce different identity hashes."""
        job_a = normalize_job(
            "linkedin",
            {
                "external_url": "https://company.wd1.myworkdayjobs.com/job/111",
                "job_title": "Engineer",
                "company_name": "Acme",
                "location": "Remote",
            },
        )
        job_b = normalize_job(
            "linkedin",
            {
                "external_url": "https://company.wd1.myworkdayjobs.com/job/222",
                "job_title": "Engineer",
                "company_name": "Acme",
                "location": "Remote",
            },
        )

        self.assertIsNotNone(job_a)
        self.assertIsNotNone(job_b)
        assert job_a is not None and job_b is not None
        self.assertNotEqual(
            job_a.identity_hash,
            job_b.identity_hash,
            "Different ATS URLs must produce different identity hashes",
        )

    def test_tracking_params_in_url_dont_change_identity(self) -> None:
        """Same Workday URL with/without tracking params → same identity_hash."""
        job_a = normalize_job(
            "linkedin",
            {
                "external_url": "https://ameresco.wd1.myworkdayjobs.com/Job/123",
                "job_title": "Dev",
                "company_name": "Co",
                "location": "Remote",
            },
        )
        job_b = normalize_job(
            "linkedin",
            {
                "external_url": "https://ameresco.wd1.myworkdayjobs.com/Job/123?source=linkedin&utm_campaign=recruiting",
                "job_title": "Dev",
                "company_name": "Co",
                "location": "Remote",
            },
        )

        self.assertIsNotNone(job_a)
        self.assertIsNotNone(job_b)
        assert job_a is not None and job_b is not None
        self.assertEqual(
            job_a.identity_hash,
            job_b.identity_hash,
            "Tracking params must not affect identity hash",
        )

    def test_existing_identity_via_apply_url_still_works(self) -> None:
        """Non-LinkedIn, non-ATS apply_url still produces identity."""
        job = normalize_job(
            "linkedin",
            {
                "apply url": "https://example.com/apply/123",
                "job_title": "Engineer",
                "company_name": "Acme",
                "location": "Remote",
            },
        )
        self.assertIsNotNone(job)
        assert job is not None
        self.assertNotEqual(job.identity_hash, "")

    def test_linkedin_apply_url_preserved_as_metadata(self) -> None:
        """LinkedIn URL should still be stored as apply_url, just not used for identity."""
        job = normalize_job(
            "linkedin",
            {
                "apply_url": "https://www.linkedin.com/jobs/view/999",
                "external_url": "https://careers.example.com/job/555",
                "job_title": "Engineer",
                "company_name": "Acme",
                "location": "Remote",
            },
        )
        self.assertIsNotNone(job)
        assert job is not None
        self.assertEqual(job.apply_url, "https://www.linkedin.com/jobs/view/999")
        self.assertEqual(job.external_url, "https://careers.example.com/job/555")


if __name__ == "__main__":
    unittest.main()
