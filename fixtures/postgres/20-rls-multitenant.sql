\set ON_ERROR_STOP on

CREATE SCHEMA rls_fixture;

CREATE TABLE rls_fixture.orders (
  order_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  external_id text NOT NULL,
  tenant_id text NOT NULL CHECK (tenant_id IN ('tenant_a', 'tenant_b')),
  amount numeric(18, 2) NOT NULL CHECK (amount >= 0),
  UNIQUE (tenant_id, external_id)
);

ALTER TABLE rls_fixture.orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE rls_fixture.orders FORCE ROW LEVEL SECURITY;

CREATE POLICY tenant_a_orders ON rls_fixture.orders
  FOR SELECT
  TO postgresem_tenant_a
  USING (tenant_id = 'tenant_a');

CREATE POLICY tenant_b_orders ON rls_fixture.orders
  FOR SELECT
  TO postgresem_tenant_b
  USING (tenant_id = 'tenant_b');

CREATE POLICY tenant_a_orders_write ON rls_fixture.orders
  FOR ALL
  TO postgresem_tenant_a_writer
  USING (tenant_id = 'tenant_a')
  WITH CHECK (tenant_id = 'tenant_a');

CREATE POLICY tenant_b_orders_write ON rls_fixture.orders
  FOR ALL
  TO postgresem_tenant_b_writer
  USING (tenant_id = 'tenant_b')
  WITH CHECK (tenant_id = 'tenant_b');

GRANT USAGE ON SCHEMA rls_fixture TO
  postgresem_tenant_a,
  postgresem_tenant_b,
  postgresem_tenant_a_writer,
  postgresem_tenant_b_writer;
GRANT SELECT ON rls_fixture.orders TO postgresem_tenant_a, postgresem_tenant_b;
GRANT SELECT (order_id, external_id, tenant_id, amount),
  INSERT (external_id, tenant_id, amount),
  UPDATE (amount)
ON rls_fixture.orders TO postgresem_tenant_a_writer, postgresem_tenant_b_writer;
GRANT USAGE, SELECT ON SEQUENCE rls_fixture.orders_order_id_seq
  TO postgresem_tenant_a_writer, postgresem_tenant_b_writer;

INSERT INTO rls_fixture.orders (external_id, tenant_id, amount)
VALUES
  ('fixture-a-1', 'tenant_a', 100.00),
  ('fixture-a-2', 'tenant_a', 150.00),
  ('fixture-b-1', 'tenant_b', 999.00);

ALTER TABLE rls_fixture.orders OWNER TO postgresem_source_owner;
GRANT USAGE ON SCHEMA rls_fixture TO postgresem_source_owner;
