# Changelog

## [Unreleased]

## [0.5.0] - 2026-09-01

### Added

- Deterministic PostgreSQL catalog snapshot v2 and drift comparison with
  canonical fingerprint verification, fixed deparser search path,
  role/database binding, role authorization state, relation ownership, and
  compatible, review-required, and breaking classifications.
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
