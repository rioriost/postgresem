# ADR-003: LSQ v1 types, time, numeric, and NULL semantics

- Status: Accepted
- Date: 2026-08-31

## Context

LSQ must produce the same meaning independently of JSON object ordering and
the PostgreSQL session timezone. Invalid typed literals must fail before a
database query starts.

## Decision

LSQ v1 uses explicit literal types. Numeric values are decimal strings in the
public result contract and cannot use exponent, NaN, or infinity forms. Dates
use valid ISO 8601 calendar dates. Timestamp literals use RFC 3339 with an
explicit `Z` or numeric offset.

The compiler distinguishes PostgreSQL `timestamp` from `timestamptz`.
Time-grain operations on `timestamptz` require a model timezone and compile
through PostgreSQL's `timezone` function before truncation. Comparing a
`timestamptz` field to a date interprets midnight in the model timezone, not
the connection's `TimeZone` setting. The timezone is passed as a bind
parameter and is covered by the query hash.

All LSQ literals, model-filter literals, timezone values, and limits are bind
parameters. Parameters may be reused when type and value are identical.
Identifier quoting is limited to immutable semantic snapshot content.

Dimension projections are grouped even when no metric is selected. This makes
a dimension-only query return semantic groups rather than exposing arbitrary
row-level duplicates.

Aggregate functions preserve PostgreSQL NULL behavior in v1. In particular,
`sum` and `avg` return NULL for a group with no matching rows after a metric
filter. The compiler does not silently coalesce these values to zero.

## Consequences

Models containing `timestamptz` time dimensions must declare a valid PostgreSQL
timezone before publication. Publisher validation will verify timezone names
against the target PostgreSQL version. Supporting local timestamp literals or
configurable zero-fill semantics requires a future LSQ version or explicit
model capability.

