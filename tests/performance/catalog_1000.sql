\set ON_ERROR_STOP on

DROP SCHEMA IF EXISTS scale_catalog CASCADE;
CREATE SCHEMA scale_catalog;

DO $$
DECLARE
  relation_number integer;
BEGIN
  FOR relation_number IN 1..1000 LOOP
    EXECUTE format(
      'CREATE TABLE scale_catalog.model_%s (
         id bigint PRIMARY KEY,
         tenant_id bigint NOT NULL,
         amount numeric(18,2),
         created_at timestamptz NOT NULL
       )',
      lpad(relation_number::text, 4, '0')
    );
  END LOOP;
END;
$$;
