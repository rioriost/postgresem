# ADR 0017: Release-candidate contract freeze and evidence boundary

- Status: Accepted
- Date: 2026-09-03

## Context

M11 is the final compatibility stage before 1.0. The repository already has
versioned LSQ, LSM, Semantic Snapshot, catalog, MCP, compiler, migration, and
audit contracts, but their versions and change-control rules are distributed
across source, schemas, SQL, and documentation. A release candidate needs one
reviewable inventory and a gate that makes accidental drift visible.

M11 also requires security review, pilot evidence, and upgrade/rollback
rehearsal. Repository automation can prove deterministic contracts, supported
platform execution, isolated recovery, and absence of known high-severity
findings. It cannot self-certify independent external review or production
field operation.

## Decision

1. Version `0.9.0` is the release candidate. The candidate freezes:
   - LSQ v1 and LSM v1;
   - Semantic Snapshot v1/v2 loading and Snapshot v2 authoring;
   - compiler semantics `0.2.0` and mutation compiler semantics `0.1.0`;
   - catalog snapshot v2 and catalog diff v1;
   - MCP stdio `2024-11-05`, HTTP `2026-07-28`, tool schema v1, and the
     documented resource URI forms;
   - current CLI command names and structured output schema versions;
   - public error-code meanings;
   - migrations `0001` through `0010` and their audit/report function
     signatures.
2. `postgresem contract show` emits the deterministic RC inventory. A checked
   manifest and artifact hashes gate public schema, implementation, error,
   migration, and audit drift.
3. Changes to a frozen candidate surface require an ADR, a compatibility
   classification, tests, documentation, and an intentional manifest refresh.
   Breaking changes require a versioned replacement unless they close a
   correctness, security, or privacy defect.
4. `report beta` remains supported but deprecated in favor of
   `report operations`. It will not be removed before 1.0 without a security or
   correctness reason. Catalog scaffold and OSI import remain review-only
   authoring surfaces; neither publishes database state.
5. Automatic materialized-view routing, pre-aggregation, connection pooling,
   distributed rate limits, dynamic identity discovery, general update/delete,
   and down migrations remain explicitly deferred.
6. The PostgreSQL 18 release-qualification recovery gate must restore a
   pre-upgrade backup under the same database name and execute query and
   ingestion canaries with the previous release binary. Other compatibility
   legs may run the same recovery suite without the previous binary, but must
   report that the rollback rehearsal was not requested.
7. External security-review and production-pilot evidence is recorded
   separately and may not be replaced by maintainer or agent assertions.

## Consequences

- Accidental changes to frozen public surfaces fail a deterministic repository
  gate.
- Internal refactoring in contract-bearing files may require an intentional
  hash refresh even when behavior is unchanged; the review records that
  judgment.
- The repository can become RC-ready while external evidence remains visibly
  outstanding. M11 completion cannot be claimed until those independent gates
  are satisfied.
- PostgreSQL remains the only execution and authorization authority; the
  contract inventory does not create a second runtime source of truth.
