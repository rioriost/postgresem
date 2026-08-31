# PostgreSQL Semantic Gateway

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts versioned Logical Semantic Queries (LSQ), validates
them against a published semantic model, and compiles them into deterministic,
parameterized PostgreSQL queries.

The project is in the M0 foundation phase. The current implementation provides
the Rust workspace, LSQ v1 contract, typed parsing, structural validation,
canonical query hashing, deterministic catalog scanning, and DB-backed
published semantic snapshot export. It also includes the guarded database
execution MVP with mandatory audit lifecycle records, fixed role mapping, RLS
preservation, read-only transactions, and bounded JSON results.

## Requirements

- Rust 1.85 or later
- Apple Container 1.0.0 or a compatible OCI runtime
- `container-compose` 1.1.0

## Development

```sh
make doctor
make test
make check
```

Start PostgreSQL 18, apply forward-only migrations, and publish the idempotent
development semantic fixture:

```sh
cp .env.example .env
# Replace all placeholder password and connection URL values in .env.
make dev-up
make test-db
make test-execution
```

`make dev-up` waits for PostgreSQL, applies migrations, then runs the one-shot
semantic seed. Repeating it does not create another revision. The long-lived
Gateway service will be added after the execution and MCP transport boundaries
are implemented.

Validate an LSQ document:

```sh
cargo run -p postgresem -- query validate path/to/query.json
```

Compile an LSQ document against an immutable semantic snapshot:

```sh
cargo run -p postgresem -- query compile path/to/query.json \
  --snapshot fixtures/evals/m0-semantic-snapshot.json
```

Calculate the canonical hash after editing a semantic snapshot:

```sh
cargo run -p postgresem -- snapshot hash \
  fixtures/evals/m0-semantic-snapshot.json
```

Scan the visible, non-system PostgreSQL catalog into a deterministic JSON
snapshot:

```sh
export DATABASE_URL='postgresql://postgresem_introspector:password@localhost/app'
cargo run -p postgresem -- catalog scan > catalog-snapshot.json
```

The connection URL is read only from an environment variable so it is not
placed in process arguments. Name a different variable when needed:

```sh
export POSTGRESEM_SCAN_URL='postgresql://postgresem_introspector:password@localhost/app'
cargo run -p postgresem -- catalog scan \
  --database-url-env POSTGRESEM_SCAN_URL
```

Use a dedicated least-privilege introspection role. The scan runs in a
`READ ONLY`, `REPEATABLE READ` transaction. Catalog comments are included and
may be sensitive; CHECK and RLS expressions are never persisted, only their
SHA-256 hashes. The MVP client currently supports local or otherwise
non-TLS connections only.

Export a named project's current published semantic revision:

```sh
export DATABASE_URL='******localhost/postgresem_dev'
cargo run -p postgresem -- model export --project commerce
```

As with catalog scan, the URL is accepted only through an environment variable.
Use `--database-url-env POSTGRESEM_MODEL_URL` to name a different variable.
Export runs in a `READ ONLY`, `REPEATABLE READ` transaction, parses the
normalized schema fail-closed, canonicalizes collection order, and rejects a
snapshot whose calculated hash differs from the published revision hash.

Execute an LSQ against a project's published revision:

```sh
export DATABASE_URL='postgresql://postgresem_runtime:<runtime-password>@127.0.0.1:55432/postgresem_dev'
export POSTGRESEM_AUDIT_DATABASE_URL='postgresql://postgresem_audit_writer:<audit-password>@127.0.0.1:55432/postgresem_dev'
export POSTGRESEM_DB_ROLE='postgresem_analyst'

cargo run -p postgresem -- query execute path/to/query.json \
  --project commerce
```

The URLs and mapped role cannot be passed as CLI values. To use differently
named environment variables, name only those variables:

```sh
cargo run -p postgresem -- query execute path/to/query.json \
  --project commerce \
  --database-url-env MY_RUNTIME_URL \
  --audit-database-url-env MY_AUDIT_URL \
  --db-role-env MY_MAPPED_ROLE
```

The runtime login is granted membership in local `NOLOGIN` roles.
`postgresem_analyst` can select the commerce and billing fixtures, while
`postgresem_tenant_a` and `postgresem_tenant_b` exercise source RLS. The
executor rejects missing memberships, superuser or `BYPASSRLS` roles, and a
role that owns any physical source relation in the compiled lineage.

Execution writes a mandatory `started` audit row through the dedicated audit
connection before opening the source transaction. It then runs only the
compiled single `SELECT` in a `READ ONLY` transaction with transaction-local
role and timeout settings, followed by a terminal audit update. The response
contains `schema_version`, `query_id`, `semantic_revision`, `columns`, `rows`,
`truncated`, `lineage`, and `warnings`. Numeric JSON values are strings;
integers and booleans retain their JSON types.

Optional nonsecret execution limits are configured with:

```sh
export POSTGRESEM_MAX_RESULT_BYTES=1048576
export POSTGRESEM_STATEMENT_TIMEOUT_MS=30000
export POSTGRESEM_LOCK_TIMEOUT_MS=5000
export POSTGRESEM_IDLE_IN_TRANSACTION_SESSION_TIMEOUT_MS=5000
```

See
[`docs/adr/0006-guarded-database-execution.md`](docs/adr/0006-guarded-database-execution.md)
for the execution trust boundary and audit decisions.

See
[`docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md`](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)
for the architecture, scope, and milestone gates.
