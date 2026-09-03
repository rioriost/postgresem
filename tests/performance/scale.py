#!/usr/bin/env python3
import json
import os
import platform
import subprocess
import sys
import time


def run(command, *, env=None):
    return subprocess.run(
        command,
        check=True,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout


def conninfo_value(value):
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


def normalized_architecture():
    machine = platform.machine()
    if machine in ("x86_64", "amd64"):
        return "amd64"
    if machine in ("aarch64", "arm64"):
        return "arm64"
    raise AssertionError(f"unsupported scale baseline architecture: {machine}")


run(
    [
        "psql",
        "--no-psqlrc",
        "-v",
        "ON_ERROR_STOP=1",
        "-f",
        "/tests/performance/catalog_1000.sql",
    ]
)

environment = os.environ.copy()
base_conninfo = (
    f"host={environment['PGHOST']} port={environment['PGPORT']} "
    f"dbname={environment['PGDATABASE']}"
)
environment["SCALE_CATALOG_URL"] = (
    f"{base_conninfo} user={environment['PGUSER']} "
    f"password={conninfo_value(environment['PGPASSWORD'])} sslmode=disable"
)
environment["SCALE_RUNTIME_URL"] = (
    f"{base_conninfo} user=postgresem_runtime "
    f"password={conninfo_value(environment['POSTGRESEM_RUNTIME_PASSWORD'])} "
    "sslmode=disable"
)
environment["SCALE_AUDIT_URL"] = (
    f"{base_conninfo} user=postgresem_audit_writer "
    f"password={conninfo_value(environment['POSTGRESEM_AUDIT_WRITER_PASSWORD'])} "
    "sslmode=disable"
)
environment["SCALE_DB_ROLE"] = "postgresem_analyst"
environment["SCALE_AUDIT_PASSWORD"] = environment[
    "POSTGRESEM_AUDIT_WRITER_PASSWORD"
]


def scan():
    started = time.perf_counter()
    snapshot = json.loads(
        run(
            [
                "postgresem",
                "catalog",
                "scan",
                "--database-url-env",
                "SCALE_CATALOG_URL",
            ],
            env=environment,
        )
    )
    return snapshot, (time.perf_counter() - started) * 1000


first, first_ms = scan()
second, second_ms = scan()
scale_relations = [
    relation
    for relation in first["relations"]
    if relation["schema"] == "scale_catalog"
]
if len(scale_relations) != 1000:
    raise AssertionError(f"expected 1000 scale relations, got {len(scale_relations)}")
if first["fingerprint"] != second["fingerprint"]:
    raise AssertionError("catalog fingerprint changed between identical scale scans")
with open("/tmp/scale-catalog.json", "w", encoding="utf-8") as stream:
    json.dump(first, stream, separators=(",", ":"))


def scaffold():
    return json.loads(
        run(
            [
                "postgresem",
                "model",
                "scaffold",
                "/tests/performance/scaffold.json",
                "--catalog",
                "/tmp/scale-catalog.json",
            ]
        )
    )


first_scaffold = scaffold()
second_scaffold = scaffold()
if first_scaffold["selected_relations"] != 1000:
    raise AssertionError("large-model scaffold did not select 1000 relations")
if (
    first_scaffold["snapshot"]["revision_hash"]
    != second_scaffold["snapshot"]["revision_hash"]
):
    raise AssertionError("large-model scaffold revision hash was not deterministic")

compiler = json.loads(
    run(
        [
            "postgresem",
            "benchmark",
            "compiler",
            "--models",
            "1000",
            "--warmup",
            "100",
            "--iterations",
            "1000",
            "--threshold-ms",
            "50",
        ]
    )
)
execution = json.loads(
    run(
        [
            "postgresem",
            "benchmark",
            "execution",
            "/tests/integration/queries/commerce-revenue.json",
            "--project",
            "commerce",
            "--database-url-env",
            "SCALE_RUNTIME_URL",
            "--audit-database-url-env",
            "SCALE_AUDIT_URL",
            "--db-role-env",
            "SCALE_DB_ROLE",
            "--warmup",
            "5",
            "--iterations",
            "25",
            "--threshold-ms",
            "1000",
        ],
        env=environment,
    )
)
operations = json.loads(
    run(
        [
            "postgresem",
            "report",
            "operations",
            "--audit-database-url-env",
            "SCALE_AUDIT_URL",
            "--audit-password-env",
            "SCALE_AUDIT_PASSWORD",
            "--window-hours",
            "1",
        ],
        env=environment,
    )
)
if operations["catalog"]["user_relations"] < 1000:
    raise AssertionError("operational report omitted scale catalog relations")
if operations["migrations"]["current"] != "0010_m10_operational_report":
    raise AssertionError("operational report did not expose the current migration")
if not operations["objectives"]["query_audit_complete"]:
    raise AssertionError("operational report found incomplete query audit rows")

architecture = normalized_architecture()
expected_architecture = environment.get("POSTGRESEM_EXPECTED_ARCH")
if expected_architecture and expected_architecture != architecture:
    raise AssertionError(
        f"expected {expected_architecture}, measured scale baseline on {architecture}"
    )
postgresql_version = int(
    run(["psql", "--no-psqlrc", "--tuples-only", "--no-align", "--command", "SHOW server_version_num"])
)

print(
    json.dumps(
        {
            "schema_version": "2",
            "architecture": {
                "os": platform.system(),
                "machine": platform.machine(),
                "normalized": architecture,
            },
            "postgresql_major": postgresql_version // 10000,
            "catalog": {
                "model_relations": len(scale_relations),
                "first_scan_ms": round(first_ms, 3),
                "second_scan_ms": round(second_ms, 3),
                "threshold_ms": 1000.0,
                "deterministic": True,
                "passed": max(first_ms, second_ms) < 1000.0,
            },
            "authoring": {
                "model_count": first_scaffold["selected_relations"],
                "omitted_unselectable_columns": first_scaffold[
                    "omitted_unselectable_columns"
                ],
                "revision_hash": first_scaffold["snapshot"]["revision_hash"],
                "deterministic": True,
            },
            "compiler": compiler,
            "guarded_execution": execution,
            "operations": operations,
        },
        separators=(",", ":"),
    )
)
if max(first_ms, second_ms) >= 1000.0:
    raise AssertionError("catalog scale threshold exceeded")
print("M10 scale baseline checks passed", file=sys.stderr)
