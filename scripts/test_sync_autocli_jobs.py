import unittest

from scripts.sync_autocli_jobs import normalize_job


class TestSyncAutoCliJobs(unittest.TestCase):
    def test_identity_prefers_apply_url(self) -> None:
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

    def test_fallback_identity_requires_title_and_company(self) -> None:
        job = normalize_job("linkedin", {"location": "NYC"})
        self.assertIsNone(job)

    def test_fallback_identity_uses_title_company_location(self) -> None:
        job = normalize_job(
            "linkedin",
            {"job_title": "Engineer", "company_name": "Acme", "location": "Remote"},
        )
        self.assertIsNotNone(job)
        assert job is not None
        self.assertNotEqual(job.identity_hash, "")

    def test_description_hash_empty_when_missing(self) -> None:
        job = normalize_job(
            "linkedin",
            {"job_title": "Engineer", "company_name": "Acme", "location": "Remote"},
        )
        assert job is not None
        self.assertEqual(job.description_hash, "")


if __name__ == "__main__":
    unittest.main()
