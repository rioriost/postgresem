# ADR-004: Synchronous PostgreSQL catalog scanner

- Status: Accepted
- Date: 2026-08-31

## Context

The administrative CLI needs an MVP catalog scan before the asynchronous
gateway execution boundary exists. The scan must be deterministic, must not
weaken PostgreSQL authorization, and must not persist CHECK or RLS expression
bodies.

## Decision

Use the synchronous `postgres` crate in the `postgresem` binary. It matches the
short-lived CLI workflow and keeps database I/O out of
`postgresem-compiler`. The initial implementation used `NoTls`. The M5
security remediation replaces it with the platform-native TLS connector for
all PostgreSQL clients. Connections must explicitly select `sslmode=require`
or `sslmode=disable`; omitted and downgrade-capable `sslmode=prefer`
configurations fail closed.

`postgresem catalog scan` accepts only the name of an environment variable
containing the connection URL. It does not accept the URL as a command-line
argument and does not include the URL or client errors in user-facing error
messages. The default variable is `DATABASE_URL`.

The scanner:

- opens a `READ ONLY`, `REPEATABLE READ` transaction;
- uses fixed catalog SQL and parameters rather than interpolated identifiers;
- scans visible non-system relations under the connected PostgreSQL role;
- treats effective GRANT checks as visibility hints, not authorization truth
  for later execution;
- records canonical database, schema, relation, column, constraint, role, and
  policy names, never OIDs as persistent identifiers;
- hashes CHECK and RLS expressions with SHA-256 immediately after reading them
  and exposes only hashes plus structured metadata;
- sorts every set-like collection before hashing and serializing the snapshot.

The canonical snapshot fingerprint is SHA-256 over compact JSON with the
fingerprint field cleared. The pretty JSON representation is presentation
only and does not affect the fingerprint.

## Security boundary

PostgreSQL remains authoritative for GRANT and RLS enforcement. A scan should
use a dedicated, least-privilege introspection role and does not change
catalogs or semantic revisions. Snapshot output contains relation and column
comments and therefore must be handled as potentially sensitive metadata.
Connection credentials exist only in the selected process environment and the
client connection configuration.

## Consequences

The CLI can produce reproducible source evidence without adding asynchronous
runtime machinery to the compiler. Remote databases can use
`sslmode=require` with platform trust roots and hostname validation. Custom
trust roots and client certificates require a follow-up decision and are not
yet configurable.
