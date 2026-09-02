# Authenticated MCP HTTP

postgresem 0.7 adds a stateless Streamable HTTP adapter for MCP
`2026-07-28`. The existing `postgresem mcp serve` command remains the
MCP `2024-11-05` stdio compatibility adapter.

The HTTP adapter is an OAuth resource server. It does not issue tokens, fetch
runtime discovery or JWKS documents, terminate TLS, accept caller-selected
PostgreSQL roles, or expose raw SQL.

## Deployment boundary

Run the listener only on loopback:

```sh
export POSTGRESEM_MCP_HTTP_AUTHORITY_FILE=/run/secrets/postgresem-authority.json
export POSTGRESEM_MCP_HTTP_BIND=127.0.0.1:8080
postgresem mcp serve-http
```

Place a colocated HTTPS reverse proxy in front of it. The proxy must:

- preserve the original public `Host`;
- reject oversized headers before forwarding;
- propagate client disconnects to the upstream request;
- disable buffering for `text/event-stream`;
- rate-limit failed authentication by client network source;
- never make forwarded identity or role headers authoritative.

Do not bind the Rust listener to `0.0.0.0` or publish its loopback port
directly from a container. A proxy in another ordinary bridge-network
container cannot reach this loopback listener. Use a same-pod sidecar, share
the gateway network namespace (for example Docker
`network_mode: service:<gateway>`), or supervise both processes in one
container.

The authority document follows
[`schemas/mcp-http/v1.authority.schema.json`](../schemas/mcp-http/v1.authority.schema.json).
Start from
[`fixtures/mcp-http/authority.example.json`](../fixtures/mcp-http/authority.example.json),
replace every example URI/role/subject, and store the document, JWKS, and
principal HMAC key as read-only secrets. `audience` must equal `resource`.
Each `authority_id` is an immutable operator identifier used to namespace
mutation retry and reconciliation; changing it while keys remain retryable can
make prior state unreachable.

The gateway validates all configured roles at startup. PostgreSQL membership,
GRANT, RLS, constraints, triggers, relation ownership, superuser status, and
`BYPASSRLS` remain the final authorization boundary.

## OAuth discovery

For resource `https://mcp.example.test/mcp`, clients discover protected
resource metadata at:

```text
https://mcp.example.test/.well-known/oauth-protected-resource/mcp
```

An unauthenticated MCP request returns HTTP 401 with a
`WWW-Authenticate: Bearer` challenge containing that metadata URL and the
query scope. A valid identity without the operation's scope receives an
`insufficient_scope` challenge. Discovery and capability responses are
identity-dependent and use `Cache-Control: no-store`,
`Vary: Authorization`, and MCP `cacheScope: private`.

## TypeScript SDK v2

Use the official `@modelcontextprotocol/client` v2 package. If another
component already obtains the access token:

```ts
import {
  Client,
  StreamableHTTPClientTransport
} from "@modelcontextprotocol/client";

const client = new Client(
  { name: "reporting-agent", version: "1.0.0" },
  { versionNegotiation: { mode: "auto" } }
);
const transport = new StreamableHTTPClientTransport(
  new URL("https://mcp.example.test/mcp"),
  { authProvider: { token: async () => process.env.POSTGRESEM_ACCESS_TOKEN! } }
);

await client.connect(transport);
const tools = await client.listTools();
const result = await client.callTool({
  name: "query_semantic_model",
  arguments: {
    schema_version: "1",
    lsq: {
      schema_version: "1",
      model: "orders",
      metrics: [{ metric: "revenue" }]
    }
  }
});
await client.close();
```

For machine-to-machine OAuth discovery and client credentials, use the SDK's
`ClientCredentialsProvider` instead of handling the token endpoint in
application code. Pin the expected authorization-server issuer.

## Python SDK v2

Use the official `mcp` v2 package. Its client-credentials provider consumes the
RFC 9728 metadata and attaches tokens through `httpx2`:

```python
import asyncio
import os

import httpx2
from mcp import Client
from mcp.client.auth.extensions.client_credentials import (
    ClientCredentialsOAuthProvider,
)
from mcp.client.streamable_http import streamable_http_client

# Use a persistent TokenStorage implementation in deployed applications.
oauth = ClientCredentialsOAuthProvider(
    server_url="https://mcp.example.test/mcp",
    storage=token_storage,
    client_id=os.environ["MCP_CLIENT_ID"],
    client_secret=os.environ["MCP_CLIENT_SECRET"],
    scope="postgresem.query",
)


async def main(token_storage) -> None:
    async with httpx2.AsyncClient(auth=oauth, follow_redirects=True) as http:
        transport = streamable_http_client(
            "https://mcp.example.test/mcp",
            http_client=http,
        )
        async with Client(transport) as client:
            tools = await client.list_tools()
            print([tool.name for tool in tools.tools])

```

The fragment deliberately receives `token_storage` from the application.
Provide a persistent implementation appropriate to the deployment, then call
`asyncio.run(main(token_storage))`; do not use an in-memory placeholder for a
long-running client.

## Mutation retries

Remote mutation is not advertised unless it is enabled globally and the
verified identity has the configured mutation scope and mapped writer role.
On `MUTATION_COMMIT_INDETERMINATE`, retry the same LSM with the same
idempotency key. To inspect the outcome without executing DML, call
`reconcile_semantic_mutation` with that key. A different authority ID or
writer role cannot read or replay the state.

Closing a request SSE stream cancels in-flight PostgreSQL work. If the
disconnect races with commit, the outcome remains indeterminate and must be
retried or reconciled; the gateway does not report such a mutation as safely
cancelled.
