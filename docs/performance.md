# M10 scale baseline

M10 starts with a reproducible PostgreSQL-native baseline before introducing
connection pools, prepared-plan caches, materialized views, or optional
pre-aggregation. See
[ADR 0016](adr/0016-postgresql-native-scale-baseline.md).

Run:

```sh
make test-performance
```

The PostgreSQL 18 fixture now:

1. creates 1,000 relations and requires two complete catalog scans to have the
   same fingerprint and remain below a 1,000 ms regression ceiling;
2. scaffolds the 1,000 catalog relations twice and requires the same revision
   hash;
3. compiles against 1,000 synthetic models with a 50 ms p95 regression ceiling;
4. runs 5 warmup and 25 measured guarded executions with a broad 1,000 ms p95
   regression ceiling; and
5. requires every guarded execution to produce the same semantic-result hash
   after excluding its unique audit query ID and verifies `report operations`.

The guarded-execution measurement includes connection establishment, published
model loading, LSQ validation and compilation, mandatory audit writes,
PostgreSQL authority checks, GRANT/RLS execution, serialization, and terminal
audit. It deliberately does not isolate PostgreSQL data execution from gateway
cost yet; the first baseline identifies where follow-up instrumentation and
optimization are justified.

CI runs the complete baseline on native Linux amd64 and arm64 and retains the
PostgreSQL 18 matrix run. Evidence is compact JSON and contains no LSQ, SQL,
parameters, credentials, principals, query IDs, or result rows.

These thresholds are repository-fixture regression signals, not production
SLOs, capacity guidance, or hardware-independent latency guarantees.

## Initial Apple Container arm64 measurement

The first M10 run was recorded on **2026-09-03** from commit `4fccbff` plus the
uncommitted M10 baseline changes, using macOS 26.6.2, Apple Container 1.3.1,
its recommended Kata 3.32.0 arm64 kernel, PostgreSQL 18.6, and a release
`postgresem` binary:

| Measurement | Result |
|---|---:|
| first 1,000-relation catalog scan | 2,208.054 ms |
| second 1,000-relation catalog scan | 2,061.241 ms |
| catalog fingerprint | deterministic |
| 1,000-model compiler p50 | 0.856541 ms |
| 1,000-model compiler p95 | 0.903458 ms |
| 1,000-model compiler max | 1.046500 ms |
| guarded execution p50 | 13.887500 ms |
| guarded execution p95 | 15.149375 ms |
| guarded execution max | 15.404625 ms |
| guarded execution result | deterministic |

For this fixture, complete catalog scanning is the first measured dominant
path. That observation prioritizes catalog-scale investigation before adding
connection pooling, prepared-plan caching, or persisted acceleration. It does
not establish that catalog scanning is the limiting path for another database.

## Post-optimization Apple Container arm64 measurement

After replacing the relation/column/constraint/policy and function-grant N+1
lookups with set-based PostgreSQL scans, the same environment and fixture
recorded:

| Measurement | Result |
|---|---:|
| first 1,000-relation catalog scan | 139.832 ms |
| second 1,000-relation catalog scan | 99.245 ms |
| catalog threshold | 1,000 ms, passed |
| scaffolded models | 1,000, deterministic |
| 1,000-model compiler p50 | 0.802875 ms |
| 1,000-model compiler p95 | 0.813375 ms |
| 1,000-model compiler max | 0.828625 ms |
| guarded execution p50 | 14.249917 ms |
| guarded execution p95 | 14.628500 ms |
| guarded execution max | 15.088083 ms |
| guarded execution result | deterministic |
| current migration reported | `0010_m10_operational_report` |

The catalog path improved by more than an order of magnitude while preserving
the complete canonical fingerprint. Guarded-execution latency did not indicate
a need for connection pooling, prepared-plan caching, or persisted
pre-aggregation in this fixture, so M10 does not add those mechanisms.

## Apple Container amd64/Rosetta reproduction

The same M10 gate also passed in an amd64 userspace under Apple Container
Rosetta on the arm64 maintainer host:

| Measurement | Result |
|---|---:|
| first/second catalog scan | 225.016 / 181.502 ms |
| compiler p95 | 2.440041 ms |
| guarded execution p95 | 22.496333 ms |
| scaffold revision hash | identical to arm64 |
| semantic-result hash | identical to arm64 |

This is useful local cross-architecture reproduction, but it is not native
amd64 evidence. CI runs the scale gate on native Linux amd64 and native Linux
arm64 and retains each JSON artifact.

## M4 historical baseline

The developer preview includes two reproducible performance/correctness
surfaces:

1. a standalone synthetic compiler benchmark; and
2. the original integrated fixture, which combined a 100-relation PostgreSQL
   catalog determinism check with the compiler benchmark.

These are development regression signals, not production SLOs, capacity
guidance, or universal latency guarantees.

The dated Apple Container result below preserves the historical M4 measurement.

## Standalone compiler benchmark

Run an installed release binary:

```sh
postgresem benchmark compiler \
  --models 100 \
  --warmup 100 \
  --iterations 1000 \
  --threshold-ms 50
```

From a source checkout, use a release build so the result is comparable to the
container gate:

```sh
cargo run --release -p postgresem -- benchmark compiler \
  --models 100 \
  --warmup 100 \
  --iterations 1000 \
  --threshold-ms 50
```

The benchmark:

- builds a deterministic synthetic Semantic Snapshot v1 with the requested
  number of queryable models;
- gives each model an integer entity key, a timestamp time dimension, and a
  `row_count` metric;
- targets the last model so name resolution covers the full 100-model
  snapshot;
- performs the requested warmup iterations;
- measures LSQ normalization plus compilation for each measured iteration;
- sorts elapsed samples and reports nearest-rank p50 and p95;
- passes only when `p95_ms < threshold_ms`.

Output is deterministic in shape, but timing values are inherently
environment-dependent:

```json
{
  "schema_version": "1",
  "model_count": 100,
  "warmup_iterations": 100,
  "measured_iterations": 1000,
  "threshold_ms": 50.0,
  "p50_ms": 0.0,
  "p95_ms": 0.0,
  "max_ms": 0.0,
  "passed": true
}
```

The real fields contain floating-point milliseconds rather than the placeholder
zeros above. The command prints the JSON before exiting nonzero when `passed`
is false. The measurement excludes PostgreSQL connection, query execution,
result serialization, MCP framing, and audit latency.

Model counts, warmups, iterations, and threshold must be positive. The current
implementation rejects more than 10,000 synthetic models.

## Historical integrated 100-relation check

The M4 test:

1. creates 100 PostgreSQL relations in the `preview_catalog` fixture schema;
2. runs `postgresem catalog scan` twice;
3. requires exactly 100 matching fixture relations;
4. requires the two complete catalog fingerprints to be identical;
5. runs the compiler benchmark with 100 models, 100 warmups, 1,000 measured
   iterations, and a 50 ms threshold;
6. printed one compact combined JSON record.

The combined output shape is:

```json
{
  "schema_version": "1",
  "catalog": {
    "model_relations": 100,
    "first_scan_ms": 0.0,
    "second_scan_ms": 0.0,
    "deterministic": true
  },
  "compiler": {
    "schema_version": "1",
    "model_count": 100,
    "warmup_iterations": 100,
    "measured_iterations": 1000,
    "threshold_ms": 50.0,
    "p50_ms": 0.0,
    "p95_ms": 0.0,
    "max_ms": 0.0,
    "passed": true
  }
}
```

Catalog times are Python wall-clock measurements around the complete scanner
process, including process startup, connection, scan, JSON generation, and
parsing. The test does not enforce a catalog latency threshold; it enforces
relation count and deterministic fingerprint. Database contents outside the
fixture can also affect total scan time.

## Reference Apple Container run

The latest supplied reference run was recorded on **2026-08-31** in the
maintainer's macOS/aarch64 Apple Container development environment using the
PostgreSQL 18 integration image and release compiler binary:

| Measurement | Result |
|---|---:|
| first 100-relation catalog scan | 138.755 ms |
| second 100-relation catalog scan | 150.192 ms |
| catalog fingerprint | deterministic across both scans |
| compiler p50 | 0.148666 ms |
| compiler p95 | 0.176875 ms |
| compiler max | 0.224667 ms |
| compiler threshold | 50 ms |
| compiler result | passed |

These values are a dated reference point only. They are not promised latency,
an SLO, a hardware-independent expectation, or evidence for production
capacity. CPU generation, power state, concurrent load, container/runtime
versions, filesystem state, PostgreSQL contents, build profile, and toolchain
can change the result.

The 50 ms value is deliberately a broad M4 compiler regression ceiling. It
must not be quoted as expected p95, and it does not apply to catalog scanning
or database execution.

## Recording a comparable M10 run

Capture:

- date, commit, package version, and whether the binary is a release build;
- Linux architecture and container runtime versions;
- PostgreSQL image;
- exact benchmark arguments;
- 1,000-relation catalog scan times and whether fingerprints matched;
- compiler p50, p95, max, threshold, and `passed`;
- guarded-execution p50, p95, max, result hash, determinism, threshold, and
  `passed`;
- background load or other material environment differences.

Do not include `.env`, credentials, connection strings, source data, raw query
results, or unredacted logs. Add dated results rather than replacing reference
numbers without preserving their environment context.

## Relationship to compatibility

Performance passing does not make a semantic change compatible. Use the
deterministic model compatibility surface independently:

```sh
postgresem model diff \
  --from BEFORE.json --to AFTER.json --fail-on-breaking
```

See [compatibility.md](compatibility.md#semantic-model-changes) for its JSON
contract and breaking-change behavior.
