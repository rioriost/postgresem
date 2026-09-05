"""Real loopback HTTP parsing tests with fake application dependencies."""

import http.client
import json
import socket
import threading
import unittest
from unittest.mock import Mock, patch

from application import DemoApplication, DemoError
from planner import PlannerFailure
from runtime import ROLES
from server import DemoRequestHandler, DemoServer, MAX_REQUEST_BYTES
from smoke import SmokeFailure


class ServerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.log_patch = patch.object(DemoRequestHandler, "log_message")
        cls.log_patch.start()
        cls.addClassCleanup(cls.log_patch.stop)
        cls.server = DemoServer(("127.0.0.1", 0), Mock())
        cls.server.handle_error = Mock()
        cls.thread = threading.Thread(
            target=cls.server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True,
        )
        cls.thread.start()
        cls.addClassCleanup(cls.stop_server)
        cls.host = f"127.0.0.1:{cls.server.server_port}"
        cls.origin = "http://" + cls.host

    @classmethod
    def stop_server(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)
        if cls.thread.is_alive():
            raise AssertionError("unit HTTP server failed to stop")

    def setUp(self):
        self.application = Mock()
        self.application.failed = False
        self.application.bootstrap.return_value = {"schema_version": "1", "models": []}
        self.application.compare.return_value = {"comparison": "unit"}
        self.application.ingest.return_value = {"consistent": True}
        self.application.guards.return_value = {"passed": True}

        def fail_connection():
            self.application.failed = True

        self.application.fail_connection.side_effect = fail_connection
        self.server.application = self.application
        self.server.handle_error.reset_mock()

    def request(self, method="GET", path="/healthz", body=b"", headers=None):
        if headers is None:
            headers = [("Host", self.host)]
            if method == "POST":
                headers += [("Content-Type", "application/json"), ("Content-Length", str(len(body)))]
        request = (
            f"{method} {path} HTTP/1.1\r\n"
            + "".join(f"{name}: {value}\r\n" for name, value in headers)
            + "\r\n"
        ).encode("ascii") + body
        with socket.create_connection(("127.0.0.1", self.server.server_port), timeout=3) as connection:
            connection.sendall(request)
            connection.shutdown(socket.SHUT_WR)
            response = http.client.HTTPResponse(connection)
            response.begin()
            data = response.read()
            result = (response.status, dict(response.getheaders()), data)
            response.close()
            return result

    def post(self, payload, path="/api/compare", extra_headers=()):
        body = json.dumps(payload).encode("utf-8")
        return self.request("POST", path, body, [
            ("Host", self.host), ("Content-Type", "application/json"),
            ("Content-Length", str(len(body))), *extra_headers,
        ])

    def assert_error(self, response, status, code):
        actual_status, headers, data = response
        self.assertEqual(actual_status, status, data)
        self.assertEqual(headers["Content-Type"], "application/json; charset=utf-8")
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
        payload = json.loads(data)
        self.assertEqual(payload["error"]["code"], code)
        self.assertIsInstance(payload["error"]["message"], str)
        return payload

    def assert_no_application_calls(self):
        for method in ("bootstrap", "compare", "ingest", "guards", "fail_connection"):
            getattr(self.application, method).assert_not_called()

    def test_server_is_responsive_health_and_bootstrap_are_json(self):
        self.assertTrue(self.thread.is_alive())
        status, headers, body = self.request()
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), {"status": "ok"})
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assert_no_application_calls()
        status, _, body = self.request(path="/api/bootstrap")
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body), {"schema_version": "1", "models": []})
        self.application.bootstrap.assert_called_once_with()

    def test_assets_are_same_origin_nostore_and_have_strict_csp(self):
        for path, content_type in (
            ("/", "text/html; charset=utf-8"),
            ("/app.js", "text/javascript; charset=utf-8"),
            ("/styles.css", "text/css; charset=utf-8"),
        ):
            with self.subTest(path=path):
                status, headers, body = self.request(path=path)
                self.assertEqual(status, 200)
                self.assertEqual(headers["Content-Type"], content_type)
                self.assertEqual(int(headers["Content-Length"]), len(body))
                self.assertEqual(headers["Cache-Control"], "no-store")
                self.assertEqual(headers["Referrer-Policy"], "no-referrer")
                self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
                csp = headers["Content-Security-Policy"]
                for directive in (
                    "default-src 'self'", "script-src 'self'", "style-src 'self'",
                    "connect-src 'self'", "object-src 'none'", "base-uri 'none'",
                    "frame-ancestors 'none'", "form-action 'none'",
                ):
                    self.assertIn(directive, csp)
                self.assertNotIn("'unsafe-inline'", csp)
                self.assertNotIn("'unsafe-eval'", csp)
                self.assertNotIn("Access-Control-Allow-Origin", headers)
        self.assert_no_application_calls()

    def test_asset_read_failure_is_sanitized(self):
        with patch("server.Path.read_bytes", side_effect=OSError("private/path/secret")):
            response = self.request(path="/app.js")
        self.assert_error(response, 500, "DEMO_ASSET_UNAVAILABLE")
        self.assertNotIn(b"private", response[2])

    def test_missing_duplicate_nonloopback_or_wrong_port_host_rejected(self):
        for host_headers in (
            [], [("Host", "localhost:" + str(self.server.server_port))],
            [("Host", "127.0.0.1:1")], [("Host", "attacker.invalid")],
            [("Host", self.host), ("Host", self.host)],
            [("Host", self.host), ("Host", "attacker.invalid")],
        ):
            for method, path in (("GET", "/api/bootstrap"), ("POST", "/api/guards")):
                with self.subTest(headers=host_headers, method=method):
                    headers = host_headers + [
                        ("Content-Type", "application/json"), ("Content-Length", "2"),
                    ]
                    self.assert_error(self.request(method, path, b"{}", headers),
                                      421, "DEMO_HOST_REJECTED")
        self.assert_no_application_calls()

    def test_cross_origin_null_and_duplicate_origins_rejected_before_application(self):
        for origins in (
            ["http://attacker.invalid"], ["null"], ["https://" + self.host],
            ["http://localhost:" + str(self.server.server_port)],
            [self.origin, self.origin], [self.origin, "http://attacker.invalid"],
        ):
            with self.subTest(origins=origins):
                response = self.post({}, "/api/guards", [("Origin", origin) for origin in origins])
                self.assert_error(response, 403, "DEMO_ORIGIN_REJECTED")
        self.assert_no_application_calls()

    def test_matching_or_absent_origin_dispatches_documented_actions(self):
        for origin_headers in ([], [("Origin", self.origin)]):
            for path, method, payload in (
                ("/api/compare", "compare", {"scenario": "recognized-revenue", "mode": "deterministic"}),
                ("/api/ingest", "ingest", {"action": "record-paid-order"}),
                ("/api/guards", "guards", {}),
            ):
                with self.subTest(path=path, headers=origin_headers):
                    status, _, body = self.post(payload, path, origin_headers)
                    self.assertEqual(status, 200, body)
                    getattr(self.application, method).assert_called_with(payload)

    def test_json_content_type_required(self):
        for content_type in (None, "", "text/plain", "application/x-www-form-urlencoded", "text/json"):
            with self.subTest(content_type=content_type):
                headers = [("Host", self.host), ("Content-Length", "2")]
                if content_type is not None:
                    headers.append(("Content-Type", content_type))
                self.assert_error(self.request("POST", "/api/guards", b"{}", headers),
                                  415, "DEMO_CONTENT_TYPE_REQUIRED")
        self.assert_no_application_calls()

    def test_json_content_type_with_charset_is_accepted(self):
        headers = [
            ("Host", self.host), ("Content-Length", "2"),
            ("Content-Type", "Application/JSON; charset=utf-8"),
        ]
        self.assertEqual(self.request("POST", "/api/guards", b"{}", headers)[0], 200)
        self.application.guards.assert_called_once_with({})

    def test_missing_duplicate_or_invalid_content_length_rejected(self):
        lengths = [[], ["2", "2"], ["2", "3"], [""], ["-1"], ["+2"], ["2, 2"], ["1.5"], ["NaN"]]
        for values in lengths:
            with self.subTest(values=values):
                headers = [("Host", self.host), ("Content-Type", "application/json")]
                headers.extend(("Content-Length", value) for value in values)
                self.assert_error(self.request("POST", "/api/guards", b"{}", headers),
                                  400, "DEMO_INVALID_LENGTH")
        self.assert_no_application_calls()

    def test_zero_oversized_and_huge_length_rejected_without_reading_body(self):
        for length in ("0", str(MAX_REQUEST_BYTES + 1), "9" * 100, "000000002"):
            with self.subTest(length=length):
                headers = [
                    ("Host", self.host), ("Content-Type", "application/json"),
                    ("Content-Length", length),
                ]
                self.assert_error(self.request("POST", "/api/guards", b"", headers),
                                  413, "DEMO_REQUEST_TOO_LARGE")
        self.assert_no_application_calls()

    def test_transfer_encoding_never_accepted_even_alongside_length(self):
        for encoding in ("chunked", "identity", ""):
            for with_length in (True, False):
                with self.subTest(encoding=encoding, length=with_length):
                    headers = [
                        ("Host", self.host), ("Content-Type", "application/json"),
                        ("Transfer-Encoding", encoding),
                    ]
                    if with_length:
                        headers.append(("Content-Length", "2"))
                    self.assert_error(self.request("POST", "/api/guards", b"{}", headers),
                                      400, "DEMO_INVALID_LENGTH")
        self.assert_no_application_calls()

    def test_maximum_sized_json_body_is_accepted(self):
        body = b"{}" + b" " * (MAX_REQUEST_BYTES - 2)
        self.assertEqual(self.request("POST", "/api/guards", body)[0], 200)
        self.application.guards.assert_called_once_with({})

    def test_invalid_utf8_json_and_duplicate_keys_rejected(self):
        bodies = [
            b"\xff", b'{"key":"\xff"}', b'{"unfinished":', b"{} trailing", b"{",
            b'{"mode":"deterministic","mode":"planner"}',
            b'{"outer":{"duplicate":1,"duplicate":2}}',
            b'{"choice":"A","cho\\u0069ce":"B"}',
        ]
        for body in bodies:
            with self.subTest(body=body[:70]):
                self.assert_error(self.request("POST", "/api/compare", body),
                                  400, "DEMO_INVALID_JSON")
        self.assert_no_application_calls()

    def test_json_recursion_failure_is_a_sanitized_client_error(self):
        with patch("server.json.loads", side_effect=RecursionError("private-decoder-details")):
            response = self.request("POST", "/api/guards", b"{}")
        self.assert_error(response, 400, "DEMO_INVALID_JSON")
        self.assertNotIn(b"private", response[2])
        self.assert_no_application_calls()

    def test_unknown_routes_queries_and_traversal_are_not_dispatched(self):
        for method, path in (
            ("GET", "/unknown"), ("GET", "/api/compare"),
            ("GET", "/api/bootstrap?sql=SELECT%201"), ("GET", "/../application.py"),
            ("GET", "/%2e%2e/application.py"), ("GET", "/.env"),
            ("POST", "/api/query"), ("POST", "/api/compare?role=postgres"),
            ("POST", "/api/bootstrap"), ("POST", "/"),
        ):
            with self.subTest(method=method, path=path):
                self.assert_error(self.request(method, path, b"{}"), 404, "DEMO_NOT_FOUND")
        self.assert_no_application_calls()

    def test_unknown_fields_raw_sql_and_roles_fail_without_db_or_model_calls(self):
        clients = {profile: Mock() for profile in ROLES}
        probe, planner = Mock(), Mock(enabled=False)
        self.server.application = DemoApplication(clients, probe, planner)
        valid = {"scenario": "recognized-revenue", "mode": "deterministic"}
        payloads = [
            ("/api/compare", {**valid, field: value})
            for field, value in (("sql", "SELECT 1"), ("raw_sql", "SELECT 1"),
                                 ("role", "postgres"), ("model", "secret"), ("lsq", {}))
        ]
        payloads += [
            ("/api/compare", None), ("/api/compare", []),
            ("/api/compare", {**valid, "scenario": {}}),
            ("/api/compare", {**valid, "mode": "anything"}),
            ("/api/ingest", {"action": "record-paid-order", "sql": "DELETE FROM orders"}),
            ("/api/ingest", {"action": "record-paid-order", "role": "postgres"}),
            ("/api/guards", {"role": "postgres"}),
        ]
        for path, payload in payloads:
            with self.subTest(path=path, payload=payload):
                self.assert_error(self.post(payload, path), 400, "DEMO_INVALID_REQUEST")
        self.assert_error(self.post({**valid, "mode": "planner"}), 409, "DEMO_PLANNER_DISABLED")
        self.assertEqual(probe.mock_calls, [])
        self.assertEqual(planner.mock_calls, [])
        for client in clients.values():
            self.assertEqual(client.mock_calls, [])

    def test_application_conflict_preserves_status_and_does_not_poison_connection(self):
        self.application.compare.side_effect = DemoError(
            409, "DEMO_SOURCE_CHANGED", "data or publication changed",
        )
        self.assert_error(self.post({}), 409, "DEMO_SOURCE_CHANGED")
        self.application.fail_connection.assert_not_called()
        self.assertFalse(self.application.failed)

    def test_planner_failure_is_recoverable_not_a_fatal_mcp_connection_error(self):
        self.application.compare.side_effect = PlannerFailure("No supported plan; nothing executed.")
        self.assert_error(self.post({}), 502, "DEMO_PLANNER_FAILED")
        self.application.fail_connection.assert_not_called()
        self.assertFalse(self.application.failed)
        self.application.compare.side_effect = None
        self.assertEqual(self.post({})[0], 200)
        self.assertEqual(json.loads(self.request()[2]), {"status": "ok"})

    def test_browser_disconnect_after_success_does_not_poison_mcp_or_repeat_operation(self):
        for error in (BrokenPipeError("browser closed"), ConnectionResetError("browser reset")):
            with self.subTest(error=type(error).__name__):
                handler = object.__new__(DemoRequestHandler)
                handler.server = self.server
                handler._json = Mock(side_effect=error)
                handler._error = Mock()
                receipt = {"mutation_id": "committed-once", "replayed": False}
                operation = Mock(return_value=receipt)
                with patch("server.sys.stderr"):
                    handler._run(operation)
                operation.assert_called_once_with()
                handler._json.assert_called_once_with(200, receipt)
                handler._error.assert_not_called()
                self.application.fail_connection.assert_not_called()
                self.assertFalse(self.application.failed)

    def test_gateway_exception_sanitized_and_health_transitions_to_failed(self):
        self.application.ingest.side_effect = SmokeFailure(
            "password=private-secret SQL=secret-table http://private-host",
        )
        with patch("server.sys.stderr"):
            response = self.post({"action": "record-paid-order"}, "/api/ingest")
        self.assert_error(response, 502, "DEMO_GATEWAY_UNAVAILABLE")
        self.assertNotIn(b"private", response[2])
        self.assertNotIn(b"secret", response[2])
        self.assertIn(b"may have committed", response[2])
        self.application.fail_connection.assert_called_once_with()
        self.assertEqual(json.loads(self.request()[2]), {"status": "failed"})

    def test_unexpected_application_exception_aborts_instead_of_masking_a_bug(self):
        self.application.compare.side_effect = RuntimeError("private-secret-unexpected")
        with self.assertRaises(http.client.RemoteDisconnected):
            self.post({})
        self.server.handle_error.assert_called_once()
        self.application.fail_connection.assert_not_called()

    def test_unexpected_handler_error_log_does_not_print_exception_details(self):
        with patch("server.sys.stderr") as stderr:
            DemoServer.handle_error(self.server, Mock(), ("127.0.0.1", 1))
        stderr.write.assert_called_once_with(
            "[meaning lab] unexpected HTTP handler failure; request aborted\n",
        )

    def test_utf16_and_utf32_are_not_accepted_as_utf8_json(self):
        for encoding in ("utf-16", "utf-32"):
            with self.subTest(encoding=encoding):
                self.assert_error(self.request("POST", "/api/guards", "{}".encode(encoding)),
                                  400, "DEMO_INVALID_JSON")
                self.assert_no_application_calls()

    def test_short_body_does_not_dispatch_even_if_available_prefix_is_valid_json(self):
        headers = [
            ("Host", self.host), ("Content-Type", "application/json"), ("Content-Length", "3"),
        ]
        response = self.request("POST", "/api/guards", b"{}", headers)
        self.assertEqual(response[0], 400, response[2])
        self.assert_no_application_calls()


if __name__ == "__main__":
    unittest.main()
