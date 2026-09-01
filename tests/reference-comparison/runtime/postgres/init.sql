CREATE SCHEMA commerce;

CREATE TABLE commerce.orders (
  order_id bigint PRIMARY KEY,
  customer_id bigint NOT NULL,
  ordered_at timestamptz NOT NULL,
  status text NOT NULL,
  amount numeric(18, 2) NOT NULL
);

CREATE TABLE commerce.metricflow_time_spine (
  date_day date PRIMARY KEY
);

INSERT INTO commerce.orders (
  order_id,
  customer_id,
  ordered_at,
  status,
  amount
)
VALUES
  (1, 1, '2026-01-15T10:00:00Z', 'paid', 120.00),
  (2, 1, '2026-02-10T11:30:00Z', 'paid', 80.50),
  (3, 2, '2026-02-12T09:15:00Z', 'pending', 45.00),
  (4, 3, '2026-03-01T15:45:00Z', 'cancelled', 300.00);

INSERT INTO commerce.metricflow_time_spine (date_day)
SELECT day::date
FROM generate_series(
  '2025-01-01'::date,
  '2027-12-31'::date,
  interval '1 day'
) AS day;

CREATE ROLE semantic_user LOGIN PASSWORD 'semantic-runtime';
GRANT CONNECT ON DATABASE reference TO semantic_user;
GRANT USAGE ON SCHEMA commerce TO semantic_user;
GRANT SELECT ON
  commerce.orders,
  commerce.metricflow_time_spine
TO semantic_user;
