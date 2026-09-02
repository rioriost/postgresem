\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

ALTER TABLE semantic.mutation_idempotency
  ADD COLUMN database_role text,
  ADD COLUMN authority_scheme text NOT NULL DEFAULT 'legacy-v1';

UPDATE semantic.mutation_idempotency AS idempotency
SET database_role = source.database_role
FROM (
  SELECT DISTINCT ON (mutation_id)
    mutation_id,
    policy_context ->> 'database_role' AS database_role
  FROM semantic.mutation_audit
  WHERE policy_context ->> 'database_role' ~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'
  ORDER BY mutation_id, started_at
) AS source
WHERE source.mutation_id = idempotency.mutation_id;

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM semantic.mutation_idempotency
    WHERE database_role IS NULL
  ) THEN
    RAISE EXCEPTION 'cannot migrate mutation idempotency without database role evidence';
  END IF;
END;
$$;

ALTER TABLE semantic.mutation_idempotency
  ALTER COLUMN database_role SET NOT NULL,
  ALTER COLUMN authority_scheme SET DEFAULT 'principal-v1',
  ADD CONSTRAINT mutation_idempotency_database_role_check
    CHECK (database_role ~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'),
  ADD CONSTRAINT mutation_idempotency_authority_scheme_check
    CHECK (authority_scheme IN ('legacy-v1', 'principal-v1')),
  DROP CONSTRAINT mutation_idempotency_pkey,
  ADD PRIMARY KEY (project, authority_hash, idempotency_key_hash);

CREATE OR REPLACE FUNCTION semantic.claim_mutation(
  p_project text,
  p_idempotency_key_hash text,
  p_authority_hash text,
  p_lsm_schema_version text,
  p_lsm_hash text,
  p_revision_id uuid,
  p_semantic_revision_hash text,
  p_principal_subject_hash text,
  p_compiler_version text,
  p_config_profile text,
  p_operation text,
  p_model text,
  p_statement_hash text,
  p_compiler_mutation_hash text,
  p_parameter_types jsonb,
  p_lineage jsonb,
  p_policy_context jsonb,
  p_requested_rows bigint,
  p_validation_duration_ms bigint,
  p_compile_duration_ms bigint
)
RETURNS TABLE (
  disposition text,
  mutation_id uuid,
  attempt_id uuid,
  affected_rows bigint,
  result jsonb
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
  v_mutation_id uuid := gen_random_uuid();
  v_attempt_id uuid;
  v_existing semantic.mutation_idempotency%ROWTYPE;
  v_database_role text := p_policy_context ->> 'database_role';
  v_legacy_authority_hash text := p_policy_context ->> 'legacy_authority_hash';
BEGIN
  IF v_database_role IS NULL
    OR v_database_role !~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'
    OR v_legacy_authority_hash IS NULL
    OR v_legacy_authority_hash !~ '^sha256:[0-9a-f]{64}$'
  THEN
    RAISE EXCEPTION 'invalid mutation authority context';
  END IF;

  SELECT *
  INTO v_existing
  FROM semantic.mutation_idempotency
  WHERE project = p_project
    AND authority_hash = p_authority_hash
    AND authority_scheme = 'principal-v1'
    AND idempotency_key_hash = p_idempotency_key_hash
  FOR UPDATE;

  IF NOT FOUND THEN
    SELECT *
    INTO v_existing
    FROM semantic.mutation_idempotency
    WHERE project = p_project
      AND authority_scheme = 'legacy-v1'
      AND idempotency_key_hash = p_idempotency_key_hash
    ORDER BY created_at
    LIMIT 1
    FOR UPDATE;
  END IF;

  IF NOT FOUND THEN
    INSERT INTO semantic.mutation_idempotency (
      project,
      idempotency_key_hash,
      authority_hash,
      database_role,
      authority_scheme,
      lsm_hash,
      revision_id,
      semantic_revision_hash,
      mutation_id,
      status
    )
    VALUES (
      p_project,
      p_idempotency_key_hash,
      p_authority_hash,
      v_database_role,
      'principal-v1',
      p_lsm_hash,
      p_revision_id,
      p_semantic_revision_hash,
      v_mutation_id,
      'started'
    )
    ON CONFLICT (project, authority_hash, idempotency_key_hash) DO NOTHING;

    IF FOUND THEN
      INSERT INTO semantic.mutation_audit (
        mutation_id,
        project,
        principal_subject_hash,
        lsm_schema_version,
        lsm_hash,
        revision_id,
        semantic_revision_hash,
        compiler_version,
        config_profile,
        operation,
        model,
        idempotency_key_hash,
        statement_hash,
        compiler_mutation_hash,
        parameter_types,
        lineage,
        policy_context,
        status,
        requested_rows,
        validation_duration_ms,
        compile_duration_ms
      )
      VALUES (
        v_mutation_id,
        p_project,
        p_principal_subject_hash,
        p_lsm_schema_version,
        p_lsm_hash,
        p_revision_id,
        p_semantic_revision_hash,
        p_compiler_version,
        p_config_profile,
        p_operation,
        p_model,
        p_idempotency_key_hash,
        p_statement_hash,
        p_compiler_mutation_hash,
        p_parameter_types,
        p_lineage,
        p_policy_context,
        'started',
        p_requested_rows,
        p_validation_duration_ms,
        p_compile_duration_ms
      )
      RETURNING semantic.mutation_audit.attempt_id INTO v_attempt_id;

      RETURN QUERY
        SELECT 'execute', v_mutation_id, v_attempt_id, NULL::bigint, NULL::jsonb;
      RETURN;
    END IF;

    SELECT *
    INTO STRICT v_existing
    FROM semantic.mutation_idempotency
    WHERE project = p_project
      AND authority_hash = p_authority_hash
      AND authority_scheme = 'principal-v1'
      AND idempotency_key_hash = p_idempotency_key_hash
    FOR UPDATE;
  END IF;

  IF (
      v_existing.authority_scheme = 'legacy-v1'
      AND v_existing.authority_hash <> v_legacy_authority_hash
    )
    OR v_existing.database_role <> v_database_role
    OR v_existing.lsm_hash <> p_lsm_hash
    OR v_existing.revision_id <> p_revision_id
    OR v_existing.semantic_revision_hash <> p_semantic_revision_hash
  THEN
    INSERT INTO semantic.mutation_audit (
      mutation_id,
      project,
      principal_subject_hash,
      lsm_schema_version,
      lsm_hash,
      revision_id,
      semantic_revision_hash,
      compiler_version,
      config_profile,
      operation,
      model,
      idempotency_key_hash,
      statement_hash,
      compiler_mutation_hash,
      parameter_types,
      lineage,
      policy_context,
      status,
      error_code,
      requested_rows,
      affected_rows,
      completed_at,
      validation_duration_ms,
      compile_duration_ms
    )
    VALUES (
      v_mutation_id,
      p_project,
      p_principal_subject_hash,
      p_lsm_schema_version,
      p_lsm_hash,
      p_revision_id,
      p_semantic_revision_hash,
      p_compiler_version,
      p_config_profile,
      p_operation,
      p_model,
      p_idempotency_key_hash,
      p_statement_hash,
      p_compiler_mutation_hash,
      p_parameter_types,
      p_lineage,
      p_policy_context,
      'rejected',
      'MUTATION_IDEMPOTENCY_CONFLICT',
      p_requested_rows,
      0,
      clock_timestamp(),
      p_validation_duration_ms,
      p_compile_duration_ms
    )
    RETURNING semantic.mutation_audit.attempt_id INTO v_attempt_id;

    RETURN QUERY
      SELECT 'conflict', v_mutation_id, v_attempt_id, 0::bigint, NULL::jsonb;
    RETURN;
  END IF;

  IF v_existing.status <> 'committed' THEN
    RAISE EXCEPTION 'idempotency record is not committed';
  END IF;

  UPDATE semantic.mutation_idempotency
  SET
    replay_count = replay_count + 1,
    last_replayed_at = clock_timestamp()
  WHERE project = p_project
    AND authority_hash = v_existing.authority_hash
    AND idempotency_key_hash = p_idempotency_key_hash;

  INSERT INTO semantic.mutation_audit (
    mutation_id,
    project,
    principal_subject_hash,
    lsm_schema_version,
    lsm_hash,
    revision_id,
    semantic_revision_hash,
    compiler_version,
    config_profile,
    operation,
    model,
    idempotency_key_hash,
    statement_hash,
    compiler_mutation_hash,
    parameter_types,
    lineage,
    policy_context,
    status,
    requested_rows,
    affected_rows,
    replayed,
    completed_at,
    validation_duration_ms,
    compile_duration_ms
  )
  VALUES (
    v_existing.mutation_id,
    p_project,
    p_principal_subject_hash,
    p_lsm_schema_version,
    p_lsm_hash,
    p_revision_id,
    p_semantic_revision_hash,
    p_compiler_version,
    p_config_profile,
    p_operation,
    p_model,
    p_idempotency_key_hash,
    p_statement_hash,
    p_compiler_mutation_hash,
    p_parameter_types,
    p_lineage,
    p_policy_context,
    'committed',
    p_requested_rows,
    v_existing.affected_rows,
    true,
    clock_timestamp(),
    p_validation_duration_ms,
    p_compile_duration_ms
  )
  RETURNING semantic.mutation_audit.attempt_id INTO v_attempt_id;

  RETURN QUERY
    SELECT
      'replay',
      v_existing.mutation_id,
      v_attempt_id,
      v_existing.affected_rows,
      v_existing.result;
END;
$$;

REVOKE ALL ON FUNCTION semantic.lookup_mutation_idempotency(text, text)
FROM PUBLIC, postgresem_auditor;
DROP FUNCTION semantic.lookup_mutation_idempotency(text, text);

CREATE FUNCTION semantic.lookup_mutation_idempotency(
  p_project text,
  p_authority_hash text,
  p_legacy_authority_hash text,
  p_idempotency_key_hash text
)
RETURNS jsonb
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
  SELECT jsonb_build_object(
    'mutation_id', mutation_id,
    'status', status,
    'affected_rows', affected_rows,
    'replay_count', replay_count,
    'committed_at', committed_at,
    'last_replayed_at', last_replayed_at
  )
  FROM semantic.mutation_idempotency
  WHERE project = p_project
    AND idempotency_key_hash = p_idempotency_key_hash
    AND (
      (
        authority_scheme = 'principal-v1'
        AND authority_hash = p_authority_hash
      )
      OR (
        authority_scheme = 'legacy-v1'
        AND authority_hash = p_legacy_authority_hash
      )
    );
$$;

REVOKE ALL ON FUNCTION semantic.claim_mutation(
  text, text, text, text, text, uuid, text, text, text, text, text, text, text,
  text, jsonb, jsonb, jsonb, bigint, bigint, bigint
) FROM PUBLIC;
REVOKE ALL ON FUNCTION semantic.lookup_mutation_idempotency(
  text, text, text, text
) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION semantic.lookup_mutation_idempotency(
  text, text, text, text
) TO postgresem_auditor;

INSERT INTO semantic.schema_migration(version)
VALUES ('0008_mutation_authority_idempotency');

COMMIT;
