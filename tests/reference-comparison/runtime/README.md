# Reference runtime comparison

These harnesses execute one aggregate task against the same PostgreSQL 18
dataset:

```text
SUM(commerce.orders.amount) = 545.50
```

Each runner installs or starts an isolated, pinned OSS reference runtime,
executes its native query surface, verifies the expected decimal value, and
writes `<engine>-runtime.json`.

| Runner | Executed runtime |
|---|---|
| `run-wren.sh` | `wrenai 0.13.3` with `wren-core-py 0.7.5` |
| `run-cube.sh` | `cubejs/cube:v1.7.31` |
| `run-malloy.sh` | npm `@malloydata/malloy` and `@malloydata/db-postgres` `0.0.432` |
| `run-metricflow.sh` | MetricFlow `0.212.0` through `dbt-metricflow 0.14.0` |

Malloy `v0.0.433` is the evaluated source release, but no matching `0.0.433`
npm artifacts were published. The Malloy evidence therefore names both the
source reference and latest installable runtime rather than pretending they
are identical.

The GitHub Actions
[`Reference runtime comparison`](../../../.github/workflows/reference-comparison.yml)
workflow creates the database from [`postgres/init.sql`](postgres/init.sql),
runs all four harnesses on Linux amd64, and retains the JSON evidence. It does
not add any reference implementation to the postgresem runtime or trust
boundary.
