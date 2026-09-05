# 1.0 differentiation statement

The final 1.0 comparison preserves postgresem's PostgreSQL-only position. It
does not pursue feature-count parity with Wren AI, Cube, Malloy, or MetricFlow.
The evaluated upstream refs, reproducible aggregate result, and detailed
capability matrix are recorded in the
[M10 reference comparison](reference-comparison/2026-09-03.md).

The runnable [Meaning Lab](../examples/semantic_demo/README.md) makes the
single-database value observable: status-aware revenue, root-grain aggregation,
active MRR, and governed writes with replay/reconciliation on real PostgreSQL.
Its authored SQL mistakes are teaching cases, not measured LLM failures.
The optional bounded planner can select correct SQL; this demo does not claim
that PostgreSQL or an informed SQL author cannot produce the same answers.

## Stable differentiation

| Boundary | postgresem 1.0 position |
|---|---|
| semantic authority | reviewed definitions are published as immutable PostgreSQL revisions; files and importers are candidates, not a second live authority |
| query API | strict typed LSQ; no raw SQL input or generated-SQL output contract |
| mutation API | strict bounded LSM insert/upsert projections; no arbitrary DML |
| authorization | mapped PostgreSQL roles, GRANT, RLS, constraints, and triggers remain final authority |
| execution | PostgreSQL only, preserving native types, transactions, audit, restore, and role behavior |
| ambiguity | unsupported joins, fan-out, types, authority, and drift fail closed |
| acceleration | no mandatory result cache, pre-aggregation service, or second authoritative datastore |
| evidence | deterministic contract hashes, append-only migrations, native Linux execution, upgrade/restore rehearsal, and privacy-preserving audit reporting |

## Why the non-parity is intentional

Wren AI and Malloy cover broader engines; Cube provides Semantic SQL and
runtime-managed pre-aggregation; MetricFlow integrates warehouse APIs, caches,
and exports. Those capabilities are valuable for their deployment models but
would enlarge postgresem's trust boundary or create a second operational source
of truth. postgresem instead optimizes for PostgreSQL-centered systems that
need AI/application access to governed meaning without weakening existing
database authorization.

The resulting tradeoff is explicit: postgresem has a narrower engine and
mutation surface, but a stronger single-database authority model. Features
such as dynamic identity discovery, connection pooling, general update/delete,
automatic materialized-view routing, pre-aggregation, and non-PostgreSQL
execution remain deferred rather than being represented as stable 1.0
capabilities.
