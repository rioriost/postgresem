# Error reference

This is the public/stored taxonomy implemented by
`0.2.0-alpha.1`. Codes are stable within this preview contract but remain
subject to the pre-1.0 compatibility policy.

## Response envelopes

JSON-RPC/protocol failures use:

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"invalid method parameters","data":{"code":"MCP_INVALID_PARAMS"}}}
```

Tool-operation failures are successful JSON-RPC responses with:

```json
{"result":{"content":[{"type":"text","text":"{\"error\":{\"code\":\"...\",\"message\":\"...\"}}"}],"structuredContent":{"error":{"code":"...","message":"..."}},"isError":true}}
```

`validate_semantic_query` is special: invalid LSQ/semantics returns
`isError: false` with `structuredContent.valid: false` and an `error` object.

## Retry guidance

- **No**: the same request will fail until input/configuration changes.
- **Relist**: discard revision-bound state, rediscover, then retry.
- **Conditional**: retry only after the stated dependency or operator action.

## MCP protocol and tool codes

| Public code | JSON-RPC code/location | Retry | Meaning |
|---|---:|---|---|
| `MCP_PARSE_ERROR` | `-32700` | No | line is not valid JSON |
| `MCP_INVALID_REQUEST` | `-32600` | No | invalid JSON-RPC object, version, method, or ID |
| `MCP_REQUEST_TOO_LARGE` | `-32600` | No | input line exceeds 1 MiB |
| `MCP_METHOD_NOT_FOUND` | `-32601` | No | method is outside the implemented MCP surface |
| `MCP_INVALID_PARAMS` | `-32602` | No | method parameter envelope is invalid |
| `MCP_INVALID_TOOL_ARGUMENTS` | `-32602` | No | strict tool argument schema mismatch |
| `MCP_TOOL_NOT_FOUND` | `-32602` | No | requested tool is not one of the five public tools |
| `MCP_TOOL_SCHEMA_VERSION_UNSUPPORTED` | tool error | No | tool `schema_version` is not `"1"` |
| `MCP_INVALID_PAGINATION` | tool error | No | model-list limit is outside 1–100 |
| `MCP_INVALID_CURSOR` | tool error | Relist | malformed, out-of-range, or revision-mismatched model cursor |
| `MCP_RESOURCE_NOT_FOUND` | `-32002` | Relist | resource URI is not currently available |
| `MCP_INTERNAL_ERROR` | tool error or `-32002` | Conditional | serialization/resource/internal operation failed |
| `SEMANTIC_SNAPSHOT_UNAVAILABLE` | tool error or `-32002` | Conditional | current published snapshot could not be loaded |

`OK` is a stderr completion-log code, not an error. `MCP_TOOL_ERROR` exists
only as an internal fallback log label if a malformed tool-error result lacks a
code; clients must not depend on receiving it.

## LSQ v1 codes

These appear in validation results, tool errors, and CLI/library errors.

| Code | Retry | Meaning |
|---|---|---|
| `LSQ_INVALID_JSON` | No | LSQ does not deserialize as the strict v1 shape |
| `LSQ_UNSUPPORTED_SCHEMA_VERSION` | No | LSQ `schema_version` is not `"1"` |
| `LSQ_EMPTY_MODEL` | No | model is empty |
| `LSQ_EMPTY_PROJECTION` | No | neither a dimension nor metric is projected |
| `LSQ_EMPTY_REFERENCE` | No | a semantic reference is empty |
| `LSQ_DUPLICATE_REFERENCE` | No | duplicate projected semantic reference |
| `LSQ_DUPLICATE_ORDER_REFERENCE` | No | duplicate order reference |
| `LSQ_INVALID_LITERAL_VALUE` | No | typed date/timestamp/numeric literal is invalid |
| `LSQ_INVALID_LIMIT` | No | limit is outside 1–10,000 |
| `LSQ_FILTER_TOO_DEEP` | No | filter nesting exceeds 16 |
| `LSQ_FILTER_TOO_LARGE` | No | filter node count exceeds 128 |
| `LSQ_EMPTY_LOGICAL_FILTER` | No | `and`/`or` has no arguments |
| `LSQ_INVALID_IN_FILTER_SIZE` | No | `in` contains zero or more than 100 values |

## Semantic/compiler codes

Unknown and hidden fields/metrics intentionally share the same public codes.

| Code | Retry | Meaning |
|---|---|---|
| `SEMANTIC_UNSUPPORTED_SNAPSHOT_VERSION` | No | compiler snapshot version is unsupported |
| `SEMANTIC_INVALID_REVISION_HASH` | No | snapshot revision hash is invalid |
| `SEMANTIC_MODEL_NOT_AVAILABLE` | Relist | model is unknown, hidden from query, or nonqueryable |
| `SEMANTIC_FIELD_NOT_AVAILABLE` | No | field is unknown or hidden |
| `SEMANTIC_METRIC_NOT_AVAILABLE` | No | metric is unknown or hidden |
| `SEMANTIC_INVALID_TIME_GRAIN` | No | grain is not valid for that field |
| `SEMANTIC_LITERAL_TYPE_MISMATCH` | No | filter literal type does not match its field |
| `SEMANTIC_ORDER_REFERENCE_NOT_PROJECTED` | No | order reference is not a projected output |
| `SEMANTIC_RELATIONSHIP_NOT_AVAILABLE` | No | required relationship is unavailable |
| `SEMANTIC_UNSAFE_RELATIONSHIP` | No | relationship cardinality/direction is outside compiler safety rules |
| `SEMANTIC_INVALID_METRIC_FIELD` | No | metric refers to an invalid field |
| `SEMANTIC_JOINED_METRIC_FIELD` | No | preview compiler refuses aggregation of a joined field |
| `SEMANTIC_COUNT_DISTINCT_REQUIRES_ENTITY_KEY` | No | distinct-count source is not an entity key |
| `SEMANTIC_COUNT_REQUIRES_ENTITY_KEY` | No | count source is not an entity key |
| `SEMANTIC_INVALID_METRIC_TYPE` | No | metric result type conflicts with aggregation |
| `SEMANTIC_INVALID_RELATIONSHIP_TARGET` | No | relationship target model is invalid |
| `SEMANTIC_RELATIONSHIP_TARGET_NOT_ENTITY_KEY` | No | relationship target column is not an entity key |
| `SEMANTIC_MISSING_TIMEZONE` | No | timestamp-with-time-zone model lacks required timezone |
| `COMPILER_HASH_SERIALIZATION_FAILED` | Conditional | internal canonical-hash serialization failed |
| `COMPILER_INVALID_LIMIT_CONFIGURATION` | No | compiler default/hard limits are inconsistent |
| `COMPILER_LIMIT_EXCEEDED` | No | requested limit exceeds compiler hard limit |

The MCP adapter maps several less common compiler failures to the generic
message “semantic query is not valid for the current revision” while retaining
the code.

## Executor codes exposed by MCP

| Code | Retry | Meaning |
|---|---|---|
| `EXECUTOR_QUERY_CANCELLED` | Conditional | PostgreSQL cancelled the query, normally due to statement timeout |
| `EXECUTOR_RESULT_SERIALIZATION_FAILED` | Conditional | returned row shape/value could not be serialized |
| `EXECUTOR_QUERY_FAILED` | Conditional | generic privacy-preserving execution failure |

MCP deliberately collapses role, connection, source, audit, and commit details
into `EXECUTOR_QUERY_FAILED`. Inspect protected diagnostics and the audit row;
do not infer authorization from the public message.

## Executor codes stored in query audit

After the durable `started` record exists, source-stage failures use these
implemented `semantic.query_audit.error_code` values:

| Code | Retry | Meaning |
|---|---|---|
| `EXECUTOR_DATABASE_ROLE_NOT_FOUND` | No | mapped role does not exist |
| `EXECUTOR_DATABASE_ROLE_MEMBERSHIP_DENIED` | No | runtime login is not a member |
| `EXECUTOR_UNSAFE_DATABASE_ROLE` | No | mapped role is superuser or `BYPASSRLS` |
| `EXECUTOR_SOURCE_RELATION_NOT_FOUND` | No | compiled source relation is unavailable |
| `EXECUTOR_SOURCE_RELATION_OWNER` | No | mapped role owns a source relation |
| `EXECUTOR_QUERY_CANCELLED` | Conditional | source statement was cancelled |
| `EXECUTOR_SOURCE_QUERY_FAILED` | Conditional | PostgreSQL source query failed |
| `EXECUTOR_SOURCE_COMMIT_FAILED` | Conditional | read-only transaction commit failed |
| `EXECUTOR_INVALID_ROW_SHAPE` | No | generated result wrapper returned an invalid row shape |
| `EXECUTOR_ROW_SERIALIZATION_FAILED` | Conditional | source row JSON serialization failed |
| `EXECUTOR_GUARD_CONFIGURATION_FAILED` | No | another guarded-transaction/configuration failure |

Connection, published-model, LSQ, compiler, and initial audit failures can occur
before a query ID exists and therefore may have no query-audit row.

## Privacy behavior

The public MCP boundary:

- never returns generated SQL, physical source columns, credentials, roles,
  principals, or result data inside errors;
- does not echo unknown tool names, private requested names, or unavailable
  model names;
- makes hidden and unknown fields/metrics indistinguishable;
- logs only recognized method/tool names and public codes to stderr;
- returns generic snapshot and execution failures.

Direct CLI and Rust-library errors are an operator/developer interface and can
contain semantic object names or environment-variable names. Do not expose
their stderr verbatim to untrusted users.

No additional reserved error-code ranges are promised in M4. New categories
must be documented before clients rely on them.
