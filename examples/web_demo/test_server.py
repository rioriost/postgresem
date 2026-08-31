#!/usr/bin/env python3

from __future__ import annotations

from http import HTTPStatus
import http.client
import json
from pathlib import Path
import sys
import threading
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).resolve().parent))

from server import DemoApplication, DemoServer  # noqa: E402


class FakeClient:
    def __init__(self) -> None:
        self.calls: list[str] = []

    def request(self, method: str, params: dict) -> dict:
        if method != "tools/call":
            raise AssertionError(f"unexpected method {method}")
        name = params["name"]
        self.calls.append(name)
        if name == "list_semantic_models":
            content = {
                "semantic_revision": "sha256:" + "a" * 64,
                "models": [{"name": "orders", "field_count": 7, "metric_count": 4}],
            }
        elif name == "validate_semantic_query":
            content = {"valid": True, "normalized_lsq_hash": "sha256:" + "b" * 64}
        elif name == "explain_semantic_query":
            content = {"semantic_models": ["orders"], "semantic_relationships": []}
        elif name == "query_semantic_model":
            content = {
                "query_id": "00000000-0000-0000-0000-000000000001",
                "semantic_revision": "sha256:" + "a" * 64,
                "columns": [{"name": "revenue", "type": "numeric"}],
                "rows": [["200.50"]],
                "truncated": False,
                "warnings": [],
            }
        else:
            raise AssertionError(f"unexpected tool {name}")
        return {"structuredContent": content, "isError": False}


class DemoServerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.client = FakeClient()
        self.server = DemoServer(
            ("127.0.0.1", 0),
            DemoApplication(self.client),
            Path(__file__).resolve().parent / "static",
        )
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base_url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def get(self, path: str) -> tuple[int, dict[str, str], bytes]:
        with urlopen(self.base_url + path, timeout=2) as response:
            return response.status, dict(response.headers), response.read()

    def post(self, payload: object) -> tuple[int, dict[str, str], bytes]:
        request = Request(
            self.base_url + "/api/run",
            data=json.dumps(payload).encode(),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urlopen(request, timeout=2) as response:
                return response.status, dict(response.headers), response.read()
        except HTTPError as error:
            body = error.read()
            headers = dict(error.headers)
            error.close()
            return error.code, headers, body

    def raw_post(
        self, body: bytes, headers: dict[str, str]
    ) -> tuple[int, dict[str, str], bytes]:
        connection = http.client.HTTPConnection(
            "127.0.0.1", self.server.server_port, timeout=2
        )
        connection.request("POST", "/api/run", body=body, headers=headers)
        response = connection.getresponse()
        result = response.status, dict(response.headers), response.read()
        connection.close()
        return result

    def test_serves_static_page_with_security_headers(self) -> None:
        status, headers, body = self.get("/")
        self.assertEqual(status, HTTPStatus.OK)
        self.assertIn(b"Postgresem Commerce Demo", body)
        self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
        self.assertIn("default-src 'self'", headers["Content-Security-Policy"])

    def test_bootstrap_uses_semantic_model_tool(self) -> None:
        status, _, body = self.get("/api/bootstrap")
        payload = json.loads(body)
        self.assertEqual(status, HTTPStatus.OK)
        self.assertEqual(payload["models"][0]["name"], "orders")
        self.assertEqual(len(payload["examples"]), 4)
        self.assertEqual(self.client.calls, ["list_semantic_models"])

    def test_run_uses_validate_explain_and_guarded_query_tools(self) -> None:
        status, _, body = self.post({"example": "orders-revenue"})
        payload = json.loads(body)
        self.assertEqual(status, HTTPStatus.OK)
        self.assertTrue(payload["validation"]["valid"])
        self.assertEqual(payload["result"]["rows"], [["200.50"]])
        self.assertEqual(
            self.client.calls,
            [
                "validate_semantic_query",
                "explain_semantic_query",
                "query_semantic_model",
            ],
        )

    def test_rejects_browser_selected_configuration(self) -> None:
        status, _, body = self.post(
            {"example": "orders-revenue", "database_role": "postgres"}
        )
        payload = json.loads(body)
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)
        self.assertEqual(payload["error"]["code"], "DEMO_INVALID_REQUEST")
        self.assertEqual(self.client.calls, [])

    def test_rejects_unknown_example(self) -> None:
        status, _, body = self.post({"example": "raw-sql"})
        payload = json.loads(body)
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)
        self.assertEqual(payload["error"]["code"], "DEMO_EXAMPLE_NOT_AVAILABLE")

    def test_rejects_dns_rebinding_host(self) -> None:
        status, _, body = self.raw_post(
            json.dumps({"example": "orders-revenue"}).encode(),
            {
                "Content-Type": "application/json",
                "Host": "attacker.example",
            },
        )
        payload = json.loads(body)
        self.assertEqual(status, HTTPStatus.MISDIRECTED_REQUEST)
        self.assertEqual(payload["error"]["code"], "DEMO_HOST_REJECTED")
        self.assertEqual(self.client.calls, [])

    def test_rejects_invalid_utf8_as_json_error(self) -> None:
        status, _, body = self.raw_post(
            b"\xff",
            {
                "Content-Type": "application/json",
                "Host": f"127.0.0.1:{self.server.server_port}",
            },
        )
        payload = json.loads(body)
        self.assertEqual(status, HTTPStatus.BAD_REQUEST)
        self.assertEqual(payload["error"]["code"], "DEMO_INVALID_JSON")


if __name__ == "__main__":
    unittest.main()
