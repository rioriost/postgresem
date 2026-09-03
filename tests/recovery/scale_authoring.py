#!/usr/bin/env python3
import json
import os
import subprocess
import tempfile


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
credential = conninfo_value(environment["PGPASSWORD"])
environment["RECOVERY_CATALOG_URL"] = (
    f"host={environment['PGHOST']} port={environment['PGPORT']} "
    f"dbname={environment['PGDATABASE']} user={environment['PGUSER']} "
    + "password="
    + credential
    + " sslmode=disable"
)

catalog = json.loads(
    run(
        [
            "postgresem",
            "catalog",
            "scan",
            "--database-url-env",
            "RECOVERY_CATALOG_URL",
        ],
        env=environment,
    )
)
relations = [
    relation
    for relation in catalog["relations"]
    if relation["schema"] == "scale_catalog"
]
if len(relations) != 1000:
    raise AssertionError(f"expected 1000 recovered scale relations, got {len(relations)}")

with tempfile.NamedTemporaryFile(
    mode="w", encoding="utf-8", suffix=".json"
) as catalog_file:
    json.dump(catalog, catalog_file, separators=(",", ":"))
    catalog_file.flush()
    command = [
        "postgresem",
        "model",
        "scaffold",
        "/tests/performance/scaffold.json",
        "--catalog",
        catalog_file.name,
    ]
    first = json.loads(run(command))
    second = json.loads(run(command))

if first["selected_relations"] != 1000:
    raise AssertionError("recovered scaffold did not select 1000 relations")
if first["snapshot"]["revision_hash"] != second["snapshot"]["revision_hash"]:
    raise AssertionError("recovered scaffold revision hash was not deterministic")

print("M10 scale authoring recovery check passed")
