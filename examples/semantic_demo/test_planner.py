"""Candidate-only planner tests with an in-memory HTTP opener."""

from copy import deepcopy
from http.client import HTTPException, IncompleteRead
import json
import os
from pathlib import Path
import unittest
from unittest.mock import Mock, patch
import urllib.error
import urllib.request

from planner import NoRedirects, PHYSICAL_SCHEMA, Planner, PlannerFailure, SETTINGS, load_settings
from scenarios import FIXTURE_REVISION, SCENARIOS
from smoke import SmokeFailure


def completion(plan=None, *, finish_reason="stop", refusal=None):
    plan = {"choice": "A", "reason": "The chosen metric answers the question."} if plan is None else plan
    message = {"content": json.dumps(plan), "refusal": refusal}
    return json.dumps({"choices": [{"finish_reason": finish_reason, "message": message}]}).encode()


def publication():
    return {
        "semantic_revision": FIXTURE_REVISION,
        "model": {
            "name": "orders",
            "fields": [{"name": "status", "type": "text"}],
            "metrics": [{"name": "revenue", "description": "Sum paid order amounts."}],
        },
        "rows": [["private-row-must-not-travel"]],
        "credentials": "private-catalog-credential-must-not-travel",
    }


class SettingsTests(unittest.TestCase):
    def setUp(self):
        self.environment = patch.dict(os.environ, {}, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.path = Mock(spec=Path)
        self.path.exists.return_value = True

    def test_missing_file_returns_only_known_empty_settings(self):
        self.path.exists.return_value = False
        self.assertEqual(load_settings(self.path), dict.fromkeys(SETTINGS, ""))
        self.path.read_text.assert_not_called()

    def test_environment_takes_precedence_including_explicit_empty_values(self):
        self.path.read_text.return_value = (
            "POSTGRESEM_DEMO_OPENAI=1\nOPENAI_API_KEY=file-key\nOPENAI_MODEL=file-model\n"
        )
        with patch.dict(os.environ, {"OPENAI_API_KEY": "environment-key", "OPENAI_MODEL": ""}):
            values = load_settings(self.path)
        self.assertEqual(values, {
            "POSTGRESEM_DEMO_OPENAI": "1", "OPENAI_API_KEY": "environment-key", "OPENAI_MODEL": "",
        })
        self.path.read_text.assert_called_once_with(encoding="utf-8")

    def test_values_are_literal_not_shell_expanded_or_executed(self):
        self.path.read_text.return_value = (
            " # a comment\n POSTGRESEM_DEMO_OPENAI = '1'\n"
            'OPENAI_API_KEY = "$(touch never-created)"\n'
            "OPENAI_MODEL = '${MODEL:-gpt-example}'\n"
            "export OPENAI_API_KEY=ignored-export\n"
        )
        with patch.dict(os.environ, {"MODEL": "do-not-expand"}):
            values = load_settings(self.path)
        self.assertEqual(values["POSTGRESEM_DEMO_OPENAI"], "1")
        self.assertEqual(values["OPENAI_API_KEY"], "$(touch never-created)")
        self.assertEqual(values["OPENAI_MODEL"], "${MODEL:-gpt-example}")

    def test_unknown_settings_and_credentials_are_not_retained(self):
        self.path.read_text.return_value = (
            "DATABASE_URL=postgres://private\nPOSTGRESEM_RUNTIME_PASSWORD=secret\n"
            "OPENAI_BASE_URL=https://unapproved.invalid\nOTHER=value\nOTHER=duplicate\n"
            "OPENAI_MODEL=gpt-example\nnot-an-assignment\n"
        )
        with patch.dict(os.environ, {"DATABASE_URL": "private-env", "UNRELATED": "other-secret"}):
            values = load_settings(self.path)
        self.assertEqual(set(values), set(SETTINGS))
        self.assertEqual(values["OPENAI_MODEL"], "gpt-example")
        self.assertNotIn("secret", json.dumps(values))

    def test_duplicate_known_setting_rejected_even_if_environment_overrides(self):
        for key in SETTINGS:
            with self.subTest(key=key):
                self.path.read_text.return_value = f"{key}=one\n {key} = two\n"
                with patch.dict(os.environ, {key: "override"}):
                    with self.assertRaisesRegex(SmokeFailure, "duplicate"):
                        load_settings(self.path)

    def test_unterminated_or_mismatched_quotes_rejected(self):
        for value in ('"unterminated', "'unterminated", "\"mismatch'", "'", '"'):
            with self.subTest(value=value):
                self.path.read_text.return_value = "OPENAI_API_KEY=" + value
                with self.assertRaises(SmokeFailure):
                    load_settings(self.path)


class PlannerHarness(unittest.TestCase):
    def setUp(self):
        factory = patch("planner.urllib.request.build_opener")
        self.factory = factory.start()
        self.addCleanup(factory.stop)
        self.opener = self.factory.return_value
        self.response = self.opener.open.return_value.__enter__.return_value
        self.response.read.return_value = completion()

    def planner(self, **overrides):
        settings = {
            "POSTGRESEM_DEMO_OPENAI": "1",
            "OPENAI_API_KEY": "unit-test-noncredential",
            "OPENAI_MODEL": "gpt-unit-test",
        }
        settings.update(overrides)
        return Planner(settings)

    def assert_no_fallback(self, body):
        self.response.read.return_value = body
        with self.assertRaises(PlannerFailure):
            self.planner().choose(SCENARIOS["recognized-revenue"], publication())
        self.opener.open.assert_called_once()


class PlannerConfigurationTests(PlannerHarness):
    def test_opt_in_must_be_exactly_one(self):
        for opt_in in ("", "0", "true", "yes", " 1", 1, True, None):
            with self.subTest(opt_in=opt_in):
                planner = self.planner(POSTGRESEM_DEMO_OPENAI=opt_in)
                self.assertFalse(planner.enabled)
                with self.assertRaises(PlannerFailure):
                    planner.choose(SCENARIOS["recognized-revenue"], publication())
        self.opener.open.assert_not_called()

    def test_missing_key_and_invalid_configuration_are_rejected(self):
        for settings in (
            {"OPENAI_API_KEY": ""}, {"OPENAI_API_KEY": "key\ninjection"},
            {"OPENAI_API_KEY": "key with spaces"}, {"OPENAI_API_KEY": "nonascii-é"},
            {"OPENAI_MODEL": "model with spaces"}, {"OPENAI_MODEL": "bad\nmodel"},
            {"OPENAI_MODEL": "x" * 102}, {"OPENAI_MODEL": "$MODEL"},
        ):
            with self.subTest(settings=settings):
                with self.assertRaises(SmokeFailure):
                    self.planner(**settings)
        with self.assertRaises(SmokeFailure):
            Planner({"POSTGRESEM_DEMO_OPENAI": "1"})
        self.opener.open.assert_not_called()

    def test_disabled_config_does_not_require_credentials_and_model_has_default(self):
        planner = Planner({})
        self.assertFalse(planner.enabled)
        self.assertEqual(planner.model, "gpt-4.1-mini")
        self.opener.open.assert_not_called()

    def test_unknown_publication_rejected_before_network(self):
        for revision in ("sha256:unapproved", None, ""):
            with self.subTest(revision=revision):
                with self.assertRaisesRegex(PlannerFailure, "publication"):
                    self.planner().choose(SCENARIOS["recognized-revenue"], {
                        "semantic_revision": revision, "model": {"rows": "private"},
                    })
        self.opener.open.assert_not_called()

    def test_proxy_and_redirect_handlers_are_disabled(self):
        self.planner()
        handlers = self.factory.call_args.args
        proxy = next(item for item in handlers if isinstance(item, urllib.request.ProxyHandler))
        self.assertEqual(proxy.proxies, {})
        redirect = next(item for item in handlers if isinstance(item, NoRedirects))
        request = urllib.request.Request("https://api.openai.com/v1/chat/completions")
        for code in (301, 302, 303, 307, 308):
            with self.subTest(code=code):
                with self.assertRaises(urllib.error.HTTPError) as caught:
                    redirect.redirect_request(request, None, code, "redirect", {},
                                              "https://unapproved.invalid/collect")
                caught.exception.close()


class PlannerRequestTests(PlannerHarness):
    def test_separate_contexts_share_question_model_system_and_approved_candidates(self):
        scenario = deepcopy(SCENARIOS["recognized-revenue"])
        catalog = publication()
        original = deepcopy((scenario, catalog))
        self.response.read.side_effect = [
            completion({"choice": "B", "reason": "Only paid orders are recognized."}),
            completion({"choice": "A", "reason": "Use the published revenue metric."}),
        ]
        with patch.dict(os.environ, {
            "DATABASE_URL": "postgres://do-not-send",
            "POSTGRESEM_RUNTIME_PASSWORD": "private-password-must-not-travel",
            "OPENAI_BASE_URL": "https://unapproved.invalid",
        }):
            planner = self.planner()
            baseline, semantic = planner.choose(scenario, catalog)
        self.assertEqual(baseline["choice"], "B")
        self.assertEqual(semantic["choice"], "A")
        self.assertEqual(self.opener.open.call_count, 2)
        payloads, contexts = [], []
        for call in self.opener.open.call_args_list:
            request = call.args[0]
            self.assertEqual(request.full_url, "https://api.openai.com/v1/chat/completions")
            self.assertEqual(request.get_method(), "POST")
            self.assertEqual(request.get_header("Content-type"), "application/json")
            self.assertEqual(request.get_header("Authorization"), "Bearer unit-test-noncredential")
            self.assertEqual(call.kwargs, {"timeout": 30})
            body = request.data.decode("utf-8")
            for forbidden in (
                "private-row-must-not-travel", "private-catalog-credential-must-not-travel",
                "postgres://do-not-send", "private-password-must-not-travel",
                "unit-test-noncredential", "unapproved.invalid",
            ):
                self.assertNotIn(forbidden, body)
            payload = json.loads(body)
            payloads.append(payload)
            self.assertEqual(payload["model"], "gpt-unit-test")
            self.assertIs(payload["store"], False)
            self.assertEqual(payload["max_completion_tokens"], 600)
            self.assertEqual(payload["response_format"], {"type": "json_object"})
            self.assertEqual([m["role"] for m in payload["messages"]], ["system", "user"])
            user = json.loads(payload["messages"][1]["content"])
            self.assertEqual(set(user), {"question", "context", "candidates"})
            self.assertEqual(user["question"], scenario["question"])
            contexts.append(user)
        self.assertEqual(payloads[0]["messages"][0], payloads[1]["messages"][0])
        self.assertEqual(contexts[0]["context"], PHYSICAL_SCHEMA)
        self.assertEqual(contexts[1]["context"], catalog["model"])
        self.assertNotEqual(contexts[0]["context"], contexts[1]["context"])
        self.assertEqual(contexts[0]["candidates"], scenario["baseline"])
        self.assertEqual(contexts[1]["candidates"], scenario["semantic"])
        self.assertEqual((scenario, catalog), original)

    def test_every_approved_candidate_can_be_returned_without_forcing_a_mistake(self):
        planner = self.planner()
        for scenario in SCENARIOS.values():
            for choice in scenario["baseline"]:
                with self.subTest(scenario=scenario["id"], choice=choice):
                    plan = {"choice": choice, "reason": "An actual approved selection."}
                    self.response.read.return_value = completion(plan)
                    baseline, semantic = planner.choose(scenario, publication())
                    self.assertEqual(baseline, plan)
                    self.assertEqual(semantic, plan)

    def test_reply_is_read_with_a_hard_byte_cap(self):
        self.response.read.return_value = b"x" * 65_537
        with self.assertRaisesRegex(PlannerFailure, "limits"):
            self.planner().choose(SCENARIOS["recognized-revenue"], publication())
        self.response.read.assert_called_once_with(65_537)
        self.opener.open.assert_called_once()

    def test_valid_reply_at_limit_is_accepted(self):
        body = completion()
        self.response.read.return_value = body + b" " * (65_536 - len(body))
        baseline, semantic = self.planner().choose(SCENARIOS["recognized-revenue"], publication())
        self.assertEqual(baseline["choice"], "A")
        self.assertEqual(semantic["choice"], "A")


class PlannerFailureTests(PlannerHarness):
    def test_invalid_plan_choice_reason_or_schema_has_no_fallback(self):
        plans = [
            None, [], "A", 1, True, {},
            {"choice": "A"}, {"reason": "missing choice"},
            {"choice": "C", "reason": "unknown"},
            {"choice": "a", "reason": "case mismatch"},
            {"choice": 1, "reason": "not a string"},
            {"choice": ["A"], "reason": "not a string"},
            {"choice": "A", "reason": None},
            {"choice": "A", "reason": 1},
            {"choice": "A", "reason": ""},
            {"choice": "A", "reason": "x" * 1001},
            {"choice": "A", "reason": "extra SQL", "sql": "SELECT 1"},
            {"choice": "A", "reason": "extra field", "role": "postgres"},
        ]
        for plan in plans:
            with self.subTest(plan=plan):
                body = json.dumps({"choices": [{
                    "finish_reason": "stop", "message": {"content": json.dumps(plan)},
                }]}).encode()
                self.opener.open.reset_mock()
                self.assert_no_fallback(body)

    def test_malformed_envelopes_and_duplicate_keys_are_rejected(self):
        bodies = [
            b"not-json", b"\xff", b"", b"null", b"[]", b"{}",
            b'{"choices":[{"finish_reason":"stop","message":',
            b'{"choices":[]}', b'{"choices":null}', b'{"choices":[null]}',
            b'{"choices":[{"finish_reason":"stop","message":{}}]}',
            b'{"choices":[{"finish_reason":"stop","message":{"content":null}}]}',
            b'{"choices":[{"finish_reason":"stop","message":{"content":"not-json"}}]}',
            b'{"choices":[],"choices":[]}',
            json.dumps({"choices": [{
                "finish_reason": "stop", "message": {
                    "content": '{"choice":"B","choice":"A","reason":"duplicate"}',
                },
            }]}).encode(),
        ]
        for body in bodies:
            with self.subTest(body=body):
                self.opener.open.reset_mock()
                self.assert_no_fallback(body)

    def test_truncated_tool_call_and_content_filter_completion_are_rejected(self):
        for reason in ("length", "content_filter", "tool_calls", None, ""):
            with self.subTest(finish_reason=reason):
                self.opener.open.reset_mock()
                self.assert_no_fallback(completion(finish_reason=reason))

    def test_explicit_refusal_is_rejected_even_if_content_looks_like_a_plan(self):
        self.assert_no_fallback(completion(refusal="I cannot provide a plan."))

    def test_transport_failures_are_explicit_and_sanitized(self):
        errors = [
            urllib.error.URLError("sensitive upstream detail"),
            urllib.error.HTTPError("https://api.openai.com", 401, "sensitive upstream detail", {}, None),
            TimeoutError("sensitive upstream detail"),
            OSError("sensitive upstream detail"),
            HTTPException("sensitive upstream detail"),
        ]
        for error in errors:
            if isinstance(error, urllib.error.HTTPError):
                self.addCleanup(error.close)
        for error in errors:
            with self.subTest(error=type(error).__name__):
                self.opener.open.reset_mock()
                self.opener.open.side_effect = error
                with self.assertRaises(PlannerFailure) as caught:
                    self.planner().choose(SCENARIOS["recognized-revenue"], publication())
                self.assertNotIn("sensitive", str(caught.exception))
                self.assertIn("no fallback", str(caught.exception))
                self.opener.open.assert_called_once()

    def test_second_plan_failure_does_not_return_partial_first_plan(self):
        self.response.read.side_effect = [completion({"choice": "B", "reason": "correct"}), b"bad-json"]
        with self.assertRaises(PlannerFailure):
            self.planner().choose(SCENARIOS["recognized-revenue"], publication())
        self.assertEqual(self.opener.open.call_count, 2)

    def test_incomplete_chunked_body_is_sanitized_and_never_returns_a_partial_plan(self):
        for failed_selection in (1, 2):
            with self.subTest(failed_selection=failed_selection):
                self.opener.open.reset_mock()
                error = IncompleteRead(b"private-partial-provider-body", 100)
                self.response.read.side_effect = [completion()] * (failed_selection - 1) + [error]
                with self.assertRaises(PlannerFailure) as caught:
                    self.planner().choose(SCENARIOS["recognized-revenue"], publication())
                self.assertIs(caught.exception.__cause__, error)
                self.assertNotIn("private", str(caught.exception))
                self.assertIn("no fallback", str(caught.exception))
                self.assertEqual(self.opener.open.call_count, failed_selection)
                self.response.read.assert_called_with(65_537)
                self.assertEqual(self.opener.open.return_value.__exit__.call_count, failed_selection)

    def test_deeply_nested_envelope_or_plan_is_rejected_without_fallback(self):
        nested = "[" * 2048 + "0" + "]" * 2048
        nested_plan = json.dumps({"choices": [{
            "finish_reason": "stop", "message": {"content": nested},
        }]}).encode()
        for body in (nested.encode(), nested_plan):
            with self.subTest(stage="envelope" if body == nested.encode() else "plan"):
                self.opener.open.reset_mock()
                self.assert_no_fallback(body)

    def test_decoder_recursion_failure_at_either_json_layer_is_explicit(self):
        envelope = json.loads(completion())
        for failed_layer in ("envelope", "plan"):
            with self.subTest(failed_layer=failed_layer):
                self.opener.open.reset_mock()
                error = RecursionError("private-decoder-stack-details")
                results = [error] if failed_layer == "envelope" else [envelope, error]
                with patch("planner.json.loads", side_effect=results):
                    with self.assertRaises(PlannerFailure) as caught:
                        self.planner().choose(SCENARIOS["recognized-revenue"], publication())
                self.assertIs(caught.exception.__cause__, error)
                self.assertNotIn("private", str(caught.exception))
                self.assertIn("no fallback", str(caught.exception))
                self.opener.open.assert_called_once()


if __name__ == "__main__":
    unittest.main()
