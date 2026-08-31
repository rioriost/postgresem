#!/usr/bin/env python3
"""Loopback-only Web demo backed by the postgresem MCP stdio contract."""

from __future__ import annotations

import argparse
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys
import threading
from typing import Any

COMMERCE_DIR = Path(__file__).resolve().parents[1] / "commerce"
sys.path.insert(0, str(COMMERCE_DIR))

from mcp_smoke import McpClient, SmokeFailure, call_tool  # noqa: E402


MAX_REQUEST_BYTES = 16_384
LOOPBACK_HOST = "127.0.0.1"
EXAMPLES = {
    "orders-revenue": {
        "title": "Paid order revenue",
        "description": "One audited revenue metric over paid commerce orders.",
        "model": "orders",
        "path": COMMERCE_DIR / "orders-revenue.json",
    },
    "revenue-by-month": {
        "title": "Revenue by month",
        "description": "A typed time-grain query with a date filter.",
        "model": "orders",
        "path": COMMERCE_DIR / "revenue-by-month.json",
    },
    "revenue-by-region": {
        "title": "Revenue by region",
        "description": "A safe many-to-one relationship dimension.",
        "model": "orders",
        "path": COMMERCE_DIR / "revenue-by-region.json",
    },
    "active-subscriptions": {
        "title": "Active subscription MRR",
        "description": "Recurring revenue grouped by subscription plan.",
        "model": "subscriptions",
        "path": COMMERCE_DIR / "active-subscriptions.json",
    },
}


class DemoError(RuntimeError):
    def __init__(self, status: HTTPStatus, code: str, message: str) -> None:
        super().__init__(message)
        self.status = status
        self.code = code
        self.message = message


class DemoApplication:
    def __init__(self, client: McpClient) -> None:
        self.client = client
        self.lock = threading.Lock()
        self.failed = False

    def bootstrap(self) -> dict[str, Any]:
        with self.lock:
            self._require_connection()
            listed = call_tool(
                self.client,
                "list_semantic_models",
                {"schema_version": "1", "limit": 100},
            )
        return {
            "schema_version": "1",
            "server": "postgresem",
            "semantic_revision": listed.get("semantic_revision"),
            "models": listed.get("models", []),
            "examples": [
                {
                    "id": example_id,
                    "title": definition["title"],
                    "description": definition["description"],
                    "model": definition["model"],
                }
                for example_id, definition in EXAMPLES.items()
            ],
        }

    def run_example(self, payload: Any) -> dict[str, Any]:
        if not isinstance(payload, dict) or set(payload) != {"example"}:
            raise DemoError(
                HTTPStatus.BAD_REQUEST,
                "DEMO_INVALID_REQUEST",
                "request must contain only the example field",
            )
        example_id = payload["example"]
        if not isinstance(example_id, str) or example_id not in EXAMPLES:
            raise DemoError(
                HTTPStatus.BAD_REQUEST,
                "DEMO_EXAMPLE_NOT_AVAILABLE",
                "requested example is not available",
            )
        try:
            lsq = json.loads(EXAMPLES[example_id]["path"].read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise DemoError(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                "DEMO_EXAMPLE_UNAVAILABLE",
                "example query is unavailable",
            ) from error

        arguments = {"schema_version": "1", "lsq": lsq}
        with self.lock:
            self._require_connection()
            validation = call_tool(
                self.client, "validate_semantic_query", arguments
            )
            if validation.get("valid") is not True:
                return {
                    "schema_version": "1",
                    "example": example_id,
                    "validation": validation,
                    "explanation": None,
                    "result": None,
                }
            explanation = call_tool(
                self.client, "explain_semantic_query", arguments
            )
            result = call_tool(self.client, "query_semantic_model", arguments)
        return {
            "schema_version": "1",
            "example": example_id,
            "validation": validation,
            "explanation": explanation,
            "result": result,
        }

    def fail_connection(self) -> None:
        with self.lock:
            self.failed = True
            abort = getattr(self.client, "abort", None)
            if callable(abort):
                abort()

    def _require_connection(self) -> None:
        if self.failed:
            raise SmokeFailure(
                "MCP connection is unavailable; restart the Web demo"
            )


class DemoServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(
        self,
        server_address: tuple[str, int],
        application: DemoApplication,
        static_dir: Path,
    ) -> None:
        super().__init__(server_address, DemoRequestHandler)
        self.application = application
        self.static_dir = static_dir


class DemoRequestHandler(BaseHTTPRequestHandler):
    server: DemoServer

    def do_GET(self) -> None:
        if not self._valid_host():
            return
        if self.path == "/":
            self._send_file("index.html", "text/html; charset=utf-8")
        elif self.path == "/app.js":
            self._send_file("app.js", "text/javascript; charset=utf-8")
        elif self.path == "/styles.css":
            self._send_file("styles.css", "text/css; charset=utf-8")
        elif self.path == "/healthz":
            self._send_json(HTTPStatus.OK, {"status": "ok"})
        elif self.path == "/api/bootstrap":
            self._run_json(self.server.application.bootstrap)
        else:
            self._send_error(
                HTTPStatus.NOT_FOUND, "DEMO_NOT_FOUND", "resource not found"
            )

    def do_POST(self) -> None:
        if not self._valid_host():
            return
        if self.path != "/api/run":
            self._send_error(
                HTTPStatus.NOT_FOUND, "DEMO_NOT_FOUND", "resource not found"
            )
            return
        content_type = self.headers.get("Content-Type", "")
        if content_type.split(";", 1)[0].strip().lower() != "application/json":
            self._send_error(
                HTTPStatus.UNSUPPORTED_MEDIA_TYPE,
                "DEMO_CONTENT_TYPE_REQUIRED",
                "Content-Type must be application/json",
            )
            return
        raw_length = self.headers.get("Content-Length")
        try:
            length = int(raw_length) if raw_length is not None else -1
        except ValueError:
            length = -1
        if not 0 <= length <= MAX_REQUEST_BYTES:
            self._send_error(
                HTTPStatus.REQUEST_ENTITY_TOO_LARGE,
                "DEMO_REQUEST_TOO_LARGE",
                "request body is missing or too large",
            )
            return
        body = self.rfile.read(length)
        try:
            payload = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            self._send_error(
                HTTPStatus.BAD_REQUEST,
                "DEMO_INVALID_JSON",
                "request body must be valid JSON",
            )
            return
        self._run_json(lambda: self.server.application.run_example(payload))

    def log_message(self, format: str, *args: object) -> None:
        sys.stderr.write(
            f"[web demo] {self.address_string()} {format % args}\n"
        )

    def _run_json(self, operation: Any) -> None:
        try:
            self._send_json(HTTPStatus.OK, operation())
        except DemoError as error:
            self._send_error(error.status, error.code, error.message)
        except SmokeFailure:
            self.server.application.fail_connection()
            self._send_error(
                HTTPStatus.BAD_GATEWAY,
                "DEMO_GATEWAY_UNAVAILABLE",
                "postgresem MCP operation failed; restart the Web demo",
            )

    def _valid_host(self) -> bool:
        hosts = self.headers.get_all("Host", failobj=[])
        expected = f"{LOOPBACK_HOST}:{self.server.server_port}"
        if len(hosts) == 1 and hosts[0].strip().lower() == expected:
            return True
        self._send_error(
            HTTPStatus.MISDIRECTED_REQUEST,
            "DEMO_HOST_REJECTED",
            "request Host is not the configured loopback endpoint",
        )
        return False

    def _send_file(self, name: str, content_type: str) -> None:
        try:
            content = (self.server.static_dir / name).read_bytes()
        except OSError:
            self._send_error(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                "DEMO_ASSET_UNAVAILABLE",
                "demo asset is unavailable",
            )
            return
        self._send(HTTPStatus.OK, content_type, content)

    def _send_json(self, status: HTTPStatus, value: Any) -> None:
        content = json.dumps(
            value, ensure_ascii=True, separators=(",", ":")
        ).encode("utf-8")
        self._send(status, "application/json; charset=utf-8", content)

    def _send_error(
        self, status: HTTPStatus, code: str, message: str
    ) -> None:
        self._send_json(
            status,
            {
                "error": {
                    "code": code,
                    "message": message,
                }
            },
        )

    def _send(
        self, status: HTTPStatus, content_type: str, content: bytes
    ) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(content)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; "
            "connect-src 'self'; img-src 'self' data:; object-src 'none'; "
            "base-uri 'none'; frame-ancestors 'none'",
        )
        self.end_headers()
        self.wfile.write(content)


def initialize_client(client: McpClient) -> None:
    result = client.request(
        "initialize",
        {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "postgresem-web-demo", "version": "1"},
        },
    )
    if (
        result.get("protocolVersion") != "2024-11-05"
        or result.get("serverInfo", {}).get("name") != "postgresem"
    ):
        raise SmokeFailure("MCP server returned an unexpected initialize response")
    client.notify("notifications/initialized", {})


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Serve the loopback-only Postgresem commerce Web demo."
    )
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="MCP command and arguments, normally after --",
    )
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("provide an MCP command, for example: -- make mcp")
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")

    client: McpClient | None = None
    server: DemoServer | None = None
    try:
        client = McpClient(command, args.timeout)
        initialize_client(client)
        server = DemoServer(
            (LOOPBACK_HOST, args.port),
            DemoApplication(client),
            Path(__file__).resolve().parent / "static",
        )
        print(
            f"Postgresem Web demo: "
            f"http://{LOOPBACK_HOST}:{server.server_port}"
        )
        print("Local demonstration only; remote binding is intentionally disabled.")
        server.serve_forever()
    except (OSError, SmokeFailure) as error:
        print(f"web demo failed: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0
    finally:
        if server is not None:
            server.server_close()
        if client is not None:
            try:
                client.close()
            except SmokeFailure as error:
                print(f"web demo shutdown failed: {error}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
