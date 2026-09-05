"""Opt-in, candidate-limited OpenAI planning; never sends source rows."""

import json
import os
from http.client import HTTPException
from pathlib import Path
import re
import urllib.error
import urllib.request

from scenarios import FIXTURE_REVISION
from smoke import SmokeFailure, unique_object

SETTINGS = ("POSTGRESEM_DEMO_OPENAI", "OPENAI_API_KEY", "OPENAI_MODEL")


class PlannerFailure(SmokeFailure):
    pass


PHYSICAL_SCHEMA = {
    "commerce.orders": {"order_id": "bigint primary key", "external_id": "text unique",
                        "status": "pending | paid | cancelled", "amount": "numeric(18,2)"},
    "commerce.order_item": {"order_item_id": "bigint primary key",
                           "order_id": "bigint references commerce.orders",
                           "sku": "text", "quantity": "integer"},
    "billing.subscriptions": {"subscription_id": "bigint primary key", "plan": "text",
                              "monthly_amount": "numeric(18,2)", "active": "boolean"},
}


def load_settings(path: Path):
    """Only three literal settings, not shell sourcing or general dotenv expansion."""
    values = {}
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            key, separator, value = line.partition("=")
            key = key.strip()
            if not separator or key not in SETTINGS:
                continue
            if key in values:
                raise SmokeFailure("duplicate demo OpenAI setting in .env")
            value = value.strip()
            if value.startswith(("'", '"')):
                if len(value) < 2 or value[-1] != value[0]:
                    raise SmokeFailure("invalid quoted demo OpenAI setting")
                value = value[1:-1]
            values[key] = value
    return {key: os.environ.get(key, values.get(key, "")) for key in SETTINGS}


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise urllib.error.HTTPError(req.full_url, code, "redirect rejected", headers, fp)


class Planner:
    def __init__(self, settings):
        self.enabled = settings.get("POSTGRESEM_DEMO_OPENAI") == "1"
        self.key = settings.get("OPENAI_API_KEY", "")
        self.model = settings.get("OPENAI_MODEL") or "gpt-4.1-mini"
        if self.enabled and (
                not self.key or not re.fullmatch(r"[!-~]+", self.key)
                or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,100}", self.model)):
            raise SmokeFailure("OpenAI demo opt-in requires a valid API key and model")
        # Do not forward the authorization header through environment-configured proxies.
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), NoRedirects(),
        )

    def choose(self, scenario, catalog):
        if not self.enabled:
            raise PlannerFailure("live planner is disabled")
        if catalog.get("semantic_revision") != FIXTURE_REVISION:
            raise PlannerFailure("live planner only accepts the bundled fictional publication")
        baseline = self._select(scenario["question"], PHYSICAL_SCHEMA, scenario["baseline"])
        semantic = self._select(scenario["question"], catalog["model"], scenario["semantic"])
        return baseline, semantic

    def _select(self, question, context, candidates):
        payload = {
            "model": self.model, "store": False, "max_completion_tokens": 600,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": (
                    "Choose the plan that best answers the question using only the given "
                    "context. Plans are restricted candidates, not examples to imitate. "
                    "Return JSON with exactly two string keys: choice (candidate ID), "
                    "reason (brief explanation). Do not invent data or results."
                )},
                {"role": "user", "content": json.dumps({
                    "question": question, "context": context, "candidates": candidates,
                })},
            ],
        }
        request = urllib.request.Request(
            "https://api.openai.com/v1/chat/completions",
            data=json.dumps(payload).encode(),
            headers={"Authorization": f"Bearer {self.key}", "Content-Type": "application/json"},
            method="POST",
        )
        try:
            with self.opener.open(request, timeout=30) as response:
                body = response.read(65_537)
            if len(body) > 65_536:
                raise PlannerFailure("OpenAI planner response exceeds demo limits")
            envelope = json.loads(body, object_pairs_hook=unique_object)
            completion = envelope["choices"][0]
            if completion["finish_reason"] != "stop":
                raise PlannerFailure("OpenAI planner did not finish a plan")
            if completion["message"].get("refusal") is not None:
                raise PlannerFailure("OpenAI declined to select a plan; nothing was executed")
            plan = json.loads(completion["message"]["content"], object_pairs_hook=unique_object)
        except (urllib.error.URLError, HTTPException, TimeoutError, OSError, ValueError,
                KeyError, IndexError, TypeError, RecursionError) as error:
            raise PlannerFailure("OpenAI planning failed; no fallback plan was executed") from error
        if (not isinstance(plan, dict) or set(plan) != {"choice", "reason"}
                or not isinstance(plan["choice"], str) or plan["choice"] not in candidates
                or not isinstance(plan["reason"], str) or not 1 <= len(plan["reason"]) <= 1000):
            raise PlannerFailure("OpenAI returned an unsupported plan; nothing was executed")
        return plan
