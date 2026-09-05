"""End-to-end read comparison and governed-write workflows."""

from copy import deepcopy
from decimal import Decimal, InvalidOperation
from http import HTTPStatus
import threading

from runtime import ROLES
from scenarios import DISCLAIMER, PAID_ORDER, SCENARIOS, expected_answer, order_query, public_scenario
from smoke import SmokeFailure, call_tool


class DemoError(RuntimeError):
    def __init__(self, status, code, message):
        super().__init__(message)
        self.status, self.code, self.message = status, code, message


def invalid_request():
    return DemoError(HTTPStatus.BAD_REQUEST, "DEMO_INVALID_REQUEST",
                     "only the documented demonstration actions are accepted")


def scalar(result):
    rows = result.get("rows")
    if (result.get("truncated") is not False or not isinstance(rows, list)
            or len(rows) != 1 or not isinstance(rows[0], list) or len(rows[0]) != 1):
        raise SmokeFailure("demo expected one untruncated scalar result")
    try:
        value = Decimal(str(rows[0][0]))
    except InvalidOperation as error:
        raise SmokeFailure("demo query did not return a numeric value") from error
    if not value.is_finite():
        raise SmokeFailure("demo query returned a nonfinite number")
    return value


class DemoApplication:
    def __init__(self, clients, probe, planner):
        self.clients, self.probe, self.planner = clients, probe, planner
        self.lock = threading.Lock()
        self.failed = False

    def _call(self, name, arguments, profile="analyst"):
        if self.failed:
            raise SmokeFailure("restart the demo after an MCP connection failure")
        return call_tool(self.clients[profile], name, {"schema_version": "1", **arguments})

    def bootstrap(self):
        with self.lock:
            listed = self._call("list_semantic_models", {"limit": 100})
        return {
            "schema_version": "1", "title": "PostgreSQL Meaning Lab",
            "scenarios": [public_scenario(s) for s in SCENARIOS.values()],
            "models": listed["models"], "semantic_revision": listed["semantic_revision"],
            "contract": {"read": "LSQ", "write": "LSM", "raw_sql": False,
                         "authorization": "PostgreSQL GRANT + RLS"},
            "planner": {"enabled": self.planner.enabled,
                        "model": self.planner.model if self.planner.enabled else None},
            "disclaimer": DISCLAIMER,
        }

    def _query(self, lsq, profile="analyst"):
        return self._call("query_semantic_model", {"lsq": lsq}, profile)

    def compare(self, payload):
        if (not isinstance(payload, dict) or set(payload) != {"scenario", "mode"}
                or not isinstance(payload["scenario"], str)
                or payload["scenario"] not in SCENARIOS
                or payload["mode"] not in ("deterministic", "planner")):
            raise invalid_request()
        if payload["mode"] == "planner" and not self.planner.enabled:
            raise DemoError(HTTPStatus.CONFLICT, "DEMO_PLANNER_DISABLED",
                            "OpenAI planning requires explicit server-side opt-in")
        scenario = SCENARIOS[payload["scenario"]]
        with self.lock:
            catalog = self._call("describe_semantic_model", {"model": scenario["model"]})
            baseline_plan = {"choice": "A", "reason": "Authored schema-only mistake; not a live model output."}
            semantic_plan = {"choice": "A", "reason": "Authored request for the published business metric."}
            planner = None
            if payload["mode"] == "planner":
                baseline_plan, semantic_plan = self.planner.choose(scenario, catalog)
                planner = {"model": self.planner.model,
                           "baseline_reason": baseline_plan["reason"],
                           "semantic_reason": semantic_plan["reason"]}
            source = self.probe.snapshot()
            baseline = self.probe.baseline(scenario["id"], baseline_plan["choice"])
            lsq = deepcopy(scenario["semantic"][semantic_plan["choice"]])
            validation = self._call("validate_semantic_query", {"lsq": lsq})
            if validation.get("valid") is not True:
                raise SmokeFailure("reviewed demo query was rejected by the published model")
            explanation = self._call("explain_semantic_query", {"lsq": lsq})
            result = self._query(lsq)
            after = self.probe.snapshot()
        stable = source["fingerprint"] == after["fingerprint"]
        revision = catalog["semantic_revision"]
        stable_revision = all(value.get("semantic_revision") == revision
                              for value in (validation, explanation, result))
        if not stable or not stable_revision:
            raise DemoError(HTTPStatus.CONFLICT, "DEMO_SOURCE_CHANGED",
                            "data or publication changed during comparison; rerun without concurrent writers")
        expected = expected_answer(scenario["id"], source)
        baseline_value, semantic_value = Decimal(baseline["data"]), scalar(result)
        baseline_correct = baseline_value == Decimal(expected["value"])
        semantic_correct = semantic_value == Decimal(expected["value"])
        return {
            "schema_version": "1", "scenario": public_scenario(scenario),
            "mode": payload["mode"], "question": scenario["question"],
            "baseline": {
                "label": "Direct PostgreSQL / schema-only plan",
                "choice": baseline_plan["choice"],
                "sql": scenario["baseline"][baseline_plan["choice"]],
                "value": str(baseline_value), "correct": baseline_correct,
                "role": baseline["role"], "explanation": baseline_plan["reason"],
            },
            "semantic": {
                "label": "postgresem / published semantic plan",
                "choice": semantic_plan["choice"], "lsq": lsq,
                "value": str(semantic_value), "correct": semantic_correct,
                "validation": validation, "explanation": explanation, "result": result,
                "catalog": catalog,
            },
            "expected": expected, "source": source,
            "comparison": {
                "same_role": baseline["role"] == ROLES["analyst"], "stable_source": stable,
                "verdict": (
                    "Both plans agree with the business rule; SQL can be correct too."
                    if baseline_correct and semantic_correct else
                    "Compare each actual result with the independently calculated business answer."
                ),
            },
            "planner": planner,
        }

    def ingest(self, payload):
        if payload != {"action": "record-paid-order"}:
            raise invalid_request()
        with self.lock:
            before = self._query(order_query())
            arguments = {"lsm": deepcopy(PAID_ORDER)}
            validation = self._call("validate_semantic_mutation", arguments)
            if validation.get("valid") is not True:
                raise SmokeFailure("reviewed demo mutation is not valid")
            first = self._call("mutate_semantic_model", arguments)
            replay = self._call("mutate_semantic_model", arguments)
            reconciliation = self._call("reconcile_semantic_mutation", {
                "idempotency_key": PAID_ORDER["idempotency_key"],
            })
            conflicting = deepcopy(PAID_ORDER)
            conflicting["rows"][0]["amount"]["value"] = "999.00"
            conflict = self._rejection("mutate_semantic_model", {"lsm": conflicting})
            after = self._query(order_query())
            source = self.probe.snapshot()
        expected_delta = Decimal("0.00") if first["replayed"] else Decimal("45.00")
        actual_delta = scalar(after) - scalar(before)
        persisted = [row for row in source["orders"]
                     if row["external_id"] == "meaning-lab-paid-order"]
        consistent = (
            actual_delta == expected_delta and replay["replayed"] is True
            and first["mutation_id"] == replay["mutation_id"]
            and reconciliation["state"].get("mutation_id") == first["mutation_id"]
            and conflict["code"] == "MUTATION_IDEMPOTENCY_CONFLICT"
            and before["semantic_revision"] == first["semantic_revision"] == after["semantic_revision"]
            and len(persisted) == 1 and persisted[0]["status"] == "paid"
            and Decimal(persisted[0]["amount"]) == Decimal("45.00")
        )
        return {
            "schema_version": "1", "before": before, "validation": validation,
            "first": first, "replay": replay, "reconciliation": reconciliation,
            "conflicting_retry": conflict, "after": after,
            "expected_delta": str(expected_delta), "actual_delta": str(actual_delta),
            "consistent": consistent,
        }

    def _rejection(self, tool, arguments):
        if self.failed:
            raise SmokeFailure("restart the demo after an MCP connection failure")
        envelope = self.clients["analyst"].request("tools/call", {
            "name": tool, "arguments": {"schema_version": "1", **arguments},
        })
        content = envelope.get("structuredContent", {})
        rejected = envelope.get("isError") is True or content.get("valid") is False
        code = content.get("error", {}).get("code")
        if not rejected or not isinstance(code, str):
            raise SmokeFailure("gateway did not reject the prohibited demonstration request")
        return {"rejected": True, "code": code}

    def guards(self, payload):
        if payload != {}:
            raise invalid_request()
        with self.lock:
            hidden = self._rejection("validate_semantic_query", {"lsq": order_query("internal_revenue")})
            unknown = self._rejection("validate_semantic_query", {"lsq": order_query("does_not_exist")})
            raw = self._rejection("validate_semantic_query", {"lsq": {**order_query(), "sql": "SELECT 1"}})
            tenants = []
            for profile, expected in (("tenant_a", "250.00"), ("tenant_b", "999.00")):
                lsq = {
                    "schema_version": "1", "model": "tenant_orders",
                    "metrics": [{"metric": "revenue"}],
                    "filters": {"op": "in", "field": "external_id", "values": [
                        {"type": "text", "value": key}
                        for key in ("fixture-a-1", "fixture-a-2", "fixture-b-1")
                    ]},
                }
                direct = self.probe.tenant(profile)
                result = self._query(lsq, profile)
                tenants.append({
                    "name": "Tenant A" if profile == "tenant_a" else "Tenant B",
                    "role": ROLES[profile], "direct_value": direct["data"],
                    "semantic_value": str(scalar(result)), "query_id": result["query_id"],
                    "rls_enforced": Decimal(direct["data"]) == scalar(result) == Decimal(expected),
                })
        return {
            "schema_version": "1",
            "rejections": [{"name": name, **result} for name, result in
                           (("Hidden metric", hidden), ("Unknown metric", unknown), ("Raw SQL", raw))],
            "tenants": tenants,
            "passed": hidden["code"] == unknown["code"] and all(t["rls_enforced"] for t in tenants),
        }

    def fail_connection(self):
        with self.lock:
            self.failed = True
            for client in self.clients.values():
                client.abort()
