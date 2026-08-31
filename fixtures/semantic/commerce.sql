\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

DO $$
DECLARE
  v_project_id uuid := '10000000-0000-0000-0000-000000000001';
  v_revision_id uuid := '10000000-0000-0000-0000-000000000002';
  v_hash text := 'sha256:b88fb0ed27ee611f69fa81deb28167a57e720606bcd26ccb224d24715fb90bbd';
BEGIN
  INSERT INTO semantic.project (project_id, semantic_name, display_name, description)
  VALUES (
    v_project_id,
    'commerce',
    'Commerce development semantics',
    'Idempotent development fixture matching fixtures/evals/m0-semantic-snapshot.json'
  )
  ON CONFLICT (semantic_name) DO NOTHING;

  SELECT project_id
  INTO v_project_id
  FROM semantic.project
  WHERE semantic_name = 'commerce';

  IF EXISTS (
    SELECT 1
    FROM semantic.revision
    WHERE project_id = v_project_id
      AND status = 'published'
      AND canonical_hash = v_hash
  ) THEN
    RETURN;
  END IF;

  IF EXISTS (
    SELECT 1
    FROM semantic.revision
    WHERE revision_id = v_revision_id
       OR (project_id = v_project_id AND revision_number = 1)
       OR (project_id = v_project_id AND canonical_hash = v_hash)
  ) THEN
    RAISE EXCEPTION 'commerce semantic seed conflicts with an unexpected existing revision';
  END IF;

  IF EXISTS (
    SELECT 1
    FROM semantic.revision
    WHERE project_id = v_project_id AND status = 'published'
  ) THEN
    RAISE EXCEPTION 'commerce project already has a different published revision';
  END IF;

  INSERT INTO semantic.revision (
    revision_id,
    project_id,
    revision_number,
    status,
    schema_version,
    canonical_hash,
    compiler_semantic_version
  )
  VALUES (v_revision_id, v_project_id, 1, 'draft', '1', v_hash, '0.1.0');

  INSERT INTO semantic.model (
    model_id, revision_id, semantic_name, display_name, model_kind,
    source_database, source_schema, source_relation, source_relation_kind,
    default_timezone, queryable
  )
  VALUES
    ('10000000-0000-0000-0000-000000000010', v_revision_id, 'orders', 'Orders', 'fact',
     current_database(), 'commerce', 'orders', 'table', 'UTC', true),
    ('10000000-0000-0000-0000-000000000011', v_revision_id, 'customers', 'Customers', 'dimension',
     current_database(), 'commerce', 'customer', 'table', NULL, false),
    ('10000000-0000-0000-0000-000000000013', v_revision_id, 'tenant_orders', 'Tenant orders', 'fact',
     current_database(), 'rls_fixture', 'orders', 'table', NULL, true),
    ('10000000-0000-0000-0000-000000000014', v_revision_id, 'subscriptions', 'Subscriptions', 'fact',
     current_database(), 'billing', 'subscriptions', 'table', 'UTC', true);

  INSERT INTO semantic.field (
    field_id, revision_id, model_id, semantic_name, display_name, field_kind,
    logical_type, source_column, nullable, hidden
  )
  VALUES
    ('10000000-0000-0000-0000-000000000101', v_revision_id, '10000000-0000-0000-0000-000000000010', 'order_id', 'Order ID', 'entity_key', 'integer', 'order_id', false, false),
    ('10000000-0000-0000-0000-000000000102', v_revision_id, '10000000-0000-0000-0000-000000000010', 'customer_id', 'Customer ID', 'dimension', 'integer', 'customer_id', false, false),
    ('10000000-0000-0000-0000-000000000103', v_revision_id, '10000000-0000-0000-0000-000000000010', 'ordered_at', 'Ordered at', 'time_dimension', 'timestamp_tz', 'ordered_at', false, false),
    ('10000000-0000-0000-0000-000000000104', v_revision_id, '10000000-0000-0000-0000-000000000010', 'status', 'Status', 'dimension', 'text', 'status', false, false),
    ('10000000-0000-0000-0000-000000000105', v_revision_id, '10000000-0000-0000-0000-000000000010', 'amount', 'Amount', 'dimension', 'numeric', 'amount', false, false),
    ('10000000-0000-0000-0000-000000000111', v_revision_id, '10000000-0000-0000-0000-000000000011', 'customer_id', 'Customer ID', 'entity_key', 'integer', 'customer_id', false, false),
    ('10000000-0000-0000-0000-000000000112', v_revision_id, '10000000-0000-0000-0000-000000000011', 'region', 'Region', 'dimension', 'text', 'region', false, false),
    ('10000000-0000-0000-0000-000000000113', v_revision_id, '10000000-0000-0000-0000-000000000011', 'credit_limit', 'Credit limit', 'dimension', 'numeric', 'credit_limit', false, false),
    ('10000000-0000-0000-0000-000000000131', v_revision_id, '10000000-0000-0000-0000-000000000013', 'order_id', 'Order ID', 'entity_key', 'integer', 'order_id', false, false),
    ('10000000-0000-0000-0000-000000000132', v_revision_id, '10000000-0000-0000-0000-000000000013', 'tenant_id', 'Tenant ID', 'dimension', 'text', 'tenant_id', false, false),
    ('10000000-0000-0000-0000-000000000133', v_revision_id, '10000000-0000-0000-0000-000000000013', 'amount', 'Amount', 'dimension', 'numeric', 'amount', false, false),
    ('10000000-0000-0000-0000-000000000141', v_revision_id, '10000000-0000-0000-0000-000000000014', 'subscription_id', 'Subscription ID', 'entity_key', 'integer', 'subscription_id', false, false),
    ('10000000-0000-0000-0000-000000000142', v_revision_id, '10000000-0000-0000-0000-000000000014', 'account_id', 'Account ID', 'dimension', 'integer', 'account_id', false, false),
    ('10000000-0000-0000-0000-000000000143', v_revision_id, '10000000-0000-0000-0000-000000000014', 'started_on', 'Started on', 'time_dimension', 'date', 'started_on', false, false),
    ('10000000-0000-0000-0000-000000000144', v_revision_id, '10000000-0000-0000-0000-000000000014', 'plan', 'Plan', 'dimension', 'text', 'plan', false, false),
    ('10000000-0000-0000-0000-000000000145', v_revision_id, '10000000-0000-0000-0000-000000000014', 'monthly_amount', 'Monthly amount', 'dimension', 'numeric', 'monthly_amount', false, false),
    ('10000000-0000-0000-0000-000000000146', v_revision_id, '10000000-0000-0000-0000-000000000014', 'active', 'Active', 'dimension', 'boolean', 'active', false, false);

  INSERT INTO semantic.relationship (
    relationship_id, revision_id, semantic_name, from_model_id, to_model_id,
    cardinality, join_type, allowed_direction, priority
  )
  VALUES
    ('10000000-0000-0000-0000-000000000201', v_revision_id, 'customer',
     '10000000-0000-0000-0000-000000000010', '10000000-0000-0000-0000-000000000011',
     'many_to_one', 'left', 'forward', 0);

  INSERT INTO semantic.relationship_column (
    relationship_id, revision_id, ordinal, from_field_id, to_field_id
  )
  VALUES
    ('10000000-0000-0000-0000-000000000201', v_revision_id, 1,
     '10000000-0000-0000-0000-000000000102', '10000000-0000-0000-0000-000000000111');

  INSERT INTO semantic.field (
    field_id, revision_id, model_id, semantic_name, display_name, field_kind,
    logical_type, source_column, source_relationship_id, nullable, hidden
  )
  VALUES
    ('10000000-0000-0000-0000-000000000106', v_revision_id, '10000000-0000-0000-0000-000000000010', 'customer_region', 'Customer region', 'dimension', 'text', 'region', '10000000-0000-0000-0000-000000000201', false, false),
    ('10000000-0000-0000-0000-000000000107', v_revision_id, '10000000-0000-0000-0000-000000000010', 'customer_credit_limit', 'Customer credit limit', 'dimension', 'numeric', 'credit_limit', '10000000-0000-0000-0000-000000000201', false, false);

  INSERT INTO semantic.metric (
    metric_id, revision_id, model_id, semantic_name, display_name, result_type,
    expression, metric_filter, additivity, hidden
  )
  VALUES
    ('10000000-0000-0000-0000-000000000301', v_revision_id, '10000000-0000-0000-0000-000000000010', 'order_count', 'Order count', 'integer',
     '{"version":"1","kind":"aggregation","aggregation":"count","field":"order_id"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000302', v_revision_id, '10000000-0000-0000-0000-000000000010', 'revenue', 'Revenue', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}', '{"field":"status","value":{"type":"text","value":"paid"}}', 'additive', false),
    ('10000000-0000-0000-0000-000000000303', v_revision_id, '10000000-0000-0000-0000-000000000010', 'average_order_value', 'Average order value', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"avg","field":"amount"}', '{"field":"status","value":{"type":"text","value":"paid"}}', 'non_additive', false),
    ('10000000-0000-0000-0000-000000000304', v_revision_id, '10000000-0000-0000-0000-000000000010', 'distinct_orders', 'Distinct orders', 'integer',
     '{"version":"1","kind":"aggregation","aggregation":"count_distinct","field":"order_id"}', NULL, 'non_additive', false),
    ('10000000-0000-0000-0000-000000000311', v_revision_id, '10000000-0000-0000-0000-000000000013', 'order_count', 'Order count', 'integer',
     '{"version":"1","kind":"aggregation","aggregation":"count","field":"order_id"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000312', v_revision_id, '10000000-0000-0000-0000-000000000013', 'revenue', 'Revenue', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000321', v_revision_id, '10000000-0000-0000-0000-000000000014', 'subscription_count', 'Subscription count', 'integer',
     '{"version":"1","kind":"aggregation","aggregation":"count","field":"subscription_id"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000322', v_revision_id, '10000000-0000-0000-0000-000000000014', 'mrr', 'MRR', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"sum","field":"monthly_amount"}', '{"field":"active","value":{"type":"boolean","value":true}}', 'additive', false);

  UPDATE semantic.revision
  SET status = 'published', published_at = clock_timestamp()
  WHERE revision_id = v_revision_id;
END;
$$;

COMMIT;
