#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = ROOT / "contracts" / "rc-v1.json"
CONTRACT_ARTIFACTS = [
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    "Containerfile",
    "Dockerfile",
    "IntegrationContainerfile",
    "Makefile",
    "compose.ci.yaml",
    "compose.yaml",
    "crates/postgresem-compiler/src/compiler.rs",
    "crates/postgresem-compiler/src/diff.rs",
    "crates/postgresem-compiler/src/hash.rs",
    "crates/postgresem-compiler/src/lib.rs",
    "crates/postgresem-compiler/src/lsm.rs",
    "crates/postgresem-compiler/src/lsq.rs",
    "crates/postgresem-compiler/src/mutation.rs",
    "crates/postgresem-compiler/src/semantic.rs",
    "crates/postgresem/src/authoring.rs",
    "crates/postgresem/src/benchmark.rs",
    "crates/postgresem/src/catalog.rs",
    "crates/postgresem/src/catalog_diff.rs",
    "crates/postgresem/src/catalog_types.rs",
    "crates/postgresem/src/contract.rs",
    "crates/postgresem/src/database.rs",
    "crates/postgresem/src/executor.rs",
    "crates/postgresem/src/hash.rs",
    "crates/postgresem/src/main.rs",
    "crates/postgresem/src/mcp.rs",
    "crates/postgresem/src/mcp_http.rs",
    "crates/postgresem/src/mcp_http_auth.rs",
    "crates/postgresem/src/mcp_http_rate.rs",
    "crates/postgresem/src/mutation_executor.rs",
    "crates/postgresem/src/osi.rs",
    "crates/postgresem/src/published_model.rs",
    "crates/postgresem/src/report.rs",
    "docs/error-reference.md",
    "schemas/lsm/v1.schema.json",
    "schemas/lsq/v1.schema.json",
    "schemas/mcp-http/v1.authority.schema.json",
    "schemas/semantic-expression/v1.schema.json",
    "migrations/run.sh",
    "scripts/backup.sh",
    "scripts/install.sh",
    "scripts/upgrade-local.sh",
    "scripts/verify-backup.sh",
    "tests/integration/mcp_http.py",
    "tests/rc/operator.sh",
    "tests/recovery/run.sh",
]


def digest(path):
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def expected_artifacts():
    migration_paths = sorted(
        path.relative_to(ROOT).as_posix()
        for path in (ROOT / "migrations").glob("[0-9][0-9][0-9][0-9]_*.sql")
    )
    paths = sorted(CONTRACT_ARTIFACTS + migration_paths)
    if len(paths) != len(set(paths)):
        raise AssertionError("contract artifact inventory contains duplicates")
    return [
        {"path": relative, "sha256": digest(ROOT / relative)}
        for relative in paths
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--actual-manifest", type=Path)
    parser.add_argument("--refresh", action="store_true")
    arguments = parser.parse_args()

    document = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    artifacts = expected_artifacts()
    if arguments.refresh:
        document["artifacts"] = artifacts
        MANIFEST_PATH.write_text(
            json.dumps(document, indent=2, sort_keys=False) + "\n",
            encoding="utf-8",
        )
        return

    if document.get("artifacts") != artifacts:
        raise AssertionError(
            "release-candidate contract artifacts changed; "
            "classify the change and refresh contracts/rc-v1.json"
        )
    if arguments.actual_manifest is not None:
        actual = json.loads(arguments.actual_manifest.read_text(encoding="utf-8"))
        if actual != document["manifest"]:
            raise AssertionError(
                "postgresem contract show does not match the frozen manifest"
            )


if __name__ == "__main__":
    main()
