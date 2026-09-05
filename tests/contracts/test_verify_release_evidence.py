#!/usr/bin/env python3
import copy
import hashlib
import importlib.util
import json
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


def exception_evidence():
    return {
        "schema_version": "1",
        "release": "1.0.0",
        "status": "accepted",
        "source_security_review": {
            "kind": "automated-source-review",
            "evidence_url": MODULE.EXCEPTION_REVIEW_URL,
            "reviewed_commit": MODULE.EXCEPTION_REVIEW_COMMIT,
            "reviewed_contract_digest": MODULE.EXCEPTION_REVIEW_DIGEST,
            "reviewed_image_digest": None,
            "retest_completed_at": "2026-09-05",
            "unresolved_p0_p1": 0,
        },
        "field_pilots": [],
        "maintainer_exception": {
            "scope": "v1.0.0-only",
            "maintainer": "rioriost",
            "accepted_at": "2026-09-05",
            "decision_url": (
                "https://github.com/rioriost/postgresem/blob/"
                + "d" * 40 + "/" + MODULE.EXCEPTION_ADR_PATH
            ),
            "waived_requirements": MODULE.WAIVED_REQUIREMENTS.copy(),
        },
    }


class MaintainerExceptionTests(unittest.TestCase):
    def test_accepts_explicit_source_only_exception(self):
        evidence = exception_evidence()
        MODULE.validate(evidence)
        MODULE.validate_checklist(evidence, "\n".join([
            evidence["source_security_review"]["evidence_url"],
            evidence["maintainer_exception"]["decision_url"],
        ]))

    def test_rejects_pending_exception(self):
        evidence = exception_evidence()
        evidence["status"] = "pending"
        with self.assertRaisesRegex(AssertionError, "not accepted"):
            MODULE.validate(evidence)

    def test_allow_pending_cannot_skip_exception_validation(self):
        evidence = exception_evidence()
        evidence["status"] = "pending"
        with (
            mock.patch("sys.argv", ["verify_release_evidence.py", "--allow-pending"]),
            mock.patch.object(MODULE, "DEFAULT_EVIDENCE", mock.Mock(
                read_text=mock.Mock(return_value=json.dumps(evidence)),
            )),
        ):
            with self.assertRaisesRegex(AssertionError, "not accepted"):
                MODULE.main()

    def test_allow_pending_cannot_authorize_ordinary_release(self):
        evidence = accepted_evidence()
        evidence["status"] = "pending"
        with (
            mock.patch("sys.argv", [
                "verify_release_evidence.py", "--allow-pending",
                "--released-tag", "v1.0.0", "--released-commit", "f" * 40,
            ]),
            mock.patch.object(MODULE, "DEFAULT_EVIDENCE", mock.Mock(
                read_text=mock.Mock(return_value=json.dumps(evidence)),
            )),
        ):
            with self.assertRaisesRegex(AssertionError, "cannot authorize a release"):
                MODULE.main()

    def test_rejects_later_release_and_false_independence(self):
        for changes in (
            {"release": "1.0.1"}, {"schema_version": "2"},
            {"independent_security_review": accepted_evidence()["independent_security_review"]},
        ):
            with self.subTest(changes=changes):
                evidence = exception_evidence()
                evidence.update(changes)
                with self.assertRaises(AssertionError):
                    MODULE.validate(evidence)

    def test_rejects_fabricated_pilots_and_image_review(self):
        evidence = exception_evidence()
        evidence["field_pilots"] = accepted_evidence()["field_pilots"]
        with self.assertRaisesRegex(AssertionError, "remain empty"):
            MODULE.validate(evidence)
        evidence = exception_evidence()
        evidence["source_security_review"]["reviewed_image_digest"] = "sha256:" + "a" * 64
        with self.assertRaisesRegex(AssertionError, "must not claim an image"):
            MODULE.validate(evidence)

    def test_rejects_unapproved_review_identity_dates_and_findings(self):
        for key, value in (
            ("kind", "independent-review"),
            ("evidence_url", "https://example.test/review"),
            ("reviewed_commit", "a" * 40),
            ("reviewed_contract_digest", "sha256:" + "a" * 64),
            ("retest_completed_at", "2026-09-04"),
            ("unresolved_p0_p1", 1),
            ("unresolved_p0_p1", False),
        ):
            with self.subTest(key=key):
                evidence = exception_evidence()
                evidence["source_security_review"][key] = value
                with self.assertRaises(AssertionError):
                    MODULE.validate(evidence)

    def test_rejects_unapproved_maintainer_scope_and_waivers(self):
        for key, value in (
            ("scope", "all-1.x"),
            ("maintainer", "someone-else"),
            ("accepted_at", "2026-09-06"),
            ("waived_requirements", MODULE.WAIVED_REQUIREMENTS[:-1]),
            ("waived_requirements", [*MODULE.WAIVED_REQUIREMENTS, "runtime_security"]),
        ):
            with self.subTest(key=key):
                evidence = exception_evidence()
                evidence["maintainer_exception"][key] = value
                with self.assertRaises(AssertionError):
                    MODULE.validate(evidence)

    def test_requires_exact_immutable_project_decision_url(self):
        for url in (
            "https://github.com/rioriost/postgresem/blob/main/" + MODULE.EXCEPTION_ADR_PATH,
            "https://github.com/elsewhere/postgresem/blob/" + "d" * 40 + "/" + MODULE.EXCEPTION_ADR_PATH,
            "https://github.com/rioriost/postgresem/blob/" + "d" * 40 + "/README.md",
            exception_evidence()["maintainer_exception"]["decision_url"] + "?raw=1",
            exception_evidence()["maintainer_exception"]["decision_url"] + "#decision",
        ):
            with self.subTest(url=url):
                evidence = exception_evidence()
                evidence["maintainer_exception"]["decision_url"] = url
                with self.assertRaises(AssertionError):
                    MODULE.validate(evidence)

    def test_checklist_requires_both_review_and_decision_urls(self):
        evidence = exception_evidence()
        for url in (
            evidence["source_security_review"]["evidence_url"],
            evidence["maintainer_exception"]["decision_url"],
        ):
            with self.subTest(url=url):
                with self.assertRaisesRegex(AssertionError, "missing from"):
                    MODULE.validate_checklist(evidence, url)


class ExceptionIdentityTests(unittest.TestCase):
    def setUp(self):
        self.released_commit = "f" * 40
        self.baseline = {
            "manifest": {"release": "1.0.0", "contracts": {"lsq": ["1"]}},
            "artifacts": [
                {"path": MODULE.EVIDENCE_VALIDATOR_PATH, "sha256": "sha256:" + "a" * 64},
                {"path": "crates/postgresem/src/executor.rs", "sha256": "sha256:" + "b" * 64},
            ],
        }
        self.released = copy.deepcopy(self.baseline)
        self.released["artifacts"][0]["sha256"] = "sha256:" + "c" * 64
        baseline_bytes = json.dumps(self.baseline).encode()
        digest = "sha256:" + hashlib.sha256(baseline_bytes).hexdigest()
        self.enterContext(mock.patch.object(MODULE, "EXCEPTION_REVIEW_DIGEST", digest))
        self.evidence = exception_evidence()
        self.blobs = {
            f"{MODULE.EXCEPTION_REVIEW_COMMIT}:contracts/stable-v1.json": baseline_bytes,
        }
        for commit, path in (
            ("2797160ee431ee12722d339e23def6d8c8e7fbd5", MODULE.EXCEPTION_REVIEW_PATH),
            ("d" * 40, MODULE.EXCEPTION_ADR_PATH),
        ):
            self.blobs[f"{commit}:{path}"] = b"immutable document"
            self.blobs[f"{self.released_commit}:{path}"] = b"immutable document"
        self.changed_paths = [MODULE.EVIDENCE_VALIDATOR_PATH, "contracts/stable-v1.json", MODULE.EXCEPTION_ADR_PATH]
        self.working_manifest = mock.Mock()
        self.working_manifest.relative_to.return_value = Path("contracts/stable-v1.json")
        self.enterContext(mock.patch.object(MODULE, "STABLE_MANIFEST", self.working_manifest))
        self.run = self.enterContext(mock.patch.object(
            MODULE.subprocess, "run", return_value=mock.Mock(returncode=0),
        ))
        self.git = self.enterContext(mock.patch.object(MODULE, "git_bytes", side_effect=self.git_bytes))
        self.sync_manifest()

    def sync_manifest(self):
        value = json.dumps(self.released).encode()
        self.blobs[f"{self.released_commit}:contracts/stable-v1.json"] = value
        self.working_manifest.read_bytes.return_value = value

    def git_bytes(self, *arguments):
        if arguments[0] == "show":
            return self.blobs[arguments[1]]
        self.assertEqual(arguments, (
            "diff", "--name-only", "--no-renames", "-z",
            MODULE.EXCEPTION_REVIEW_COMMIT, self.released_commit, "--",
        ))
        return b"\0".join(path.encode() for path in self.changed_paths) + b"\0"

    def validate(self, tag="v1.0.0"):
        MODULE.validate_release_identity(self.evidence, tag, self.released_commit)

    def test_allows_only_gate_hash_refresh_and_exact_documented_changes(self):
        self.validate()
        self.assertEqual(self.run.call_count, 3)

    def test_exception_cannot_authorize_any_later_stable_tag(self):
        for tag in ("v1.0.1", "v1.1.0", "v1.0.0-rc.1"):
            with self.subTest(tag=tag):
                with self.assertRaises(AssertionError):
                    self.validate(tag)

    def test_requires_ancestry_for_source_and_both_documents(self):
        for index in range(3):
            with self.subTest(index=index):
                self.run.side_effect = [
                    mock.Mock(returncode=1 if call == index else 0) for call in range(3)
                ]
                with self.assertRaisesRegex(AssertionError, "not an ancestor"):
                    self.validate()

    def test_rejects_changed_review_or_approval_document(self):
        for path in (MODULE.EXCEPTION_REVIEW_PATH, MODULE.EXCEPTION_ADR_PATH):
            with self.subTest(path=path):
                key = f"{self.released_commit}:{path}"
                self.blobs[key] = b"changed"
                with self.assertRaisesRegex(AssertionError, "differs from its immutable"):
                    self.validate()
                self.blobs[key] = b"immutable document"

    def test_rejects_changed_reviewed_baseline_bytes(self):
        key = f"{MODULE.EXCEPTION_REVIEW_COMMIT}:contracts/stable-v1.json"
        self.blobs[key] += b"\n"
        with self.assertRaisesRegex(AssertionError, "baseline contract digest"):
            self.validate()

    def test_rejects_public_contract_and_other_frozen_artifact_changes(self):
        self.released["manifest"]["contracts"]["lsq"] = ["2"]
        self.sync_manifest()
        with self.assertRaisesRegex(AssertionError, "runtime contract or frozen"):
            self.validate()
        self.released["manifest"] = copy.deepcopy(self.baseline["manifest"])
        self.released["artifacts"][1]["sha256"] = "sha256:" + "d" * 64
        self.sync_manifest()
        with self.assertRaisesRegex(AssertionError, "runtime contract or frozen"):
            self.validate()

    def test_rejects_unreviewed_code_dependencies_fixtures_and_packaging(self):
        for path in (
            "Cargo.lock", "Cargo.toml", "crates/postgresem/Cargo.toml",
            "crates/postgresem/src/new.rs", "fixtures/postgres/00-roles.sql",
            ".github/workflows/release.yml", "Dockerfile", "scripts/install.sh",
            "docs/unapproved.md",
        ):
            with self.subTest(path=path):
                self.changed_paths = [path]
                with self.assertRaisesRegex(AssertionError, "outside release governance"):
                    self.validate()

    def test_rejects_worktree_contract_different_from_release(self):
        self.working_manifest.read_bytes.return_value = b"changed"
        with self.assertRaisesRegex(AssertionError, "working stable contract"):
            self.validate()

    def test_rejects_duplicate_validator_inventory_entries(self):
        self.released["artifacts"].append(self.released["artifacts"][0].copy())
        self.sync_manifest()
        with self.assertRaisesRegex(AssertionError, "exactly one evidence validator"):
            self.validate()


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
