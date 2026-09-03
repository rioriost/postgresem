#!/usr/bin/env python3
import base64
import hashlib
import http.client
import json
import os
import socket
import subprocess
import tempfile
import time


HOST = "mcp.example.test:8443"
ORIGIN = "https://agent.example.test"
RESOURCE = f"https://{HOST}/mcp"
ISSUER = "https://identity.example.test"
PROTOCOL = "2026-07-28"
PORT = 18080
server = None
lock_holder = None


def fail(message):
    diagnostics = []
    for process, label in [(server, "MCP HTTP stderr"), (lock_holder, "lock holder stderr")]:
        if process is None:
            continue
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
        if process.stderr is not None:
            output = process.stderr.read()
            if output:
                diagnostics.append(f"{label}:\n{output}")
    if diagnostics:
        message = f"{message}\n" + "\n".join(diagnostics)
    raise AssertionError(message)


def psql(sql):
    return subprocess.run(
        ["psql", "--no-psqlrc", "-At", "-v", "ON_ERROR_STOP=1", "-c", sql],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout.strip()


def b64url(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")


def generate_signing_material(directory):
    private_key = os.path.join(directory, "jwt-private.pem")
    subprocess.run(
        [
            "openssl",
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:2048",
            "-out",
            private_key,
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    modulus_output = subprocess.run(
        ["openssl", "rsa", "-in", private_key, "-noout", "-modulus"],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    ).stdout.strip()
    modulus = bytes.fromhex(modulus_output.split("=", 1)[1])
    exponent = (65537).to_bytes(3, "big")
    jwks = {
        "keys": [
            {
                "kty": "RSA",
                "alg": "RS256",
                "use": "sig",
                "kid": "integration-rsa",
                "n": b64url(modulus),
                "e": b64url(exponent),
            }
        ]
    }
    jwks_path = os.path.join(directory, "jwks.json")
    with open(jwks_path, "w", encoding="utf-8") as output:
        json.dump(jwks, output, separators=(",", ":"))
    return private_key, jwks_path


def sign_token(private_key, subject, scope, **overrides):
    now = int(time.time())
    header = {
        "alg": "RS256",
        "kid": "integration-rsa",
        "typ": "at+jwt",
    }
    header.update(overrides.pop("header", {}))
    claims = {
        "iss": ISSUER,
        "aud": RESOURCE,
        "sub": subject,
        "iat": now - 5,
        "nbf": now - 5,
        "exp": now + 300,
        "scope": scope,
    }
    claims.update(overrides)
    signing_input = (
        b64url(json.dumps(header, separators=(",", ":")).encode("utf-8"))
        + "."
        + b64url(json.dumps(claims, separators=(",", ":")).encode("utf-8"))
    )
    signature = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", private_key],
        input=signing_input.encode("ascii"),
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    ).stdout
    return f"{signing_input}.{b64url(signature)}"


def authority_document(jwks_path, hmac_path):
    return {
        "schema_version": "1",
        "resource": RESOURCE,
        "issuer": ISSUER,
        "audience": RESOURCE,
        "authorization_servers": [ISSUER],
        "scopes_supported": ["postgresem.query", "postgresem.mutate"],
        "jwks_path": jwks_path,
        "principal_hmac_key_path": hmac_path,
        "allowed_token_types": ["at+jwt"],
        "allowed_algorithms": ["RS256"],
        "scope_claims": ["scope"],
        "clock_skew_seconds": 5,
        "max_token_age_seconds": 600,
        "allowed_hosts": [HOST],
        "allowed_origins": [ORIGIN],
        "query_scope": "postgresem.query",
        "mutation_scope": "postgresem.mutate",
        "remote_mutation_enabled": True,
        "server_limits": {
            "max_request_body_bytes": 1048576,
            "max_token_bytes": 16384,
            "max_header_bytes": 32768,
            "max_execution_seconds": 30,
            "max_result_bytes": 1048576,
            "max_concurrent_requests": 16,
            "max_pre_auth_concurrent_requests": 4,
            "max_database_connections": 8,
            "sse_keepalive_seconds": 1,
            "max_sse_seconds": 30,
        },
        "principals": [
            {
                "subject": "tenant-a-agent",
                "authority_id": "tenant-a",
                "query_role": "postgresem_tenant_a",
                "allowed_scopes": ["postgresem.query"],
                "rate_limit": {
                    "requests_per_minute": 120,
                    "burst": 50,
                    "max_concurrent": 2,
                },
            },
            {
                "subject": "tenant-b-agent",
                "authority_id": "tenant-b",
                "query_role": "postgresem_tenant_b",
                "mutation_role": "postgresem_tenant_b_writer",
                "allowed_scopes": ["postgresem.query", "postgresem.mutate"],
                "rate_limit": {
                    "requests_per_minute": 120,
                    "burst": 50,
                    "max_concurrent": 1,
                },
            },
            {
                "subject": "rate-agent",
                "authority_id": "rate-test",
                "query_role": "postgresem_tenant_a",
                "allowed_scopes": ["postgresem.query"],
                "rate_limit": {
                    "requests_per_minute": 1,
                    "burst": 1,
                    "max_concurrent": 1,
                },
            },
        ],
    }


def request(method, path, headers=None, body=None, timeout=10):
    connection = http.client.HTTPConnection("127.0.0.1", PORT, timeout=timeout)
    connection.request(method, path, body=body, headers=headers or {})
    response = connection.getresponse()
    payload = response.read()
    result = (response.status, dict(response.getheaders()), payload)
    connection.close()
    return result


def metadata_request(host=HOST):
    return request("GET", "/.well-known/oauth-protected-resource/mcp", {"Host": host})


def request_body(method, params, request_id="integration"):
    enriched = dict(params)
    enriched["_meta"] = {
        "io.modelcontextprotocol/protocolVersion": PROTOCOL,
        "io.modelcontextprotocol/clientInfo": {
            "name": "postgresem-http-integration",
            "version": "1",
        },
        "io.modelcontextprotocol/clientCapabilities": {},
    }
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": enriched,
    }


def mcp_request(
    token,
    method,
    params,
    *,
    name=None,
    request_id="integration",
    host=HOST,
    origin=None,
    protocol=PROTOCOL,
    mirrored_method=None,
):
    body = request_body(method, params, request_id)
    body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = protocol
    headers = {
        "Host": host,
        "Accept": "application/json, text/event-stream",
        "Content-Type": "application/json",
        "MCP-Protocol-Version": protocol,
        "Mcp-Method": mirrored_method or method,
    }
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    if origin is not None:
        headers["Origin"] = origin
    if name is not None:
        headers["Mcp-Name"] = name
    return request(
        "POST",
        "/mcp",
        headers,
        json.dumps(body, separators=(",", ":")).encode("utf-8"),
        timeout=35,
    )


def json_payload(response):
    status, _, payload = response
    try:
        return status, json.loads(payload)
    except json.JSONDecodeError as error:
        fail(f"response is not JSON: status={status}, payload={payload!r}: {error}")


def sse_payload(response):
    status, headers, payload = response
    if status != 200:
        fail(f"unexpected SSE status: {status}: {payload!r}")
    if not headers.get("content-type", "").startswith("text/event-stream"):
        fail(f"missing SSE content type: {headers}")
    events = []
    for line in payload.decode("utf-8").splitlines():
        if line.startswith("data:"):
            events.append(json.loads(line[5:].strip()))
    if len(events) != 1:
        fail(f"unexpected SSE event count: {events!r}")
    return events[0]


def tool_result(response, *, error=False):
    if response[1].get("content-type", "").startswith("text/event-stream"):
        rpc = sse_payload(response)
    else:
        status, rpc = json_payload(response)
        if status != 200:
            fail(f"unexpected tool response status: {status}: {rpc}")
    result = rpc.get("result", {})
    if result.get("resultType") != "complete":
        fail(f"tool result is not complete: {rpc}")
    if bool(result.get("isError")) != error:
        fail(f"unexpected tool error state: {rpc}")
    content = result.get("content", [])
    if not content or content[0].get("type") != "text":
        fail(f"tool result has no text content: {rpc}")
    return json.loads(content[0]["text"])


def call_tool(token, name, arguments, request_id):
    return mcp_request(
        token,
        "tools/call",
        {"name": name, "arguments": arguments},
        name=name,
        request_id=request_id,
    )


def open_stream(token, name, arguments, request_id):
    body = request_body(
        "tools/call",
        {"name": name, "arguments": arguments},
        request_id,
    )
    headers = {
        "Host": HOST,
        "Accept": "application/json, text/event-stream",
        "Content-Type": "application/json",
        "Authorization": f"Bearer {token}",
        "MCP-Protocol-Version": PROTOCOL,
        "Mcp-Method": "tools/call",
        "Mcp-Name": name,
    }
    connection = http.client.HTTPConnection("127.0.0.1", PORT, timeout=10)
    connection.request(
        "POST",
        "/mcp",
        body=json.dumps(body, separators=(",", ":")).encode("utf-8"),
        headers=headers,
    )
    response = connection.getresponse()
    if response.status != 200:
        fail(f"stream did not start: {response.status}: {response.read()!r}")
    return connection, response


def wait_for_server():
    deadline = time.time() + 20
    while time.time() < deadline:
        try:
            status, _, _ = metadata_request()
            if status == 200:
                return
        except (ConnectionRefusedError, socket.timeout, OSError):
            pass
        if server.poll() is not None:
            fail("MCP HTTP server exited during startup")
        time.sleep(0.1)
    fail("MCP HTTP server did not become ready")


def slow_body_request(token):
    body_length = 1024
    authorization = f"Bearer {token}"
    request_headers = (
        "POST /mcp HTTP/1.1\r\n"
        f"Host: {HOST}\r\n"
        "Accept: application/json, text/event-stream\r\n"
        "Content-Type: application/json\r\n"
        f"Authorization: {authorization}\r\n"
        f"MCP-Protocol-Version: {PROTOCOL}\r\n"
        "Mcp-Method: ping\r\n"
        f"Content-Length: {body_length}\r\n"
        "\r\n"
    ).encode("ascii")
    connection = socket.create_connection(("127.0.0.1", PORT), timeout=2)
    try:
        connection.sendall(request_headers + b"{")
        connection.settimeout(8)
        response = http.client.HTTPResponse(connection)
        response.begin()
        payload = response.read()
        return response.status, json.loads(payload) if payload else {}
    finally:
        connection.close()


def assert_private(headers):
    if headers.get("cache-control") != "no-store":
        fail(f"response is cacheable: {headers}")
    if headers.get("vary") != "Authorization":
        fail(f"response does not vary on authorization: {headers}")


def main():
    global server, lock_holder

    psql(
        """
        TRUNCATE semantic.query_audit, semantic.mutation_audit,
          semantic.mutation_idempotency;
        DELETE FROM rls_fixture.orders
        WHERE external_id LIKE 'mcp-http-%';
        """
    )

    with tempfile.TemporaryDirectory(prefix="postgresem-mcp-http-") as directory:
        private_key, jwks_path = generate_signing_material(directory)
        hmac_path = os.path.join(directory, "principal-hmac.key")
        with open(hmac_path, "wb") as output:
            output.write(os.urandom(32))
        authority_path = os.path.join(directory, "authority.json")
        with open(authority_path, "w", encoding="utf-8") as output:
            json.dump(
                authority_document(jwks_path, hmac_path),
                output,
                separators=(",", ":"),
            )

        tenant_a = sign_token(
            private_key, "tenant-a-agent", "postgresem.query"
        )
        tenant_b = sign_token(
            private_key,
            "tenant-b-agent",
            "postgresem.query postgresem.mutate",
        )
        rate_token = sign_token(private_key, "rate-agent", "postgresem.query")

        environment = os.environ.copy()
        database_host = environment.get("PGHOST")
        if database_host:
            for name in (
                "MCP_RUNTIME_DATABASE_URL",
                "MCP_AUDIT_DATABASE_URL",
                "MCP_MUTATION_DATABASE_URL",
            ):
                if name in environment:
                    environment[name] = environment[name].replace(
                        "host=db ",
                        f"host={database_host} ",
                        1,
                    )
        environment.update(
            {
                "POSTGRESEM_MCP_HTTP_AUTHORITY_FILE": authority_path,
                "POSTGRESEM_MCP_HTTP_BIND": f"127.0.0.1:{PORT}",
            }
        )
        server = subprocess.Popen(
            ["postgresem", "mcp", "serve-http"],
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        wait_for_server()

        status, headers, payload = metadata_request()
        if status != 200:
            fail(f"metadata request failed: {status}: {payload!r}")
        assert_private(headers)
        metadata = json.loads(payload)
        if metadata != {
            "resource": RESOURCE,
            "authorization_servers": [ISSUER],
            "scopes_supported": ["postgresem.mutate", "postgresem.query"],
            "bearer_methods_supported": ["header"],
        }:
            fail(f"protected resource metadata is incorrect: {metadata}")

        if metadata_request("evil.example.test")[0] != 403:
            fail("invalid Host was accepted")
        if mcp_request(
            tenant_a,
            "ping",
            {},
            origin="https://evil.example.test",
        )[0] != 403:
            fail("invalid Origin was accepted")

        slow_status, slow_body = slow_body_request(tenant_a)
        if (
            slow_status != 408
            or slow_body.get("error", {}).get("data", {}).get("code")
            != "MCP_REQUEST_BODY_TIMEOUT"
        ):
            fail(f"slow request body was not rejected: {slow_status}: {slow_body}")

        status, headers, _ = mcp_request(None, "ping", {})
        if status != 401:
            fail(f"missing bearer token did not return 401: {status}")
        challenge = headers.get("www-authenticate", "")
        expected_metadata = (
            "https://mcp.example.test:8443/"
            ".well-known/oauth-protected-resource/mcp"
        )
        if expected_metadata not in challenge or 'scope="postgresem.query"' not in challenge:
            fail(f"bearer challenge is incomplete: {challenge!r}")

        invalid_tokens = [
            sign_token(
                private_key,
                "tenant-a-agent",
                "postgresem.query",
                aud="https://other.example.test/mcp",
            ),
            sign_token(
                private_key,
                "tenant-a-agent",
                "postgresem.query",
                header={"typ": "JWT"},
            ),
            sign_token(
                private_key,
                "tenant-a-agent",
                "postgresem.query",
                header={"kid": "unknown"},
            ),
            sign_token(
                private_key,
                "tenant-a-agent",
                "postgresem.query",
                header={"alg": "HS256"},
            ),
        ]
        now = int(time.time())
        invalid_tokens.extend(
            [
                sign_token(
                    private_key,
                    "tenant-a-agent",
                    "postgresem.query",
                    iss="https://other-issuer.example.test",
                ),
                sign_token(
                    private_key,
                    "tenant-a-agent",
                    "postgresem.query",
                    iat=now - 100,
                    nbf=now - 100,
                    exp=now - 10,
                ),
                sign_token(
                    private_key,
                    "tenant-a-agent",
                    "postgresem.query",
                    nbf=now + 100,
                    exp=now + 300,
                ),
                sign_token(
                    private_key,
                    "tenant-a-agent",
                    "postgresem.query",
                    iat=now - 1000,
                    nbf=now - 1000,
                    exp=now + 300,
                ),
            ]
        )
        signature_parts = tenant_a.split(".")
        signature_parts[2] = (
            ("A" if signature_parts[2][0] != "A" else "B")
            + signature_parts[2][1:]
        )
        invalid_tokens.append(".".join(signature_parts))
        for token in invalid_tokens:
            if mcp_request(token, "ping", {})[0] != 401:
                fail("invalid access token was accepted")
        unknown = sign_token(private_key, "unknown-agent", "postgresem.query")
        if mcp_request(unknown, "ping", {})[0] != 403:
            fail("unmapped verified subject was not denied")

        mismatch = mcp_request(
            tenant_a,
            "ping",
            {},
            mirrored_method="tools/list",
        )
        status, mismatch_body = json_payload(mismatch)
        if status != 400 or mismatch_body["error"]["data"]["code"] != "MCP_HEADER_MISMATCH":
            fail(f"header/body mismatch was accepted: {mismatch_body}")

        missing_name = mcp_request(
            tenant_a,
            "tools/call",
            {
                "name": "list_semantic_models",
                "arguments": {"schema_version": "1"},
            },
        )
        if json_payload(missing_name)[1]["error"]["data"]["code"] != "MCP_HEADER_MISMATCH":
            fail("missing Mcp-Name was accepted")
        wrong_name = mcp_request(
            tenant_a,
            "tools/call",
            {
                "name": "list_semantic_models",
                "arguments": {"schema_version": "1"},
            },
            name="describe_semantic_model",
        )
        if json_payload(wrong_name)[1]["error"]["data"]["code"] != "MCP_HEADER_MISMATCH":
            fail("mismatched Mcp-Name was accepted")

        unsupported = mcp_request(
            tenant_a,
            "ping",
            {},
            protocol="2025-03-26",
        )
        status, unsupported_body = json_payload(unsupported)
        if (
            status != 400
            or unsupported_body["error"]["data"]
            != {
                "code": "MCP_UNSUPPORTED_PROTOCOL_VERSION",
                "supported": [PROTOCOL],
                "requested": "2025-03-26",
            }
        ):
            fail(f"unsupported protocol was accepted: {unsupported_body}")

        ping_body = json.dumps(
            request_body("ping", {}, "invalid-http-headers"),
            separators=(",", ":"),
        ).encode("utf-8")
        base_headers = {
            "Host": HOST,
            "Authorization": f"Bearer {tenant_a}",
            "MCP-Protocol-Version": PROTOCOL,
            "Mcp-Method": "ping",
        }
        if request(
            "POST",
            "/mcp",
            {**base_headers, "Accept": "application/json", "Content-Type": "application/json"},
            ping_body,
        )[0] != 400:
            fail("incomplete Accept header was accepted")
        if request(
            "POST",
            "/mcp",
            {
                **base_headers,
                "Accept": "application/json, text/event-stream",
                "Content-Type": "text/plain",
            },
            ping_body,
        )[0] != 400:
            fail("non-JSON content type was accepted")
        notification = request_body("ping", {}, "notification")
        del notification["id"]
        notification_response = request(
            "POST",
            "/mcp",
            {
                **base_headers,
                "Accept": "application/json, text/event-stream",
                "Content-Type": "application/json",
            },
            json.dumps(notification, separators=(",", ":")).encode("utf-8"),
        )
        if (
            json_payload(notification_response)[1]["error"]["data"]["code"]
            != "MCP_NOTIFICATION_UNSUPPORTED"
        ):
            fail("HTTP notification was accepted")
        oversized = request(
            "POST",
            "/mcp",
            {
                **base_headers,
                "Accept": "application/json, text/event-stream",
                "Content-Type": "application/json",
            },
            b" " * 1048577,
        )
        if oversized[0] != 413:
            fail(f"oversized HTTP body was accepted: {oversized[0]}")

        unknown_method = mcp_request(tenant_a, "unknown/method", {})
        status, unknown_body = json_payload(unknown_method)
        if status != 404 or unknown_body["error"]["code"] != -32601:
            fail(f"unknown method was not rejected: {unknown_body}")

        discover = mcp_request(tenant_a, "server/discover", {})
        status, discover_body = json_payload(discover)
        if status != 200 or discover_body["result"]["resultType"] != "complete":
            fail(f"server discovery is incomplete: {discover_body}")
        if (
            discover_body["result"]["cacheScope"] != "private"
            or discover_body["result"]["supportedVersions"] != [PROTOCOL]
        ):
            fail(f"server discovery is not private/current: {discover_body}")
        assert_private(discover[1])

        tenant_a_tools = json_payload(mcp_request(tenant_a, "tools/list", {}))[1][
            "result"
        ]
        tenant_b_tools = json_payload(mcp_request(tenant_b, "tools/list", {}))[1][
            "result"
        ]
        names_a = {tool["name"] for tool in tenant_a_tools["tools"]}
        names_b = {tool["name"] for tool in tenant_b_tools["tools"]}
        if "mutate_semantic_model" in names_a or "reconcile_semantic_mutation" in names_a:
            fail(f"query-only principal received mutation capability: {names_a}")
        if not {"mutate_semantic_model", "reconcile_semantic_mutation"} <= names_b:
            fail(f"mutation principal did not receive mutation capability: {names_b}")
        tenant_b_query_only = sign_token(
            private_key, "tenant-b-agent", "postgresem.query"
        )
        scoped_tools = json_payload(
            mcp_request(tenant_b_query_only, "tools/list", {})
        )[1]["result"]["tools"]
        if any(
            tool["name"]
            in {"mutate_semantic_model", "reconcile_semantic_mutation"}
            for tool in scoped_tools
        ):
            fail("token without mutation scope received mutation capability")

        first_page = tool_result(
            call_tool(
                tenant_a,
                "list_semantic_models",
                {"schema_version": "1", "limit": 1},
                "tenant-a-page",
            )
        )
        foreign_cursor = tool_result(
            call_tool(
                tenant_b,
                "list_semantic_models",
                {
                    "schema_version": "1",
                    "limit": 1,
                    "cursor": first_page["next_cursor"],
                },
                "tenant-b-foreign-cursor",
            ),
            error=True,
        )
        if foreign_cursor["error"]["code"] != "MCP_INVALID_CURSOR":
            fail(f"cursor crossed authenticated authority: {foreign_cursor}")

        tenant_query = {
            "schema_version": "1",
            "lsq": {
                "schema_version": "1",
                "model": "tenant_orders",
                "metrics": [{"metric": "revenue"}],
            },
        }
        result_a = tool_result(
            call_tool(
                tenant_a,
                "query_semantic_model",
                tenant_query,
                "tenant-a-query",
            )
        )
        result_b = tool_result(
            call_tool(
                tenant_b,
                "query_semantic_model",
                tenant_query,
                "tenant-b-query",
            )
        )
        if result_a["rows"] != [["250.00"]] or result_b["rows"] != [["999.00"]]:
            fail(f"RLS identities did not receive distinct results: {result_a}, {result_b}")

        selected_role = dict(tenant_query)
        selected_role["database_role"] = "postgresem_tenant_b"
        selected = sse_payload(
            call_tool(
                tenant_a,
                "query_semantic_model",
                selected_role,
                "selected-role",
            )
        )
        if selected.get("error", {}).get("data", {}).get("code") != "MCP_INVALID_TOOL_ARGUMENTS":
            fail(f"request-selected role was not rejected: {selected}")

        query_only_mutation = tool_result(
            call_tool(
                tenant_a,
                "mutate_semantic_model",
                {
                    "schema_version": "1",
                    "lsm": {
                        "schema_version": "1",
                        "operation": "insert",
                        "model": "tenant_orders",
                        "idempotency_key": "mcp-http-query-only",
                        "rows": [
                            {
                                "external_id": {
                                    "type": "text",
                                    "value": "mcp-http-query-only",
                                },
                                "tenant_id": {
                                    "type": "text",
                                    "value": "tenant_a",
                                },
                                "amount": {"type": "numeric", "value": "1.00"},
                            }
                        ],
                    },
                },
                "query-only-mutation",
            ),
            error=True,
        )
        if query_only_mutation["error"]["code"] != "MUTATION_CAPABILITY_DISABLED":
            fail(f"query-only identity executed mutation: {query_only_mutation}")

        lsm = {
            "schema_version": "1",
            "operation": "insert",
            "model": "tenant_orders",
            "idempotency_key": "mcp-http-tenant-b",
            "rows": [
                {
                    "external_id": {
                        "type": "text",
                        "value": "mcp-http-tenant-b",
                    },
                    "tenant_id": {"type": "text", "value": "tenant_b"},
                    "amount": {"type": "numeric", "value": "28.50"},
                }
            ],
        }
        mutation = tool_result(
            call_tool(
                tenant_b,
                "mutate_semantic_model",
                {"schema_version": "1", "lsm": lsm},
                "tenant-b-mutation",
            )
        )
        replay = tool_result(
            call_tool(
                tenant_b,
                "mutate_semantic_model",
                {"schema_version": "1", "lsm": lsm},
                "tenant-b-replay",
            )
        )
        if (
            mutation["affected_rows"] != 1
            or mutation["replayed"]
            or not replay["replayed"]
            or replay["mutation_id"] != mutation["mutation_id"]
        ):
            fail(f"remote mutation replay is incorrect: {mutation}, {replay}")
        reconciled = tool_result(
            call_tool(
                tenant_b,
                "reconcile_semantic_mutation",
                {
                    "schema_version": "1",
                    "idempotency_key": "mcp-http-tenant-b",
                },
                "tenant-b-reconcile",
            )
        )
        if reconciled["state"]["mutation_id"] != mutation["mutation_id"]:
            fail(f"remote mutation reconciliation is incorrect: {reconciled}")
        isolated_reconcile = tool_result(
            call_tool(
                tenant_a,
                "reconcile_semantic_mutation",
                {
                    "schema_version": "1",
                    "idempotency_key": "mcp-http-tenant-b",
                },
                "tenant-a-reconcile",
            ),
            error=True,
        )
        if isolated_reconcile["error"]["code"] != "MUTATION_CAPABILITY_DISABLED":
            fail(f"query-only authority observed mutation state: {isolated_reconcile}")

        cross_tenant = dict(lsm)
        cross_tenant["idempotency_key"] = "mcp-http-cross-tenant"
        cross_tenant["rows"] = [
            {
                "external_id": {
                    "type": "text",
                    "value": "mcp-http-cross-tenant",
                },
                "tenant_id": {"type": "text", "value": "tenant_a"},
                "amount": {"type": "numeric", "value": "999.00"},
            }
        ]
        denied = tool_result(
            call_tool(
                tenant_b,
                "mutate_semantic_model",
                {"schema_version": "1", "lsm": cross_tenant},
                "cross-tenant-mutation",
            ),
            error=True,
        )
        if denied["error"]["code"] != "MUTATION_DATABASE_REJECTED":
            fail(f"PostgreSQL RLS did not deny cross-tenant mutation: {denied}")

        if mcp_request(rate_token, "ping", {})[0] != 200:
            fail("first rate-limited request was not admitted")
        rate_limited = mcp_request(rate_token, "ping", {})
        if rate_limited[0] != 429:
            fail(f"second rate-limited request was admitted: {rate_limited[0]}")

        lock_holder = subprocess.Popen(
            [
                "psql",
                "--no-psqlrc",
                "-qAt",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                (
                    "BEGIN; LOCK TABLE rls_fixture.orders IN ACCESS EXCLUSIVE MODE; "
                    "SELECT 'locked'; SELECT pg_sleep(30); COMMIT;"
                ),
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if lock_holder.stdout.readline().strip() != "locked":
            fail("failed to acquire cancellation test lock")
        connection, response = open_stream(
            tenant_b,
            "query_semantic_model",
            tenant_query,
            "cancelled-query",
        )
        concurrent = mcp_request(tenant_b, "ping", {})
        if concurrent[0] != 429:
            fail(f"per-principal concurrency limit was bypassed: {concurrent[0]}")
        response.close()
        connection.close()

        deadline = time.time() + 12
        cancelled = False
        while time.time() < deadline:
            status = psql(
                """
                SELECT status
                FROM semantic.query_audit
                WHERE config_profile = 'mcp-http'
                ORDER BY started_at DESC
                LIMIT 1
                """
            )
            if status == "cancelled":
                cancelled = True
                break
            time.sleep(0.25)
        if not cancelled:
            fail("HTTP disconnect did not cancel PostgreSQL or close query audit")
        lock_holder.terminate()
        lock_holder.wait(timeout=5)
        lock_holder = None

        if psql(
            """
            SELECT count(DISTINCT principal_subject_hash)
            FROM semantic.query_audit
            WHERE config_profile = 'mcp-http'
            """
        ) != "2":
            fail("authenticated identities were not separated in query audit")
        if (
            psql(
                """
                SELECT count(*)
                FROM semantic.query_audit
                WHERE row_to_json(query_audit)::text
                  LIKE '%tenant-a-agent%'
                   OR row_to_json(query_audit)::text
                  LIKE '%tenant-b-agent%'
                """
            )
            != "0"
        ):
            fail("raw OAuth subject leaked into query audit")
        expected_authority_hash = "sha256:" + hashlib.sha256(b"tenant-b").hexdigest()
        if (
            psql(
                """
                SELECT authority_hash
                FROM semantic.mutation_idempotency
                WHERE project = 'commerce'
                  AND idempotency_key_hash =
                    'sha256:b83b49a4ff8d54a4813413acb1555134c8b418de1591bf371cf97a6584aac094'
                """
            )
            != expected_authority_hash
        ):
            fail("remote mutation was not namespaced by stable authority ID")

        server.terminate()
        server.wait(timeout=5)
        server = None

        disabled_authority_path = os.path.join(directory, "authority-disabled.json")
        disabled_document = authority_document(jwks_path, hmac_path)
        disabled_document["remote_mutation_enabled"] = False
        with open(disabled_authority_path, "w", encoding="utf-8") as output:
            json.dump(disabled_document, output, separators=(",", ":"))
        environment["POSTGRESEM_MCP_HTTP_AUTHORITY_FILE"] = disabled_authority_path
        server = subprocess.Popen(
            ["postgresem", "mcp", "serve-http"],
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        wait_for_server()
        globally_disabled = json_payload(
            mcp_request(tenant_b, "tools/list", {})
        )[1]["result"]["tools"]
        if any(
            tool["name"]
            in {"mutate_semantic_model", "reconcile_semantic_mutation"}
            for tool in globally_disabled
        ):
            fail("global remote mutation gate did not withhold mutation tools")
        server.terminate()
        server.wait(timeout=5)
        server = None

    psql(
        """
        DELETE FROM rls_fixture.orders
        WHERE external_id LIKE 'mcp-http-%';
        TRUNCATE semantic.query_audit, semantic.mutation_audit,
          semantic.mutation_idempotency;
        """
    )
    print("authenticated MCP HTTP integration checks passed")


if __name__ == "__main__":
    main()
