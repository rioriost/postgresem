# ADR-013: Reference-driven differentiation and Apache Ossie import

- Status: Accepted
- Date: 2026-09-01

## Context

M7 compares postgresem 0.4 with current Wren AI, Cube, Malloy, and MetricFlow
releases. Those projects are materially ahead in some areas: broader authoring
languages, fan-out-safe multi-fact planning, time semantics, caches and
pre-aggregations, SDKs, and presentation integrations.

The comparison also confirms a category difference. None of the evaluated OSS
execution paths make the target PostgreSQL instance the semantic and
authorization authority while exposing a bounded typed mutation contract.
Their model source of truth is a file or manifest; access control is enforced
in a semantic service or left to the caller/platform; MetricFlow does not
execute at all. postgresem instead binds execution to an immutable revision and
allows PostgreSQL GRANT, RLS, constraints, and triggers to make the final
decision.

Apache Ossie, formerly Open Semantic Interchange, is the strongest
interoperability candidate found by the comparison. It is vendor-neutral,
Apache-2.0, supported by multiple semantic-layer ecosystems, and carries model
exchange metadata without defining a competing runtime, role, RLS, or
publication authority.

## Decision

1. Treat PostgreSQL-native authority as the primary differentiator, not
   feature-count parity.
2. Add deterministic catalog drift comparison using catalog snapshot v2.
   Every input fingerprint is verified, deparsed expressions use
   transaction-local `search_path = pg_catalog`, comparisons are role-bound,
   and GRANT/RLS/constraint drift is classified as breaking. The fingerprint
   also binds the scanning role's inheritance, superuser, `BYPASSRLS`,
   effective/settable role closure, and each relation owner so authorization
   drift cannot hide behind an unchanged role name.
   Default security-definer views additionally bind the owner's authorization
   attributes and role closure. Because PostgreSQL does not expose complete
   dependency edges for every string-bodied SQL or PL/pgSQL function, snapshot
   v2 conservatively fingerprints every non-system function, window function,
   and aggregate definition, owner, normalized EXECUTE grants, and, for
   `SECURITY DEFINER`, owner authorization. Function bodies remain hash-only
   in serialized evidence. A normalized ACL fingerprint also covers the
   current database plus non-system schemas, relations, sequences, and
   routines so owner object privileges cannot change invisibly. Any such
   executable or ACL change is breaking rather than risking an unchanged
   policy, CHECK, or view fingerprint.
   Aggregate evidence uses deparsed function, type, and operator identities
   rather than database-local OIDs. The snapshot also fingerprints the
   complete role attribute and direct-membership graph, including PostgreSQL
   16+ `INHERIT` and `SET` membership options, so runtime and policy-role
   authority changes are visible even when the scan uses a separate
   introspector.
   Unique constraints include PostgreSQL `NULLS NOT DISTINCT` semantics so a
   conflict and data-integrity behavior change cannot retain the same
   fingerprint. Views bind a normalized definition hash plus
   `security_invoker` and `security_barrier`. Constraints also bind
   enforcement, temporal `PERIOD`/`WITHOUT OVERLAPS`, and selective
   `ON DELETE SET NULL/DEFAULT` columns. Import uses only enforced,
   non-temporal primary and foreign keys.
3. Add a one-way Apache Ossie importer pinned to core specification `0.1.1`.
   The current `0.2.0.dev0` draft is intentionally rejected until a stable
   specification is published and reviewed.
4. Require a verified `postgresem catalog scan` snapshot for import. PostgreSQL
   catalog types, nullability, primary keys, foreign keys, relation existence,
   and effective visibility override or reject external claims.
5. Accept only one `ANSI_SQL` expression per imported field or metric. A field
   expression must be one portable column identifier. A metric expression must
   be one supported aggregate over one `dataset.field`. Arbitrary SQL,
   computed fields, multi-dialect expressions, cross-dataset metrics, custom
   extensions, unique-key semantics that cannot be represented, composite
   primary keys, and composite relationships are rejected rather than
   approximated.
   A verified single-column relationship projects target fields into the source
   model as `<relationship>_<field>` so the existing LSQ compiler can select
   them without adding path syntax or inferring joins.
   `dimension.is_time: true` is accepted only for PostgreSQL `date` or
   timestamp-without-time-zone columns. Ossie 0.1.1 has no reviewed model
   timezone, so timestamp-with-time-zone time roles are rejected instead of
   assuming UTC.
6. Produce a reviewable `SemanticSnapshot` candidate and structured warnings.
   Import never writes Semantic Schema rows, never publishes a revision, and
   never creates a writable projection.
7. Keep fan-out-safe multi-fact semantics, cumulative/time-spine metrics,
   PostgreSQL wire serving, and pre-aggregation in later milestones behind
   separate ADRs. A wire endpoint remains deferred because accepting arbitrary
   PostgreSQL SQL would violate the no-raw-SQL public contract.

## Consequences

Teams can migrate a conservative subset of a portable semantic model without
copying physical PostgreSQL type and key metadata into a second authority. The
same candidate is deterministic for identical Ossie bytes and catalog
evidence, and its revision hash can be reviewed with the existing model diff
workflow.

The adapter is intentionally narrower than the Ossie specification. It does
not silently downgrade unsupported semantics into prose or generic text types.
Descriptive and AI context currently produce warnings because the executable
snapshot projection does not preserve all authoring metadata. A later
publication workflow may map reviewed context into `semantic.term` and
description rows, but that is not part of this decision.

Adding YAML parsing increases the binary dependency set. The parser is used
only for offline administrative import, input is strictly deserialized, and
dependency audit remains a release gate.
