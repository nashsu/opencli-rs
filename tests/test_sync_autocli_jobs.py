"""Tests for sync/upsert — TDD RED phase.

Tests for NormalizedJob dataclass field passthrough.
Database-level tests (idempotency, source_records) will be added later.
"""

import json
import unittest


def _make_raw_record(**overrides) -> dict:
    """Helper to build a raw record in the format clean_linkedin_jobs.py outputs."""
    defaults = {
        "title": "Cloud Engineer",
        "company": "Example Corp",
        "location": "Remote",
        "source": "linkedin",
        "source_channel": "recommended",
        "apply_type": "easy_apply",
        "url": "https://linkedin.com/jobs/view/123",
        "url_normalized": "https://linkedin.com/jobs/view/123",
        "url_hash": "abc123def456",
        "external_url": "https://example.com/apply",
        "easy_apply": "true",
        "jd": "We need a cloud engineer...",
        "salary": {"raw": "$100k-$150k", "min": 100000, "max": 150000, "currency": "USD", "period": "year"},
        "posted_time": "2026-05-01",
    }
    defaults.update(overrides)
    return defaults


class TestNormalizedJobFields(unittest.TestCase):
    """Tests that NormalizedJob passes through new fields."""

    def test_normalize_job_passes_source_channel(self):
        """NormalizedJob should include source_channel."""
        from scripts.sync_autocli_jobs import normalize_job

        rec = _make_raw_record(source_channel="recommended")
        job = normalize_job("linkedin", rec)

        self.assertIsNotNone(job)
        assert job is not None
        self.assertEqual(job.source_channel, "recommended")

    def test_normalize_job_passes_apply_type(self):
        """NormalizedJob should include apply_type."""
        from scripts.sync_autocli_jobs import normalize_job

        rec = _make_raw_record(apply_type="easy_apply")
        job = normalize_job("linkedin", rec)

        self.assertIsNotNone(job)
        assert job is not None
        self.assertEqual(job.apply_type, "easy_apply")

    def test_normalize_job_passes_url_hash(self):
        """NormalizedJob should include url_hash."""
        from scripts.sync_autocli_jobs import normalize_job

        rec = _make_raw_record(url_hash="xyz789")
        job = normalize_job("linkedin", rec)

        self.assertIsNotNone(job)
        assert job is not None
        self.assertEqual(job.url_hash, "xyz789")

    def test_normalize_job_missing_new_fields_defaults_empty(self):
        """Missing source_channel/apply_type/url_hash should default to empty string."""
        from scripts.sync_autocli_jobs import normalize_job

        rec = _make_raw_record()
        # Remove new fields and easy_apply (which triggers apply_type inference)
        rec.pop("source_channel", None)
        rec.pop("apply_type", None)
        rec.pop("easy_apply", None)
        rec.pop("url_hash", None)
        job = normalize_job("linkedin", rec)

        self.assertIsNotNone(job)
        assert job is not None
        self.assertEqual(job.source_channel, "")
        self.assertEqual(job.apply_type, "")
        self.assertEqual(job.url_hash, "")


if __name__ == "__main__":
    unittest.main()
