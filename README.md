# PostgreSQL Semantic Gateway

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts versioned Logical Semantic Queries (LSQ), validates
them against a published semantic model, and compiles them into deterministic,
parameterized PostgreSQL queries.

The project is in the M0 foundation phase. The current implementation provides
the Rust workspace, LSQ v1 contract, typed parsing, structural validation, and
canonical query hashing.

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

Validate an LSQ document:

```sh
cargo run -p postgresem -- query validate path/to/query.json
```

See
[`docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md`](docs/POSTGRESQL_SEMANTIC_GATEWAY_IMPLEMENTATION_PLAN.md)
for the architecture, scope, and milestone gates.

