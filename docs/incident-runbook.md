# Pre-1.0 incident runbook

This runbook is for query and governed-mutation pilot deployments. It does not
replace an organization's production incident process.

## Immediate priorities

1. **Contain:** stop gateway/Web demo processes and prevent new requests.
2. **Preserve:** retain structured stderr logs, migration state, aggregate
   report, relevant audit rows, deployment version, and database activity
   evidence under restricted access.
3. **Protect:** rotate exposed credentials and revoke unsafe role membership.
4. **Recover:** restore a validated backup or correct the configuration before
   resuming.
5. **Verify:** run security, migration, MCP, and guarded-query canaries.
6. **Communicate:** state observed impact and uncertainty without including
   credentials, SQL, LSQ literals, private model names, or result rows.

## Suspected RLS or cross-tenant exposure

- stop request processing immediately;
- preserve the query ID, semantic revision, database role, policy definitions,
  and database logs;
- revoke the mapped role from `postgresem_runtime`;
- rotate runtime and audit credentials;
- reproduce only with synthetic data;
- run the tenant isolation integration suite before re-enabling access.

Treat any confirmed cross-tenant result as P0.

## Mutation ambiguity or unexpected write

- stop mutation processing while preserving read-only access only if the
  incident scope permits it;
- retain the mutation ID, hashed idempotency key, semantic revision, mapped
  writer role, terminal audit status, and PostgreSQL transaction evidence;
- do not blindly retry `MUTATION_COMMIT_INDETERMINATE`;
- set the original key in `POSTGRESEM_IDEMPOTENCY_KEY` and run
  `postgresem mutation reconcile --project <project>`;
- revoke the mapped writer role from `postgresem_mutation_runtime` if GRANT or
  RLS scope is suspect;
- inspect PostgreSQL RLS `USING`/`WITH CHECK`, column privileges, constraints,
  and triggers as the final authorization and integrity boundary;
- resume only after the same-key outcome is known and insert/upsert, replay,
  rollback, audit-failure, and cross-tenant canaries pass.

Never resolve ambiguity by deleting idempotency state or issuing an ad hoc
write. Preserve evidence and use a new reviewed request/key only after the
original outcome is established.

## Audit lifecycle gap

- stop execution if a durable `started` row or terminal update cannot be
  written;
- run `postgresem report beta` and inspect incomplete counts;
- preserve incomplete rows as evidence and do not rewrite them as successful;
- determine whether the gateway process and PostgreSQL backend that owned each
  query have terminated before reconciliation;
- repair permissions or database availability, then use the restricted audit
  lifecycle function to record an explicit `failed` or `cancelled` terminal
  state according to the deployment's incident policy;
- use an error code that records uncertainty or confirmed process termination,
  and retain the supporting process/database evidence under restricted access;
- rerun `postgresem report beta` and resume only when the expected window has
  no unexplained incomplete records.

Postgresem does not automatically convert old `started` rows into terminal
states. Age alone cannot prove whether a database query is still running, and
silently marking uncertain execution as successful or failed would weaken the
audit record.

## Migration or restore failure

- do not continue partially;
- retain the failing migration output and `semantic.schema_migration`;
- do not apply ad hoc down SQL;
- follow [backup and restore](backup-restore.md);
- resume only after revision hashes and guarded canary queries match expected
  behavior.

## Excessive latency, cancellation, or lock timeout

- distinguish validation/compilation latency from PostgreSQL execution time;
- check `postgresem report beta`, statement timeout, lock timeout, indexes,
  locks, and source query plans through DBA-controlled tooling;
- narrow LSQ time ranges and result limits rather than raising every budget;
- verify cancelled transactions leave no incomplete application response or
  audit lifecycle.

## Suspected release compromise

- stop installing or pulling the affected version;
- record release URL, digest, certificate identity, and verification output;
- compare against the GitHub release workflow and repository tag;
- rotate any credentials used by affected deployments;
- publish a corrected release rather than replacing existing immutable assets.

Report security issues using the private process in [SECURITY.md](../SECURITY.md).
