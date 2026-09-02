# Changelog

## [Unreleased]

## [0.7.0] - 2026-09-02

### Added

- Authenticated stateless Streamable HTTP for MCP `2026-07-28`, while
  preserving the MCP `2024-11-05` line-delimited stdio adapter.
- Local-file OAuth resource-server authority with RFC 9728 protected-resource
  metadata, strict asymmetric JWT verification, exact subject-to-role
  mappings, HMAC-pseudonymized audit identities, and startup role preflight.
- Identity-dependent `server/discover`, private capability discovery,
  per-authority token-bucket/concurrency limits, bounded JSON/SSE responses,
  and disconnect-to-PostgreSQL cancellation.
- Capability-gated remote mutation and authenticated reconciliation with
  stable authority-scoped idempotency.
- PostgreSQL 16–18 integration coverage for invalid hosts, origins, tokens,
  protocol metadata, distinct tenant RLS results, mutation isolation, limits,
  and cancellation audit completion.
- TypeScript and Python MCP v2 client guidance plus a strict authority schema
  and example configuration.

### Changed

- Advance the source package to M9 `0.7.0` and database migrations through
  `0009_mutation_reconcile_precedence`.
- Add `reconcile_semantic_mutation` as the eighth mutation-enabled MCP tool.
- Require the HTTP listener to bind to explicit loopback and operate behind a
  colocated HTTPS reverse proxy that preserves public Host and disconnects.
- Keep local JWKS/config files authoritative; the gateway performs no runtime
  OIDC discovery, JWKS fetch, token issuance, or caller-selected role mapping.

### Security

- Reject duplicate authority identities, unsafe PostgreSQL roles, embedded or
  remote JOSE keys, symmetric/unknown algorithms, invalid token type,
  issuer/audience/time/scope claims, and mismatched MCP header/body metadata.
- Keep remote mutation disabled by default and advertise it only when the
  global gate, verified scope, exact identity mapping, writer role, GRANT/RLS,
  idempotency, and mandatory audit boundaries all pass.
- Namespace new mutation idempotency records by immutable authority ID while
  preserving fail-closed reconciliation for pre-0.7 legacy records.
- Abort before database execution when an HTTP client disconnects before the
  PostgreSQL cancel handle is registered, closing the audit lifecycle without
  starting an unobserved statement.

## [0.6.0] - 2026-09-02

### Added

- Semantic Snapshot v2 metric additivity and explicit aggregation anchors,
  while preserving loading and canonical hashes for existing Snapshot v1
  revisions.
- Deterministic two-stage PostgreSQL aggregation for direct one-to-many
  dimensions and filters without `SUM(DISTINCT ...)` or inferred grain.
- Balanced accepted/rejected compiler evaluations plus PostgreSQL oracle,
  duplicate-child, multi-branch fan-out, and root/child RLS execution fixtures.
- A Docker-standard `Dockerfile` kept in parity with the Apple Container
  definition, a nonroot Linux Compose overlay, and rootless Podman Quadlet
  units for PostgreSQL migration, fixture publication, and gateway startup.
- CI gates that build and run the image with Docker and Podman, start the
  Docker Compose stack, verify UID 10001, and reject invalid Quadlet units.

### Changed

- Advance query compiler semantics to `0.2.0` and database migrations through
  `0007_fanout_anchor_invariants`.
- Expose metric additivity and aggregation anchors through semantic discovery
  and public semantic lineage without exposing generated SQL or physical
  relation names.
- Keep LSQ v1 and LSM v1 unchanged; Snapshot v2 remains valid for the existing
  bounded insert/approved-upsert mutation compiler.
- Serialize semantic child authoring with revision publication and enforce
  anchor eligibility through concurrency-safe PostgreSQL foreign keys.

### Security

- Reject missing, invalid, or mixed anchors, joined metric inputs or filters,
  semi-additive fan-out, many-to-many relationships, and dimension-only
  one-to-many traversal.
- Keep every fan-out source relation in guarded-execution lineage so ownership,
  GRANT, and RLS checks cover both root and child relations.
- Validate database-authored anchors as direct entity keys in the same
  schema-v2 model and immutable draft revision.

## [0.5.0] - 2026-09-01

### Added

- Deterministic PostgreSQL catalog snapshot v2 and drift comparison with
  canonical fingerprint verification, fixed deparser search path,
  role/database binding, complete role-graph and security-definer view-owner
  authorization state, relation ownership, normalized object ACL evidence,
  OID-independent non-system function/window/aggregate definition and
  EXECUTE-grant evidence, and compatible, review-required, and breaking
  classifications.
- One-way Apache Ossie `0.1.1` import into a reviewable query-only candidate,
  with PostgreSQL-authoritative type, nullability, key, relationship, and
  visibility checks.
- Rejection coverage for unsupported expressions, lossy key semantics,
  aggregate/type mismatches, unsafe time roles, and tampered catalog evidence.
- Pinned Wren AI, Cube, Malloy, and MetricFlow runtime harnesses over one
  immutable PostgreSQL 18 task, including locked transitive dependencies and
  machine-readable GitHub Actions evidence.
- Maintainer acceptance evidence for catalog-bound import and
  authorization-aware drift as the selected M7 user-value gaps.

### Changed

- Make PostgreSQL-native authority, governed mutation, immutable publication,
  no-raw-SQL requests, and authorization drift the primary reference
  differentiators instead of pursuing feature-count parity.
- Defer fan-out-safe aggregation anchors, cumulative/time-spine metrics,
  PostgreSQL wire serving, and pre-aggregation to later milestones behind
  separate design decisions.

### Security

- Fail closed when imported semantics are ambiguous, computed, multi-dialect,
  cross-dataset, type-incompatible, composite, or insufficiently
  timezone-qualified.
- Treat relation-owner, role-membership, inheritance, superuser, and
  `BYPASSRLS` drift as breaking authorization evidence.
- Capture PostgreSQL membership `SET` capability separately from inherited
  privileges and treat unique-constraint `NULLS NOT DISTINCT` changes as
  breaking.
- Treat view-definition, `security_invoker`, `security_barrier`, constraint
  enforcement, temporal `PERIOD`/`WITHOUT OVERLAPS`, and selective
  `ON DELETE SET NULL/DEFAULT` column changes as breaking drift.
- Keep external models and reference runtimes outside the postgresem runtime,
  publication, credential, GRANT, RLS, and audit authority.

## [0.4.0] - 2026-09-01

### Added

- LSM v1 with strict typed values, duplicate-key rejection, bounded batches,
  deterministic normalization, and mandatory idempotency keys.
- Published writable-model projections and a deterministic compiler for
  bounded inserts and approved idempotent upserts.
- Separate mutation credentials, writer roles, guarded transactions,
  PostgreSQL RLS `WITH CHECK`, atomic audit/idempotency state, replay, and
  reconciliation.
- CLI and capability-gated MCP mutation validation/execution surfaces without
  raw SQL or physical identifiers.
- Native Linux amd64/arm64 CI and release gates that execute runtime images and
  packaged binaries against PostgreSQL 18.
- Linux installer success and architecture-selection coverage.

### Changed

- Redefine M6 as the `0.4` governed-ingestion and Linux portability milestone,
  followed by comparison-driven `0.5`–`0.9` stages before `1.0`.
- Plan a separate typed insert/upsert contract without weakening the existing
  read-only query executor or PostgreSQL GRANT/RLS enforcement.
- Make executed Linux amd64 and arm64 artifact coverage an M6 release gate.

### Security

- Pin every GitHub Actions dependency to an immutable commit SHA, including
  the release-publishing action that receives `contents: write`.
- Remove runtime membership in test superuser and `BYPASSRLS` roles.
- Add TLS-capable PostgreSQL connections and reject omitted or
  downgrade-capable TLS modes.
- Require exact Sigstore workflow/tag verification before the installer trusts
  release checksums.
- Fix the guarded transaction `search_path` to `pg_catalog`.
- Keep query execution transaction-level `READ ONLY` while placing all writes
  behind the separate mutation contract and credentials.

## [0.3.0-beta.1] - 2026-08-31

Beta release.

### Added

- M5 N-1 migration and same-name backup/restore validation.
- Privacy-safe `postgresem report beta` audit aggregation.
- Loopback-only commerce Web demo over the existing MCP stdio boundary.
- Beta incident, recovery, SLO/adoption, and release-verification guidance.
- Keyless GitHub OIDC signing for checksum and GHCR release artifacts.

All notable changes to PostgreSQL Semantic Gateway are documented here.

The project follows Semantic Versioning. Before 1.0, minor and prerelease
versions may contain documented breaking changes.

## [0.2.0-alpha.1] - 2026-08-31

Developer preview release.

### Added

- MCP 2024-11-05 stdio server with five semantic-only tools and three resource
  forms.
- Guarded read-only PostgreSQL execution with fixed role mapping, RLS
  preservation, time and result budgets, and mandatory audit lifecycle writes.
- Deterministic PostgreSQL catalog scanning and DB-backed published semantic
  snapshot loading.
- Deterministic semantic model diff output with preview compatibility
  classification and an optional breaking-change gate.
- Portable environment diagnostics for Apple Container and Docker Compose.
- A 100-model compiler latency benchmark and 100-relation catalog determinism
  check.

### Security

- No raw SQL MCP tool.
- Request-supplied principals, roles, projects, connection settings, and
  passwords are rejected.
- Hidden and nonqueryable semantic objects are not distinguishable through the
  public MCP surface.

### Known limitations

- PostgreSQL connections use `NoTls`.
- MCP transport is stdio only.
- Concurrent MCP cancellation is not implemented; statement timeout is the
  current cancellation boundary.
- Backup/restore automation and N-1 migration testing are planned for M5.

## [0.1.0] - 2026-08-31

Initial executable MVP foundation with LSQ v1, Semantic Schema v1, deterministic
compilation, fixtures, and integration tests.
