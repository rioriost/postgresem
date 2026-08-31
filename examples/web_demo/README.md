# Local commerce Web demo

This sample Web application demonstrates Postgresem through its existing MCP
stdio contract. It does not add MCP Streamable HTTP and does not connect to
PostgreSQL from the browser.

The server:

- binds only to `127.0.0.1` and rejects any other HTTP `Host`;
- starts the configured MCP command without a shell;
- offers four checked-in LSQ examples;
- calls `validate_semantic_query`, `explain_semantic_query`, and
  `query_semantic_model`;
- returns the guarded result, public semantic lineage, revision, and audit
  query ID;
- never accepts SQL, credentials, principal, database role, connection URL, or
  arbitrary command arguments from the browser.

## Run

Complete the [commerce quickstart](../../docs/quickstart.md) through
`make dev-up`, then start the demo:

```sh
make web-demo
```

Open <http://127.0.0.1:8765>. Stop the server with `Ctrl-C`, then stop the
containers with:

```sh
make dev-down
```

The host requires Python 3.9 or later. The server uses only the Python standard
library and the shared commerce MCP client.

## What this proves

Each displayed result has passed the same LSQ validation, deterministic
compiler, guarded read-only PostgreSQL execution, role/RLS checks, result
limits, and mandatory audit lifecycle used by other MCP clients. The returned
query ID can be inspected using the safe audit procedure in
[operations.md](../../docs/operations.md).

This is a local demonstration surface, not a remotely deployable service.
Binding to non-loopback addresses is intentionally unavailable. See
[ADR 0009](../../docs/adr/0009-beta-operations-transport-and-evidence.md) for
the authenticated HTTP prerequisites.

## Test

```sh
make test-web-demo
```
