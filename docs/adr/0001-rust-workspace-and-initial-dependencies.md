# ADR-001: Rust workspace and initial dependencies

- Status: Accepted
- Date: 2026-08-31

## Context

The gateway needs a deterministic typed compiler, a single deployable binary,
and a pure compiler boundary that can be tested without PostgreSQL or MCP.

## Decision

Use a Rust workspace with two crates:

- `postgresem-compiler`: pure LSQ parsing, validation, planning, compilation,
  and lineage logic.
- `postgresem`: CLI, transport, PostgreSQL access, execution, audit, and
  application wiring.

The initial dependency set is intentionally small:

- `serde` and `serde_json` for versioned JSON contracts.
- `sha2` for stable content hashes.
- `thiserror` for typed compiler errors.
- `clap` for the administrative CLI.

Dependencies for PostgreSQL, MCP, JSON Schema validation, SQL AST rendering,
and async execution require follow-up ADR review before adoption.

## Consequences

Compiler behavior can remain deterministic and I/O-free. The two-crate
boundary adds a small amount of workspace wiring but avoids premature service
or crate decomposition.

