\set ON_ERROR_STOP on

PREPARE compiled_revenue_by_month(text, text, text, text) AS
SELECT
  date_trunc('month', timezone($1::text, t0."ordered_at"))::date AS "ordered_at",
  sum(t0."amount") FILTER (WHERE t0."status" = $2::text) AS "revenue"
FROM "commerce"."orders" AS t0
WHERE t0."ordered_at" >= timezone($1::text, $3::text::date::timestamp)
GROUP BY 1
ORDER BY "revenue" DESC
LIMIT $4::text::bigint;

SET TimeZone = 'Pacific/Honolulu';
EXECUTE compiled_revenue_by_month('UTC', 'paid', '2026-01-01', 100);
RESET TimeZone;
DEALLOCATE compiled_revenue_by_month;

DO $$
DECLARE
  paid_revenue numeric;
  status_count bigint;
  active_mrr numeric;
  month_count bigint;
  empty_metric_month_count bigint;
  naive_red_revenue numeric;
  anchored_red_revenue numeric;
  priority_pair_count bigint;
BEGIN
  SELECT sum(amount) FILTER (WHERE status = 'paid')
  INTO paid_revenue
  FROM commerce.orders;

  IF paid_revenue <> 200.50 THEN
    RAISE EXCEPTION 'commerce paid revenue mismatch: %', paid_revenue;
  END IF;

  SELECT count(*)
  INTO status_count
  FROM (SELECT status FROM commerce.orders GROUP BY status) AS grouped_status;

  IF status_count <> 3 THEN
    RAISE EXCEPTION 'commerce dimension grouping mismatch: %', status_count;
  END IF;

  SELECT sum(monthly_amount) FILTER (WHERE active)
  INTO active_mrr
  FROM billing.subscriptions;

  IF active_mrr <> 128.00 THEN
    RAISE EXCEPTION 'subscription active MRR mismatch: %', active_mrr;
  END IF;

  SELECT
    count(*),
    count(*) FILTER (WHERE ordered_at = '2026-03-01' AND revenue IS NULL)
  INTO month_count, empty_metric_month_count
  FROM (
    SELECT
      date_trunc('month', timezone('UTC', ordered_at))::date AS ordered_at,
      sum(amount) FILTER (WHERE status = 'paid') AS revenue
    FROM commerce.orders
    GROUP BY 1
  ) AS monthly_revenue;

  IF month_count <> 3 OR empty_metric_month_count <> 1 THEN
    RAISE EXCEPTION
      'filtered aggregate NULL semantics mismatch: months %, empty %',
      month_count,
      empty_metric_month_count;
  END IF;

  SELECT sum(orders.amount) FILTER (WHERE orders.status = 'paid')
  INTO naive_red_revenue
  FROM commerce.orders AS orders
  JOIN commerce.order_item AS item
    ON item.order_id = orders.order_id
  WHERE item.sku = 'SKU-RED';

  SELECT sum(amount)
  INTO anchored_red_revenue
  FROM (
    SELECT orders.order_id, max(orders.amount) AS amount
    FROM commerce.orders AS orders
    JOIN commerce.order_item AS item
      ON item.order_id = orders.order_id
    WHERE item.sku = 'SKU-RED'
      AND orders.status = 'paid'
    GROUP BY orders.order_id
  ) AS anchored_orders;

  IF naive_red_revenue <> 320.50 OR anchored_red_revenue <> 200.50 THEN
    RAISE EXCEPTION
      'fan-out oracle mismatch: naive %, anchored %',
      naive_red_revenue,
      anchored_red_revenue;
  END IF;

  SELECT count(*)
  INTO priority_pair_count
  FROM (
    SELECT item.sku, tag.tag, orders.order_id
    FROM commerce.orders AS orders
    JOIN commerce.order_item AS item
      ON item.order_id = orders.order_id
    JOIN commerce.order_tag AS tag
      ON tag.order_id = orders.order_id
    WHERE orders.status = 'paid'
      AND tag.tag = 'priority'
    GROUP BY item.sku, tag.tag, orders.order_id
  ) AS anchored_pairs;

  IF priority_pair_count <> 2 THEN
    RAISE EXCEPTION 'multi-branch anchor oracle mismatch: %', priority_pair_count;
  END IF;
END;
$$;

SELECT 'known-answer integration checks passed' AS result;
