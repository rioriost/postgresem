# Contributing

The project is in beta. Changes to the current query path must preserve its
narrow semantic-only, read-only security boundary and must not imply
production readiness.

Discuss changes that alter LSQ, Semantic Schema, compiler semantics,
authorization/RLS, MCP contracts, error codes, migrations, export/uninstall
boundaries, or compatibility in an ADR before implementation.

Before submitting a change:

```sh
make fmt
make check
make test
```

Use the smallest relevant integration targets for database, execution, MCP, or
performance changes. The full current preview gate is:

```sh
make preview-check
```

Compiler behavior changes require accepted-input and safe-rejection tests.
Public contracts must not accept raw SQL. MCP changes must keep stdout
protocol-only, preserve privacy-safe stderr, and document compatibility/error
effects.

M6 mutation work must use a separately versioned typed contract, compiler
entry point, writer credential/role, executor, audit lifecycle, and capability.
Do not make the current query executor conditionally writable. Add rejection
tests for physical identifiers, arbitrary DML, request-selected authority,
unsafe conflict policies, RLS `WITH CHECK`, constraints, idempotency, partial
failure, and rollback/reconciliation behavior.

Documentation and examples must use fictional data and local-only credentials.
Do not commit `.env`, database dumps, tokens, customer data, or generated
artifacts containing secrets. Do not claim TLS, HTTP, cancellation,
backup/restore, release signing, published artifacts, external feedback, or
production readiness without corresponding implementation and reproducible
evidence. Distinguish a configured workflow from a successful CI/release run.
Changes affecting packaging or native dependencies must consider and document
executed Linux amd64 and arm64 behavior; cross-compilation alone is not the M6
support gate.

Applied migration files are append-only. Add a new migration rather than
editing migration history, and document upgrade/export effects.

See [docs/compatibility.md](docs/compatibility.md),
[docs/developer-preview-checklist.md](docs/developer-preview-checklist.md), and
[SECURITY.md](SECURITY.md).
