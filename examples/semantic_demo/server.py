#!/usr/bin/env python3
"""Loopback-only Meaning Lab UI backed by real PostgreSQL and MCP."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys

from application import DemoApplication, DemoError
from planner import Planner, PlannerFailure, load_settings
from runtime import ContainerRuntime, DatabaseProbe, ROOT, ROLES
from smoke import SmokeFailure, unique_object

MAX_REQUEST_BYTES = 16_384
LOOPBACK_HOST = "127.0.0.1"
STATIC_DIR = Path(__file__).resolve().parent / "static"


class DemoServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, application, static_dir=STATIC_DIR):
        super().__init__(address, DemoRequestHandler)
        self.application, self.static_dir = application, static_dir

    def handle_error(self, request, client_address):
        sys.stderr.write("[meaning lab] unexpected HTTP handler failure; request aborted\n")


class DemoRequestHandler(BaseHTTPRequestHandler):
    def setup(self):
        super().setup()
        self.connection.settimeout(10)

    def do_GET(self):
        if not self._valid_host():
            return
        files = {
            "/": ("index.html", "text/html; charset=utf-8"),
            "/app.js": ("app.js", "text/javascript; charset=utf-8"),
            "/styles.css": ("styles.css", "text/css; charset=utf-8"),
        }
        if self.path in files:
            name, mime = files[self.path]
            try:
                data = (self.server.static_dir / name).read_bytes()
            except OSError:
                self._error(500, "DEMO_ASSET_UNAVAILABLE", "demo asset is unavailable")
                return
            self._send(200, mime, data)
        elif self.path == "/healthz":
            self._json(200, {"status": "failed" if self.server.application.failed else "ok"})
        elif self.path == "/api/bootstrap":
            self._run(self.server.application.bootstrap)
        else:
            self._error(404, "DEMO_NOT_FOUND", "resource not found")

    def do_POST(self):
        if not self._valid_host():
            return
        actions = {
            "/api/compare": self.server.application.compare,
            "/api/ingest": self.server.application.ingest,
            "/api/guards": self.server.application.guards,
        }
        if self.path not in actions:
            self._error(404, "DEMO_NOT_FOUND", "resource not found")
            return
        origins = self.headers.get_all("Origin", [])
        if origins and origins != [f"http://{LOOPBACK_HOST}:{self.server.server_port}"]:
            self._error(403, "DEMO_ORIGIN_REJECTED", "cross-origin actions are not allowed")
            return
        if self.headers.get("Content-Type", "").split(";")[0].strip().lower() != "application/json":
            self._error(415, "DEMO_CONTENT_TYPE_REQUIRED", "Content-Type must be application/json")
            return
        lengths = self.headers.get_all("Content-Length", [])
        if len(lengths) != 1 or not lengths[0].isascii() or not lengths[0].isdigit() or self.headers.get_all("Transfer-Encoding"):
            self._error(400, "DEMO_INVALID_LENGTH", "one Content-Length is required")
            return
        if len(lengths[0]) > 8 or not 0 < int(lengths[0]) <= MAX_REQUEST_BYTES:
            self._error(413, "DEMO_REQUEST_TOO_LARGE", "request body is missing or too large")
            return
        try:
            body = self.rfile.read(int(lengths[0]))
            if len(body) != int(lengths[0]):
                raise ValueError("incomplete request body")
            payload = json.loads(body.decode("utf-8"), object_pairs_hook=unique_object)
        except TimeoutError:
            self._error(408, "DEMO_REQUEST_TIMEOUT", "request body did not arrive in time")
            return
        except (UnicodeDecodeError, ValueError, RecursionError):
            self._error(400, "DEMO_INVALID_JSON", "request body must be unambiguous JSON")
            return
        self._run(lambda: actions[self.path](payload))

    def _valid_host(self):
        hosts = self.headers.get_all("Host", [])
        if hosts == [f"{LOOPBACK_HOST}:{self.server.server_port}"]:
            return True
        self._error(421, "DEMO_HOST_REJECTED", "Host must match the configured loopback endpoint")
        return False

    def _run(self, operation):
        try:
            result = operation()
        except DemoError as error:
            self._error(error.status, error.code, error.message)
        except PlannerFailure as error:
            self._error(502, "DEMO_PLANNER_FAILED", str(error))
        except SmokeFailure:
            self.server.application.fail_connection()
            print("Meaning Lab: a database/MCP operation failed; restart required", file=sys.stderr)
            self._error(502, "DEMO_GATEWAY_UNAVAILABLE",
                        "database/MCP operation failed; restart the demo. A write may have committed; retry only the same idempotency key")
        else:
            try:
                self._json(200, result)
            except (BrokenPipeError, ConnectionResetError):
                sys.stderr.write("[meaning lab] client disconnected; completed operation outcome is retained\n")

    def _json(self, status, value):
        self._send(status, "application/json; charset=utf-8",
                   json.dumps(value, ensure_ascii=True, allow_nan=False).encode())

    def _error(self, status, code, message):
        self._json(status, {"error": {"code": code, "message": message}})

    def _send(self, status, mime, data):
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy",
                         "default-src 'self'; script-src 'self'; style-src 'self'; "
                         "connect-src 'self'; img-src 'self'; object-src 'none'; "
                         "base-uri 'none'; frame-ancestors 'none'; form-action 'none'")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, format, *args):
        # Do not log user-controlled URLs, bodies, upstream messages or credentials.
        sys.stderr.write("[meaning lab] HTTP request completed\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", choices=("auto", "apple", "docker", "podman"), default="auto")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--no-start", action="store_true", help="use an already running fixture stack")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    clients, server, application = {}, None, None
    try:
        runtime = ContainerRuntime(args.runtime)
        planner = Planner(load_settings(ROOT / ".env"))
        if not args.no_start:
            runtime.start()
        for profile in ROLES:
            clients[profile] = runtime.client(profile)
        probe = DatabaseProbe(runtime)
        probe.snapshot()
        application = DemoApplication(clients, probe, planner)
        application.bootstrap()
        server = DemoServer((LOOPBACK_HOST, args.port), application)
        print(f"Meaning Lab: http://{LOOPBACK_HOST}:{args.port}", flush=True)
        print("Fictional local fixture only. Writes persist. Remote binding is disabled.", flush=True)
        server.serve_forever()
    except (OSError, SmokeFailure):
        print("Meaning Lab could not start; check the local fixture stack and configuration", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0
    finally:
        if server is not None:
            server.server_close()
        if application is not None:
            application.lock.acquire()
        try:
            for client in clients.values():
                client.abort()
        finally:
            if application is not None:
                application.lock.release()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
