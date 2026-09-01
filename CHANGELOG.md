# Changelog

## [Unreleased]

### Security

- Pin every GitHub Actions dependency to an immutable commit SHA, including
  the release-publishing action that receives `contents: write`.
- Remove runtime membership in test superuser and `BYPASSRLS` roles.
- Add TLS-capable PostgreSQL connections and reject omitted or
  downgrade-capable TLS modes.
- Require exact Sigstore workflow/tag verification before the installer trusts
  release checksums.
- Fix the guarded transaction `search_path` to `pg_catalog`.

## [0.3.0-beta.1] - 2026-08-31

Beta release.

### Added

- M5 N-1 migration and same-name backup/restore validation.
- Privacy-safe `postgresem report beta` audit aggregation.
- Loopback-only commerce Web demo over the existing MCP stdio boundary.
- Beta incident, recovery, SLO/adoption, and release-verification guidance.
- Keyless GitHub OIDC signing for future checksum and GHCR releases.

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
