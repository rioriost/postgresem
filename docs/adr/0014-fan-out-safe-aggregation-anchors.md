# ADR-014: Explicit fan-out-safe aggregation anchors

- Status: Accepted
- Date: 2026-09-02

## Context

M7 demonstrated that mature semantic layers protect aggregates from fan-out,
while postgresem 0.5 rejects every one-to-many relationship. That rejection is
safe but prevents common PostgreSQL reporting such as order revenue by item
SKU. A naive join followed by `SUM`, or `SUM(DISTINCT measure)`, is not correct:
duplicate child rows multiply the measure, and distinct measure values collapse
unrelated facts that happen to have the same value.

postgresem must add this capability without inferring a grain, accepting raw
SQL, creating a second execution engine, or weakening PostgreSQL GRANT and RLS.
The existing LSQ v1 contract already identifies one root model and its
dimensions and metrics, so a bounded single-fact solution does not require a
new query language.

## Decision

1. Semantic Snapshot v2 adds an optional `aggregation_anchor` field to each
   metric. The value is a field in the metric's root model. A valid anchor is a
   direct, non-relationship entity-key field. Snapshot v1 remains loadable and
   retains its existing canonical hash because absent anchors are not
   serialized.
2. A query may traverse direct `one_to_many` relationships only when it
   projects at least one metric and every projected metric declares the same
   valid anchor. `many_to_many`, reverse traversal, multi-hop paths, mixed
   anchors, unanchored metrics, joined metric inputs, and joined metric filters
   remain rejected.
3. The compiler renders one deterministic PostgreSQL statement with two
   aggregation stages:
   - the inner stage joins required relationships, applies the LSQ filter, and
     groups by projected dimensions plus the anchor;
   - each metric input is reduced with `max` inside an anchor group. This is a
     value-preserving reduction because metric inputs and metric filters are
     required to be local to the root row;
   - the outer stage applies the declared metric aggregate across the unique
     anchor groups and then applies ordering and the bounded limit.
4. Metric filters keep PostgreSQL filtered-aggregate NULL and empty-set
   semantics by projecting a typed `CASE` input in the inner stage. Global
   filters may reference an approved one-to-many field; duplicate matches are
   removed by the dimension-plus-anchor grouping.
5. Every joined relation and source column remains in compiler lineage.
   Guarded execution continues to validate all source relation owners and
   role attributes and executes under the configured PostgreSQL role in a
   read-only transaction, so GRANT and RLS apply to every participating row.
6. Adding, removing, or changing a metric anchor is a breaking semantic-model
   change. Adding a relationship remains compatible only while it cannot alter
   routing for the current direct, explicitly named field bindings.
7. PostgreSQL stores the anchor as a foreign key from `semantic.metric` to
   `semantic.field`. A validation trigger requires the same revision and model,
   a direct `entity_key`, and Semantic Snapshot schema version 2.
8. Apache Ossie 0.1.1 does not declare a reviewed aggregation anchor. The
   importer therefore continues to emit Snapshot v1 candidates without
   anchors and does not infer fan-out safety.
9. Multi-fact LSQ, multi-hop and reverse routing, bridge allocation,
   semi-additive time axes, cumulative metrics, time spines, custom calendars,
   and typed update/delete remain deferred to separate ADRs.

## Consequences

M8 can answer bounded one-root-model fan-out queries correctly, including
duplicate child rows and multiple direct one-to-many branches, while preserving
LSQ v1 and the no-raw-SQL public contract. The approach intentionally favors an
explicit authoring obligation and deterministic rejection over broad automatic
join inference.

An anchored metric can be repeated across different dimension groups by
definition; the anchor prevents accidental multiplication within a group but
does not allocate a fact across groups or guarantee that group totals add to a
grand total. Authors must not represent allocation semantics with this feature.
That limitation is exposed in discovery and compatibility documentation.
