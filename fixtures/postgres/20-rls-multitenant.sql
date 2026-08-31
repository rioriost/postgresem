\set ON_ERROR_STOP on

CREATE SCHEMA rls_fixture;

CREATE TABLE rls_fixture.orders (
  order_id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant_id text NOT NULL CHECK (tenant_id IN ('tenant_a', 'tenant_b')),
  amount numeric(18, 2) NOT NULL CHECK (amount >= 0)
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

GRANT USAGE ON SCHEMA rls_fixture TO postgresem_tenant_a, postgresem_tenant_b;
GRANT SELECT ON rls_fixture.orders TO postgresem_tenant_a, postgresem_tenant_b;

INSERT INTO rls_fixture.orders (tenant_id, amount)
VALUES
  ('tenant_a', 100.00),
  ('tenant_a', 150.00),
  ('tenant_b', 999.00);

