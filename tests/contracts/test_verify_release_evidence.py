#!/usr/bin/env python3
import copy
import importlib.util
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("verify_release_evidence.py")
SPEC = importlib.util.spec_from_file_location("verify_release_evidence", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def accepted_evidence():
    return {
        "schema_version": "1",
        "release": "1.0.0",
        "status": "accepted",
        "independent_security_review": {
            "evidence_url": "https://example.test/security-review",
            "reviewed_commit": "a" * 40,
            "reviewed_image_digest": "sha256:" + "b" * 64,
            "reviewed_contract_digest": "sha256:" + "e" * 64,
            "retest_completed_at": "2026-08-01",
            "unresolved_p0_p1": 0,
        },
        "field_pilots": [
            {
                "pseudonym": "pilot-a",
                "evidence_url": "https://example.test/pilot-a",
                "postgresql_version": "18.0",
                "deployment": {"kind": "commit", "value": "c" * 40},
                "started_at": "2026-07-01",
                "completed_at": "2026-07-29",
                "weekly_checkpoints": [
                    "2026-07-08",
                    "2026-07-15",
                    "2026-07-22",
                    "2026-07-29",
                ],
                "unresolved_p0_p1": 0,
            },
            {
                "pseudonym": "pilot-b",
                "evidence_url": "https://example.test/pilot-b",
                "postgresql_version": "17.4",
                "deployment": {
                    "kind": "image",
                    "value": "sha256:" + "d" * 64,
                },
                "started_at": "2026-07-02",
                "completed_at": "2026-07-30",
                "weekly_checkpoints": [
                    "2026-07-09",
                    "2026-07-16",
                    "2026-07-23",
                    "2026-07-30",
                ],
                "unresolved_p0_p1": 0,
            },
        ],
    }


class ReleaseEvidenceTests(unittest.TestCase):
    def test_accepts_complete_evidence(self):
        MODULE.validate(accepted_evidence())

    def test_rejects_pending_evidence(self):
        evidence = accepted_evidence()
        evidence["status"] = "pending"
        with self.assertRaisesRegex(AssertionError, "not accepted"):
            MODULE.validate(evidence)

    def test_rejects_unknown_fields(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["notes"] = "unreviewed"
        with self.assertRaisesRegex(AssertionError, "unknown"):
            MODULE.validate(evidence)

    def test_rejects_duplicate_pilots(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][1] = copy.deepcopy(evidence["field_pilots"][0])
        with self.assertRaisesRegex(AssertionError, "pseudonyms must be distinct"):
            MODULE.validate(evidence)

    def test_rejects_short_pilot(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["completed_at"] = "2026-07-28"
        with self.assertRaisesRegex(AssertionError, "fewer than 28 days"):
            MODULE.validate(evidence)

    def test_rejects_nonweekly_checkpoints(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["weekly_checkpoints"][1] = "2026-07-17"
        with self.assertRaisesRegex(AssertionError, "must be weekly"):
            MODULE.validate(evidence)

    def test_rejects_mutable_deployment_identity(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["deployment"] = {
            "kind": "release",
            "value": "latest",
        }
        with self.assertRaisesRegex(AssertionError, "kind is unsupported"):
            MODULE.validate(evidence)

    def test_rejects_future_dates(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["completed_at"] = "2999-01-01"
        with self.assertRaisesRegex(AssertionError, "must not be in the future"):
            MODULE.validate(evidence)

    def test_rejects_url_decorated_duplicate(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][1]["evidence_url"] = (
            evidence["field_pilots"][0]["evidence_url"] + "#second"
        )
        with self.assertRaisesRegex(AssertionError, "fragment"):
            MODULE.validate(evidence)

    def test_rejects_canonical_url_duplicate(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][1]["evidence_url"] = (
            "https://EXAMPLE.test:443/pilot-a/"
        )
        with self.assertRaisesRegex(AssertionError, "URLs must be distinct"):
            MODULE.validate(evidence)

    def test_rejects_security_review_url_reused_for_pilot(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["evidence_url"] = (
            evidence["independent_security_review"]["evidence_url"]
        )
        with self.assertRaisesRegex(AssertionError, "URLs must be distinct"):
            MODULE.validate(evidence)

    def test_rejects_whitespace_equivalent_pseudonyms(self):
        evidence = accepted_evidence()
        evidence["field_pilots"][0]["pseudonym"] = "pilot a"
        evidence["field_pilots"][1]["pseudonym"] = "pilot  a"
        with self.assertRaisesRegex(AssertionError, "pseudonyms must be distinct"):
            MODULE.validate(evidence)

    def test_requires_accepted_urls_in_beta_checklist(self):
        evidence = accepted_evidence()
        checklist = "\n".join(
            [
                evidence["independent_security_review"]["evidence_url"],
                evidence["field_pilots"][0]["evidence_url"],
            ]
        )
        with self.assertRaisesRegex(AssertionError, "missing from"):
            MODULE.validate_checklist(evidence, checklist)

    def test_release_identity_requires_reviewed_contract_digest(self):
        evidence = accepted_evidence()
        result = mock.Mock(returncode=0)
        with (
            mock.patch.object(MODULE, "digest", return_value="sha256:" + "f" * 64),
            mock.patch.object(MODULE.subprocess, "run", return_value=result),
            self.assertRaisesRegex(AssertionError, "differs"),
        ):
            MODULE.validate_release_identity(evidence, "v1.0.0", "f" * 40)

    def test_release_identity_rejects_prerelease_tag(self):
        with self.assertRaisesRegex(AssertionError, "stable v1 SemVer"):
            MODULE.validate_release_identity(
                accepted_evidence(),
                "v1.0.0-rc.1",
                "f" * 40,
            )

    def test_release_identity_requires_reviewed_commit_ancestry(self):
        evidence = accepted_evidence()
        result = mock.Mock(returncode=1)
        with (
            mock.patch.object(MODULE.subprocess, "run", return_value=result),
            self.assertRaisesRegex(AssertionError, "not an ancestor"),
        ):
            MODULE.validate_release_identity(evidence, "v1.0.0", "f" * 40)


if __name__ == "__main__":
    unittest.main()
