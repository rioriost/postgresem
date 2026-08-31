#!/usr/bin/env python3
import json
import os
import subprocess
import time


def run(command, *, env=None):
    return subprocess.run(
        command,
        check=True,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout


run(
    [
        "psql",
        "--no-psqlrc",
        "-v",
        "ON_ERROR_STOP=1",
        "-f",
        "/tests/performance/catalog_100.sql",
    ]
)

environment = os.environ.copy()
environment["PREVIEW_CATALOG_URL"] = (
    f"host={environment['PGHOST']} port={environment['PGPORT']} "
    f"dbname={environment['PGDATABASE']} user={environment['PGUSER']} "
    f"password={environment['PGPASSWORD']}"
)


def scan():
    started = time.perf_counter()
    snapshot = json.loads(
        run(
            [
                "postgresem",
                "catalog",
                "scan",
                "--database-url-env",
                "PREVIEW_CATALOG_URL",
            ],
            env=environment,
        )
    )
    return snapshot, (time.perf_counter() - started) * 1000


first, first_ms = scan()
second, second_ms = scan()
preview_relations = [
    relation
    for relation in first["relations"]
    if relation["schema"] == "preview_catalog"
]
if len(preview_relations) != 100:
    raise AssertionError(f"expected 100 preview relations, got {len(preview_relations)}")
if first["fingerprint"] != second["fingerprint"]:
    raise AssertionError("catalog fingerprint changed between identical scans")

compiler = json.loads(
    run(
        [
            "postgresem",
            "benchmark",
            "compiler",
            "--models",
            "100",
            "--warmup",
            "100",
            "--iterations",
            "1000",
            "--threshold-ms",
            "50",
        ]
    )
)

print(
    json.dumps(
        {
            "schema_version": "1",
            "catalog": {
                "model_relations": len(preview_relations),
                "first_scan_ms": round(first_ms, 3),
                "second_scan_ms": round(second_ms, 3),
                "deterministic": True,
            },
            "compiler": compiler,
        },
        separators=(",", ":"),
    )
)
print("developer preview performance checks passed")
