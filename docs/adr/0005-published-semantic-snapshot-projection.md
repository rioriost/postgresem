# ADR-005: Strict projection of published revisions to SemanticSnapshot v1

- Status: Accepted
- Date: 2026-08-31

## Context

The compiler consumes an immutable `SemanticSnapshot`, while semantic metadata
is stored as normalized PostgreSQL rows. Database row order is not semantic,
and silently ignoring unsupported database values could change compiler
behavior without changing a published revision's identity.

## Decision

Load only the named project's current published revision with fixed,
parameterized SQL in a read-only, repeatable-read transaction. Project the
normalized rows into `SemanticSnapshot` v1 with strict parsers for logical
types, versioned aggregation metadata, typed literal filters, visibility,
relationship field bindings, cardinality, and join type.

Metric expressions use versioned structured JSON:

```json
{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}
```

They never contain raw SQL. Snapshot hashing clears `revision_hash` and sorts
models and each model's fields, metrics, and relationships before
serialization. The loader rejects unsupported values, unsupported normalized
features, multi-column relationships, and canonical hash mismatches.

Fields bind to relationships only by revision-scoped UUID. Unresolved
relationship names are rejected before publication rather than persisted as a
fallback string.

## Consequences

Database insertion order cannot affect revision identity or export output.
Schema and compiler evolution requires an explicit projection update rather
than implicit coercion. Snapshot v1 cannot represent calculated fields,
multi-column relationships, relationship routing options, or cross-database
sources, so the loader fails closed when those normalized features are used.
