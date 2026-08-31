# M4 performance baseline

The developer preview includes two reproducible performance/correctness
surfaces:

1. a standalone synthetic compiler benchmark; and
2. `make test-performance`, which combines a 100-relation PostgreSQL catalog
   determinism check with the compiler benchmark.

These are development regression signals, not production SLOs, capacity
guidance, or universal latency guarantees.

The [CI workflow](../.github/workflows/ci.yml) configures the integrated
performance service only for its PostgreSQL 18 matrix job. That configuration
does not establish passing GitHub evidence; the dated Apple Container result
below is the current recorded measurement.

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

## Integrated 100-relation check

With a local-only `.env`:

```sh
make test-performance
```

The target builds the integration image with a release `postgresem` binary and
starts `postgresem-performance-test`. The test:

1. creates 100 PostgreSQL relations in the `preview_catalog` fixture schema;
2. runs `postgresem catalog scan` twice;
3. requires exactly 100 matching fixture relations;
4. requires the two complete catalog fingerprints to be identical;
5. runs the compiler benchmark with 100 models, 100 warmups, 1,000 measured
   iterations, and a 50 ms threshold;
6. prints one compact combined JSON record and
   `developer preview performance checks passed`.

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

## Recording a comparable run

Capture:

- date, commit, package version, and whether the binary is a release build;
- macOS architecture and runtime versions from `make doctor`;
- PostgreSQL image;
- exact benchmark arguments;
- catalog scan times and whether fingerprints matched;
- compiler p50, p95, max, threshold, and `passed`;
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
