# Contributing

The project is in developer preview. Changes must preserve the narrow
semantic-only, read-only security boundary and must not imply production
readiness.

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

Documentation and examples must use fictional data and local-only credentials.
Do not commit `.env`, database dumps, tokens, customer data, or generated
artifacts containing secrets. Do not claim TLS, HTTP, cancellation,
backup/restore, release signing, published artifacts, external feedback, or
production readiness without corresponding implementation and reproducible
evidence. Distinguish a configured workflow from a successful CI/release run.

Applied migration files are append-only. Add a new migration rather than
editing migration history, and document upgrade/export effects.

See [docs/compatibility.md](docs/compatibility.md),
[docs/developer-preview-checklist.md](docs/developer-preview-checklist.md), and
[SECURITY.md](SECURITY.md).
