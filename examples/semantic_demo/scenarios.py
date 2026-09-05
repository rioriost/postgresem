"""Closed fictional-data scenarios, not an arbitrary SQL execution API."""

from copy import deepcopy
from decimal import Decimal


ORDER_IDS = [
    "fixture-order-1", "fixture-order-2", "fixture-order-3", "fixture-order-4",
    "meaning-lab-paid-order",
]
ORDER_SCOPE = "o.external_id IN (" + ", ".join(f"'{key}'" for key in ORDER_IDS) + ")"
ORDER_FILTER = {
    "op": "in", "field": "external_id",
    "values": [{"type": "text", "value": key} for key in ORDER_IDS],
}
FIXTURE_REVISION = "sha256:dc6fe2f9a25e995dc1bf8a8d156ea245e05e2a9232b2613d9e960dd63b11150f"
DISCLAIMER = (
    "The default SQL plans are authored examples of plausible planning mistakes, "
    "not measured LLM outputs. Both paths execute on the same PostgreSQL data "
    "under the same reader role. Correct SQL can also produce the correct answer. "
    "The optional live planner selects from reviewed plans, including correct SQL; "
    "it is not an unrestricted text-to-SQL benchmark."
)


def order_query(metric="revenue", *, sku=False):
    filters = deepcopy(ORDER_FILTER)
    if sku:
        filters = {
            "op": "and",
            "args": [
                filters,
                {"op": "eq", "field": "item_sku",
                 "value": {"type": "text", "value": "SKU-RED"}},
            ],
        }
    return {
        "schema_version": "1", "model": "orders",
        "metrics": [{"metric": metric}], "filters": filters, "limit": 10,
    }


SCENARIOS = {
    "recognized-revenue": {
        "id": "recognized-revenue",
        "title": "Recognized revenue / paid orders only",
        "question": "What is recognized order revenue in the demonstration ledger?",
        "pitfall": "Summing the amount column counts pending and cancelled orders.",
        "business_rule": "Recognize the full order amount only when status is paid.",
        "model": "orders",
        "baseline": {
            "A": f"SELECT sum(o.amount)::text FROM commerce.orders o WHERE {ORDER_SCOPE}",
            "B": f"SELECT sum(o.amount)::text FROM commerce.orders o WHERE {ORDER_SCOPE} AND o.status = 'paid'",
        },
        "semantic": {"A": order_query(), "B": order_query("average_order_value")},
    },
    "sku-fanout": {
        "id": "sku-fanout",
        "title": "SKU-RED / one order, several matching items",
        "question": "What is recognized revenue of orders containing SKU-RED? Count each order once.",
        "pitfall": "Joining matching line items multiplies the order amount.",
        "business_rule": "Paid orders containing SKU-RED contribute their full amount once, at order_id grain; this is not allocated SKU sales.",
        "model": "orders",
        "baseline": {
            "A": f"SELECT sum(o.amount)::text FROM commerce.orders o JOIN commerce.order_item i ON i.order_id = o.order_id WHERE {ORDER_SCOPE} AND o.status = 'paid' AND i.sku = 'SKU-RED'",
            "B": f"SELECT sum(o.amount)::text FROM commerce.orders o WHERE {ORDER_SCOPE} AND o.status = 'paid' AND EXISTS (SELECT 1 FROM commerce.order_item i WHERE i.order_id = o.order_id AND i.sku = 'SKU-RED')",
        },
        "semantic": {"A": order_query(sku=True), "B": order_query("order_count", sku=True)},
    },
    "active-mrr": {
        "id": "active-mrr",
        "title": "MRR / active subscriptions",
        "question": "What is the current MRR for demonstration subscriptions 1, 2 and 3?",
        "pitfall": "A subscription row can remain after cancellation; summing every monthly_amount includes inactive contracts.",
        "business_rule": "Only active subscriptions contribute monthly recurring revenue.",
        "model": "subscriptions",
        "baseline": {
            "A": "SELECT sum(monthly_amount)::text FROM billing.subscriptions WHERE subscription_id IN (1, 2, 3)",
            "B": "SELECT sum(monthly_amount)::text FROM billing.subscriptions WHERE subscription_id IN (1, 2, 3) AND active",
        },
        "semantic": {
            choice: {
                "schema_version": "1", "model": "subscriptions",
                "metrics": [{"metric": metric}],
                "filters": {"op": "in", "field": "subscription_id",
                            "values": [{"type": "integer", "value": key} for key in (1, 2, 3)]},
                "limit": 10,
            }
            for choice, metric in (("A", "mrr"), ("B", "subscription_count"))
        },
    },
}

PAID_ORDER = {
    "schema_version": "1", "operation": "insert", "model": "orders",
    "idempotency_key": "meaning-lab-paid-order-v1",
    "rows": [{
        "external_id": {"type": "text", "value": "meaning-lab-paid-order"},
        "customer_id": {"type": "integer", "value": 1},
        "ordered_at": {"type": "timestamp", "value": "2026-09-01T09:00:00Z"},
        "status": {"type": "text", "value": "paid"},
        "amount": {"type": "numeric", "value": "45.00"},
    }],
}


def public_scenario(scenario):
    return {key: scenario[key] for key in ("id", "title", "question", "pitfall", "business_rule")}


def expected_answer(scenario_id, source):
    """Independent ledger arithmetic: no compiler output or baseline result."""
    if scenario_id == "active-mrr":
        rows = [row for row in source["subscriptions"] if row["active"]]
        values = [Decimal(row["monthly_amount"]) for row in rows]
        explanation = "Sum monthly amounts of active subscriptions only."
    else:
        rows = [row for row in source["orders"] if row["status"] == "paid"]
        if scenario_id == "sku-fanout":
            matching_orders = {
                row["order_id"] for row in source["items"] if row["sku"] == "SKU-RED"
            }
            rows = [row for row in rows if row["order_id"] in matching_orders]
        values = [Decimal(row["amount"]) for row in rows]
        explanation = " + ".join(str(value) for value in values) + " (each qualifying order once)"
    return {
        "value": format(sum(values, Decimal(0)), ".2f"),
        "derivation": explanation,
        "contributing_rows": rows,
    }
