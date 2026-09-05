#!/usr/bin/env python3
"""Exercise the postgresem MCP stdio developer-preview surface."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import selectors
import subprocess
import sys
import threading
from typing import Any


EXPECTED_TOOLS = [
    "list_semantic_models",
    "describe_semantic_model",
    "validate_semantic_query",
    "query_semantic_model",
    "explain_semantic_query",
    "validate_semantic_mutation",
    "mutate_semantic_model",
    "reconcile_semantic_mutation",
]


class SmokeFailure(RuntimeError):
    pass


class McpClient:
    def __init__(self, command: list[str], timeout: float) -> None:
        self.timeout = timeout
        self.next_id = 1
        try:
            self.process = subprocess.Popen(
                command,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
            )
        except OSError as error:
            raise SmokeFailure(f"could not launch {command!r}: {error}") from error
        if self.process.stdin is None or self.process.stdout is None or self.process.stderr is None:
            raise SmokeFailure("failed to create MCP stdio pipes")
        self._stderr_thread = threading.Thread(target=self._copy_stderr, daemon=True)
        self._stderr_thread.start()

    def _copy_stderr(self) -> None:
        assert self.process.stderr is not None
        for line in self.process.stderr:
            sys.stderr.write(f"[mcp stderr] {line}")
            sys.stderr.flush()

    def request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        message = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        }
        self._write(message)
        response = self._read()
        if response.get("id") != request_id:
            raise SmokeFailure(f"response ID mismatch for {method}: {response!r}")
        if "error" in response:
            error = response["error"]
            public = error.get("data", {}).get("code", "unknown")
            raise SmokeFailure(
                f"{method} failed: rpc={error.get('code')} public={public} "
                f"message={error.get('message')}"
            )
        result = response.get("result")
        if not isinstance(result, dict):
            raise SmokeFailure(f"{method} returned no object result")
        return result

    def notify(self, method: str, params: dict[str, Any]) -> None:
        self._write({"jsonrpc": "2.0", "method": method, "params": params})

    def _write(self, message: dict[str, Any]) -> None:
        assert self.process.stdin is not None
        if self.process.poll() is not None:
            raise SmokeFailure(f"MCP command exited early with {self.process.returncode}")
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()

    def _read(self) -> dict[str, Any]:
        assert self.process.stdout is not None
        selector = selectors.DefaultSelector()
        selector.register(self.process.stdout, selectors.EVENT_READ)
        try:
            if not selector.select(self.timeout):
                raise SmokeFailure(f"timed out after {self.timeout:g}s waiting for MCP response")
            line = self.process.stdout.readline()
        finally:
            selector.close()
        if not line:
            raise SmokeFailure(
                f"MCP stdout closed (exit={self.process.poll()}) before a response"
            )
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise SmokeFailure(f"non-JSON MCP stdout: {line!r}") from error
        if not isinstance(response, dict):
            raise SmokeFailure(f"MCP response is not an object: {response!r}")
        return response

    def close(self) -> None:
        if self.process.stdin is not None and not self.process.stdin.closed:
            self.process.stdin.close()
        try:
            return_code = self.process.wait(timeout=self.timeout)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=2)
            raise SmokeFailure("MCP command did not exit after stdin closed")
        self._stderr_thread.join(timeout=1)
        if return_code != 0:
            raise SmokeFailure(f"MCP command exited with status {return_code}")

    def abort(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=2)


def structured_tool(result: dict[str, Any], name: str) -> dict[str, Any]:
    if result.get("isError") is True:
        error = result.get("structuredContent", {}).get("error", {})
        raise SmokeFailure(
            f"{name} failed: {error.get('code', 'unknown')}: "
            f"{error.get('message', 'tool operation failed')}"
        )
    content = result.get("structuredContent")
    if not isinstance(content, dict):
        blocks = result.get("content")
        if not isinstance(blocks, list) or not blocks:
            raise SmokeFailure(f"{name} returned no structured or text content")
        try:
            content = json.loads(blocks[0]["text"])
        except (KeyError, TypeError, json.JSONDecodeError) as error:
            raise SmokeFailure(f"{name} returned invalid text content") from error
    return content


def hash_shape(value: Any) -> str:
    if isinstance(value, str) and value.startswith("sha256:") and len(value) == 71:
        return "sha256:<64 hex>"
    return "<missing>"


def call_tool(
    client: McpClient, name: str, arguments: dict[str, Any]
) -> dict[str, Any]:
    result = client.request("tools/call", {"name": name, "arguments": arguments})
    return structured_tool(result, name)


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(
        description="Run all postgresem MCP tools and resources over stdio."
    )
    parser.add_argument(
        "--lsq",
        type=Path,
        default=root / "examples/commerce/orders-revenue.json",
        help="LSQ JSON used for validate, explain, and query",
    )
    parser.add_argument(
        "--model", default="orders", help="model used for describe_semantic_model"
    )
    parser.add_argument(
        "--lsm",
        type=Path,
        default=root / "examples/commerce/order-insert.json",
        help="LSM JSON used for mutation validation and execution",
    )
    parser.add_argument(
        "--timeout", type=float, default=30.0, help="seconds per response/exit"
    )
    parser.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="MCP command and arguments, normally after --",
    )
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("provide an MCP command, for example: -- make mcp")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")

    try:
        lsq = json.loads(args.lsq.read_text())
    except (OSError, json.JSONDecodeError) as error:
        print(f"FAIL: could not read LSQ {args.lsq}: {error}", file=sys.stderr)
        return 2
    if not isinstance(lsq, dict):
        print(f"FAIL: LSQ {args.lsq} is not a JSON object", file=sys.stderr)
        return 2
    try:
        lsm = json.loads(args.lsm.read_text())
    except (OSError, json.JSONDecodeError) as error:
        print(f"FAIL: could not read LSM {args.lsm}: {error}", file=sys.stderr)
        return 2
    if not isinstance(lsm, dict):
        print(f"FAIL: LSM {args.lsm} is not a JSON object", file=sys.stderr)
        return 2

    client: McpClient | None = None
    try:
        client = McpClient(command, args.timeout)
        initialized = client.request(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "commerce-smoke", "version": "1"},
            },
        )
        protocol = initialized.get("protocolVersion")
        server = initialized.get("serverInfo", {})
        if protocol != "2024-11-05" or server.get("name") != "postgresem":
            raise SmokeFailure(f"unexpected initialize response: {initialized!r}")
        print(
            f"initialize: protocol={protocol} "
            f"server={server.get('name')}/{server.get('version')}"
        )
        client.notify("notifications/initialized", {})

        definitions = client.request("tools/list", {}).get("tools")
        if not isinstance(definitions, list):
            raise SmokeFailure("tools/list returned no tools array")
        names = [tool.get("name") for tool in definitions]
        if names != EXPECTED_TOOLS:
            raise SmokeFailure(f"unexpected tool list: {names!r}")
        print(f"tools/list: {len(names)} tools ({', '.join(names)})")

        listed = call_tool(
            client, "list_semantic_models", {"schema_version": "1"}
        )
        models = [model.get("name") for model in listed.get("models", [])]
        print(
            "list_semantic_models: "
            f"models=[{', '.join(str(model) for model in models)}] "
            f"revision={hash_shape(listed.get('semantic_revision'))}"
        )

        described = call_tool(
            client,
            "describe_semantic_model",
            {"schema_version": "1", "model": args.model},
        )
        model = described.get("model", {})
        print(
            "describe_semantic_model: "
            f"model={model.get('name')} fields={len(model.get('fields', []))} "
            f"metrics={len(model.get('metrics', []))}"
        )

        tool_arguments = {"schema_version": "1", "lsq": lsq}
        validated = call_tool(client, "validate_semantic_query", tool_arguments)
        if validated.get("valid") is not True:
            error = validated.get("error", {})
            raise SmokeFailure(
                f"validation rejected LSQ: {error.get('code')}: {error.get('message')}"
            )
        print(
            "validate_semantic_query: "
            f"valid=True hash={hash_shape(validated.get('normalized_lsq_hash'))}"
        )

        explained = call_tool(client, "explain_semantic_query", tool_arguments)
        explained_models = explained.get("semantic_models", [])
        print(
            "explain_semantic_query: "
            f"models=[{', '.join(str(model) for model in explained_models)}] "
            f"effective_limit={explained.get('limits', {}).get('effective')}"
        )

        queried = call_tool(client, "query_semantic_model", tool_arguments)
        columns = [
            f"{column.get('name')}:{column.get('type')}"
            for column in queried.get("columns", [])
        ]
        print(
            "query_semantic_model: "
            f"columns=[{', '.join(columns)}] rows={len(queried.get('rows', []))} "
            f"truncated={queried.get('truncated')}"
        )

        mutation_arguments = {"schema_version": "1", "lsm": lsm}
        validated_mutation = call_tool(
            client, "validate_semantic_mutation", mutation_arguments
        )
        if validated_mutation.get("valid") is not True:
            error = validated_mutation.get("error", {})
            raise SmokeFailure(
                f"mutation validation rejected LSM: "
                f"{error.get('code')}: {error.get('message')}"
            )
        print(
            "validate_semantic_mutation: "
            f"valid=True operation={validated_mutation.get('operation')} "
            f"rows={validated_mutation.get('expected_rows')}"
        )

        mutated = call_tool(client, "mutate_semantic_model", mutation_arguments)
        print(
            "mutate_semantic_model: "
            f"rows={mutated.get('affected_rows')} replayed={mutated.get('replayed')}"
        )

        resources = client.request("resources/list", {}).get("resources")
        if not isinstance(resources, list) or not resources:
            raise SmokeFailure("resources/list returned no resources")
        print(f"resources/list: {len(resources)} resources")
        read_count = 0
        for resource in resources:
            uri = resource.get("uri")
            if not isinstance(uri, str):
                raise SmokeFailure(f"resource has no URI: {resource!r}")
            result = client.request("resources/read", {"uri": uri})
            contents = result.get("contents")
            if not isinstance(contents, list) or not contents:
                raise SmokeFailure(f"resource {uri} returned no contents")
            text = contents[0].get("text")
            if not isinstance(text, str):
                raise SmokeFailure(f"resource {uri} returned no text")
            try:
                json.loads(text)
            except json.JSONDecodeError as error:
                raise SmokeFailure(f"resource {uri} text is not JSON") from error
            read_count += 1
        print(f"resources/read: {read_count}/{len(resources)} resources")

        client.close()
        client = None
        print("PASS: MCP stdio commerce smoke completed")
        return 0
    except (SmokeFailure, BrokenPipeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        if client is not None:
            client.abort()
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
