\set ON_ERROR_STOP on

BEGIN;

DO $$
DECLARE
  v_project_id uuid := '00000000-0000-0000-0000-000000000001';
  v_revision_id uuid := '00000000-0000-0000-0000-000000000002';
  v_model_id uuid := '00000000-0000-0000-0000-000000000003';
BEGIN
  INSERT INTO semantic.project (project_id, semantic_name, display_name)
  VALUES (v_project_id, 'commerce', 'Commerce');

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
