"""Tests for LinkedIn job cleaning — TDD RED phase.

Tests that are expected to fail because the corresponding
features (source_channel, apply_type) are not yet implemented.
"""

import json
import unittest


def _build_raw_record(**overrides) -> dict:
    """Helper to build a minimal raw LinkedIn job record."""
    defaults = {
        "title": "Cloud Engineer",
        "company": "Example Corp",
        "location": "Remote",
        "url": "https://www.linkedin.com/jobs/view/123",
        "external_url": "https://example.com/apply",
    }
    defaults.update(overrides)
    return defaults


class TestSourceNormalization(unittest.TestCase):
    """source=linkedin_recommended → source=linkedin, source_channel=recommended"""

    def test_linkedin_recommended_source_maps_to_linkedin(self):
        """linkedin_recommended source should become source=linkedin."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record()
        result = clean_job_record(record)

        self.assertEqual(result["source"], "linkedin")

    def test_linkedin_recommended_source_sets_channel(self):
        """linkedin_recommended source should set source_channel=recommended."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record()
        result = clean_job_record(record)

        self.assertEqual(result["source_channel"], "recommended")

    def test_other_source_preserves_channel(self):
        """Non-linkedin source should leave source_channel as unknown."""
        from scripts.clean_linkedin_jobs import clean_job_record

        # Simulate a non-LinkedIn source by setting source_prefix override
        record = _build_raw_record()
        result = clean_job_record(record, source_prefix="indeed")

        self.assertEqual(result["source"], "indeed")
        self.assertEqual(result["source_channel"], "unknown")


class TestApplyTypeMapping(unittest.TestCase):
    """easy_apply → apply_type mapping."""

    def test_easy_apply_true_maps_to_easy_apply(self):
        """easy_apply=True should set apply_type='easy_apply'."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(easy_apply="true")
        result = clean_job_record(record)

        self.assertEqual(result["apply_type"], "easy_apply")

    def test_easy_apply_false_maps_to_external(self):
        """easy_apply=False should set apply_type='external'."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(easy_apply="false")
        result = clean_job_record(record)

        self.assertEqual(result["apply_type"], "external")

    def test_missing_easy_apply_maps_to_unknown(self):
        """Missing easy_apply should set apply_type='unknown'."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record()
        # Ensure no easy_apply key at all
        record.pop("easy_apply", None)
        result = clean_job_record(record)

        self.assertEqual(result["apply_type"], "unknown")

    def test_easy_apply_boolean_true_from_raw_json(self):
        """Boolean True easy_apply from JSON should map to 'easy_apply'."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(easy_apply=True)
        result = clean_job_record(record)

        self.assertEqual(result["apply_type"], "easy_apply")


class TestRawRecordPreservation(unittest.TestCase):
    """raw_record should retain original input fields."""

    def test_raw_record_contains_original_easy_apply(self):
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(easy_apply="true")
        result = clean_job_record(record)

        self.assertIn("raw_record", result)
        self.assertEqual(result["raw_record"]["easy_apply"], "true")

    def test_raw_record_contains_title_company(self):
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(title="Senior Engineer", company="Acme")
        result = clean_job_record(record)

        self.assertEqual(result["raw_record"]["title"], "Senior Engineer")
        self.assertEqual(result["raw_record"]["company"], "Acme")


class TestUrlAndApplyUrlMapping(unittest.TestCase):
    """URL and apply_url mapping from clean_job_record through to sync row."""

    def test_url_is_raw_linkedin_url_not_normalized(self):
        """url should be the raw LinkedIn URL (url_normalized is separate)."""
        from scripts.clean_linkedin_jobs import map_row_for_sync

        cleaned = {
            "url": "https://www.linkedin.com/jobs/view/123?trk=guest",
            "external_url": "",
            "url_normalized": "https://www.linkedin.com/jobs/view/123",
            "url_hash": "abc123",
            "apply_type": "easy_apply",
            "source": "linkedin",
            "source_channel": "recommended",
            "raw_record": {},
        }
        row = map_row_for_sync(cleaned)
        # url is the LinkedIn job URL for reference
        self.assertEqual(row["url"], "https://www.linkedin.com/jobs/view/123?trk=guest")
        # apply_url is empty for easy_apply
        self.assertEqual(row["apply_url"], "")

    def test_external_job_apply_url_is_external_url(self):
        """External jobs should have apply_url set to external_url in sync row."""
        from scripts.clean_linkedin_jobs import map_row_for_sync

        cleaned = {
            "url": "https://www.linkedin.com/jobs/view/456",
            "external_url": "https://example.com/apply",
            "url_normalized": "https://www.linkedin.com/jobs/view/456",
            "url_hash": "def456",
            "apply_type": "external",
            "source": "linkedin",
            "source_channel": "recommended",
            "raw_record": {},
        }
        row = map_row_for_sync(cleaned)
        self.assertEqual(row["url"], "https://www.linkedin.com/jobs/view/456")
        self.assertEqual(row["apply_url"], "https://example.com/apply")

    def test_easy_apply_job_has_empty_apply_url(self):
        """Easy apply jobs should have empty apply_url in sync row."""
        from scripts.clean_linkedin_jobs import map_row_for_sync

        cleaned = {
            "url": "https://www.linkedin.com/jobs/view/789",
            "external_url": "",
            "url_normalized": "https://www.linkedin.com/jobs/view/789",
            "url_hash": "ghi789",
            "apply_type": "easy_apply",
            "source": "linkedin",
            "source_channel": "recommended",
            "raw_record": {},
        }
        row = map_row_for_sync(cleaned)
        self.assertEqual(row["url"], "https://www.linkedin.com/jobs/view/789")
        self.assertEqual(row["apply_url"], "")


class TestLinkedInValidationRejection(unittest.TestCase):
    """LinkedIn records without easy_apply=true or external_url should be rejected."""

    def test_rejects_linkedin_without_external_or_easy_apply(self):
        """LinkedIn row with easy_apply=false and no external_url should be rejected."""
        from scripts.clean_linkedin_jobs import validate_record

        record = {
            "title": "Engineer",
            "company": "Acme",
            "location": "Remote",
            "url": "https://linkedin.com/jobs/view/123",
            "external_url": "",
            "source": "linkedin",
            "easy_apply": False,
        }
        ok, reason = validate_record(record)
        self.assertFalse(ok)
        self.assertIn("external_url", reason.lower())

    def test_rejects_linkedin_missing_apply_and_url(self):
        """LinkedIn row without easy_apply field and no external_url should be rejected."""
        from scripts.clean_linkedin_jobs import validate_record

        record = {
            "title": "Engineer",
            "company": "Acme",
            "location": "Remote",
            "url": "https://linkedin.com/jobs/view/123",
            "external_url": "",
            "source": "linkedin",
        }
        ok, reason = validate_record(record)
        self.assertFalse(ok)
        self.assertIn("external_url", reason.lower())

    def test_accepts_linkedin_with_easy_apply_and_no_external(self):
        """LinkedIn row with easy_apply=true but no external_url should be accepted."""
        from scripts.clean_linkedin_jobs import validate_record

        record = {
            "title": "Engineer",
            "company": "Acme",
            "location": "Remote",
            "url": "https://linkedin.com/jobs/view/123",
            "external_url": "",
            "source": "linkedin",
            "easy_apply": True,
        }
        ok, reason = validate_record(record)
        self.assertTrue(ok)

    def test_accepts_linkedin_with_external_url(self):
        """LinkedIn row with external_url but easy_apply=false should be accepted."""
        from scripts.clean_linkedin_jobs import validate_record

        record = {
            "title": "Engineer",
            "company": "Acme",
            "location": "Remote",
            "url": "https://linkedin.com/jobs/view/123",
            "external_url": "https://example.com/apply",
            "source": "linkedin",
            "easy_apply": False,
        }
        ok, reason = validate_record(record)
        self.assertTrue(ok)

    def test_accepts_non_linkedin_without_external(self):
        """Non-LinkedIn row without external_url should not be rejected."""
        from scripts.clean_linkedin_jobs import validate_record

        record = {
            "title": "Engineer",
            "company": "Acme",
            "location": "Remote",
            "url": "https://indeed.com/job/123",
            "external_url": "",
            "source": "indeed",
        }
        ok, reason = validate_record(record)
        self.assertTrue(ok)


class TestUrlExtraction(unittest.TestCase):
    """URL should be extracted from alternative LinkedIn field names."""

    def test_extracts_url_from_linkedin_url_field(self):
        """When url is empty, extract from linkedin_url."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(url="", linkedin_url="https://linkedin.com/jobs/view/123")
        result = clean_job_record(record)
        self.assertEqual(result["url"], "https://linkedin.com/jobs/view/123")

    def test_extracts_url_from_source_url_field(self):
        """When url is missing, extract from source_url."""
        from scripts.clean_linkedin_jobs import clean_job_record

        record = _build_raw_record(url="", source_url="https://linkedin.com/jobs/view/456")
        result = clean_job_record(record)
        self.assertEqual(result["url"], "https://linkedin.com/jobs/view/456")


if __name__ == "__main__":
    unittest.main()
