# Changelog

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
