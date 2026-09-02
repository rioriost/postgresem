\set ON_ERROR_STOP on

CREATE SCHEMA commerce;

CREATE TABLE commerce.customer (
  customer_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  customer_name text NOT NULL,
  region text NOT NULL CHECK (region IN ('apac', 'emea', 'amer')),
  credit_limit numeric(18, 2) NOT NULL CHECK (credit_limit >= 0)
);

COMMENT ON TABLE commerce.customer IS 'Customers that place commerce orders.';
COMMENT ON COLUMN commerce.customer.region IS 'Reporting region for the customer.';

CREATE TABLE commerce.orders (
  order_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  external_id text NOT NULL UNIQUE,
  customer_id bigint NOT NULL REFERENCES commerce.customer(customer_id),
  ordered_at timestamptz NOT NULL,
  status text NOT NULL CHECK (status IN ('pending', 'paid', 'cancelled')),
  amount numeric(18, 2) NOT NULL CHECK (amount >= 0)
);

COMMENT ON TABLE commerce.orders IS 'One row per customer order.';
COMMENT ON COLUMN commerce.orders.amount IS 'Order amount in the fixture currency.';

CREATE TABLE commerce.order_item (
  order_item_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  order_id bigint NOT NULL REFERENCES commerce.orders(order_id),
  sku text NOT NULL,
  quantity integer NOT NULL CHECK (quantity > 0)
);

CREATE TABLE commerce.order_tag (
  order_tag_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  order_id bigint NOT NULL REFERENCES commerce.orders(order_id),
  tag text NOT NULL
);

INSERT INTO commerce.customer (customer_name, region, credit_limit)
VALUES
  ('Aster Trading', 'apac', 1000.00),
  ('Birch Retail', 'emea', 2000.00),
  ('Cedar Market', 'amer', 3000.00);

INSERT INTO commerce.orders (external_id, customer_id, ordered_at, status, amount)
VALUES
  ('fixture-order-1', 1, '2026-01-15T10:00:00Z', 'paid', 120.00),
  ('fixture-order-2', 1, '2026-02-10T11:30:00Z', 'paid', 80.50),
  ('fixture-order-3', 2, '2026-02-12T09:15:00Z', 'pending', 45.00),
  ('fixture-order-4', 3, '2026-03-01T15:45:00Z', 'cancelled', 300.00);

INSERT INTO commerce.order_item (order_id, sku, quantity)
VALUES
  (1, 'SKU-RED', 1),
  (1, 'SKU-BLUE', 2),
  (1, 'SKU-RED', 1),
  (2, 'SKU-GREEN', 1),
  (2, 'SKU-RED', 1),
  (3, 'SKU-RED', 1);

INSERT INTO commerce.order_tag (order_id, tag)
VALUES
  (1, 'priority'),
  (1, 'priority'),
  (2, 'standard'),
  (3, 'review');

ALTER TABLE commerce.customer OWNER TO postgresem_source_owner;
ALTER TABLE commerce.orders OWNER TO postgresem_source_owner;
ALTER TABLE commerce.order_item OWNER TO postgresem_source_owner;
ALTER TABLE commerce.order_tag OWNER TO postgresem_source_owner;

GRANT USAGE ON SCHEMA commerce TO postgresem_source_owner;
GRANT USAGE ON SCHEMA commerce TO postgresem_analyst;
GRANT SELECT ON ALL TABLES IN SCHEMA commerce TO postgresem_analyst;

GRANT USAGE ON SCHEMA commerce TO postgresem_order_writer;
GRANT INSERT (external_id, customer_id, ordered_at, status, amount)
  ON commerce.orders TO postgresem_order_writer;
GRANT UPDATE (customer_id, ordered_at, status, amount)
  ON commerce.orders TO postgresem_order_writer;
GRANT SELECT (order_id, external_id, customer_id, ordered_at, status, amount)
  ON commerce.orders TO postgresem_order_writer;
GRANT USAGE, SELECT ON SEQUENCE commerce.orders_order_id_seq
  TO postgresem_order_writer;
