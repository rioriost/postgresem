# PostgreSQL Semantic Gateway

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts versioned Logical Semantic Queries (LSQ), validates
them against a published semantic model, and compiles them into deterministic,
parameterized PostgreSQL queries.

The project now has an executable M3 MVP. It provides the LSQ v1 contract,
typed validation, deterministic compilation, catalog scanning, DB-backed
published semantic snapshots, guarded execution with mandatory audit records,
fixed role mapping, RLS preservation, read-only transactions, bounded JSON
results, and a semantic-only MCP stdio adapter with tools and resources.

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
make test-mcp
```

`make dev-up` waits for PostgreSQL, applies migrations, runs the one-shot
semantic seed, and starts the long-lived gateway container. Repeating it does
not create another revision.

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

## MCP stdio server

`postgresem mcp serve` implements the MCP JSON-RPC 2.0 stdio transport using
one JSON object per line. Standard output is reserved for protocol messages;
diagnostics are written to standard error. The MVP supports `initialize`,
`notifications/initialized`, `ping`, `tools/list`, `tools/call`,
`resources/list`, and `resources/read`.

The server exposes exactly these tools:

- `list_semantic_models`
- `describe_semantic_model`
- `validate_semantic_query`
- `query_semantic_model`
- `explain_semantic_query`

There is no raw query or compiler tool. Validation and explanation return only
semantic outputs and public lineage, never generated physical queries. Model
and resource responses omit nonqueryable models and hidden fields or metrics.

All MCP configuration is read once from environment variables at startup:

```sh
export POSTGRESEM_MCP_PROJECT=commerce
export MCP_RUNTIME_DATABASE_URL='host=127.0.0.1 port=55432 dbname=postgresem_dev user=postgresem_runtime'
export POSTGRESEM_RUNTIME_PASSWORD='<runtime-password>'
export MCP_AUDIT_DATABASE_URL='host=127.0.0.1 port=55432 dbname=postgresem_dev user=postgresem_audit_writer'
export POSTGRESEM_AUDIT_WRITER_PASSWORD='<audit-password>'
export POSTGRESEM_MCP_RUNTIME_URL_ENV=MCP_RUNTIME_DATABASE_URL
export POSTGRESEM_MCP_RUNTIME_PASSWORD_ENV=POSTGRESEM_RUNTIME_PASSWORD
export POSTGRESEM_MCP_AUDIT_URL_ENV=MCP_AUDIT_DATABASE_URL
export POSTGRESEM_MCP_AUDIT_PASSWORD_ENV=POSTGRESEM_AUDIT_WRITER_PASSWORD
export POSTGRESEM_MCP_DB_ROLE_ENV=POSTGRESEM_DB_ROLE

postgresem mcp serve
```

The five `*_ENV` values name the environment variables containing the runtime
conninfo, runtime password, audit conninfo, audit password, and mapped database
role. Environment-variable names are validated strictly. The two conninfo
values can remain passwordless: the server parses each with `postgres::Config`
and applies the corresponding password in memory before connecting. Requests
cannot override the project, connection, password, role, principal, or
execution profile. MCP executions use the mandatory guarded executor and are
audited with profile `mcp-stdio` and a fixed MCP stdio principal subject.

Blank stdio lines are ignored. MCP protocol parameter envelopes accept standard
extensions such as `_meta`, while each tool's actual argument object remains
strict and rejects undeclared properties. `initialize` requires a string
`protocolVersion`, an object `capabilities`, and a `clientInfo` object with
nonempty string `name` and `version` values. Request IDs must be non-null
strings or integer JSON numbers; invalid IDs receive `MCP_INVALID_REQUEST` with
a null response ID. Structured JSON request completion and error logs are
written only to standard error and omit LSQ values, database configuration,
physical queries, result rows, and principal data.

Published resources use these URI forms:

```text
semantic://projects/{project}/revisions/current
semantic://projects/{project}/models/{model}
semantic://schemas/lsq/v1
```

The LSQ resource is the same bundled `schemas/lsq/v1.schema.json` used by the
project. `make test-mcp` builds the integration image, starts the server as a
stdio child, exercises all tools and resources, and checks the audit row.

Start the seeded database and long-lived gateway, then attach the MCP server's
stdin and stdout through Apple Container:

```sh
make dev-up
make mcp
```

`make mcp` uses `container exec -i` to run the server as the image's
unprivileged `postgresem` user. The exec process inherits the gateway
container's startup-fixed MCP configuration, `host=db` database connection
settings, and password variables. Passwords are not interpolated into shell
commands, conninfo strings, or process arguments. MCP is intentionally not
exposed over HTTP in this milestone.

The MVP does not implement concurrent protocol cancellation. The guarded
executor's configured PostgreSQL statement timeout is the current cancellation
boundary.

See
[`docs/adr/0006-guarded-database-execution.md`](docs/adr/0006-guarded-database-execution.md)
for the execution trust boundary and audit decisions.

See
[`docs/adr/0007-mcp-stdio-mvp-adapter.md`](docs/adr/0007-mcp-stdio-mvp-adapter.md)
for the MCP transport, visibility, configuration, and public-error decisions.

See
[`docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md`](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)
for the architecture, scope, and milestone gates.
