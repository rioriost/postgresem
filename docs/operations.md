# Developer-preview operations

This guide describes the current local/process-oriented preview. It is not a
production runbook.

## Roles and credentials

| Role | Login | Current purpose |
|---|---:|---|
| `postgres` | yes | local fixture initialization and migrations |
| `postgresem_owner` | no | owner of the `semantic` schema |
| `postgresem_runtime` | yes, `NOINHERIT` | loads published metadata and assumes one mapped source role |
| `postgresem_audit_writer` | yes | member of `postgresem_auditor`; executes audit security-definer functions |
| `postgresem_auditor` | no | can start/finish audit rows, not select the audit table |
| `postgresem_analyst` | no | default fixture source-data role |
| `postgresem_tenant_a` / `postgresem_tenant_b` | no | fixture roles that exercise source RLS |
| `postgresem_editor`, `postgresem_publisher`, `postgresem_introspector` | no | schema roles reserved for controlled metadata workflows |

Use distinct runtime, audit-writer, and administration credentials. Never use a
production credential in the sample stack. MCP reads the project, conninfo
variable names, separate passwords, and mapped role once at startup. Requests
cannot supply or override them.

The executor verifies that the runtime login is a member of the mapped role.
It rejects nonexistent roles, superusers, `BYPASSRLS` roles, and roles that own
any source relation in compiled lineage.

## Startup, checks, and shutdown

```sh
make doctor
make dev-up
container list --all | grep 'postgresem-\(db\|gateway\)'
```

`make dev-up` requires `.env`, waits for PostgreSQL health, applies unapplied
forward migrations, runs the idempotent commerce seed, and starts a gateway
container. For Apple Container compatibility, Compose overrides the image's
configured user to root so `container-compose` can apply its `/etc/hosts`
fallback before startup. The container command immediately executes:

```text
gosu postgresem sleep infinity
```

`gosu` replaces itself after changing UID/GID, so the resulting PID 1 idle
process runs as the unprivileged `postgresem` user. The Compose container
configuration itself is root and must not be described as a nonroot-container
configuration.

Database and MCP smoke checks:

```sh
container exec postgresem-db \
  psql --no-psqlrc -U postgres -d postgresem_dev -Atc \
  'SELECT version FROM semantic.schema_migration ORDER BY version'

python3 examples/commerce/mcp_smoke.py -- make mcp
```

Stop services with `make dev-down`. The named volume remains. There is no
preview command that safely deletes or restores it.

## MCP transport and logs

`make mcp` runs:

```text
container exec -i --user postgresem postgresem-gateway postgresem mcp serve
```

Standard input and output are reserved for one JSON-RPC object per line.
Standard error receives one JSON completion/error record for every handled
request:

```json
{"event":"mcp_request","method":"tools/call","tool":"query_semantic_model","status":"success","code":"OK","elapsed_ms":12}
```

The record has `event`, recognized `method`, recognized `tool` or `null`,
`status`, public `code`, and `elapsed_ms`. It intentionally omits LSQ values,
database configuration, generated SQL, rows, principal data, and private
requested names. The `make mcp` wrapper may print compose startup diagnostics
to stderr before MCP JSON begins; the server itself emits JSON lines.

The explicit `--user postgresem` is a security-relevant part of the command.
Do not omit it when attaching manually: because Compose sets the gateway
container's configuration user to root for Apple Container host-file patching,
an exec without `--user` would not preserve the intended application-process
privilege boundary. Both the idle process and MCP process are unprivileged.

The gateway container has no HTTP listener and normally has no request log to
retrieve with `container logs`; the MCP process is attached to its invoking
client. Capture stderr in the supervising client if operational retention is
required, applying local access controls and retention limits.

## Audit inspection

Every guarded query must write a durable `started` row before source execution
and then update it to `succeeded`, `failed`, or `cancelled`. If the initial
audit write fails, the source query does not run. If the terminal update fails,
the caller does not receive a success-shaped result.

For the local fixture, inspect a minimal view as the container administrator:

```sh
container exec postgresem-db \
  psql --no-psqlrc -U postgres -d postgresem_dev -P pager=off -c "
    BEGIN READ ONLY;
    SELECT query_id, status, error_code, semantic_revision_hash,
           compiler_version, config_profile,
           validation_duration_ms, compile_duration_ms,
           database_duration_ms, serialization_duration_ms,
           row_count, byte_count, truncated, started_at, completed_at
    FROM semantic.query_audit
    ORDER BY started_at DESC
    LIMIT 50;
    COMMIT;"
```

`lineage`, `policy_context`, query/SQL hashes, parameter types, and principal
hashes are operationally sensitive correlation metadata. Restrict access even
though raw LSQ literals, credentials, SQL text, and result rows are not stored.
The preview has no audit retention or audit-reader provisioning automation.
Do not grant the audit-writer login direct read access merely for convenience.

## Time, row, request, and result budgets

| Control | Default/current bound | Behavior |
|---|---:|---|
| LSQ limit | 100 rows when omitted | compiler default |
| LSQ hard limit | 10,000 rows | larger values are rejected |
| result JSON bytes | 1,048,576 | returns complete rows up to the boundary, then `truncated: true` and a warning |
| statement timeout | 30,000 ms | PostgreSQL cancels the statement |
| lock timeout | 5,000 ms | transaction-local |
| idle transaction timeout | 5,000 ms | transaction-local |
| MCP input line | 1,048,576 bytes | oversized line is consumed and rejected |
| model page | 50 default, 100 maximum | revision-bound opaque cursor |

Configure the execution values with positive integer environment variables:
`POSTGRESEM_MAX_RESULT_BYTES`, `POSTGRESEM_STATEMENT_TIMEOUT_MS`,
`POSTGRESEM_LOCK_TIMEOUT_MS`, and
`POSTGRESEM_IDLE_IN_TRANSACTION_SESSION_TIMEOUT_MS`. They are read at MCP
startup, so restart the attached MCP process after changes.

There is no concurrent MCP cancellation. Narrow the LSQ or adjust a locally
approved statement timeout; do not treat a larger timeout as a correctness fix.

## RLS and source grants

Source execution opens a `READ ONLY` transaction, verifies role safety and
source ownership, then applies `SET LOCAL ROLE <mapped-role>` and UTC/timeouts.
PostgreSQL GRANT and RLS remain the final authorization boundary.

The fixture maps MCP to `postgresem_analyst`. The tenant roles demonstrate RLS
but are not selected by request data. Changing the mapped role requires trusted
deployment configuration and a new MCP process. Discovery reflects semantic
visibility, not a preflight of every source GRANT, so a visible model can still
fail at query time if the mapped role lacks source access.

## Credential rotation

The init scripts create login roles only when absent. Editing `.env` alone does
not change passwords in an existing retained database volume.

For a local retained volume:

1. Stop attached MCP clients.
2. Rotate each login interactively, avoiding shell history:

   ```sh
   container exec -it postgresem-db \
     psql --no-psqlrc -U postgres -d postgresem_dev
   ```

   At the `psql` prompt use `\password postgresem_runtime` and
   `\password postgresem_audit_writer`.
3. Put the matching new local values in `.env`.
4. Recreate/start the gateway with `make dev-up`.
5. Run the smoke client and inspect the audit.

Administration credential rotation is environment-specific and not automated
by this preview.

## Published revision rotation

Each project has at most one `published` revision. Published semantic rows are
immutable; a replacement must be a new draft and publication transition, not
an in-place edit. MCP reloads the current revision for each operation, and
model-list cursors are bound to a revision.

The preview has `model export` and `model diff`, but no general-purpose
import/publish management command. Revision creation/publication remains a
controlled DBA/deployment SQL workflow. Before changing it:

1. export the current revision;
2. build and hash the candidate snapshot;
3. run `model diff --fail-on-breaking`;
4. publish/retire atomically through reviewed deployment SQL;
5. relist resources and run validate/explain/query smoke checks.

Never mutate the current published rows or canonical hash to force acceptance.

## Upgrade order

Forward migrations `0001` through `0004`, idempotent reruns, N-1 execution, and
N-1-to-current migration are tested.

For a disposable/local preview upgrade:

1. stop MCP clients;
2. record the current version and applied migrations;
3. export every current published project with `postgresem model export`;
4. take an environment-approved database backup if the data matters;
5. update source/image;
6. run migrations before starting the new gateway (`make dev-up` does this);
7. run model diff/validation, the MCP smoke, and audit checks;
8. retain the old environment until acceptance.

The current binary is tested against the latest N-1 schema only. Do not assume
older combinations or downgrade support. There are no down migrations.

## Backup, export, and uninstall boundary

The local reference backup and isolated same-name restore validation are
documented in [backup and restore](backup-restore.md). Production backup,
encryption, retention, replication, RPO/RTO, and cutover remain operator-owned.
`dev-down` retaining a volume is not a backup. `model export` exports only the
current published semantic snapshot; it does not export audit history,
catalog/import state, source data, roles, or grants.

Any manual removal of the `semantic` schema, roles, or volume is destructive
DBA work outside the developer-preview support boundary. Export and back up
first, inspect dependencies, and do not present manual deletion as a reversible
uninstall.

## Common failure modes

| Symptom/code | Check | Action |
|---|---|---|
| doctor reports no runtime | Apple Container and `container-compose` versions | install/start the required local tools |
| `make dev-up` says `.env` is missing | local configuration | copy `.env.example`, generate local-only values |
| database never becomes healthy | `container logs postgresem-db` | check port/storage/startup errors; do not delete the volume as a first response |
| migrate or seed service stops | `container logs postgresem-migrate` / `postgresem-seed` | fix the reported migration/fixture conflict; never edit applied migration history |
| MCP exits before initialize | attached stderr | verify gateway startup-fixed environment and database availability |
| `SEMANTIC_SNAPSHOT_UNAVAILABLE` | published revision and runtime connection | restore availability or publish a valid revision, then retry |
| `MCP_INVALID_CURSOR` | publication may have changed | discard cursor and list models again |
| LSQ/semantic error | validation result | correct the query or select a supported visible object |
| `EXECUTOR_QUERY_CANCELLED` | statement timeout/audit row | narrow or optimize the read-only query before a controlled retry |
| `EXECUTOR_QUERY_FAILED` | minimal audit fields and protected server diagnostics | resolve GRANT/RLS/source/database failure; the public error is deliberately generic |
| `truncated: true` | `byte_count`, requested outputs | narrow projection/filter/limit; never assume partial data is complete |

See [error-reference.md](error-reference.md) for the exact current codes.

## Performance and compatibility checks

Before publishing a semantic revision, run the deterministic compatibility
diff and inspect every breaking classification:

```sh
postgresem model diff \
  --from BEFORE.json --to AFTER.json --fail-on-breaking
```

For the M4 scale smoke, run:

```sh
make test-performance
```

This creates 100 fixture relations, scans the catalog twice, requires an
identical fingerprint, and runs the release compiler benchmark with 100 models,
100 warmups, 1,000 measured iterations, and a 50 ms p95 threshold. On failure,
inspect:

```sh
container logs postgresem-performance-test
```

The catalog check has no latency pass/fail threshold; its enforced conditions
are the 100-relation count and deterministic fingerprint. The compiler
threshold is a local regression ceiling, not an operational SLO. See
[performance.md](performance.md) for methodology and dated reference results.

## Current network boundary

All Rust PostgreSQL connections use `NoTls`. The development database binds
only to `127.0.0.1:55432`, and the gateway connects to the compose-local `db`
service. Do not route these credentials over an untrusted network.

The Apple Container `compose.yaml` uses a static PostgreSQL 18 image because
`container-compose` does not interpolate `${POSTGRES_IMAGE}` variables.
PostgreSQL 16/17 are not selectable through the local `.env` or Apple Container
path. Docker CI applies `compose.ci.yaml` through `COMPOSE_FILE` for its
configured version matrix.

MCP is stdio only. There is no HTTP endpoint, TLS termination, bearer-token
authentication, streamable HTTP, or remote multi-user service in M4.
