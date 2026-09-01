#!/usr/bin/env python3
import hashlib
import json
import os
import subprocess
import sys

process = None


def child_diagnostics():
    if process is None:
        return ""
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)
    return process.stderr.read()


def fail(message):
    diagnostics = child_diagnostics()
    if diagnostics:
        message = f"{message}\nMCP stderr:\n{diagnostics}"
    raise AssertionError(message)


def psql(sql):
    subprocess.run(
        ["psql", "--no-psqlrc", "-v", "ON_ERROR_STOP=1", "-c", sql],
        check=True,
        stdout=subprocess.DEVNULL,
    )


def structured(response):
    result = response.get("result", {})
    if result.get("isError"):
        fail(f"tool call failed: {result}")
    content = result.get("content")
    if (
        not isinstance(content, list)
        or not content
        or content[0].get("type") != "text"
        or not isinstance(content[0].get("text"), str)
    ):
        fail(f"tool result has no baseline text content: {result}")
    try:
        parsed = json.loads(content[0]["text"])
    except json.JSONDecodeError as error:
        fail(f"tool text content is not structured JSON: {error}: {result}")
    if "structuredContent" in result and result["structuredContent"] != parsed:
        fail(f"structuredContent differs from content[0].text: {result}")
    return parsed


def has_key(value, forbidden):
    if isinstance(value, dict):
        return any(key.lower() in forbidden or has_key(child, forbidden) for key, child in value.items())
    if isinstance(value, list):
        return any(has_key(child, forbidden) for child in value)
    return False


env = os.environ.copy()
env["MCP_RUNTIME_DATABASE_URL"] = (
    f"host={env['PGHOST']} port={env['PGPORT']} dbname={env['PGDATABASE']} "
    "user=postgresem_runtime sslmode=disable"
)
env["MCP_AUDIT_DATABASE_URL"] = (
    f"host={env['PGHOST']} port={env['PGPORT']} dbname={env['PGDATABASE']} "
    "user=postgresem_audit_writer sslmode=disable"
)
env["POSTGRESEM_MAX_RESULT_BYTES"] = "12"

psql(
    """
TRUNCATE semantic.query_audit, semantic.mutation_audit, semantic.mutation_idempotency;
DELETE FROM commerce.orders WHERE external_id = 'mcp-order-1';
"""
)

process = subprocess.Popen(
    ["postgresem", "mcp", "serve"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
    env=env,
)


def send_unchecked(message, expect_response=True):
    process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
    process.stdin.flush()
    if not expect_response:
        return None
    line = process.stdout.readline()
    if not line:
        fail("MCP server closed stdout")
    try:
        response = json.loads(line)
    except json.JSONDecodeError as error:
        fail(f"non-protocol stdout: {line!r}: {error}")
    return response


def send(message, expect_response=True):
    response = send_unchecked(message, expect_response)
    if not expect_response:
        return None
    if response.get("id") != message.get("id"):
        fail(f"response ID mismatch: {response}")
    return response


initialize = send(
    {
        "jsonrpc": "2.0",
        "id": "initialize-id",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "postgresem-integration", "version": "1"},
            "_meta": {"progressToken": "initialize-progress"},
        },
    }
)
if initialize["result"]["protocolVersion"] != "2024-11-05":
    fail("unexpected MCP protocol version")
if set(initialize["result"]["capabilities"]) != {"tools", "resources"}:
    fail("MCP capabilities are incomplete")

for invalid_params in [
    {},
    {
        "protocolVersion": 1,
        "capabilities": {},
        "clientInfo": {"name": "client", "version": "1"},
    },
    {
        "protocolVersion": "2024-11-05",
        "capabilities": [],
        "clientInfo": {"name": "client", "version": "1"},
    },
    {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "", "version": "1"},
    },
    {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "client", "version": ""},
    },
]:
    invalid_initialize = send(
        {
            "jsonrpc": "2.0",
            "id": "invalid-initialize",
            "method": "initialize",
            "params": invalid_params,
        }
    )
    if (
        invalid_initialize.get("error", {}).get("code") != -32602
        or invalid_initialize.get("error", {}).get("data", {}).get("code")
        != "MCP_INVALID_PARAMS"
    ):
        fail(f"invalid initialize params were accepted: {invalid_initialize}")

send(
    {
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {"_meta": {"progressToken": "initialized-progress"}},
    },
    expect_response=False,
)

process.stdin.write("{malformed}\n")
process.stdin.flush()
malformed = json.loads(process.stdout.readline())
if malformed.get("id") is not None or malformed.get("error", {}).get("code") != -32700:
    fail(f"malformed request was not safely rejected: {malformed}")

process.stdin.write("x" * (1024 * 1024 + 1) + "\n")
process.stdin.flush()
oversized = json.loads(process.stdout.readline())
if oversized.get("id") is not None or oversized.get("error", {}).get("data", {}).get("code") != "MCP_REQUEST_TOO_LARGE":
    fail(f"oversized request was not safely rejected: {oversized}")

process.stdin.write("  \t \n")
process.stdin.flush()
ping = send(
    {
        "jsonrpc": "2.0",
        "id": "ping-id",
        "method": "ping",
        "params": {"_meta": {"progressToken": "ping-progress"}},
    }
)
if ping.get("result") != {}:
    fail("ping failed after malformed, oversized, or blank input")

for invalid_id in [None, True, 1.5, ["json", "id"], {"nested": "id"}]:
    invalid_id_response = send_unchecked(
        {
            "jsonrpc": "2.0",
            "id": invalid_id,
            "method": "ping",
            "params": {},
        }
    )
    if (
        invalid_id_response.get("id") is not None
        or invalid_id_response.get("error", {}).get("code") != -32600
        or invalid_id_response.get("error", {}).get("data", {}).get("code")
        != "MCP_INVALID_REQUEST"
    ):
        fail(f"invalid request ID was not rejected: {invalid_id_response}")

tools_response = send(
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {"_meta": {"progressToken": "tools-list-progress"}},
    }
)
tools = tools_response["result"]["tools"]
tool_names = [tool["name"] for tool in tools]
expected_tools = [
    "list_semantic_models",
    "describe_semantic_model",
    "validate_semantic_query",
    "query_semantic_model",
    "explain_semantic_query",
    "validate_semantic_mutation",
    "mutate_semantic_model",
]
if tool_names != expected_tools:
    fail(f"unexpected tools: {tool_names}")
serialized_tools = json.dumps(tools).lower()
if '"sql"' in serialized_tools or "compile_semantic" in serialized_tools:
    fail("raw query compilation surface was advertised")
for tool in tools:
    schema = tool["inputSchema"]
    if schema.get("additionalProperties") is not False:
        fail(f"tool schema is extensible: {tool['name']}")
    if schema["properties"]["schema_version"].get("const") != "1":
        fail(f"tool schema is not versioned: {tool['name']}")


def call(name, arguments, request_id):
    return send(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
                "_meta": {"progressToken": f"tool-progress-{request_id}"},
            },
        }
    )


unknown_tool = call("private_tool_name", {}, "unknown-tool")
if (
    "result" in unknown_tool
    or unknown_tool.get("error", {}).get("code") != -32602
    or unknown_tool.get("error", {}).get("data", {}).get("code") != "MCP_TOOL_NOT_FOUND"
):
    fail(f"unknown tool was not a JSON-RPC error: {unknown_tool}")
if "private_tool_name" in json.dumps(unknown_tool):
    fail("unknown tool name leaked in a JSON-RPC error")

invalid_arguments = call(
    "query_semantic_model",
    {"schema_version": "1", "lsq": {}, "principal": "not-allowed"},
    "invalid-arguments",
)
if (
    "result" in invalid_arguments
    or invalid_arguments.get("error", {}).get("code") != -32602
    or invalid_arguments.get("error", {}).get("data", {}).get("code")
    != "MCP_INVALID_TOOL_ARGUMENTS"
):
    fail(f"strict tool argument mismatch was not invalid params: {invalid_arguments}")


listed = structured(
    call("list_semantic_models", {"schema_version": "1", "limit": 2}, 2)
)
if [model["name"] for model in listed["models"]] != ["orders", "subscriptions"]:
    fail(f"model pagination is not deterministic: {listed}")
if not listed["next_cursor"].startswith(
    f"v1:{listed['semantic_revision']}:"
) or not listed["next_cursor"].endswith(":2"):
    fail("model pagination cursor is missing")
stale_cursor = call(
    "list_semantic_models",
    {
        "schema_version": "1",
        "limit": 2,
        "cursor": "v1:sha256:0000000000000000000000000000000000000000000000000000000000000000:2",
    },
    "stale-cursor",
)
if (
    not stale_cursor.get("result", {}).get("isError")
    or json.loads(stale_cursor["result"]["content"][0]["text"])["error"]["code"]
    != "MCP_INVALID_CURSOR"
):
    fail(f"revision-mismatched cursor was not generically rejected: {stale_cursor}")
listed_tail = structured(
    call(
        "list_semantic_models",
        {"schema_version": "1", "limit": 2, "cursor": listed["next_cursor"]},
        3,
    )
)
if [model["name"] for model in listed_tail["models"]] != ["tenant_orders"]:
    fail("model pagination tail is incorrect")
if "customers" in json.dumps([listed, listed_tail]):
    fail("nonqueryable model leaked from list")

described = structured(
    call(
        "describe_semantic_model",
        {"schema_version": "1", "model": "orders"},
        4,
    )
)
description_text = json.dumps(described)
for hidden_name in ["internal_amount", "internal_revenue", "customers"]:
    if hidden_name in description_text:
        fail(f"private semantic name leaked from describe: {hidden_name}")
if described["model"]["relationships"] != [
    {"name": "customer", "cardinality": "many_to_one", "join_type": "left"}
]:
    fail(f"usable semantic relationships are incomplete: {described}")
ordered_at = next(
    field for field in described["model"]["fields"] if field["name"] == "ordered_at"
)
if ordered_at["supported_time_grains"] != [
    "day",
    "week",
    "month",
    "quarter",
    "year",
]:
    fail(f"time grains are incomplete: {ordered_at}")
if described["query_limits"] != {
    "default": 100,
    "hard": 10000,
    "max_result_bytes": 12,
}:
    fail(f"query limits are incomplete: {described}")
if has_key(
    described["model"]["relationships"],
    {"target", "target_model", "relation", "from_column", "to_column"},
):
    fail("describe exposed a physical relationship target")

missing_model = call(
    "describe_semantic_model",
    {"schema_version": "1", "model": "customers"},
    5,
)
if not missing_model["result"]["isError"]:
    fail("nonqueryable model was describable")
if "customers" in json.dumps(missing_model):
    fail("nonqueryable model name leaked in an error")

lsq = {
    "schema_version": "1",
    "model": "orders",
    "metrics": [{"metric": "revenue"}],
    "limit": 10,
}
validated = structured(
    call(
        "validate_semantic_query",
        {"schema_version": "1", "lsq": lsq},
        6,
    )
)
if not validated["valid"] or not validated["normalized_lsq_hash"].startswith("sha256:"):
    fail(f"validation result is incomplete: {validated}")
if validated["output_schema"] != [{"name": "revenue", "type": "numeric"}]:
    fail(f"validation output schema is not public: {validated}")
if has_key(validated["output_schema"], {"data_type"}):
    fail("validation output schema exposed compiler-internal data_type")
if has_key(validated, {"sql", "generated_sql", "source_columns"}):
    fail("validation exposed a physical query surface")

hidden_validation = structured(
    call(
        "validate_semantic_query",
        {
            "schema_version": "1",
            "lsq": {
                "schema_version": "1",
                "model": "orders",
                "metrics": [{"metric": "internal_revenue"}],
            },
        },
        7,
    )
)
unknown_validation = structured(
    call(
        "validate_semantic_query",
        {
            "schema_version": "1",
            "lsq": {
                "schema_version": "1",
                "model": "orders",
                "metrics": [{"metric": "does_not_exist"}],
            },
        },
        8,
    )
)
if hidden_validation["error"]["code"] != unknown_validation["error"]["code"]:
    fail("hidden and unknown metrics are distinguishable")
if "internal_revenue" in json.dumps(hidden_validation):
    fail("hidden metric name leaked in validation")

explained = structured(
    call(
        "explain_semantic_query",
        {"schema_version": "1", "lsq": lsq},
        9,
    )
)
if explained["semantic_models"] != ["orders"] or explained["limits"]["effective"] != 10:
    fail(f"explain result is incomplete: {explained}")
if explained["output_schema"] != [{"name": "revenue", "type": "numeric"}]:
    fail(f"explain output schema is not public: {explained}")
if has_key(explained, {"sql", "generated_sql", "source_columns"}):
    fail("explain exposed a physical query surface")

queried = structured(
    call(
        "query_semantic_model",
        {"schema_version": "1", "lsq": lsq},
        10,
    )
)
if queried["rows"] != [["200.50"]] or queried["truncated"]:
    fail(f"unexpected MCP query result: {queried}")
if queried["columns"] != [{"name": "revenue", "type": "numeric"}]:
    fail(f"query columns are not public: {queried}")
if has_key(queried, {"sql", "generated_sql", "source_columns"}):
    fail("query result exposed a physical query surface")

truncated = structured(
    call(
        "query_semantic_model",
        {
            "schema_version": "1",
            "lsq": {
                "schema_version": "1",
                "model": "orders",
                "dimensions": [{"field": "order_id"}],
                "limit": 10,
            },
        },
        "truncated-query",
    )
)
if not truncated["truncated"]:
    fail(f"byte-limited query was not truncated: {truncated}")
if truncated["warnings"] != [
    "result is incomplete because it exceeded the byte limit; narrow the query"
]:
    fail(f"truncated query warning is missing or unstable: {truncated}")

lsm = {
    "schema_version": "1",
    "operation": "insert",
    "model": "orders",
    "idempotency_key": "mcp-order-insert",
    "rows": [
        {
            "external_id": {"type": "text", "value": "mcp-order-1"},
            "customer_id": {"type": "integer", "value": 1},
            "ordered_at": {"type": "timestamp", "value": "2026-09-01T08:00:00Z"},
            "status": {"type": "text", "value": "paid"},
            "amount": {"type": "numeric", "value": "8.50"},
        }
    ],
}
validated_mutation = structured(
    call(
        "validate_semantic_mutation",
        {"schema_version": "1", "lsm": lsm},
        "validate-mutation",
    )
)
if (
    not validated_mutation["valid"]
    or validated_mutation["operation"] != "insert"
    or validated_mutation["expected_rows"] != 1
):
    fail(f"mutation validation result is incomplete: {validated_mutation}")
if has_key(validated_mutation, {"sql", "statement", "source_columns"}):
    fail("mutation validation exposed a physical mutation surface")

mutated = structured(
    call(
        "mutate_semantic_model",
        {"schema_version": "1", "lsm": lsm},
        "mutate",
    )
)
if (
    mutated["affected_rows"] != 1
    or mutated["replayed"]
    or mutated["rows"][0][1] != "mcp-order-1"
):
    fail(f"mutation result is incorrect: {mutated}")
if has_key(mutated, {"sql", "statement", "source_columns"}):
    fail("mutation result exposed a physical mutation surface")

replayed_mutation = structured(
    call(
        "mutate_semantic_model",
        {"schema_version": "1", "lsm": lsm},
        "mutate-replay",
    )
)
if (
    not replayed_mutation["replayed"]
    or replayed_mutation["mutation_id"] != mutated["mutation_id"]
):
    fail(f"MCP mutation replay is incorrect: {replayed_mutation}")

resources = send(
    {
        "jsonrpc": "2.0",
        "id": 11,
        "method": "resources/list",
        "params": {"_meta": {"progressToken": "resources-list-progress"}},
    }
)["result"]["resources"]
resource_text = json.dumps(resources)
for private_name in ["customers", "internal_amount", "internal_revenue"]:
    if private_name in resource_text:
        fail(f"private semantic name leaked from resources: {private_name}")

revision_uri = "semantic://projects/commerce/revisions/current"
model_resource_uri = "semantic://projects/commerce/models/orders"
expected_uris = {
    revision_uri,
    model_resource_uri,
    "semantic://projects/commerce/models/subscriptions",
    "semantic://projects/commerce/models/tenant_orders",
    "semantic://schemas/lsq/v1",
    "semantic://schemas/lsm/v1",
}
if {resource["uri"] for resource in resources} != expected_uris:
    fail(f"unexpected resources: {resources}")


def read_resource(uri, request_id):
    response = send(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "resources/read",
            "params": {
                "uri": uri,
                "_meta": {"progressToken": f"resource-progress-{request_id}"},
            },
        }
    )
    return json.loads(response["result"]["contents"][0]["text"])


revision_resource = read_resource(revision_uri, 12)
if revision_resource["models"] != ["orders", "subscriptions", "tenant_orders"]:
    fail("current revision resource leaked or omitted models")
model_resource = read_resource(model_resource_uri, 13)
if "internal_" in json.dumps(model_resource):
    fail("model resource leaked hidden objects")
schema_resource = read_resource("semantic://schemas/lsq/v1", 14)
if schema_resource.get("additionalProperties") is not False:
    fail("LSQ schema resource is not the strict bundled schema")
mutation_schema_resource = read_resource(
    "semantic://schemas/lsm/v1", "mutation-schema"
)
if mutation_schema_resource.get("additionalProperties") is not False:
    fail("LSM schema resource is not the strict bundled schema")

process.stdin.close()
try:
    return_code = process.wait(timeout=10)
except subprocess.TimeoutExpired:
    fail("MCP server did not exit after stdin closed")
if return_code != 0:
    fail("MCP server failed")
if process.stdout.read():
    fail("MCP server wrote unexpected trailing stdout")
stderr_output = process.stderr.read()
log_lines = [line for line in stderr_output.splitlines() if line]
if not log_lines:
    fail("MCP server emitted no structured request logs")
for line in log_lines:
    try:
        record = json.loads(line)
    except json.JSONDecodeError as error:
        fail(f"MCP stderr was not structured JSON: {line!r}: {error}")
    if record.get("event") != "mcp_request":
        fail(f"unexpected MCP stderr event: {record}")
    if not {"method", "tool", "status", "code", "elapsed_ms"} <= set(record):
        fail(f"incomplete MCP request log: {record}")
for forbidden_value in [
    "private_tool_name",
    "internal_revenue",
    "does_not_exist",
    "customers",
    "MCP_RUNTIME_DATABASE_URL",
    "postgresem_runtime",
    "SELECT ",
    "mcp:stdio",
    "mcp-order-1",
    "8.50",
]:
    if forbidden_value in stderr_output:
        fail(f"sensitive request data leaked to MCP logs: {forbidden_value}")

principal_hash = "sha256:" + hashlib.sha256(b"mcp:stdio").hexdigest()
psql(
    """
DO $$
BEGIN
  IF (SELECT count(*) FROM semantic.query_audit) <> 2 THEN
    RAISE EXCEPTION 'unexpected MCP audit count';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM semantic.query_audit
    WHERE status = 'succeeded'
      AND config_profile = 'mcp-stdio'
      AND principal_subject_hash = '%s'
      AND policy_context->>'database_role' = 'postgresem_analyst'
  ) THEN
    RAISE EXCEPTION 'MCP execution context was not audited';
  END IF;
END;
$$;
"""
    % principal_hash
)

psql(
    """
DO $$
BEGIN
  IF (SELECT count(*) FROM semantic.mutation_audit) <> 2 THEN
    RAISE EXCEPTION 'unexpected MCP mutation audit count';
  END IF;
  IF NOT EXISTS (
    SELECT 1
    FROM semantic.mutation_audit
    WHERE status = 'committed'
      AND config_profile = 'mcp-stdio'
      AND principal_subject_hash = '%s'
      AND policy_context->>'database_role' = 'postgresem_order_writer'
      AND replayed
  ) THEN
    RAISE EXCEPTION 'MCP mutation execution context or replay was not audited';
  END IF;
END;
$$;
"""
    % principal_hash
)

psql(
    """
DELETE FROM commerce.orders WHERE external_id = 'mcp-order-1';
TRUNCATE semantic.mutation_audit, semantic.mutation_idempotency;
"""
)

print("MCP stdio integration checks passed")
