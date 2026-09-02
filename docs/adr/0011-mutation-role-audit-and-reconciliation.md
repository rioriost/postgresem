# ADR 0011: Mutation role, audit, idempotency, and reconciliation boundary

- Status: Accepted
- Date: 2026-09-01

## Context

The query executor is transaction-level `READ ONLY` and cannot be generalized
into a writer without weakening an accepted security boundary. Mutations also
need durable retry semantics and audit evidence for committed, rejected,
rolled-back, and indeterminate outcomes.

PostgreSQL GRANT, row-level security, constraints, and triggers must remain the
final authority. A gateway validation result is never permission to bypass a
database denial.

## Decision

1. Mutation uses a dedicated login, mapped writer role, configuration object,
   compiler entry point, executor, result type, and audit record.
2. The mapped writer role must be a membership of the mutation login and must
   not be a relation owner, superuser, or `BYPASSRLS`. The request cannot select
   a role, connection, conflict target, isolation level, or timeout.
3. The mutation transaction fixes `search_path` to `pg_catalog`, sets the
   allowlisted writer role locally, applies bounded statement/lock/idle
   timeouts, and executes exactly one compiler-generated statement.
4. Writer roles receive only the modeled table/column privileges. Applicable
   RLS `USING` and `WITH CHECK` policies, constraints, and triggers are not
   copied into the gateway and remain authoritative.
5. A security-definer claim function creates the idempotency record and
   started audit attempt inside the same transaction. Successful data changes,
   the committed audit state, and the replayable result commit atomically.
6. A matching committed idempotency key returns the stored result without
   executing DML and records a replay attempt. The key is bound to the
   principal, configuration profile, mapped writer role, LSM hash, and
   revision. Reuse under different authority, content, or revision is rejected.
   ADR 0015 and migration 0008 supersede the cross-authority part of this
   decision: new records are namespaced by a stable authority ID, while mapped
   role, content, and revision changes still fail closed. Pre-0008 records keep
   their legacy authority binding.
7. Validation, compilation, role, RLS, constraint, trigger, timeout, and
   rollback failures are recorded through a separate restricted audit
   connection after the business transaction has rolled back. Failure-audit
   inability is surfaced and never converted into success.
8. A connection loss during commit is returned as `indeterminate`. The
   idempotency key is the reconciliation handle: retrying resolves to the
   committed result if the transaction committed, or performs the mutation if
   it rolled back. Operators can also inspect audit/idempotency state by key
   hash without storing the key or row values.
   ADR 0015 and migration 0008 require the authority namespace as well as the
   key hash for application reconciliation; key-hash-only inspection is an
   operator database procedure for legacy records, not a public lookup API.
9. Mutation audit stores hashes, types, semantic field names, counts, policy
   context, and outcome. It does not store input values by default.

## Consequences

Committed mutation and audit evidence cannot diverge. Rejected attempts remain
observable without granting writer roles direct access to semantic audit
tables. Query credentials retain no business-data write privileges.
