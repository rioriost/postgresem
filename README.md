# PostgreSQL Semantic Gateway

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts versioned Logical Semantic Queries (LSQ), validates
them against a published semantic model, and compiles them into deterministic,
parameterized PostgreSQL queries.

The project is in the M0 foundation phase. The current implementation provides
the Rust workspace, LSQ v1 contract, typed parsing, structural validation,
canonical query hashing, deterministic catalog scanning, and DB-backed
published semantic snapshot export.

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
# Replace both placeholder passwords in .env.
make dev-up
make test-db
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

See
[`docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md`](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)
for the architecture, scope, and milestone gates.
