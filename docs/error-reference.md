# Error reference

This is the public/stored taxonomy implemented through the current `0.7.0`
source. Codes are stable within this pre-1.0 contract but remain
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
| `MCP_REQUEST_BODY_TIMEOUT` | `-33006` / HTTP 408 | Conditional | HTTP request body was not received within 5 seconds |
| `MCP_HEADERS_TOO_LARGE` | `-32600` / HTTP 431 | No | HTTP headers exceed the configured byte budget |
| `MCP_INVALID_HTTP_HEADERS` | `-32600` / HTTP 400 | No | HTTP content negotiation or content type is invalid |
| `MCP_NOTIFICATION_UNSUPPORTED` | `-32600` / HTTP 400 | No | stateless HTTP does not accept client notifications |
| `MCP_METHOD_NOT_FOUND` | `-32601` | No | method is outside the implemented MCP surface |
| `MCP_INVALID_PARAMS` | `-32602` | No | method parameter envelope is invalid |
| `MCP_HEADER_MISMATCH` | `-32020` / HTTP 400 | No | MCP method/name/version headers do not match the request body metadata |
| `MCP_MISSING_REQUIRED_CLIENT_CAPABILITY` | `-32021` / HTTP 400 | No | modern request metadata omits the required client-capability object |
| `MCP_UNSUPPORTED_PROTOCOL_VERSION` | `-32022` / HTTP 400 | No | HTTP request does not use MCP `2026-07-28` |
| `MCP_RATE_LIMITED` | `-33001` / HTTP 429 | Conditional | authenticated authority exhausted its token bucket |
| `MCP_CONCURRENCY_LIMITED` | `-33002` / HTTP 429 | Conditional | principal, process, or database concurrency budget is full |
| `MCP_AUTHORITY_DENIED` | `-33003` / HTTP 403 | No | verified identity cannot construct the configured execution authority |
| `MCP_EXECUTION_TIMEOUT` | `-33004` / HTTP 504 or SSE | Conditional | HTTP execution exceeded its hard duration |
| `MCP_RESULT_TOO_LARGE` | `-33005` | No | serialized HTTP JSON-RPC result exceeded its byte budget |
| `MCP_INVALID_TOOL_ARGUMENTS` | `-32602` | No | strict tool argument schema mismatch |
| `MCP_TOOL_NOT_FOUND` | `-32602` | No | requested tool is not currently advertised |
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
| `SEMANTIC_MISSING_METRIC_METADATA` | No | Snapshot v2 metric omits required typed metadata |
| `SEMANTIC_INVALID_AGGREGATION_ANCHOR` | No | metric anchor is missing, joined, or not an entity key in the root model |
| `SEMANTIC_MISSING_AGGREGATION_ANCHOR` | No | one-to-many aggregation uses an unanchored metric |
| `SEMANTIC_MIXED_AGGREGATION_ANCHORS` | No | one query mixes metrics declared at different anchors |
| `SEMANTIC_JOINED_METRIC_FILTER` | No | anchored metric filter depends on a joined field |
| `SEMANTIC_UNSUPPORTED_FANOUT_ADDITIVITY` | No | semi-additive metric lacks a supported modeled fan-out axis |
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

## LSM and mutation compiler codes

| Code | Retry | Meaning |
|---|---|---|
| `LSM_INPUT_TOO_LARGE` | No | mutation JSON exceeds 1 MiB |
| `LSM_INVALID_JSON` | No | LSM is malformed, contains duplicate/unknown properties, or violates the strict shape |
| `LSM_UNSUPPORTED_SCHEMA_VERSION` | No | LSM `schema_version` is not `"1"` |
| `LSM_INVALID_MODEL` | No | semantic model name is empty or invalid |
| `LSM_INVALID_IDEMPOTENCY_KEY` | No | key is empty or outside the supported bound |
| `LSM_INVALID_ROW_COUNT` | No | row count is zero or exceeds the LSM bound |
| `LSM_INVALID_FIELD_COUNT` | No | a row has zero or too many fields |
| `LSM_INVALID_FIELD_NAME` | No | semantic field name is empty or invalid |
| `LSM_INCONSISTENT_ROW_FIELDS` | No | batch rows do not have the same semantic field set |
| `LSM_INVALID_VALUE` | No | typed scalar value is malformed |
| `MUTATION_MODEL_NOT_WRITABLE` | No | model is unavailable or has no published writable projection |
| `MUTATION_OPERATION_NOT_ENABLED` | No | insert/upsert is not enabled for the model |
| `MUTATION_ROW_LIMIT_EXCEEDED` | No | model-specific row limit is exceeded |
| `MUTATION_REQUEST_BYTES_EXCEEDED` | No | model-specific request-byte limit is exceeded |
| `MUTATION_INVALID_WRITABLE_MODEL` | No | published writable metadata is inconsistent or ambiguous |
| `MUTATION_FIELD_NOT_WRITABLE` | No | field is unknown, generated, hidden, or not insertable |
| `MUTATION_REQUIRED_FIELD_MISSING` | No | a required insert field is absent |
| `MUTATION_FIELD_TYPE_MISMATCH` | No | typed value does not match the published field type |
| `MUTATION_NULL_NOT_ALLOWED` | No | null was supplied to a non-nullable field |
| `MUTATION_CONFLICT_FIELD_MISSING` | No | upsert omits part of the published conflict key |
| `MUTATION_NO_UPDATABLE_FIELD` | No | upsert contains no approved mutable field |
| `MUTATION_HASH_SERIALIZATION_FAILED` | Conditional | canonical mutation hashing failed |

## Executor codes exposed by MCP

| Code | Retry | Meaning |
|---|---|---|
| `EXECUTOR_QUERY_CANCELLED` | Conditional | PostgreSQL cancelled the query, normally due to statement timeout |
| `EXECUTOR_RESULT_SERIALIZATION_FAILED` | Conditional | returned row shape/value could not be serialized |
| `EXECUTOR_QUERY_FAILED` | Conditional | generic privacy-preserving execution failure |

MCP deliberately collapses role, connection, source, audit, and commit details
into `EXECUTOR_QUERY_FAILED`. Inspect protected diagnostics and the audit row;
do not infer authorization from the public message.

Mutation MCP errors use privacy-preserving messages while retaining LSM,
compiler, or `MUTATION_*` codes. `MUTATION_COMMIT_INDETERMINATE` must not be
blindly retried; reconcile with the same idempotency key.

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

## Mutation executor and stored audit codes

| Code | Retry | Meaning |
|---|---|---|
| `MUTATION_DATABASE_ROLE_NOT_FOUND` | No | mapped writer role does not exist |
| `MUTATION_DATABASE_ROLE_MEMBERSHIP_DENIED` | No | mutation login cannot assume the writer role |
| `MUTATION_UNSAFE_DATABASE_ROLE` | No | writer role is superuser or `BYPASSRLS` |
| `MUTATION_TARGET_RELATION_NOT_FOUND` | No | published target relation is unavailable |
| `MUTATION_TARGET_RELATION_OWNER` | No | writer role owns the target relation |
| `MUTATION_UNSAFE_TARGET_RELATION` | No | target is not a supported table/partition |
| `MUTATION_IDEMPOTENCY_CONFLICT` | No | key was already used under different principal/profile/role authority or for different LSM/revision content |
| `MUTATION_CANCELLED` | Conditional | PostgreSQL cancelled the statement |
| `MUTATION_DATABASE_REJECTED` | Conditional | GRANT, RLS, constraint, trigger, or another database rule rejected the write |
| `MUTATION_INVALID_ROW_SHAPE` | No | returned row shape did not match the compiled contract |
| `MUTATION_RESULT_BYTES_EXCEEDED` | No | result exceeded the bound and the transaction rolled back |
| `MUTATION_AFFECTED_ROWS_MISMATCH` | Conditional | affected rows differed from the compiled expectation |
| `MUTATION_ATOMIC_AUDIT_FAILED` | Conditional | committed audit finalization failed and business DML rolled back |
| `MUTATION_COMMIT_INDETERMINATE` | Reconcile | connection outcome was ambiguous at commit |
| `MUTATION_INVALID_REPLAY_RESULT` | Conditional | stored replay payload is inconsistent |
| `MUTATION_RECONCILIATION_FAILED` | Conditional | idempotency reconciliation could not complete |
| `MUTATION_GUARD_CONFIGURATION_FAILED` | No | connection, timeout, role, or other guard configuration is invalid |

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

No additional reserved error-code ranges are promised during the current beta.
New categories must be documented before clients rely on them.
