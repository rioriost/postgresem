#!/usr/bin/env python3
"""Exercise the unified demo against an already running real fixture stack."""

import argparse
from decimal import Decimal
import json
import threading
import urllib.request

from application import DemoApplication, scalar
from planner import Planner
from runtime import ContainerRuntime, DatabaseProbe, ROLES
from scenarios import SCENARIOS, expected_answer
from server import DemoServer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", choices=("auto", "apple", "docker", "podman"), default="auto")
    parser.add_argument("--container-prefix", default="postgresem",
                        help="Apple/Podman container prefix for isolated qualification stacks")
    args = parser.parse_args()
    runtime = ContainerRuntime(args.runtime, container_prefix=args.container_prefix)
    clients, server, thread = {}, None, None
    try:
        for profile in ROLES:
            clients[profile] = runtime.client(profile)
        probe = DatabaseProbe(runtime)
        application = DemoApplication(clients, probe, Planner({}))
        server = DemoServer(("127.0.0.1", 0), application)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        origin = f"http://127.0.0.1:{server.server_port}"

        def request(path, payload=None):
            req = urllib.request.Request(
                origin + path,
                data=None if payload is None else json.dumps(payload).encode(),
                headers={"Content-Type": "application/json", "Origin": origin},
            )
            with urllib.request.urlopen(req, timeout=120) as response:
                assert response.status == 200, path
                assert response.headers["Cache-Control"] == "no-store", path
                return json.load(response)

        assert request("/healthz")["status"] == "ok"
        assert len(request("/api/bootstrap")["scenarios"]) == 3
        with urllib.request.urlopen(origin, timeout=10) as response:
            assert b"app.js" in response.read()
            assert "frame-ancestors 'none'" in response.headers["Content-Security-Policy"]

        for scenario_id, scenario in SCENARIOS.items():
            compared = request("/api/compare", {"scenario": scenario_id, "mode": "deterministic"})
            assert compared["semantic"]["correct"], compared
            assert not compared["baseline"]["correct"], compared
            assert compared["comparison"]["same_role"], compared
            assert compared["comparison"]["stable_source"], compared
            assert compared["semantic"]["result"]["query_id"], compared
            expected = Decimal(expected_answer(scenario_id, probe.snapshot())["value"])
            assert Decimal(probe.baseline(scenario_id, "B")["data"]) == expected
            for choice, lsq in scenario["semantic"].items():
                actual = scalar(application._query(lsq))
                assert (actual == expected) == (choice == "A"), (scenario_id, choice, actual)
            print(f"{scenario_id}: direct={compared['baseline']['value']} "
                  f"semantic={compared['semantic']['value']} expected={expected}; correct SQL agrees")

        first = request("/api/ingest", {"action": "record-paid-order"})
        assert first["consistent"], first
        assert first["replay"]["replayed"], first
        assert first["conflicting_retry"]["code"] == "MUTATION_IDEMPOTENCY_CONFLICT", first
        second = request("/api/ingest", {"action": "record-paid-order"})
        assert second["consistent"] and second["first"]["replayed"], second
        assert Decimal(second["actual_delta"]) == 0, second
        assert first["first"]["mutation_id"] == second["first"]["mutation_id"], second
        print("ingestion: persisted one paid order; replay, reconciliation and conflict rejection agree")

        for scenario_id in SCENARIOS:
            result = request("/api/compare", {"scenario": scenario_id, "mode": "deterministic"})
            assert result["semantic"]["correct"] and not result["baseline"]["correct"], result
        guards = request("/api/guards", {})
        assert guards["passed"], guards
        assert len({item["code"] for item in guards["rejections"][:2]}) == 1, guards
        assert all(item["rejected"] for item in guards["rejections"]), guards
        assert [Decimal(t["direct_value"]) for t in guards["tenants"]] == [Decimal("250"), Decimal("999")]
        print("boundaries: raw SQL / hidden metrics rejected; direct and semantic paths both enforce RLS")
        print("PASS: Meaning Lab real PostgreSQL HTTP/MCP workflow")
    finally:
        if server:
            server.shutdown()
            server.server_close()
        if thread:
            thread.join(timeout=5)
        for client in clients.values():
            client.close()


if __name__ == "__main__":
    main()
