# ADR 0015: Authenticated stateless MCP HTTP

- Status: Accepted
- Date: 2026-09-02

## Context

M9 adds a multi-user application and agent integration surface without
weakening the existing stdio, compiler, PostgreSQL authorization, mutation
idempotency, or audit boundaries.

The current MCP specification revision is `2026-07-28`. It replaces the
initialization-based HTTP session model used through `2025-11-25` with a
stateless request model:

- every HTTP request carries protocol version, client identity, and client
  capabilities;
- `server/discover` replaces an initialization handshake;
- protocol-level sessions, the standalone GET stream, and resumable
  `Last-Event-ID` streams are removed;
- request-scoped SSE remains available for progress and cancellation;
- HTTP request metadata headers must match the JSON-RPC body.

The official TypeScript, Python, Go, and C# SDKs support this revision. Adding
the older session-based Streamable HTTP protocol at the same time would add a
second lifecycle, cancellation, replay, and session-hijacking boundary without
improving the PostgreSQL-native value of the gateway.

The explicit M9 request satisfies the demand gate from ADR 0009. The threat
model gate is satisfied only if remote identity is cryptographically verified,
mapped to operator-configured database authority, rate limited, and tested
against the same GRANT/RLS and audit invariants as stdio.

Normative references:

- [MCP Streamable HTTP `2026-07-28`](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP versioning and compatibility](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
- [MCP cancellation](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/cancellation)
- [MCP server discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
- [OAuth protected resource metadata, RFC 9728](https://datatracker.ietf.org/doc/html/rfc9728)
- [OAuth resource indicators, RFC 8707](https://www.rfc-editor.org/rfc/rfc8707.html)
- [JWT access-token profile, RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html)
- [OAuth security best current practice, RFC 9700](https://datatracker.ietf.org/doc/html/rfc9700)

## Decision

### Protocol and transport

1. Add `postgresem mcp serve-http` while preserving
   `postgresem mcp serve` and its MCP `2024-11-05` line-delimited stdio
   contract.
2. The HTTP endpoint implements MCP `2026-07-28` only. It does not implement
   legacy HTTP sessions, GET streams, `Mcp-Session-Id`, `Last-Event-ID`, or
   the deprecated HTTP+SSE transport.
3. The HTTP endpoint is stateless. Every request independently supplies and
   validates:
   - `MCP-Protocol-Version`;
   - `Mcp-Method`;
   - `Mcp-Name` when required;
   - the corresponding `_meta` protocol version, client information, and
     capabilities.
4. Header/body mismatches fail closed. Unsupported versions return the
   protocol-defined error with the supported version list. Unknown modern
   methods use HTTP 404 with JSON-RPC `-32601`.
5. `server/discover`, deterministic tool/resource listing, JSON Schema
   2020-12 tool definitions, and existing revision-bound pagination are
   supported. The server advertises no prompts, sampling, elicitation, roots,
   subscriptions, or experimental extensions.
6. Simple requests may return one JSON object. Query and mutation execution
   tools use a request-scoped SSE response so disconnect can cancel PostgreSQL
   work and close the mandatory audit lifecycle according to the observed
   database outcome. Other bounded metadata/database methods retain JSON
   responses and database-side timeouts.

### Network boundary

1. The Rust listener binds only to an explicit loopback address. Remote
   deployment terminates HTTPS in a colocated reverse proxy and forwards to
   loopback. The gateway does not infer security from forwarded headers.
2. The configured canonical HTTPS resource URI is the only OAuth resource
   identifier and is exposed through RFC 9728 protected resource metadata.
3. The gateway validates `Host` and every present `Origin` against exact
   operator-configured allowlists. Wildcards, suffix matching, reflected
   origins, and caller-selected CORS policy are not supported. The reverse
   proxy must preserve the original public `Host` value.
4. Request bodies, headers, concurrent requests, execution time, result bytes,
   PostgreSQL connections, and SSE lifetime are bounded before deployment can
   enable the listener.
5. Because the loopback gateway intentionally ignores forwarded client
   addresses, the colocated reverse proxy must rate-limit failed
   authentication attempts by network source. The gateway keeps separate
   verified-principal tool budgets and a global concurrency ceiling; invalid
   traffic cannot consume a verified principal's budget.

### Authentication and authority

1. The gateway is an OAuth resource server, not an authorization server or
   OAuth proxy.
2. It accepts only signed JWT access tokens issued for the configured resource.
   Validation requires:
   - a case-insensitive `typ` value in the operator allowlist, with
     `at+jwt` and `application/at+jwt` as the strict defaults;
   - an operator-allowlisted asymmetric algorithm and matching `kid`;
   - a configured local JWKS document;
   - exact issuer and an audience containing the configured canonical
     resource;
   - nonempty subject;
   - valid expiration and, when present, not-before time;
   - issued-at time within the configured maximum token age and clock skew;
   - bounded token and claim sizes.
3. The gateway performs no runtime JWKS or authorization-server network
   fetches. Key rotation is an operator-controlled atomic file replacement
   followed by process restart, avoiding an application-side SSRF and
   availability dependency.
4. A strict operator-owned authority document maps exact `(issuer, subject)`
   identities to:
   - one stable, opaque authority ID that must not be changed while
     idempotency keys can still be retried;
   - one query PostgreSQL role;
   - an optional mutation PostgreSQL role;
   - allowed MCP scopes;
   - rate and concurrency budgets.
   Duplicate subjects, duplicate scopes, an audience different from the
   canonical resource, unsafe role names, unknown keys, invalid JWKS entries,
   and ambiguous mappings reject the entire configuration.
5. Token claims, tool arguments, HTTP headers, and MCP metadata can never name
   or select a PostgreSQL role, project, connection, credential, or execution
   profile.
6. PostgreSQL role membership, superuser/BYPASSRLS rejection, relation-owner
   rejection, GRANT, RLS, constraints, and triggers remain authoritative.
7. Audit principals are derived from the verified issuer and subject and are
   first keyed with an operator secret before entering the existing audit hash
   path. This rotatable audit pseudonym is separate from the stable authority
   ID used for idempotency. Logs do not emit the token, subject, claims,
   requested private names, SQL, connection data, or result rows.
8. Startup preflights every mapped PostgreSQL role for existence,
   runtime-login membership, and superuser/BYPASSRLS status before accepting
   traffic. Applicable relation ownership remains fail-closed during each
   compiled query or mutation because lineage/target relations are
   request-dependent.
9. Missing or invalid bearer credentials return HTTP 401 with an RFC 6750
   `WWW-Authenticate` challenge containing the RFC 9728 protected-resource
   metadata URL. A mapped identity missing a required scope receives the
   standard insufficient-scope challenge; an unmapped identity receives an
   opaque forbidden response.

### OAuth discovery and caching

1. For canonical resource `https://host/path`, the gateway serves RFC 9728
   metadata at `https://host/.well-known/oauth-protected-resource/path` and
   advertises only configured HTTPS authorization servers and supported
   scopes.
2. The metadata document and `WWW-Authenticate` resource metadata URL are
   derived from the same canonical resource and cannot be supplied by the
   request.
3. Identity-dependent `server/discover`, `tools/list`, and `resources/list`
   responses use `Cache-Control: no-store`, `Vary: Authorization`, and private
   MCP cache scope. Their order and content are deterministic for the same
   verified authority, not globally identical across principals.

### Mutation and replay

1. Remote mutation is disabled by default even when stdio mutation is enabled.
2. It is advertised only when all of the following are true:
   - the server-level remote-mutation gate is enabled;
   - the authenticated identity has an operator-mapped mutation role;
   - the token has the configured mutation scope;
   - the existing mutation executor is configured.
3. LSM v1 remains the only mutation input. Migration 0008 namespaces the
   PostgreSQL idempotency primary key by project, authority hash, and key hash.
   The claim function binds replay to role authority, revision, and canonical
   mutation content, while a same-key conflict receives a fresh mutation audit
   identity.
4. Reconciliation requires the same stable authority hash and mapped writer
   role and returns no state for a different authority namespace.
5. HTTP retries, disconnects, and SSE cancellation do not create a second
   idempotency mechanism. Indeterminate commits retain the existing
   retry-with-the-same-key and reconciliation behavior.
6. HTTP exposes `reconcile_semantic_mutation` only to the same mapped authority,
   mutation scope, and writer role as `mutate_semantic_model`. It accepts only
   the project-fixed idempotency key and returns the authority-scoped state.

### Rate limiting and cancellation

1. Requests are limited by verified authority using one operator-configured
   token bucket and concurrent-request ceiling per authority, plus process and
   database concurrency ceilings.
2. HTTP cancellation is signalled by closing the request SSE stream. The
   server captures a PostgreSQL cancellation token before execution, emits
   periodic SSE keep-alives with `X-Accel-Buffering: no`, propagates disconnect
   cancellation through a separate connection, and emits no later response on
   the closed stream.
3. Query audit records `cancelled` after PostgreSQL confirms cancellation or
   when disconnect is observed before source execution starts. Mutation
   cancellation before execution or commit records a terminal non-committed
   outcome; a disconnect or connection loss during commit remains
   indeterminate and must be reconciled rather than being labelled cancelled.
4. The statement timeout remains a hard upper bound independent of progress or
   client connectivity.

### Request metadata and cursors

1. The server decodes the protocol-defined `=?base64?...?=` representation
   before comparing `Mcp-Name` with the request body.
2. No postgresem tool schema publishes `x-mcp-header`; unknown
   `Mcp-Param-*` headers never become tool arguments or authority inputs.
3. Model cursors bind the revision and a hash of the immutable authority ID.
   Reuse by another principal fails with the same opaque invalid-cursor error.
4. Missing or invalid HTTP content negotiation fails with HTTP 400 and
   `MCP_INVALID_HTTP_HEADERS`. Protocol metadata or required mirrored-header
   mismatches use `-32020`; a missing required client-capability object uses
   `-32021`; unsupported protocol versions use `-32022`. The modern core
   defines no client-to-server notifications for this surface.

### Client contracts

The repository publishes:

- the supported MCP protocol profile;
- the RFC 9728 metadata shape;
- strict JSON Schemas for the authority document and public tool inputs;
- TypeScript and Python integration guidance using official SDKs;
- examples for discovery, OAuth resource selection, pagination, cancellation,
  and safe mutation retry.

These are client guidance and deployment contracts. They do not introduce a
second live semantic source of truth.

## Rejected alternatives

- **Unauthenticated HTTP or trusted caller headers:** identity would be
  caller-asserted and could select PostgreSQL authority indirectly.
- **Binding directly to a public interface without TLS:** bearer credentials
  and data would cross an unprotected transport.
- **Runtime OIDC/JWKS discovery in the gateway:** this adds SSRF, redirect,
  DNS-rebinding, and remote availability dependencies to the query path.
- **Legacy HTTP session support in 0.7:** it adds state and replay boundaries
  that the current protocol removed. Stdio remains the compatibility path for
  legacy clients.
- **A shared remote database role:** it would make authenticated identity
  irrelevant to PostgreSQL RLS and audit authority.
- **Token roles or scopes as database role names:** signed claims are still not
  database authority configuration.
- **Remote mutation enabled whenever query HTTP is enabled:** reads and writes
  have different risk, audit, replay, and operator-consent requirements.

## Consequences

- postgresem gains a current, authenticated, multi-user MCP application
  surface while retaining stdio.
- Remote clients remain constrained to semantic names and typed LSQ/LSM; no
  raw SQL or physical identifiers are introduced.
- Deployment requires an OAuth authorization server and a colocated HTTPS
  reverse proxy, but neither becomes the semantic or PostgreSQL authorization
  source of truth.
- Legacy Streamable HTTP clients must upgrade or use the stdio adapter.
- M9 is not complete until independent identities demonstrate distinct
  PostgreSQL RLS results and mutation authority through the remote transport.
