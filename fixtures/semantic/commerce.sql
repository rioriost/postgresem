\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

DO $$
DECLARE
  v_project_id uuid := '10000000-0000-0000-0000-000000000001';
  v_revision_id uuid := '10000000-0000-0000-0000-000000000002';
  v_mutation_supported boolean := to_regclass('semantic.mutation_model') IS NOT NULL;
  v_anchor_supported boolean :=
    to_regprocedure('semantic.validate_metric_aggregation_anchor()') IS NOT NULL;
  v_hash text;
BEGIN
  v_hash := CASE
    WHEN v_anchor_supported
      THEN 'sha256:dc6fe2f9a25e995dc1bf8a8d156ea245e05e2a9232b2613d9e960dd63b11150f'
    WHEN v_mutation_supported
      THEN 'sha256:a731347152caed2f8f3dfcecb730aac12c93c839f8cc91e6f81099128f70e58c'
    ELSE 'sha256:806f8687c1e2161f65370e0c433832760c02b6f96f8b8bc6e93fde6295d29da6'
  END;
  INSERT INTO semantic.project (project_id, semantic_name, display_name, description)
  VALUES (
    v_project_id,
    'commerce',
    'Commerce development semantics',
    'Idempotent development fixture for guarded semantic query and mutation tests'
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
  VALUES (
    v_revision_id,
    v_project_id,
    1,
    'draft',
    CASE WHEN v_anchor_supported THEN '2' ELSE '1' END,
    v_hash,
    CASE WHEN v_anchor_supported THEN '0.2.0' ELSE '0.1.0' END
  );

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

  IF v_anchor_supported THEN
    INSERT INTO semantic.model (
      model_id, revision_id, semantic_name, display_name, model_kind,
      source_database, source_schema, source_relation, source_relation_kind,
      default_timezone, queryable
    )
    VALUES
      ('10000000-0000-0000-0000-000000000012', v_revision_id, 'order_items', 'Order items', 'dimension',
       current_database(), 'commerce', 'order_item', 'table', NULL, false),
      ('10000000-0000-0000-0000-000000000015', v_revision_id, 'order_tags', 'Order tags', 'dimension',
       current_database(), 'commerce', 'order_tag', 'table', NULL, false),
      ('10000000-0000-0000-0000-000000000016', v_revision_id, 'tenant_order_items', 'Tenant order items', 'dimension',
       current_database(), 'rls_fixture', 'order_item', 'table', NULL, false);
  END IF;

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
    ('10000000-0000-0000-0000-000000000108', v_revision_id, '10000000-0000-0000-0000-000000000010', 'internal_amount', 'Internal amount', 'dimension', 'numeric', 'amount', false, true),
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

  IF v_anchor_supported THEN
    INSERT INTO semantic.field (
      field_id, revision_id, model_id, semantic_name, display_name, field_kind,
      logical_type, source_column, nullable, hidden
    )
    VALUES
      ('10000000-0000-0000-0000-000000000121', v_revision_id, '10000000-0000-0000-0000-000000000012', 'order_item_id', 'Order item ID', 'entity_key', 'integer', 'order_item_id', false, false),
      ('10000000-0000-0000-0000-000000000122', v_revision_id, '10000000-0000-0000-0000-000000000012', 'order_id', 'Order ID', 'dimension', 'integer', 'order_id', false, false),
      ('10000000-0000-0000-0000-000000000123', v_revision_id, '10000000-0000-0000-0000-000000000012', 'sku', 'SKU', 'dimension', 'text', 'sku', false, false),
      ('10000000-0000-0000-0000-000000000151', v_revision_id, '10000000-0000-0000-0000-000000000015', 'order_tag_id', 'Order tag ID', 'entity_key', 'integer', 'order_tag_id', false, false),
      ('10000000-0000-0000-0000-000000000152', v_revision_id, '10000000-0000-0000-0000-000000000015', 'order_id', 'Order ID', 'dimension', 'integer', 'order_id', false, false),
      ('10000000-0000-0000-0000-000000000153', v_revision_id, '10000000-0000-0000-0000-000000000015', 'tag', 'Tag', 'dimension', 'text', 'tag', false, false);
    INSERT INTO semantic.field (
      field_id, revision_id, model_id, semantic_name, display_name, field_kind,
      logical_type, source_column, nullable, hidden
    )
    VALUES
      ('10000000-0000-0000-0000-000000000161', v_revision_id, '10000000-0000-0000-0000-000000000016', 'order_item_id', 'Order item ID', 'entity_key', 'integer', 'order_item_id', false, false),
      ('10000000-0000-0000-0000-000000000162', v_revision_id, '10000000-0000-0000-0000-000000000016', 'order_id', 'Order ID', 'dimension', 'integer', 'order_id', false, false),
      ('10000000-0000-0000-0000-000000000163', v_revision_id, '10000000-0000-0000-0000-000000000016', 'sku', 'SKU', 'dimension', 'text', 'sku', false, false);
  END IF;

  INSERT INTO semantic.relationship (
    relationship_id, revision_id, semantic_name, from_model_id, to_model_id,
    cardinality, join_type, allowed_direction, priority
  )
  VALUES
    ('10000000-0000-0000-0000-000000000201', v_revision_id, 'customer',
     '10000000-0000-0000-0000-000000000010', '10000000-0000-0000-0000-000000000011',
     'many_to_one', 'left', 'forward', 0);

  IF v_anchor_supported THEN
    INSERT INTO semantic.relationship (
      relationship_id, revision_id, semantic_name, from_model_id, to_model_id,
      cardinality, join_type, allowed_direction, priority
    )
    VALUES
      ('10000000-0000-0000-0000-000000000202', v_revision_id, 'items',
       '10000000-0000-0000-0000-000000000010', '10000000-0000-0000-0000-000000000012',
       'one_to_many', 'left', 'forward', 0),
      ('10000000-0000-0000-0000-000000000203', v_revision_id, 'tags',
       '10000000-0000-0000-0000-000000000010', '10000000-0000-0000-0000-000000000015',
       'one_to_many', 'left', 'forward', 0),
      ('10000000-0000-0000-0000-000000000204', v_revision_id, 'tenant_items',
       '10000000-0000-0000-0000-000000000013', '10000000-0000-0000-0000-000000000016',
       'one_to_many', 'left', 'forward', 0);
  END IF;

  INSERT INTO semantic.relationship_column (
    relationship_id, revision_id, ordinal, from_field_id, to_field_id
  )
  VALUES
    ('10000000-0000-0000-0000-000000000201', v_revision_id, 1,
     '10000000-0000-0000-0000-000000000102', '10000000-0000-0000-0000-000000000111');

  IF v_anchor_supported THEN
    INSERT INTO semantic.relationship_column (
      relationship_id, revision_id, ordinal, from_field_id, to_field_id
    )
    VALUES
      ('10000000-0000-0000-0000-000000000202', v_revision_id, 1,
       '10000000-0000-0000-0000-000000000101', '10000000-0000-0000-0000-000000000122'),
      ('10000000-0000-0000-0000-000000000203', v_revision_id, 1,
       '10000000-0000-0000-0000-000000000101', '10000000-0000-0000-0000-000000000152'),
      ('10000000-0000-0000-0000-000000000204', v_revision_id, 1,
       '10000000-0000-0000-0000-000000000131', '10000000-0000-0000-0000-000000000162');
  END IF;

  INSERT INTO semantic.field (
    field_id, revision_id, model_id, semantic_name, display_name, field_kind,
    logical_type, source_column, source_relationship_id, nullable, hidden
  )
  VALUES
    ('10000000-0000-0000-0000-000000000106', v_revision_id, '10000000-0000-0000-0000-000000000010', 'customer_region', 'Customer region', 'dimension', 'text', 'region', '10000000-0000-0000-0000-000000000201', false, false),
    ('10000000-0000-0000-0000-000000000107', v_revision_id, '10000000-0000-0000-0000-000000000010', 'customer_credit_limit', 'Customer credit limit', 'dimension', 'numeric', 'credit_limit', '10000000-0000-0000-0000-000000000201', false, false);

  IF v_anchor_supported THEN
    INSERT INTO semantic.field (
      field_id, revision_id, model_id, semantic_name, display_name, field_kind,
      logical_type, source_column, source_relationship_id, nullable, hidden
    )
    VALUES
      ('10000000-0000-0000-0000-000000000110', v_revision_id, '10000000-0000-0000-0000-000000000010', 'item_sku', 'Item SKU', 'dimension', 'text', 'sku', '10000000-0000-0000-0000-000000000202', false, false),
      ('10000000-0000-0000-0000-000000000114', v_revision_id, '10000000-0000-0000-0000-000000000010', 'order_tag', 'Order tag', 'dimension', 'text', 'tag', '10000000-0000-0000-0000-000000000203', false, false),
      ('10000000-0000-0000-0000-000000000135', v_revision_id, '10000000-0000-0000-0000-000000000013', 'item_sku', 'Item SKU', 'dimension', 'text', 'sku', '10000000-0000-0000-0000-000000000204', false, false);
  END IF;

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
    ('10000000-0000-0000-0000-000000000305', v_revision_id, '10000000-0000-0000-0000-000000000010', 'internal_revenue', 'Internal revenue', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}', NULL, 'additive', true),
    ('10000000-0000-0000-0000-000000000311', v_revision_id, '10000000-0000-0000-0000-000000000013', 'order_count', 'Order count', 'integer',
     '{"version":"1","kind":"aggregation","aggregation":"count","field":"order_id"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000312', v_revision_id, '10000000-0000-0000-0000-000000000013', 'revenue', 'Revenue', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000321', v_revision_id, '10000000-0000-0000-0000-000000000014', 'subscription_count', 'Subscription count', 'integer',
     '{"version":"1","kind":"aggregation","aggregation":"count","field":"subscription_id"}', NULL, 'additive', false),
    ('10000000-0000-0000-0000-000000000322', v_revision_id, '10000000-0000-0000-0000-000000000014', 'mrr', 'MRR', 'numeric',
     '{"version":"1","kind":"aggregation","aggregation":"sum","field":"monthly_amount"}', '{"field":"active","value":{"type":"boolean","value":true}}', 'additive', false);

  IF v_anchor_supported THEN
    UPDATE semantic.metric
    SET aggregation_anchor_field_id = CASE model_id
      WHEN '10000000-0000-0000-0000-000000000010'::uuid
        THEN '10000000-0000-0000-0000-000000000101'::uuid
      WHEN '10000000-0000-0000-0000-000000000013'::uuid
        THEN '10000000-0000-0000-0000-000000000131'::uuid
      WHEN '10000000-0000-0000-0000-000000000014'::uuid
        THEN '10000000-0000-0000-0000-000000000141'::uuid
      ELSE NULL
    END
    WHERE revision_id = v_revision_id;
  END IF;

  IF v_mutation_supported THEN
    INSERT INTO semantic.field (
      field_id, revision_id, model_id, semantic_name, display_name, field_kind,
      logical_type, source_column, nullable, hidden
    )
    VALUES
      ('10000000-0000-0000-0000-000000000109', v_revision_id, '10000000-0000-0000-0000-000000000010', 'external_id', 'External ID', 'dimension', 'text', 'external_id', false, false),
      ('10000000-0000-0000-0000-000000000134', v_revision_id, '10000000-0000-0000-0000-000000000013', 'external_id', 'External ID', 'dimension', 'text', 'external_id', false, false);

    INSERT INTO semantic.mutation_model (
    model_id, revision_id, insert_enabled, upsert_enabled, max_rows,
    max_request_bytes
    )
    VALUES
      ('10000000-0000-0000-0000-000000000010', v_revision_id, true, true, 25, 65536),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, true, true, 25, 65536);

    INSERT INTO semantic.mutation_field (
    model_id, revision_id, field_id, insertable, required_on_insert,
    updatable_on_conflict, conflict_key_ordinal, returning_ordinal
    )
    VALUES
      ('10000000-0000-0000-0000-000000000010', v_revision_id, '10000000-0000-0000-0000-000000000101', false, false, false, NULL, 1),
      ('10000000-0000-0000-0000-000000000010', v_revision_id, '10000000-0000-0000-0000-000000000109', true, true, false, 1, 2),
      ('10000000-0000-0000-0000-000000000010', v_revision_id, '10000000-0000-0000-0000-000000000102', true, true, true, NULL, NULL),
      ('10000000-0000-0000-0000-000000000010', v_revision_id, '10000000-0000-0000-0000-000000000103', true, true, true, NULL, 3),
      ('10000000-0000-0000-0000-000000000010', v_revision_id, '10000000-0000-0000-0000-000000000104', true, true, true, NULL, 4),
      ('10000000-0000-0000-0000-000000000010', v_revision_id, '10000000-0000-0000-0000-000000000105', true, true, true, NULL, 5),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, '10000000-0000-0000-0000-000000000131', false, false, false, NULL, 1),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, '10000000-0000-0000-0000-000000000132', true, true, false, 1, 3),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, '10000000-0000-0000-0000-000000000134', true, true, false, 2, 2),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, '10000000-0000-0000-0000-000000000133', true, true, true, NULL, 4);

    INSERT INTO semantic.mutation_model_role (
      model_id, revision_id, database_role
    )
    VALUES
      ('10000000-0000-0000-0000-000000000010', v_revision_id, 'postgresem_order_writer'),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, 'postgresem_tenant_a_writer'),
      ('10000000-0000-0000-0000-000000000013', v_revision_id, 'postgresem_tenant_b_writer');
  END IF;

  UPDATE semantic.revision
  SET status = 'published', published_at = clock_timestamp()
  WHERE revision_id = v_revision_id;
END;
$$;

COMMIT;
