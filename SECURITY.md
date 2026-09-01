# Security Policy

## Supported versions and scope

No production-ready version has been released. `0.2.0-alpha.1` is the published
developer preview and `0.3.0-beta.1` is the active beta development line for
local evaluation and read-only pilots. There is no long-term-support promise
for 0.x prereleases.

The implemented boundary is MCP stdio plus local/non-TLS PostgreSQL
connections. There is no HTTP service, remote authentication protocol, TLS,
published signed release, production backup/restore, or production hardening
claim. The loopback Web demo is not a remotely supported transport.
The `v0.2.0-alpha.1` release includes checksums and a container-image
SBOM/provenance record. Checksums are integrity checks and are not signatures;
future beta releases are configured for keyless GitHub OIDC signing.

The Apple Container gateway service is configured with container user root
solely so `container-compose` can perform its `/etc/hosts` fallback. Its
long-lived command immediately uses `gosu` to replace itself with
`sleep infinity` as `postgresem`, and `make mcp` uses
`container exec --user postgresem`. Therefore the idle and MCP application
processes are unprivileged, but the Compose container configuration itself must
not be represented as nonroot. Manual exec commands must preserve the explicit
unprivileged user.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Use GitHub private
vulnerability reporting for this repository. Include:

- affected version or commit;
- minimal reproduction steps using fictional data;
- expected security impact;
- whether the issue crosses role/RLS, semantic visibility, audit, MCP privacy,
  or credential boundaries.

Never include production credentials, tokens, customer data, private semantic
models, raw query results, generated SQL from a private system, or database
dumps. Redact connection strings and rotate any credential accidentally
exposed during testing.

## Security invariants

Reports are especially valuable when they show a violation of an implemented
invariant:

- no raw SQL MCP input/output surface;
- request data cannot override project, connection, password, role, principal,
  or execution profile;
- hidden and unknown semantic objects are not distinguishable publicly;
- source execution is `READ ONLY` and uses a safe mapped non-owner,
  non-superuser, non-`BYPASSRLS` role;
- PostgreSQL GRANT/RLS remains enforced;
- a durable audit start precedes source execution;
- protocol stdout contains only JSON-RPC messages;
- MCP errors/logs omit credentials, SQL, rows, literals, principals, and
  private requested names.

## Safe evaluation

Use only disposable or explicitly approved non-production databases and
least-privilege credentials. PostgreSQL connections require an explicit
`sslmode`; use `sslmode=require` over untrusted networks and reserve
`sslmode=disable` for local or independently protected connections. Do not
expose `make mcp` through an ad-hoc network wrapper.

Audit metadata and semantic exports can still reveal structure and usage
patterns. Restrict them even though they omit raw result rows and credentials.
See [docs/operations.md](docs/operations.md) and
[docs/error-reference.md](docs/error-reference.md).
