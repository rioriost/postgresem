"""Unit tests for the closed Meaning Lab workflows; no database is contacted."""

from collections import deque
from copy import deepcopy
from decimal import Decimal
from http import HTTPStatus
import unittest
from unittest.mock import Mock

from application import DemoApplication, DemoError, scalar
from planner import PlannerFailure
from runtime import ROLES
from scenarios import (
    FIXTURE_REVISION, PAID_ORDER, SCENARIOS, expected_answer, order_query,
)
from smoke import SmokeFailure


def ledger():
    return {
        "orders": [
            {"order_id": 1, "external_id": "fixture-order-1", "status": "paid", "amount": "100.10"},
            {"order_id": 2, "external_id": "fixture-order-2", "status": "paid", "amount": "20.20"},
            {"order_id": 3, "external_id": "fixture-order-3", "status": "pending", "amount": "300.00"},
            {"order_id": 4, "external_id": "fixture-order-4", "status": "cancelled", "amount": "400.00"},
        ],
        "items": [
            {"order_item_id": 1, "order_id": 1, "sku": "SKU-RED", "quantity": 7},
            {"order_item_id": 2, "order_id": 1, "sku": "SKU-RED", "quantity": 9},
            {"order_item_id": 3, "order_id": 2, "sku": "SKU-BLUE", "quantity": 1},
            {"order_item_id": 4, "order_id": 3, "sku": "SKU-RED", "quantity": 1},
            {"order_item_id": 5, "order_id": 4, "sku": "SKU-RED", "quantity": 1},
        ],
        "subscriptions": [
            {"subscription_id": 1, "plan": "basic", "active": True, "monthly_amount": "10.10"},
            {"subscription_id": 2, "plan": "pro", "active": True, "monthly_amount": "20.20"},
            {"subscription_id": 3, "plan": "old", "active": False, "monthly_amount": "300.00"},
        ],
        "fingerprint": "unit-test-stable-source",
    }


def tool_result(content, *, is_error=False):
    return {"structuredContent": deepcopy(content), "isError": is_error}


def query_result(value, revision=FIXTURE_REVISION):
    return {
        "schema_version": "1", "rows": [[value]], "truncated": False,
        "columns": [{"name": "value", "type": "numeric"}],
        "query_id": "unit-query", "semantic_revision": revision,
    }


def rejection(code="SEMANTIC_METRIC_NOT_AVAILABLE"):
    return tool_result({"valid": False, "error": {"code": code}}, is_error=True)


class ScriptedMcp:
    def __init__(self, responses=(), events=None, profile="analyst"):
        self.responses = deque(deepcopy(responses))
        self.events = events if events is not None else []
        self.profile = profile
        self.calls = []
        self.aborted = False

    def request(self, method, params):
        self.calls.append((method, deepcopy(params)))
        self.events.append((self.profile, params["name"]))
        if not self.responses:
            raise AssertionError("unexpected MCP call: " + params["name"])
        result = self.responses.popleft()
        if isinstance(result, Exception):
            raise result
        return deepcopy(result)

    def abort(self):
        self.aborted = True


class ScriptedProbe:
    def __init__(self, snapshots=(), baselines=None, events=None):
        self.snapshots = deque(deepcopy(snapshots))
        self.baselines = baselines or {}
        self.events = events if events is not None else []
        self.calls = []

    def snapshot(self):
        self.events.append(("probe", "snapshot"))
        self.calls.append(("snapshot",))
        if not self.snapshots:
            raise AssertionError("unexpected source snapshot")
        return deepcopy(self.snapshots.popleft())

    def baseline(self, scenario, choice):
        self.events.append(("probe", "baseline"))
        self.calls.append(("baseline", scenario, choice))
        return {"role": ROLES["analyst"], "read_only": "on",
                "data": self.baselines[(scenario, choice)]}

    def tenant(self, profile):
        self.calls.append(("tenant", profile))
        return {"role": ROLES[profile], "read_only": "on",
                "data": {"tenant_a": "250.00", "tenant_b": "999.00"}[profile]}


class OracleTests(unittest.TestCase):
    def test_independent_decimal_business_answers(self):
        source = ledger()
        untouched = deepcopy(source)
        for scenario, expected, ids, key in (
            ("recognized-revenue", Decimal("120.30"), [1, 2], "order_id"),
            ("sku-fanout", Decimal("100.10"), [1], "order_id"),
            ("active-mrr", Decimal("30.30"), [1, 2], "subscription_id"),
        ):
            with self.subTest(scenario=scenario):
                answer = expected_answer(scenario, source)
                self.assertEqual(Decimal(answer["value"]), expected)
                self.assertEqual([row[key] for row in answer["contributing_rows"]], ids)
                self.assertTrue(answer["derivation"])
        self.assertEqual(source, untouched)

    def test_matching_items_count_order_once_not_quantity_or_item_count(self):
        source = ledger()
        original = expected_answer("sku-fanout", source)
        for item_id in range(10, 40):
            source["items"].append({
                "order_item_id": item_id, "order_id": 1, "sku": "SKU-RED", "quantity": 1000,
            })
        source["items"].append({
            "order_item_id": 40, "order_id": 999, "sku": "SKU-RED", "quantity": 1,
        })
        self.assertEqual(expected_answer("sku-fanout", source), original)
        self.assertEqual(original["value"], "100.10")

    def test_oracle_reads_ledger_not_hardcoded_fixture_totals(self):
        source = ledger()
        source["orders"][0]["amount"] = "0.10"
        source["orders"][1]["amount"] = "0.20"
        source["subscriptions"][0]["monthly_amount"] = "0.10"
        source["subscriptions"][1]["monthly_amount"] = "0.20"
        for scenario, value in (
            ("recognized-revenue", "0.30"), ("sku-fanout", "0.10"), ("active-mrr", "0.30"),
        ):
            with self.subTest(scenario=scenario):
                self.assertEqual(expected_answer(scenario, source)["value"], value)

    def test_empty_qualifying_sets_are_zero(self):
        source = ledger()
        for row in source["orders"]:
            row["status"] = "pending"
        for row in source["subscriptions"]:
            row["active"] = False
        for scenario in SCENARIOS:
            with self.subTest(scenario=scenario):
                answer = expected_answer(scenario, source)
                self.assertEqual(answer["value"], "0.00")
                self.assertEqual(answer["contributing_rows"], [])

    def test_order_query_does_not_share_mutable_candidate_filters(self):
        first = order_query(sku=True)
        first["filters"]["args"][0]["values"][0]["value"] = "changed"
        self.assertEqual(
            order_query(sku=True)["filters"]["args"][0]["values"][0]["value"],
            "fixture-order-1",
        )


class ScalarTests(unittest.TestCase):
    def test_scalar_retains_decimal_precision(self):
        self.assertEqual(scalar(query_result("9007199254740993.01")), Decimal("9007199254740993.01"))
        self.assertEqual(scalar(query_result(0)), Decimal(0))

    def test_rejects_ambiguous_or_nonfinite_results(self):
        bad_results = [
            {}, {"rows": [["1"]]}, {"rows": [["1"]], "truncated": True},
            query_result(None), query_result(True), query_result("not a number"),
            query_result("NaN"), query_result("Infinity"), query_result("-Infinity"),
        ]
        for rows in (None, {}, [], ["1"], [[]], [["1", "2"]], [["1"], ["2"]]):
            bad_results.append({"rows": rows, "truncated": False})
        for result in bad_results:
            with self.subTest(result=result):
                with self.assertRaises(SmokeFailure):
                    scalar(result)


class ApplicationHarness(unittest.TestCase):
    def make_application(self, responses=(), snapshots=(), baselines=None, enabled=False):
        self.events = []
        self.clients = {
            profile: ScriptedMcp(responses if profile == "analyst" else (), self.events, profile)
            for profile in ROLES
        }
        self.probe = ScriptedProbe(snapshots, baselines, self.events)
        self.planner = Mock(enabled=enabled, model="unit-planner")
        self.app = DemoApplication(self.clients, self.probe, self.planner)
        return self.app

    def comparison(self, scenario="recognized-revenue", *, baseline="820.30",
                   semantic="120.30", choice="A", enabled=False, after=None):
        source = ledger()
        responses = [
            tool_result({"model": {"name": SCENARIOS[scenario]["model"]},
                         "semantic_revision": FIXTURE_REVISION}),
            tool_result({"valid": True, "semantic_revision": FIXTURE_REVISION}),
            tool_result({"semantic_models": [SCENARIOS[scenario]["model"]],
                         "semantic_revision": FIXTURE_REVISION}),
            tool_result(query_result(semantic)),
        ]
        app = self.make_application(
            responses, [source, source if after is None else after],
            {(scenario, choice): baseline}, enabled=enabled,
        )
        if enabled:
            self.planner.choose.return_value = (
                {"choice": choice, "reason": "The selected SQL follows the rule."},
                {"choice": "A", "reason": "The published metric follows the rule."},
            )
        return app


class ApplicationTests(ApplicationHarness):
    def test_default_comparisons_execute_both_paths_and_observe_source(self):
        scenarios_before = deepcopy(SCENARIOS)
        for scenario, baseline, semantic in (
            ("recognized-revenue", "820.30", "120.30"),
            ("sku-fanout", "200.20", "100.10"),
            ("active-mrr", "330.30", "30.30"),
        ):
            with self.subTest(scenario=scenario):
                app = self.comparison(scenario, baseline=baseline, semantic=semantic)
                result = app.compare({"scenario": scenario, "mode": "deterministic"})
                self.assertFalse(result["baseline"]["correct"])
                self.assertTrue(result["semantic"]["correct"])
                self.assertIsNone(result["planner"])
                self.assertEqual(result["baseline"]["value"], baseline)
                self.assertEqual(result["semantic"]["value"], semantic)
                self.assertEqual(result["expected"]["value"], semantic)
                self.assertTrue(result["comparison"]["same_role"])
                self.assertTrue(result["comparison"]["stable_source"])
                self.assertEqual(self.events, [
                    ("analyst", "describe_semantic_model"), ("probe", "snapshot"),
                    ("probe", "baseline"), ("analyst", "validate_semantic_query"),
                    ("analyst", "explain_semantic_query"), ("analyst", "query_semantic_model"),
                    ("probe", "snapshot"),
                ])
                calls = self.clients["analyst"].calls
                for method, params in calls:
                    self.assertEqual(method, "tools/call")
                    self.assertEqual(params["arguments"]["schema_version"], "1")
                for _, params in calls[1:]:
                    self.assertEqual(params["arguments"]["lsq"], SCENARIOS[scenario]["semantic"]["A"])
                self.planner.choose.assert_not_called()
                self.assertFalse(self.clients["analyst"].responses)
        self.assertEqual(SCENARIOS, scenarios_before)

    def test_correct_live_sql_is_never_relabeled_as_a_failure(self):
        for scenario, value in (
            ("recognized-revenue", "120.30"), ("sku-fanout", "100.10"), ("active-mrr", "30.30"),
        ):
            with self.subTest(scenario=scenario):
                app = self.comparison(scenario, baseline=value, semantic=value, choice="B", enabled=True)
                result = app.compare({"scenario": scenario, "mode": "planner"})
                self.assertTrue(result["baseline"]["correct"])
                self.assertTrue(result["semantic"]["correct"])
                self.assertEqual(result["baseline"]["choice"], "B")
                self.assertEqual(result["baseline"]["sql"], SCENARIOS[scenario]["baseline"]["B"])
                self.assertIn("Both plans agree", result["comparison"]["verdict"])
                self.assertEqual(result["planner"]["model"], "unit-planner")
                self.assertIn(("baseline", scenario, "B"), self.probe.calls)
                self.planner.choose.assert_called_once()
                selected_scenario, catalog = self.planner.choose.call_args.args
                self.assertEqual(selected_scenario, SCENARIOS[scenario])
                self.assertEqual(catalog["semantic_revision"], FIXTURE_REVISION)

    def test_actual_wrong_semantic_result_is_not_forced_correct(self):
        app = self.comparison(semantic="60.15", enabled=True)
        self.planner.choose.return_value = (
            {"choice": "A", "reason": "schema-only"},
            {"choice": "B", "reason": "incorrect metric"},
        )
        result = app.compare({"scenario": "recognized-revenue", "mode": "planner"})
        self.assertFalse(result["semantic"]["correct"])
        self.assertEqual(result["semantic"]["value"], "60.15")
        self.assertEqual(result["semantic"]["lsq"], SCENARIOS["recognized-revenue"]["semantic"]["B"])

    def test_source_race_is_conflict(self):
        after = ledger()
        after["fingerprint"] = "concurrent-writer"
        app = self.comparison(after=after)
        with self.assertRaises(DemoError) as caught:
            app.compare({"scenario": "recognized-revenue", "mode": "deterministic"})
        self.assertEqual(caught.exception.status, HTTPStatus.CONFLICT)
        self.assertEqual(caught.exception.code, "DEMO_SOURCE_CHANGED")
        self.assertEqual(self.probe.calls.count(("snapshot",)), 2)

    def test_each_stage_must_match_described_revision(self):
        for response_index in (1, 2, 3):
            for revision in ("sha256:changed", None):
                with self.subTest(stage=response_index, revision=revision):
                    app = self.comparison()
                    response = self.clients["analyst"].responses[response_index]["structuredContent"]
                    if revision is None:
                        response.pop("semantic_revision")
                    else:
                        response["semantic_revision"] = revision
                    with self.assertRaises(DemoError) as caught:
                        app.compare({"scenario": "recognized-revenue", "mode": "deterministic"})
                    self.assertEqual(caught.exception.status, 409)
                    self.assertEqual(caught.exception.code, "DEMO_SOURCE_CHANGED")

    def test_invalid_query_validation_does_not_execute_or_explain(self):
        app = self.comparison()
        self.clients["analyst"].responses[1]["structuredContent"]["valid"] = False
        with self.assertRaises(SmokeFailure):
            app.compare({"scenario": "recognized-revenue", "mode": "deterministic"})
        names = [params["name"] for _, params in self.clients["analyst"].calls]
        self.assertEqual(names, ["describe_semantic_model", "validate_semantic_query"])

    def test_closed_payload_validation_precedes_all_io(self):
        valid = {"scenario": "recognized-revenue", "mode": "deterministic"}
        comparisons = [
            None, [], "", 1, True, {}, {"scenario": "recognized-revenue"},
            {"mode": "deterministic"}, {**valid, "scenario": "unknown"},
            {**valid, "scenario": []}, {**valid, "scenario": {}},
            {**valid, "mode": []}, {**valid, "mode": {}}, {**valid, "mode": "live"},
        ]
        for field in ("sql", "raw_sql", "role", "lsq", "prompt", "model"):
            comparisons.append({**valid, field: "not accepted"})
        for operation, payloads in (
            ("compare", comparisons),
            ("ingest", [None, [], "", 1, True, {}, {"action": "unknown"},
                        {"action": "record-paid-order", "rows": []},
                        {"action": "record-paid-order", "role": "postgres"},
                        {"action": "record-paid-order", "sql": "SELECT 1"}]),
            ("guards", [None, [], "", 1, True, {"role": "postgres"}, {"sql": "SELECT 1"}]),
        ):
            for payload in payloads:
                with self.subTest(operation=operation, payload=payload):
                    app = self.make_application()
                    with self.assertRaises(DemoError) as caught:
                        getattr(app, operation)(payload)
                    self.assertEqual(caught.exception.status, 400)
                    self.assertEqual(caught.exception.code, "DEMO_INVALID_REQUEST")
                    self.assertEqual(self.events, [])
                    self.planner.choose.assert_not_called()

    def test_live_mode_requires_opt_in_before_io(self):
        app = self.make_application()
        with self.assertRaises(DemoError) as caught:
            app.compare({"scenario": "recognized-revenue", "mode": "planner"})
        self.assertEqual(caught.exception.status, 409)
        self.assertEqual(caught.exception.code, "DEMO_PLANNER_DISABLED")
        self.assertEqual(self.events, [])
        self.planner.choose.assert_not_called()

    def test_planner_failure_never_executes_fallback_or_marks_mcp_failed(self):
        app = self.comparison(enabled=True)
        self.planner.choose.side_effect = PlannerFailure("no approved choice")
        with self.assertRaises(PlannerFailure):
            app.compare({"scenario": "recognized-revenue", "mode": "planner"})
        self.assertEqual(self.events, [("analyst", "describe_semantic_model")])
        self.assertFalse(app.failed)
        self.assertFalse(any(client.aborted for client in self.clients.values()))

    def test_bootstrap_is_bounded_public_metadata_not_source_rows(self):
        app = self.make_application([tool_result({
            "models": [{"name": "orders"}], "semantic_revision": FIXTURE_REVISION,
        })])
        result = app.bootstrap()
        self.assertEqual(result["planner"], {"enabled": False, "model": None})
        self.assertFalse(result["contract"]["raw_sql"])
        self.assertEqual(result["contract"]["authorization"], "PostgreSQL GRANT + RLS")
        self.assertEqual(len(result["scenarios"]), 3)
        self.assertNotIn("source", result)
        self.assertNotIn("baseline", result["scenarios"][0])
        self.assertEqual(self.clients["analyst"].calls[0][1]["arguments"],
                         {"schema_version": "1", "limit": 100})
        self.assertIn("not measured LLM outputs", result["disclaimer"])
        self.assertEqual(self.probe.calls, [])

    def test_fatal_connection_state_aborts_every_profile_and_refuses_further_calls(self):
        app = self.make_application()
        app.fail_connection()
        self.assertTrue(app.failed)
        self.assertTrue(all(client.aborted for client in self.clients.values()))
        for operation in (
            app.bootstrap,
            lambda: app.compare({"scenario": "recognized-revenue", "mode": "deterministic"}),
            lambda: app.ingest({"action": "record-paid-order"}),
            lambda: app.guards({}),
        ):
            with self.subTest(operation=operation):
                with self.assertRaises(SmokeFailure):
                    operation()
        self.assertEqual(self.events, [])


class IngestTests(ApplicationHarness):
    def ingest_setup(self, *, already_recorded=False, source=None):
        source = ledger() if source is None else deepcopy(source)
        source["orders"].append({
            "order_id": 5, "external_id": "meaning-lab-paid-order",
            "status": "paid", "amount": "45.00",
        })
        receipt = {
            "mutation_id": "unit-mutation", "replayed": already_recorded,
            "affected_rows": 1, "semantic_revision": FIXTURE_REVISION,
        }
        before = "165.30" if already_recorded else "120.30"
        return self.make_application([
            tool_result(query_result(before)),
            tool_result({"valid": True, "semantic_revision": FIXTURE_REVISION}),
            tool_result(receipt), tool_result({**receipt, "replayed": True}),
            tool_result({"state": {"status": "committed", "mutation_id": "unit-mutation"}}),
            rejection("MUTATION_IDEMPOTENCY_CONFLICT"),
            tool_result(query_result("165.30")),
        ], [source])

    def test_ingest_validates_identical_replay_reconciliation_conflict_and_persistence(self):
        original = deepcopy(PAID_ORDER)
        app = self.ingest_setup()
        result = app.ingest({"action": "record-paid-order"})
        self.assertTrue(result["consistent"])
        self.assertEqual(Decimal(result["expected_delta"]), Decimal("45.00"))
        self.assertEqual(Decimal(result["actual_delta"]), Decimal("45.00"))
        self.assertTrue(result["conflicting_retry"]["rejected"])
        calls = self.clients["analyst"].calls
        self.assertEqual([params["name"] for _, params in calls], [
            "query_semantic_model", "validate_semantic_mutation", "mutate_semantic_model",
            "mutate_semantic_model", "reconcile_semantic_mutation",
            "mutate_semantic_model", "query_semantic_model",
        ])
        bodies = [calls[index][1]["arguments"] for index in (1, 2, 3)]
        self.assertEqual(bodies, [{"schema_version": "1", "lsm": original}] * 3)
        self.assertEqual(calls[4][1]["arguments"], {
            "schema_version": "1", "idempotency_key": original["idempotency_key"],
        })
        conflicting = deepcopy(original)
        conflicting["rows"][0]["amount"]["value"] = "999.00"
        self.assertEqual(calls[5][1]["arguments"]["lsm"], conflicting)
        self.assertEqual(calls[0][1]["arguments"]["lsq"], calls[6][1]["arguments"]["lsq"])
        self.assertEqual(self.events[-1], ("probe", "snapshot"))
        self.assertEqual(PAID_ORDER, original)

    def test_repeated_first_receipt_is_replay_and_requires_zero_delta(self):
        app = self.ingest_setup(already_recorded=True)
        result = app.ingest({"action": "record-paid-order"})
        self.assertTrue(result["consistent"])
        self.assertTrue(result["first"]["replayed"])
        self.assertEqual(Decimal(result["expected_delta"]), Decimal(0))
        self.assertEqual(Decimal(result["actual_delta"]), Decimal(0))

    def test_replayed_receipt_cannot_hide_a_second_revenue_increment(self):
        app = self.ingest_setup(already_recorded=True)
        self.clients["analyst"].responses[6] = tool_result(query_result("210.30"))
        result = app.ingest({"action": "record-paid-order"})
        self.assertFalse(result["consistent"])
        self.assertEqual(Decimal(result["expected_delta"]), Decimal(0))
        self.assertEqual(Decimal(result["actual_delta"]), Decimal("45.00"))

    def test_validation_failure_prevents_every_mutation(self):
        app = self.ingest_setup()
        self.clients["analyst"].responses[1]["structuredContent"]["valid"] = False
        with self.assertRaises(SmokeFailure):
            app.ingest({"action": "record-paid-order"})
        self.assertEqual([params["name"] for _, params in self.clients["analyst"].calls],
                         ["query_semantic_model", "validate_semantic_mutation"])
        self.assertEqual(self.probe.calls, [])

    def test_conflicting_retry_must_actually_be_rejected(self):
        app = self.ingest_setup()
        self.clients["analyst"].responses[5] = tool_result({
            "replayed": True, "error": {"code": "MUTATION_IDEMPOTENCY_CONFLICT"},
        })
        with self.assertRaisesRegex(SmokeFailure, "did not reject"):
            app.ingest({"action": "record-paid-order"})

    def test_receipt_delta_and_publication_mismatches_are_not_consistent(self):
        changes = [
            (3, "replayed", False), (3, "mutation_id", "different"),
            (2, "semantic_revision", "changed"),
            (6, "semantic_revision", "changed"), (6, "rows", [["999.00"]]),
        ]
        for index, key, value in changes:
            with self.subTest(index=index, key=key):
                app = self.ingest_setup()
                self.clients["analyst"].responses[index]["structuredContent"][key] = value
                self.assertFalse(app.ingest({"action": "record-paid-order"})["consistent"])

    def test_reconciliation_and_conflict_code_must_match(self):
        for index, content in (
            (4, {"state": {"status": "committed", "mutation_id": "another"}}),
            (5, {"valid": False, "error": {"code": "OTHER_ERROR"}}),
        ):
            with self.subTest(index=index):
                app = self.ingest_setup()
                self.clients["analyst"].responses[index] = tool_result(content, is_error=index == 5)
                self.assertFalse(app.ingest({"action": "record-paid-order"})["consistent"])

    def test_source_must_contain_exactly_one_matching_paid_45_order(self):
        for alteration in ("missing", "duplicate", "unpaid", "wrong_amount"):
            with self.subTest(alteration=alteration):
                app = self.ingest_setup()
                orders = self.probe.snapshots[0]["orders"]
                if alteration == "missing":
                    orders.pop()
                elif alteration == "duplicate":
                    orders.append(deepcopy(orders[-1]))
                elif alteration == "unpaid":
                    orders[-1]["status"] = "pending"
                else:
                    orders[-1]["amount"] = "44.99"
                self.assertFalse(app.ingest({"action": "record-paid-order"})["consistent"])


class GuardTests(ApplicationHarness):
    def guards_setup(self):
        app = self.make_application([
            rejection(), rejection(), rejection("LSQ_INVALID_JSON"),
        ])
        self.clients["tenant_a"].responses.append(tool_result(query_result("250.00")))
        self.clients["tenant_b"].responses.append(tool_result(query_result("999.00")))
        return app

    def test_hidden_unknown_and_raw_sql_are_rejected_with_fixed_rls_identities(self):
        app = self.guards_setup()
        result = app.guards({})
        self.assertTrue(result["passed"])
        hidden, unknown, raw = result["rejections"]
        self.assertEqual(hidden["code"], unknown["code"])
        self.assertTrue(all(item["rejected"] for item in (hidden, unknown, raw)))
        self.assertEqual([item["role"] for item in result["tenants"]],
                         [ROLES["tenant_a"], ROLES["tenant_b"]])
        self.assertTrue(all(item["rls_enforced"] for item in result["tenants"]))
        rejected_lsqs = [params["arguments"]["lsq"] for _, params in self.clients["analyst"].calls]
        self.assertEqual(rejected_lsqs[0]["metrics"], [{"metric": "internal_revenue"}])
        self.assertEqual(rejected_lsqs[1]["metrics"], [{"metric": "does_not_exist"}])
        self.assertEqual(rejected_lsqs[2]["sql"], "SELECT 1")
        for profile in ("tenant_a", "tenant_b"):
            method, params = self.clients[profile].calls[0]
            self.assertEqual(method, "tools/call")
            self.assertEqual(params["name"], "query_semantic_model")
            self.assertEqual(params["arguments"]["lsq"]["model"], "tenant_orders")
            self.assertNotIn("role", params["arguments"])
        self.assertEqual(self.probe.calls, [("tenant", "tenant_a"), ("tenant", "tenant_b")])
        self.assertEqual(
            self.clients["tenant_a"].calls[0][1]["arguments"],
            self.clients["tenant_b"].calls[0][1]["arguments"],
        )

    def test_different_hidden_unknown_codes_or_wrong_rls_value_fail_guard(self):
        app = self.guards_setup()
        self.clients["analyst"].responses[1] = rejection("DIFFERENT_CODE")
        self.assertFalse(app.guards({})["passed"])
        app = self.guards_setup()
        self.clients["tenant_a"].responses[0] = tool_result(query_result("999.00"))
        result = app.guards({})
        self.assertFalse(result["passed"])
        self.assertFalse(result["tenants"][0]["rls_enforced"])

    def test_rejection_requires_error_signal_and_string_code(self):
        for envelope in (
            tool_result({"error": {"code": "SEMANTIC_METRIC_NOT_AVAILABLE"}}),
            tool_result({"valid": True}),
            tool_result({"valid": False}),
            tool_result({"valid": False, "error": {"code": 123}}),
        ):
            with self.subTest(envelope=envelope):
                app = self.make_application([envelope])
                with self.assertRaises(SmokeFailure):
                    app._rejection("validate_semantic_query", {"lsq": order_query()})


if __name__ == "__main__":
    unittest.main()
