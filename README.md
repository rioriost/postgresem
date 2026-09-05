# PostgreSQL Semantic Gateway

[日本語](README-jp.md)

`postgresem` is a PostgreSQL-native semantic gateway for AI agents and
applications. It accepts typed Logical Semantic Queries (LSQ) and Logical
Semantic Mutations (LSM), resolves them against immutable published semantic
definitions, and executes parameterized operations under PostgreSQL
authorization.

This guide describes **postgresem 1.0.0**. PostgreSQL is the only execution
engine and the authoritative store for both data and its governed meaning.

## What problem does postgresem solve?

PostgreSQL knows the structure of your data: tables, columns, types, keys,
constraints, comments, privileges, and row-level security policies. Structure
alone does not explain which timestamp defines revenue recognition, which
metric definition is approved, or which joins preserve the intended unit of
aggregation.

Those meanings often end up duplicated across application code, BI tools,
prompts, documentation, and YAML files. The copies drift independently. An
agent working only from physical schema metadata can produce valid SQL that
double-counts orders, uses the wrong business definition, or exposes more data
than the intended application interface.

`postgresem` stores reviewed semantic models, fields, relationships, metrics,
and policy bindings in PostgreSQL alongside the data. Database metadata such
as `pg_catalog`, comments, keys, and constraints provides evidence for modeling;
it does not replace human review of business meaning. Definitions are
published as immutable, hash-verified revisions.

Applications use approved semantic names instead of supplying SQL. The
compiler either produces a deterministic, bounded, parameterized operation or
rejects unsupported or ambiguous input.

| Benefit | What changes |
|---|---|
| One semantic authority | Applications and agents share published PostgreSQL definitions instead of maintaining separate live copies of business meaning. |
| Database-enforced access | PostgreSQL GRANT and RLS remain authoritative; semantic visibility cannot grant access the database denies. |
| Coordinated operations | Schema changes, semantic publication, drift detection, backup, and restore stay within the PostgreSQL operational boundary. |
| Traceability | Execution is tied to the semantic revision, compiler version, policy context, and audit record. |
| Less infrastructure | The core requires no external catalog service, vector database, policy engine, or result cache. |

## Usage Scenario

| Scenario | How postgresem is used |
|---|---|
| AI assistant for business data | An MCP client discovers approved models and asks for revenue, subscriptions, or other modeled metrics without generating SQL. |
| Application reporting | A dashboard submits LSQ using shared metric definitions, including approved joins and explicit aggregation rules. |
| Governed data ingestion | An application submits typed LSM inserts or approved upserts through a separate writer role, with idempotent replay and reconciliation. |
| Metadata and change management | Operators scan the PostgreSQL catalog, scaffold model candidates, and compare semantic or authorization changes before publication. |

For multi-tenant applications, authenticated HTTP identities map to
operator-configured PostgreSQL roles, and RLS determines accessible rows.
The [commerce example](examples/commerce/README.md) and
[local Web demo](examples/web_demo/README.md) show application integration.

The scope is intentionally narrow: no arbitrary SQL or general update/delete,
no non-PostgreSQL execution, and no automatic pre-aggregation or
materialized-view routing. Unsupported multi-fact, many-to-many, and multi-hop
aggregation is rejected rather than guessed. See the
[compatibility policy](docs/compatibility.md) for supported semantics and
[authoring and operations guidance](docs/operations.md) for using your own
database.

## Installation

Supported targets are PostgreSQL **16, 17, and 18**, and native binaries for
**Linux and macOS on amd64 and arm64**. Containers run on Linux amd64/arm64;
the Apple silicon macOS path uses Apple Container.

**Get the deployment files and examples**

```sh
git clone --branch v1.0.0 --depth 1 https://github.com/rioriost/postgresem.git
cd postgresem
```

Run the following repository commands from this directory.

**Native CLI**

Install [Cosign](https://docs.sigstore.dev/cosign/system_config/installation/),
then use the installer with `curl`, `tar`, and either `shasum` or `sha256sum`
available:

```sh
scripts/install.sh 1.0.0
export PATH="$HOME/.local/bin:$PATH"
postgresem --version
postgresem contract show
```

The installer selects the host platform, verifies the release signature and
archive checksum, and installs the binary into `~/.local/bin` without sudo.
It does not provision PostgreSQL, apply migrations, or configure credentials.
For an existing database, follow the [operations guide](docs/operations.md).

**Local container stack**

The sample stack builds the gateway from the checkout, starts PostgreSQL 18,
applies migrations, and publishes fictional commerce models. It is a local
demonstration, not a production deployment template. The native CLI is not
required for this path.

Use Git, Make, and one of the container runtimes below. The examples also
require Python 3.9 or later.

```sh
cp .env.example .env
chmod 600 .env
```

Before starting, edit `.env`: replace every password placeholder with separate,
random local-only credentials and update the corresponding connection URLs.
Do not use production credentials or commit `.env`.

| Environment | Start | Stop without deleting database data |
|---|---|---|
| Linux with Docker Engine and Compose v2 | `make docker-up` | `make docker-down` |
| Apple silicon macOS with Apple Container 1.0.0 and `container-compose` 1.1.0 | `make dev-up` | `make dev-down` |
| Linux with rootless Podman 4.9+ and systemd | Follow the [Quadlet instructions](docs/linux-containers.md#rootless-podman-quadlet) | Stop the installed user services |

See [Linux container setup](docs/linux-containers.md) or the
[Apple Container quickstart](docs/quickstart.md) for detailed configuration.

## Quick Usage

Start the local stack as described above. The commands below use Docker
Compose; on Apple Container, replace `make docker-mcp` with `make mcp`.

**Query and insert sample data through MCP**

```sh
python3 examples/commerce/mcp_smoke.py \
  --lsq examples/commerce/revenue-by-month.json \
  --lsm examples/commerce/order-insert.json \
  -- make docker-mcp
```

The client initializes MCP, discovers models, validates and executes the
query, and inserts a fictional order through the governed mutation path.
**This command writes to the sample database.** Repeating it with the same
LSM idempotency key replays the committed outcome instead of inserting another
order.

An LSQ for total order revenue looks like this:

```json
{
  "schema_version": "1",
  "model": "orders",
  "metrics": [{"metric": "revenue"}],
  "limit": 10
}
```

An MCP client sends this object as `lsq` to `query_semantic_model`, alongside
tool argument `schema_version: "1"`. Queries return column metadata, rows,
revision and audit identifiers, and truncation status. PostgreSQL `numeric` values are
represented as JSON strings to preserve precision.

**Try the Web demo**

```sh
python3 examples/web_demo/server.py -- make docker-mcp
```

Open <http://127.0.0.1:8765>. The browser uses predefined semantic queries
through MCP; it does not connect directly to PostgreSQL. Stop the demo with
`Ctrl-C`, then stop the stack using the command in the installation table.

**Connect an agent or application**

For local MCP integration, configure the client to launch `make docker-mcp`
from the checkout directory, or `make mcp` on Apple Container. These commands
serve line-delimited JSON-RPC over stdio, not an interactive shell.

| Operation | MCP tools |
|---|---|
| Discovery | `list_semantic_models`, `describe_semantic_model` |
| Query | `validate_semantic_query`, `explain_semantic_query`, `query_semantic_model` |
| Governed writes, when enabled | `validate_semantic_mutation`, `mutate_semantic_model`, `reconcile_semantic_mutation` |

For remote clients, use `postgresem mcp serve-http` with the
[authenticated HTTP deployment guide](docs/mcp-http.md). Stdio supports MCP
`2024-11-05`; authenticated stateless Streamable HTTP supports `2026-07-28`.
Use the [error reference](docs/error-reference.md) to handle rejected requests.

## Security boundary

**PostgreSQL remains the final authority.** Query execution uses a read-only
transaction, a validated non-owner, non-superuser, non-`BYPASSRLS` role, and
transaction-local timeouts. A durable audit start is required before source
execution, and result row and byte limits bound responses.

Writes use separate credentials, roles, compiler, executor, idempotency state,
and audit lifecycle. Only published insert/upsert projections are writable.
Business changes, committed replay state, and committed audit finalization
share one transaction. PostgreSQL column GRANT, RLS `USING`/`WITH CHECK`,
constraints, and triggers remain enforced.

Query and mutation requests cannot supply SQL, physical identifiers, connection
credentials, or database roles. Stdio authority is fixed at startup; HTTP authority is selected
only from configured mappings using verified identity. MCP responses do not
expose generated SQL or physical lineage. Diagnostics exclude request values,
credentials, result rows, private names, and principal data; hidden and unknown
semantic objects have the same public error.

PostgreSQL connections require an explicit `sslmode`. Use `sslmode=require`
for remote connections; certificate and hostname validation use the platform
trust store. Reserve `sslmode=disable` for local or independently protected
connections.

The HTTP listener binds only to loopback and requires a colocated HTTPS reverse
proxy. It verifies JWTs using local authority/JWKS configuration; it does not
issue tokens, trust forwarded identity headers, or discover keys dynamically.
Remote writes require explicit operator enablement, verified scope, and a
mapped writer role. Configuration changes require a process restart.

Linux Compose and Quadlet run the gateway as UID/GID `10001`. Apple Container
requires root in the Compose configuration for its hosts-file workaround, but
startup drops privileges and MCP is explicitly executed as `postgresem`.

Supply-chain monitoring is continuous: dependency checks, pinned workflow
actions, and signature verification complement application security. Production
backup retention, HA, recovery objectives, identity-provider operation, and
proxy configuration remain operator responsibilities. See
[SECURITY.md](SECURITY.md), [SUPPORT.md](SUPPORT.md), and
[backup and restore](docs/backup-restore.md).

## Packaging status

The 1.0.0 release layout is:

| Artifact | Format or target |
|---|---|
| Native archives | `postgresem-1.0.0-{linux,darwin}-{amd64,arm64}.tar.gz` |
| OCI image | `ghcr.io/rioriost/postgresem:1.0.0`, for `linux/amd64` and `linux/arm64` |
| Archive integrity | `SHA256SUMS`, `SHA256SUMS.sig`, and `SHA256SUMS.pem` |
| Image metadata | SBOM and build provenance |
| Deployment sources | `Dockerfile`, `Containerfile`, Compose files, and rootless Podman Quadlet units |

Native archives include the binary, schemas, contract manifests, and selected
policy documentation. Deployment files, migrations, and examples are supplied
in the source checkout. The installer installs only the binary.

Release automation executes Linux binaries and images on both architectures
before publication. Checksums and the immutable image digest are
Sigstore-keyless-signed using GitHub OIDC. Verification must constrain both the
expected release-workflow/tag identity and the issuer; a checksum alone does
not authenticate an artifact. Pin image digests for reproducible deployment.
Locally built images are not signed release artifacts.

Use [GitHub Releases](https://github.com/rioriost/postgresem/releases) for
downloads and the [compatibility policy](docs/compatibility.md) and
[deprecation policy](docs/deprecation-policy.md) for version guarantees.

## License

`postgresem` is licensed under the [MIT License](LICENSE).
Contribution guidance is in [CONTRIBUTING.md](CONTRIBUTING.md).
