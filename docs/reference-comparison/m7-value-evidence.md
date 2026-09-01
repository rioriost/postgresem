# M7 PostgreSQL user-value evidence

- Decision date: 2026-09-01
- Scope: `0.5` preview promotion
- Evidence owner: project maintainer acting as the initial PostgreSQL operator
  and semantic-model consumer

## Accepted user problem

A PostgreSQL team adopting an external semantic exchange file should not have
to trust duplicated physical types, nullability, keys, relation names, or
security assumptions. The same team needs a review gate when schema, GRANT,
constraint, or RLS metadata changes underneath a published semantic revision.

The M7 reference comparison showed that portable interchange is becoming
useful across Wren AI, MetricFlow, Cube participants, and Apache Ossie, while
database-authoritative publication and governed caller writes remain a
postgresem-specific position.

## Accepted M7 value

The project maintainer reviewed the comparison and approved completing M7 with
the following selected gaps:

1. **Catalog-bound Apache Ossie import.** One portable model can be converted
   into a reviewable postgresem snapshot without manually copying PostgreSQL
   types, nullability, PKs, FKs, or visibility metadata into a second source of
   truth.
2. **Authorization-aware catalog drift.** PostgreSQL GRANT, RLS, policy,
   constraint, and source-type changes become deterministic breaking evidence
   instead of an undocumented runtime surprise.
3. **Explicit non-parity.** Broader SQL surfaces, caller-asserted authorization,
   unauthenticated remote MCP, and external runtime model authority are rejected
   even when a reference implementation supports them.

## Fixture evidence

The committed Ossie fixture covers two datasets, six direct fields, two
metrics, and one FK-backed relationship. Import verifies the catalog
fingerprint and emits a query-only immutable candidate. Rejection coverage
includes raw/computed expressions, multi-dialect expressions, unsupported
PostgreSQL types, invalid aggregate/type combinations, custom extensions,
unique-key loss, composite PK/FK semantics, mismatched catalog evidence, and
unsafe time roles.

The runtime comparison uses the same four-row PostgreSQL 18 table and expected
`total_revenue` value for every reference engine. This separates query-semantic
execution evidence from documentation-only capability claims.

## Evidence boundary

This is sufficient user-value evidence for a `0.5` preview decision because the
maintainer is the initial target PostgreSQL user and accepted the implemented
workflow after reviewing the alternatives. It is not independent production
adoption evidence and does not complete the external M5 beta evidence items.
No `0.x` release is represented as production-ready or long-term supported.
