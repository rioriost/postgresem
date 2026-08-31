# ADR 0006: Guarded database execution

## Status

Accepted

## Context

The compiler produces one deterministic parameterized `SELECT`, but executing
it safely requires controls that are not part of LSQ. In particular, database
credentials and the mapped PostgreSQL role must not be supplied by a request,
and execution must preserve PostgreSQL row-level security.

Audit logging is mandatory. A source query must not run unless its `started`
record is durable, and a successful source query must not produce a
success-shaped response if its terminal audit update fails.

The synchronous PostgreSQL client also needs one bind representation that works
for every compiler literal type.

## Decision

- `postgresem query execute <path> --project <name>` reads the runtime URL,
  audit URL, and mapped role only from environment variables. CLI options may
  select environment variable names, never their values.
- Published model loading and source execution use the runtime connection.
  Audit lifecycle writes use a separate connection authenticated as a dedicated
  audit writer.
- Migration 0003 exposes security-definer functions for starting and finishing
  query audit rows. The audit role has no direct table privileges.
- A durable `started` row is written after LSQ normalization and compilation
  and before opening the source execution transaction. Every attempted guarded
  execution then records `succeeded`, `failed`, or `cancelled`.
- Source execution uses a `READ ONLY` transaction. Before `SET LOCAL ROLE`, the
  executor verifies that the configured role exists, is a membership of the
  runtime login, and is neither superuser nor `BYPASSRLS`. It also rejects a
  role that owns any physical source relation in compiled lineage.
- Role identifiers are restricted to a conservative ASCII identifier subset
  and quoted. Statement, lock, and idle-in-transaction timeouts are
  transaction-local.
- Every compiler bind value is sent as text. Generated SQL renders `$n::text`
  for text and `$n::text::<target type>` for other PostgreSQL types.
- The compiled query is wrapped by fixed generated SQL that returns each row as
  a JSONB array. Numeric values are converted to JSON strings; integers and
  booleans retain their JSON types. The executor enforces both the compiler row
  limit and a configured result-row JSON byte limit.

## Consequences

RLS identity is selected by trusted deployment configuration rather than LSQ.
Source relation owners, superusers, and `BYPASSRLS` roles cannot be mapped.
Audit availability is on the critical execution path by design. The MVP uses
local non-TLS PostgreSQL connections and synchronous execution; transport-level
cancellation can be added without changing the audit lifecycle.
