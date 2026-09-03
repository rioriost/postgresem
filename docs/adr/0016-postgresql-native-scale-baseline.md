# ADR 0016: PostgreSQL-native scale baseline before optimization

- Status: Accepted
- Date: 2026-09-03

## Context

M10 must address measured bottlenecks without adding a second authoritative
datastore, weakening deterministic compilation, or bypassing PostgreSQL GRANT
and RLS. The M4 performance fixture covers 100 catalog relations and 100
synthetic models, but it does not measure guarded execution and runs only in
the PostgreSQL 18 integration job. It therefore cannot distinguish compiler,
catalog, connection, audit, and database costs across the supported Linux
architectures.

Adding a connection pool, prepared-plan cache, materialized view, or
pre-aggregation before a reproducible measurement would create new freshness,
authority, invalidation, and failure-recovery boundaries without evidence that
the added state addresses the limiting path.

## Decision

1. The first M10 gate is a reproducible PostgreSQL 18 scale baseline on native
   Linux amd64 and arm64.
2. The baseline creates 1,000 PostgreSQL relations, scans the complete catalog
   twice, requires exactly 1,000 fixture relations, and requires identical
   complete catalog fingerprints.
3. The compiler baseline uses 1,000 synthetic models, 100 warmups, 1,000
   measured compilations, and retains the gateway-attributable 50 ms p95
   regression ceiling.
4. A guarded-execution baseline runs one fixed LSQ through published-model
   loading, validation, compilation, mandatory start and terminal audit,
   PostgreSQL role and relation-owner checks, GRANT/RLS execution, result
   serialization, and connection establishment. It uses 5 warmups and 25
   measured iterations with a broad 1,000 ms p95 regression ceiling.
5. Guarded-execution evidence contains only timing, counts, the semantic result
   hash, and pass/fail state. It excludes LSQ text, generated SQL, parameters,
   physical relation names, principals, credentials, query IDs, and result
   rows.
6. Every warmup and measured execution must produce the same semantic result
   hash after excluding the unique audit query ID. A mismatch fails closed.
7. Timing thresholds are regression ceilings for the repository fixture, not
   production latency promises or hardware-independent capacity guidance.
8. Connection pooling, prepared plans, materialized views, and optional
   pre-aggregation are admitted only after baseline evidence identifies their
   target cost and a separate change defines bounded lifecycle, invalidation,
   freshness, authorization, cancellation, audit, and recovery behavior.
9. Any future persisted acceleration remains PostgreSQL-native, derived from
   an immutable published semantic revision, and non-authoritative. The
   published semantic model and source PostgreSQL authorization remain the
   source of truth.
10. Large-model authoring, operational dashboards, upgrade automation, failure
    recovery at scale, and the M10 reference comparison remain separate
    implementation slices under this boundary.

## Consequences

M10 begins with comparable machine-readable evidence rather than speculative
stateful optimization. The baseline deliberately includes current connection
and audit costs so later changes can demonstrate which cost moved while
preserving semantic-result determinism and PostgreSQL authorization.

The initial 1,000 ms guarded-execution ceiling is intentionally loose. It
detects severe regressions but is not evidence that connection management is
complete. Architecture-specific results must be recorded before tightening
the threshold or selecting an optimization.
