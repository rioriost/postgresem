\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

ALTER TABLE semantic.query_audit
  ALTER COLUMN query_id SET DEFAULT gen_random_uuid(),
  ADD COLUMN compiler_query_hash text
    CHECK (compiler_query_hash ~ '^sha256:[0-9a-f]{64}$');

REVOKE ALL ON semantic.query_audit FROM postgresem_auditor;

CREATE FUNCTION semantic.start_query_audit(
  p_principal_subject_hash text,
  p_lsq_schema_version text,
  p_canonical_lsq_hash text,
  p_revision_id uuid,
  p_semantic_revision_hash text,
  p_compiler_version text,
  p_config_profile text,
  p_generated_sql_hash text,
  p_compiler_query_hash text,
  p_parameter_types jsonb,
  p_lineage jsonb,
  p_policy_context jsonb,
  p_validation_duration_ms bigint,
  p_compile_duration_ms bigint
)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
  v_query_id uuid;
BEGIN
  INSERT INTO semantic.query_audit (
    principal_subject_hash,
    lsq_schema_version,
    canonical_lsq_hash,
    revision_id,
    semantic_revision_hash,
    compiler_version,
    config_profile,
    generated_sql_hash,
    compiler_query_hash,
    parameter_types,
    lineage,
    policy_context,
    status,
    validation_duration_ms,
    compile_duration_ms
  )
  VALUES (
    p_principal_subject_hash,
    p_lsq_schema_version,
    p_canonical_lsq_hash,
    p_revision_id,
    p_semantic_revision_hash,
    p_compiler_version,
    p_config_profile,
    p_generated_sql_hash,
    p_compiler_query_hash,
    p_parameter_types,
    p_lineage,
    p_policy_context,
    'started',
    p_validation_duration_ms,
    p_compile_duration_ms
  )
  RETURNING query_id INTO v_query_id;

  RETURN v_query_id;
END;
$$;

CREATE FUNCTION semantic.finish_query_audit(
  p_query_id uuid,
  p_status text,
  p_error_code text,
  p_database_duration_ms bigint,
  p_serialization_duration_ms bigint,
  p_row_count bigint,
  p_byte_count bigint,
  p_truncated boolean
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
  IF p_status NOT IN ('succeeded', 'failed', 'cancelled') THEN
    RAISE EXCEPTION 'invalid terminal query audit status';
  END IF;

  UPDATE semantic.query_audit
  SET
    status = p_status,
    error_code = p_error_code,
    database_duration_ms = p_database_duration_ms,
    serialization_duration_ms = p_serialization_duration_ms,
    row_count = p_row_count,
    byte_count = p_byte_count,
    truncated = p_truncated,
    completed_at = clock_timestamp()
  WHERE query_id = p_query_id
    AND status = 'started';

  IF NOT FOUND THEN
    RAISE EXCEPTION 'query audit row is not in started state';
  END IF;
END;
$$;

REVOKE ALL ON FUNCTION semantic.start_query_audit(
  text, text, text, uuid, text, text, text, text, text, jsonb, jsonb, jsonb, bigint, bigint
) FROM PUBLIC;
REVOKE ALL ON FUNCTION semantic.finish_query_audit(
  uuid, text, text, bigint, bigint, bigint, bigint, boolean
) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION semantic.start_query_audit(
  text, text, text, uuid, text, text, text, text, text, jsonb, jsonb, jsonb, bigint, bigint
) TO postgresem_auditor;
GRANT EXECUTE ON FUNCTION semantic.finish_query_audit(
  uuid, text, text, bigint, bigint, bigint, bigint, boolean
) TO postgresem_auditor;

INSERT INTO semantic.schema_migration(version)
VALUES ('0003_guarded_execution_audit');

COMMIT;
