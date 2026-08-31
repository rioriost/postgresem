# ADR 0009: Beta operations, transport, and evidence boundaries

- Status: Accepted
- Date: 2026-08-31

## Context

M5 adds operational recovery, measurable service objectives, supply-chain
hardening, and adoption evidence. It also requires an explicit decision on MCP
Streamable HTTP.

The current gateway has a narrow security boundary:

- stdio fixes the principal, project, database role, credentials, and budgets
  at process startup;
- LSQ is the only public query input;
- PostgreSQL enforces the final GRANT and RLS boundary;
- every executed query requires durable start and terminal audit records.

Adding an unauthenticated network transport or a Web handler that talks
directly to PostgreSQL would weaken that boundary.

## Decision

1. Migrations remain forward-only. The supported beta upgrade path is the
   latest previous release schema (N-1) to the current schema.
2. Rollback means restoring a validated pre-upgrade backup into a new database
   or volume and redeploying the previous binary. Down migrations and in-place
   destructive rollback are not supported.
3. Postgresem backup tooling covers Semantic Schema state and audit records.
   Source business-data backup, cluster roles, encryption, retention, and
   disaster-recovery policy remain the database operator's responsibility.
4. Restore validation must use a disposable database, apply current
   migrations, verify the published semantic revision, and run a guarded query.
5. SLO and adoption reports are computed locally from aggregate audit data.
   Postgresem sends no telemetry to an external service by default and does not
   expose principals, LSQ documents, generated SQL, result rows, or model names
   in aggregate reports.
6. MCP Streamable HTTP is deferred. A conforming remote transport requires
   authenticated request identity, issuer/audience/expiry validation,
   principal-to-role mapping, request limits, origin policy, cancellation, and
   equivalent authorization tests. Current demand does not justify introducing
   that attack surface.
7. The sample Web application is not an MCP HTTP transport. It binds only to
   loopback and uses the existing `postgresem mcp serve` stdio contract. The
   browser cannot choose commands, credentials, principals, database roles, or
   raw SQL.
8. Release signing should use short-lived GitHub OIDC identity rather than a
   long-lived private key when the release workflow is extended.
9. Repository tests and maintainer runs cannot satisfy the beta field gate or
   an independent security review.

## Consequences

- Migration and restore behavior can be tested without claiming ownership of a
  production PostgreSQL backup strategy.
- The Web demo exercises the same MCP validation, execution, RLS, and audit
  path that external clients use.
- Remote browser access is intentionally unsupported until the HTTP identity
  model is implemented.
- M5 can publish implementation evidence, but beta completion remains blocked
  until two non-fixture databases complete four weeks of operation without a
  P0/P1 security or correctness defect.

