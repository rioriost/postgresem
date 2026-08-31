\set ON_ERROR_STOP on

CREATE SCHEMA commerce;

CREATE TABLE commerce.customer (
  customer_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  customer_name text NOT NULL,
  region text NOT NULL CHECK (region IN ('apac', 'emea', 'amer'))
);

COMMENT ON TABLE commerce.customer IS 'Customers that place commerce orders.';
COMMENT ON COLUMN commerce.customer.region IS 'Reporting region for the customer.';

CREATE TABLE commerce.orders (
  order_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  customer_id bigint NOT NULL REFERENCES commerce.customer(customer_id),
  ordered_at timestamptz NOT NULL,
  status text NOT NULL CHECK (status IN ('pending', 'paid', 'cancelled')),
  amount numeric(18, 2) NOT NULL CHECK (amount >= 0)
);

COMMENT ON TABLE commerce.orders IS 'One row per customer order.';
COMMENT ON COLUMN commerce.orders.amount IS 'Order amount in the fixture currency.';

INSERT INTO commerce.customer (customer_name, region)
VALUES
  ('Aster Trading', 'apac'),
  ('Birch Retail', 'emea'),
  ('Cedar Market', 'amer');

INSERT INTO commerce.orders (customer_id, ordered_at, status, amount)
VALUES
  (1, '2026-01-15T10:00:00Z', 'paid', 120.00),
  (1, '2026-02-10T11:30:00Z', 'paid', 80.50),
  (2, '2026-02-12T09:15:00Z', 'pending', 45.00),
  (3, '2026-03-01T15:45:00Z', 'cancelled', 300.00);

