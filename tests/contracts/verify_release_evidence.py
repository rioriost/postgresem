#!/usr/bin/env python3
import argparse
import datetime as dt
import hashlib
import json
import re
import subprocess
from pathlib import Path
from urllib.parse import urlparse


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_EVIDENCE = ROOT / "contracts" / "release-evidence-v1.json"
DEFAULT_CHECKLIST = ROOT / "docs" / "beta-checklist.md"
STABLE_MANIFEST = ROOT / "contracts" / "stable-v1.json"
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
STABLE_TAG_RE = re.compile(r"^v1\.[0-9]+\.[0-9]+$")
DOCUMENT_KEYS = {
    "schema_version",
    "release",
    "status",
    "independent_security_review",
    "field_pilots",
}
REVIEW_KEYS = {
    "evidence_url",
    "reviewed_commit",
    "reviewed_contract_digest",
    "reviewed_image_digest",
    "retest_completed_at",
    "unresolved_p0_p1",
}


def require_exact_keys(value, expected, field):
    if not isinstance(value, dict):
        raise AssertionError(f"{field} must be an object")
    actual = set(value)
    expected = set(expected)
    if actual != expected:
        raise AssertionError(
            f"{field} fields differ: missing={sorted(expected - actual)}, "
            f"unknown={sorted(actual - expected)}"
        )


def require_https_url(value, field):
    if not isinstance(value, str):
        raise AssertionError(f"{field} must be an HTTPS URL")
    parsed = urlparse(value)
    if parsed.scheme.lower() != "https" or not parsed.netloc:
        raise AssertionError(f"{field} must be an HTTPS URL")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise AssertionError(
            f"{field} must not contain credentials, query, or fragment"
        )
    try:
        port = parsed.port
    except ValueError as error:
        raise AssertionError(f"{field} has an invalid port") from error
    if "%" in parsed.path:
        raise AssertionError(f"{field} must not contain percent-encoded path bytes")
    path_segments = parsed.path.split("/")
    if any(segment in {".", ".."} for segment in path_segments):
        raise AssertionError(f"{field} must not contain relative path segments")
    if "//" in parsed.path:
        raise AssertionError(f"{field} must not contain empty path segments")
    host = parsed.hostname
    if host is None:
        raise AssertionError(f"{field} must contain a host")
    authority = host.casefold()
    if port not in {None, 443}:
        authority = f"{authority}:{port}"
    path = parsed.path.rstrip("/") or "/"
    return f"https://{authority}{path}"


def parse_utc_date(value, field):
    if not isinstance(value, str):
        raise AssertionError(f"{field} must be a UTC date")
    try:
        result = dt.date.fromisoformat(value)
    except ValueError as error:
        raise AssertionError(f"{field} must use YYYY-MM-DD") from error
    if result > dt.datetime.now(dt.timezone.utc).date():
        raise AssertionError(f"{field} must not be in the future")
    return result


def digest(path):
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def validate(document):
    require_exact_keys(document, DOCUMENT_KEYS, "release evidence")
    if document.get("schema_version") != "1":
        raise AssertionError("unsupported release evidence schema version")
    if document.get("release") != "1.0.0":
        raise AssertionError("release evidence must target 1.0.0")
    if document.get("status") != "accepted":
        raise AssertionError("1.0.0 external evidence is not accepted")

    review = document.get("independent_security_review")
    require_exact_keys(review, REVIEW_KEYS, "independent_security_review")
    review_url = require_https_url(
        review.get("evidence_url"),
        "security review evidence_url",
    )
    if not COMMIT_RE.fullmatch(str(review.get("reviewed_commit", ""))):
        raise AssertionError("security review reviewed_commit must be a full SHA")
    if not SHA256_RE.fullmatch(str(review.get("reviewed_contract_digest", ""))):
        raise AssertionError("security review contract digest must be sha256")
    if not SHA256_RE.fullmatch(str(review.get("reviewed_image_digest", ""))):
        raise AssertionError("security review image digest must be sha256")
    parse_utc_date(review.get("retest_completed_at"), "security review retest date")
    if review.get("unresolved_p0_p1") != 0:
        raise AssertionError("security review has unresolved P0/P1 findings")

    pilots = document.get("field_pilots")
    if not isinstance(pilots, list) or len(pilots) != 2:
        raise AssertionError("exactly two field pilot records are required")
    pseudonyms = set()
    evidence_urls = {review_url}
    for index, pilot in enumerate(pilots):
        field = f"field_pilots[{index}]"
        require_exact_keys(
            pilot,
            {
                "pseudonym",
                "evidence_url",
                "postgresql_version",
                "deployment",
                "started_at",
                "completed_at",
                "weekly_checkpoints",
                "unresolved_p0_p1",
            },
            field,
        )
        pseudonym = pilot.get("pseudonym")
        if (
            not isinstance(pseudonym, str)
            or not pseudonym
            or pseudonym != pseudonym.strip()
        ):
            raise AssertionError(
                f"{field}.pseudonym must be non-empty without edge whitespace"
            )
        normalized_pseudonym = " ".join(pseudonym.split()).casefold()
        if normalized_pseudonym in pseudonyms:
            raise AssertionError("field pilot pseudonyms must be distinct")
        pseudonyms.add(normalized_pseudonym)
        evidence_url = require_https_url(
            pilot.get("evidence_url"),
            f"{field}.evidence_url",
        )
        if evidence_url in evidence_urls:
            raise AssertionError("field pilot evidence URLs must be distinct")
        evidence_urls.add(evidence_url)
        postgresql_version = pilot.get("postgresql_version")
        if (
            not isinstance(postgresql_version, str)
            or not postgresql_version.strip()
        ):
            raise AssertionError(f"{field}.postgresql_version must be non-empty")
        deployment = pilot.get("deployment")
        require_exact_keys(deployment, {"kind", "value"}, f"{field}.deployment")
        kind = deployment.get("kind")
        value = deployment.get("value")
        if kind == "commit":
            if not COMMIT_RE.fullmatch(str(value)):
                raise AssertionError(f"{field} deployment commit must be a full SHA")
        elif kind == "image":
            if not SHA256_RE.fullmatch(str(value)):
                raise AssertionError(f"{field} deployment image must be a sha256 digest")
        else:
            raise AssertionError(f"{field} deployment kind is unsupported")
        start = parse_utc_date(pilot.get("started_at"), f"{field}.started_at")
        end = parse_utc_date(pilot.get("completed_at"), f"{field}.completed_at")
        if (end - start).days < 28:
            raise AssertionError(f"{field} covers fewer than 28 days")
        checkpoints = pilot.get("weekly_checkpoints")
        if not isinstance(checkpoints, list) or len(checkpoints) < 4:
            raise AssertionError(f"{field} requires at least four checkpoints")
        checkpoint_dates = [
            parse_utc_date(value, f"{field}.weekly_checkpoints")
            for value in checkpoints
        ]
        if checkpoint_dates != sorted(set(checkpoint_dates)):
            raise AssertionError(f"{field} checkpoints must be unique and sorted")
        if checkpoint_dates[0] < start or checkpoint_dates[-1] > end:
            raise AssertionError(f"{field} checkpoints must fall within the pilot")
        intervals = [
            (right - left).days
            for left, right in zip(checkpoint_dates, checkpoint_dates[1:])
        ]
        if any(interval < 6 or interval > 8 for interval in intervals):
            raise AssertionError(f"{field} checkpoints must be weekly")
        if (checkpoint_dates[0] - start).days > 8:
            raise AssertionError(f"{field} first checkpoint is too late")
        if (end - checkpoint_dates[-1]).days > 8:
            raise AssertionError(f"{field} final checkpoint is too early")
        if pilot.get("unresolved_p0_p1") != 0:
            raise AssertionError(f"{field} has unresolved P0/P1 defects")


def validate_checklist(document, checklist):
    urls = [document["independent_security_review"]["evidence_url"]]
    urls.extend(pilot["evidence_url"] for pilot in document["field_pilots"])
    recorded_urls = set()
    for value in re.findall(r"https://[^\s)>]+", checklist):
        try:
            recorded_urls.add(require_https_url(value, "checklist URL"))
        except AssertionError:
            continue
    for url in urls:
        normalized_url = require_https_url(url, "accepted evidence URL")
        if normalized_url not in recorded_urls:
            raise AssertionError(
                f"accepted evidence URL is missing from docs/beta-checklist.md: {url}"
            )


def validate_release_identity(document, released_tag, released_commit):
    if not STABLE_TAG_RE.fullmatch(released_tag):
        raise AssertionError("released tag must be a stable v1 SemVer tag")
    if not COMMIT_RE.fullmatch(released_commit):
        raise AssertionError("released commit must be a full SHA")
    reviewed_commit = document["independent_security_review"]["reviewed_commit"]
    ancestry = subprocess.run(
        ["git", "merge-base", "--is-ancestor", reviewed_commit, released_commit],
        cwd=ROOT,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if ancestry.returncode != 0:
        raise AssertionError(
            "security review commit is not an ancestor of the released commit"
        )
    expected_digest = document["independent_security_review"][
        "reviewed_contract_digest"
    ]
    if digest(STABLE_MANIFEST) != expected_digest:
        raise AssertionError(
            "stable contract differs from the independently reviewed contract"
        )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", type=Path, default=DEFAULT_EVIDENCE)
    parser.add_argument("--released-tag")
    parser.add_argument("--released-commit")
    parser.add_argument(
        "--allow-pending",
        action="store_true",
        help="validate only schema identity while evidence collection is pending",
    )
    arguments = parser.parse_args()
    document = json.loads(arguments.evidence.read_text(encoding="utf-8"))
    if arguments.allow_pending and document.get("status") == "pending":
        require_exact_keys(document, DOCUMENT_KEYS, "release evidence")
        if document.get("schema_version") != "1" or document.get("release") != "1.0.0":
            raise AssertionError("pending evidence has invalid schema or release")
        require_exact_keys(
            document.get("independent_security_review"),
            REVIEW_KEYS,
            "independent_security_review",
        )
        if not isinstance(document.get("field_pilots"), list):
            raise AssertionError("field_pilots must be a list")
        return
    validate(document)
    if bool(arguments.released_tag) != bool(arguments.released_commit):
        raise AssertionError(
            "--released-tag and --released-commit must be provided together"
        )
    if arguments.released_tag is not None:
        validate_release_identity(
            document,
            arguments.released_tag,
            arguments.released_commit,
        )
    validate_checklist(
        document,
        DEFAULT_CHECKLIST.read_text(encoding="utf-8"),
    )


if __name__ == "__main__":
    main()
