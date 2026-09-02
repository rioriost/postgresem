\set ON_ERROR_STOP on

BEGIN;

DO $$
DECLARE
  v_project_id uuid := '00000000-0000-0000-0000-000000000001';
  v_revision_id uuid := '00000000-0000-0000-0000-000000000002';
  v_target_revision_id uuid := '00000000-0000-0000-0000-000000000005';
  v_model_id uuid := '00000000-0000-0000-0000-000000000003';
BEGIN
  INSERT INTO semantic.project (project_id, semantic_name, display_name)
  VALUES (v_project_id, 'lifecycle_test', 'Lifecycle test');

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
    '1',
    'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    '0.1.0'
  );

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
    v_target_revision_id,
    v_project_id,
    2,
    'draft',
    '1',
    'sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
    '0.1.0'
  );

  INSERT INTO semantic.model (
    model_id,
    revision_id,
    semantic_name,
    display_name,
    model_kind,
    source_database,
    source_schema,
    source_relation,
    source_relation_kind
  )
  VALUES (
    v_model_id,
    v_revision_id,
    'orders',
    'Orders',
    'fact',
    current_database(),
    'commerce',
    'orders',
    'table'
  );

  UPDATE semantic.revision
  SET status = 'published', published_at = clock_timestamp()
  WHERE semantic.revision.revision_id = v_revision_id;

  BEGIN
    INSERT INTO semantic.model (
      model_id,
      revision_id,
      semantic_name,
      display_name,
      model_kind,
      source_database,
      source_schema,
      source_relation,
      source_relation_kind
    )
    VALUES (
      '00000000-0000-0000-0000-000000000004',
      v_revision_id,
      'forbidden_after_publish',
      'Forbidden',
      'fact',
      current_database(),
      'commerce',
      'orders',
      'table'
    );
    RAISE EXCEPTION 'published revision accepted a model mutation';
  EXCEPTION
    WHEN raise_exception THEN
      IF SQLERRM = 'published revision accepted a model mutation' THEN
        RAISE;
      END IF;
  END;

  BEGIN
    UPDATE semantic.model
    SET revision_id = v_target_revision_id
    WHERE model_id = v_model_id;
    RAISE EXCEPTION 'published revision accepted a child move';
  EXCEPTION
    WHEN check_violation THEN NULL;
  END;
END;
$$;

DO $$
DECLARE
  v_project_id uuid := '00000000-0000-0000-0000-000000000011';
  v_revision_id uuid := '00000000-0000-0000-0000-000000000012';
  v_model_id uuid := '00000000-0000-0000-0000-000000000013';
  v_anchor_id uuid := '00000000-0000-0000-0000-000000000014';
BEGIN
  INSERT INTO semantic.project (project_id, semantic_name, display_name)
  VALUES (v_project_id, 'anchor_lifecycle_test', 'Anchor lifecycle test');

  INSERT INTO semantic.revision (
    revision_id, project_id, revision_number, status, schema_version,
    canonical_hash, compiler_semantic_version
  )
  VALUES (
    v_revision_id, v_project_id, 1, 'draft', '2',
    'sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    '0.2.0'
  );

  INSERT INTO semantic.model (
    model_id, revision_id, semantic_name, display_name, model_kind,
    source_database, source_schema, source_relation, source_relation_kind
  )
  VALUES (
    v_model_id, v_revision_id, 'anchored_orders', 'Anchored orders', 'fact',
    current_database(), 'commerce', 'orders', 'table'
  );

  INSERT INTO semantic.field (
    field_id, revision_id, model_id, semantic_name, display_name, field_kind,
    logical_type, source_column, nullable
  )
  VALUES
    (v_anchor_id, v_revision_id, v_model_id, 'order_id', 'Order ID',
     'entity_key', 'integer', 'order_id', false),
    ('00000000-0000-0000-0000-000000000015', v_revision_id, v_model_id,
     'amount', 'Amount', 'dimension', 'numeric', 'amount', false);

  INSERT INTO semantic.metric (
    metric_id, revision_id, model_id, semantic_name, display_name, result_type,
    expression, additivity, aggregation_anchor_field_id
  )
  VALUES (
    '00000000-0000-0000-0000-000000000016', v_revision_id, v_model_id,
    'revenue', 'Revenue', 'numeric',
    '{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount"}',
    'additive', v_anchor_id
  );

  BEGIN
    UPDATE semantic.field
    SET field_kind = 'dimension'
    WHERE field_id = v_anchor_id;
    RAISE EXCEPTION 'aggregation anchor accepted an invalid field mutation';
  EXCEPTION
    WHEN integrity_constraint_violation THEN NULL;
  END;

  BEGIN
    UPDATE semantic.revision
    SET schema_version = '1'
    WHERE revision_id = v_revision_id;
    RAISE EXCEPTION 'aggregation anchor accepted a schema-v1 revision';
  EXCEPTION
    WHEN integrity_constraint_violation THEN NULL;
  END;

  UPDATE semantic.revision
  SET status = 'published', published_at = clock_timestamp()
  WHERE revision_id = v_revision_id;
END;
$$;

SET SESSION AUTHORIZATION postgresem_runtime;
SET ROLE postgresem_tenant_a;
DO $$
BEGIN
  IF (SELECT count(*) FROM rls_fixture.orders) <> 2 THEN
    RAISE EXCEPTION 'tenant A RLS isolation failed';
  END IF;
END;
$$;
RESET ROLE;
RESET SESSION AUTHORIZATION;

SET SESSION AUTHORIZATION postgresem_runtime;
SET ROLE postgresem_tenant_b;
DO $$
BEGIN
  IF (SELECT count(*) FROM rls_fixture.orders) <> 1 THEN
    RAISE EXCEPTION 'tenant B RLS isolation failed';
  END IF;
END;
$$;
RESET ROLE;
RESET SESSION AUTHORIZATION;

SELECT 'semantic schema integration checks passed' AS result;

ROLLBACK;
