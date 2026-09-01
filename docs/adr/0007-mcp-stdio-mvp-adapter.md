# ADR 0007: MCP stdio MVP adapter

- Status: Accepted
- Date: 2026-08-31

## Context

The gateway already loads immutable published semantic revisions, parses and
compiles LSQ v1, executes generated queries through a guarded database
boundary, and requires audit lifecycle records. AI clients need an MCP surface
without gaining access to physical query text, connection configuration,
database roles, principals, or private semantic objects.

## Decision

Add `postgresem mcp serve` as a blocking JSON-RPC 2.0 loop over stdin and
stdout. Each request and response is one JSON object on one line. The server
uses MCP protocol version `2024-11-05`, advertises tools and resources, and
supports only:

- `initialize`
- `notifications/initialized`
- `ping`
- `tools/list`
- `tools/call`
- `resources/list`
- `resources/read`

Input lines are bounded at 1 MiB. An oversized line is consumed through its
terminating newline and rejected, allowing the next request to be processed.
Blank lines are ignored. Malformed input returns stable JSON-RPC errors.
Protocol parameter envelopes accept standard extension fields such as `_meta`;
tool argument objects remain strict. Notifications never receive a response.
Request IDs must be non-null strings or integer JSON numbers and are copied
unchanged. Invalid arrays, objects, booleans, nulls, and non-integer numbers
receive JSON-RPC `-32600` with public code `MCP_INVALID_REQUEST` and a null
response ID. `initialize` requires a string `protocolVersion`, object
`capabilities`, and object `clientInfo` with nonempty string `name` and
`version` fields. Standard `_meta` and unknown protocol extension fields remain
permitted.

The tool surface is fixed to five versioned tools:

1. `list_semantic_models`
2. `describe_semantic_model`
3. `validate_semantic_query`
4. `query_semantic_model`
5. `explain_semantic_query`

Every input schema has `additionalProperties: false` and a schema version.
LSQ-bearing schemas embed the bundled LSQ v1 schema. No tool accepts or returns
generated physical query text.

The adapter reads the project and the strictly validated names of the runtime
conninfo, runtime password, audit conninfo, audit password, and mapped-role
environment variables at process startup. Runtime and audit conninfo remain
passwordless in the gateway container. Rust parses them with
`postgres::Config`, applies the separately read passwords in memory, and then
connects. This path is used for published snapshot loading and guarded runtime
and audit connections, without placing passwords in process arguments or
connection errors. Tool arguments cannot set these values or a
principal/profile. CLI execution continues to accept complete URLs through its
existing environment variables. The executor accepts an `ExecutionContext`
established by the caller. CLI execution preserves its role-derived principal
and `cli` profile; MCP uses the fixed subject `mcp:stdio` and profile
`mcp-stdio`.

Model discovery loads the current published snapshot for each operation.
Responses include only queryable models and visible fields and metrics.
Unavailable, hidden, and nonqueryable names share public “not available”
errors that do not echo the requested name. Public lineage contains semantic
model, metric, relationship, and field names only. Physical relations,
columns, generated query text, and private target models are not returned.
Model descriptions include only usable semantic relationship names,
cardinality, and join type, plus supported time grains and public query limits.
Pagination cursors are bound to the published revision so a publish between
pages fails with the same generic invalid-cursor response as a malformed
cursor.

Validation returns a stable public error code on invalid LSQ or semantic
resolution. Successful validation returns the normalized LSQ hash, semantic
revision, output schema, public lineage, and warnings. Explanation additionally
returns normalized LSQ and effective limits. Query uses the existing guarded
executor and sanitizes physical lineage from the MCP result.

Resources are:

- `semantic://projects/{project}/revisions/current`
- `semantic://projects/{project}/models/{model}`
- `semantic://schemas/lsq/v1`

The schema resource is compiled into the binary with `include_str!`.

Every handled request emits one structured JSON completion or error record to
standard error. Logs contain only the recognized method/tool, status, public
code, and elapsed milliseconds; they exclude request values, private requested
names, connection data, SQL, result rows, and principal data.

## Consequences

- MCP clients receive a small, deterministic semantic-only surface.
- The transport remains local/process-oriented; no remote HTTP listener or
  authentication protocol is introduced.
- Every MCP query retains the executor's read-only transaction, role, timeout,
  ownership, RLS, result-size, and mandatory-audit protections.
- Concurrent protocol cancellation is outside this MVP; PostgreSQL statement
  timeout is the cancellation boundary.
- Clients must use line-delimited JSON for this MVP adapter.
- Snapshot reloads favor correctness and simplicity over connection reuse.

The five-tool surface described here remains the read-only contract. A future
M6 mutation tool, if accepted, requires separate capability negotiation and
versioned schemas and must not be advertised by a read-only deployment.
