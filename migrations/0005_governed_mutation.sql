\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

CREATE TABLE semantic.mutation_model (
  model_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL,
  insert_enabled boolean NOT NULL,
  upsert_enabled boolean NOT NULL,
  max_rows integer NOT NULL CHECK (max_rows BETWEEN 1 AND 100),
  max_request_bytes integer NOT NULL
    CHECK (max_request_bytes BETWEEN 1 AND 1048576),
  UNIQUE (model_id, revision_id),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE,
  CHECK (insert_enabled OR upsert_enabled)
);

CREATE TABLE semantic.mutation_field (
  model_id uuid NOT NULL,
  revision_id uuid NOT NULL,
  field_id uuid NOT NULL,
  insertable boolean NOT NULL,
  required_on_insert boolean NOT NULL DEFAULT false,
  updatable_on_conflict boolean NOT NULL DEFAULT false,
  conflict_key_ordinal smallint CHECK (conflict_key_ordinal > 0),
  returning_ordinal smallint CHECK (returning_ordinal > 0),
  PRIMARY KEY (model_id, field_id),
  UNIQUE (model_id, conflict_key_ordinal),
  UNIQUE (model_id, returning_ordinal),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.mutation_model(model_id, revision_id) ON DELETE CASCADE,
  FOREIGN KEY (field_id, revision_id)
    REFERENCES semantic.field(field_id, revision_id) ON DELETE CASCADE,
  CHECK (NOT required_on_insert OR insertable),
  CHECK (NOT updatable_on_conflict OR insertable),
  CHECK (conflict_key_ordinal IS NULL OR (insertable AND NOT updatable_on_conflict)),
  CHECK (insertable OR returning_ordinal IS NOT NULL)
);

CREATE TABLE semantic.mutation_model_role (
  model_id uuid NOT NULL,
  revision_id uuid NOT NULL,
  database_role name NOT NULL,
  PRIMARY KEY (model_id, database_role),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.mutation_model(model_id, revision_id) ON DELETE CASCADE
);

CREATE TABLE semantic.mutation_idempotency (
  project text NOT NULL CHECK (btrim(project) <> ''),
  idempotency_key_hash text NOT NULL
    CHECK (idempotency_key_hash ~ '^sha256:[0-9a-f]{64}$'),
  lsm_hash text NOT NULL CHECK (lsm_hash ~ '^sha256:[0-9a-f]{64}$'),
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id),
  semantic_revision_hash text NOT NULL
    CHECK (semantic_revision_hash ~ '^sha256:[0-9a-f]{64}$'),
  mutation_id uuid NOT NULL UNIQUE,
  status text NOT NULL CHECK (status IN ('started', 'committed')),
  result jsonb,
  affected_rows bigint CHECK (affected_rows >= 0),
  replay_count bigint NOT NULL DEFAULT 0 CHECK (replay_count >= 0),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  committed_at timestamptz,
  last_replayed_at timestamptz,
  PRIMARY KEY (project, idempotency_key_hash),
  CHECK (
    (status = 'started' AND result IS NULL AND affected_rows IS NULL AND committed_at IS NULL)
    OR (
      status = 'committed'
      AND result IS NOT NULL
      AND affected_rows IS NOT NULL
      AND committed_at IS NOT NULL
    )
  )
);

CREATE TABLE semantic.mutation_audit (
  attempt_id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  mutation_id uuid NOT NULL,
  project text NOT NULL CHECK (btrim(project) <> ''),
  principal_subject_hash text NOT NULL
    CHECK (principal_subject_hash ~ '^sha256:[0-9a-f]{64}$'),
  lsm_schema_version text,
  lsm_hash text NOT NULL CHECK (lsm_hash ~ '^sha256:[0-9a-f]{64}$'),
  revision_id uuid REFERENCES semantic.revision(revision_id),
  semantic_revision_hash text
    CHECK (semantic_revision_hash ~ '^sha256:[0-9a-f]{64}$'),
  compiler_version text,
  config_profile text NOT NULL CHECK (btrim(config_profile) <> ''),
  operation text CHECK (operation IN ('insert', 'upsert')),
  model text,
  idempotency_key_hash text
    CHECK (idempotency_key_hash ~ '^sha256:[0-9a-f]{64}$'),
  statement_hash text CHECK (statement_hash ~ '^sha256:[0-9a-f]{64}$'),
  compiler_mutation_hash text
    CHECK (compiler_mutation_hash ~ '^sha256:[0-9a-f]{64}$'),
  parameter_types jsonb NOT NULL DEFAULT '[]'::jsonb,
  lineage jsonb NOT NULL DEFAULT '{}'::jsonb,
  policy_context jsonb NOT NULL DEFAULT '{}'::jsonb,
  status text NOT NULL
    CHECK (
      status IN (
        'started',
        'committed',
        'rejected',
        'rolled_back',
        'indeterminate',
        'reconciled'
      )
    ),
  error_code text,
  requested_rows bigint CHECK (requested_rows >= 0),
  affected_rows bigint CHECK (affected_rows >= 0),
  replayed boolean NOT NULL DEFAULT false,
  validation_duration_ms bigint CHECK (validation_duration_ms >= 0),
  compile_duration_ms bigint CHECK (compile_duration_ms >= 0),
  database_duration_ms bigint CHECK (database_duration_ms >= 0),
  started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  completed_at timestamptz,
  CHECK (
    (status = 'started' AND completed_at IS NULL)
    OR (status <> 'started' AND completed_at IS NOT NULL)
  )
);

CREATE TRIGGER mutation_model_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.mutation_model
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER mutation_field_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.mutation_field
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER mutation_model_role_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.mutation_model_role
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE FUNCTION semantic.claim_mutation(
  p_project text,
  p_idempotency_key_hash text,
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
BEGIN
  INSERT INTO semantic.mutation_idempotency (
    project,
    idempotency_key_hash,
    lsm_hash,
    revision_id,
    semantic_revision_hash,
    mutation_id,
    status
  )
  VALUES (
    p_project,
    p_idempotency_key_hash,
    p_lsm_hash,
    p_revision_id,
    p_semantic_revision_hash,
    v_mutation_id,
    'started'
  )
  ON CONFLICT (project, idempotency_key_hash) DO NOTHING;

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

    RETURN QUERY SELECT 'execute', v_mutation_id, v_attempt_id, NULL::bigint, NULL::jsonb;
    RETURN;
  END IF;

  SELECT *
  INTO STRICT v_existing
  FROM semantic.mutation_idempotency
  WHERE project = p_project
    AND idempotency_key_hash = p_idempotency_key_hash
  FOR UPDATE;

  IF v_existing.lsm_hash <> p_lsm_hash
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
      SELECT 'conflict', v_existing.mutation_id, v_attempt_id, 0::bigint, NULL::jsonb;
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

CREATE FUNCTION semantic.finish_mutation(
  p_mutation_id uuid,
  p_attempt_id uuid,
  p_result jsonb,
  p_affected_rows bigint,
  p_database_duration_ms bigint
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
BEGIN
  UPDATE semantic.mutation_idempotency
  SET
    status = 'committed',
    result = p_result,
    affected_rows = p_affected_rows,
    committed_at = clock_timestamp()
  WHERE mutation_id = p_mutation_id
    AND status = 'started';

  IF NOT FOUND THEN
    RAISE EXCEPTION 'mutation idempotency row is not in started state';
  END IF;

  UPDATE semantic.mutation_audit
  SET
    status = 'committed',
    affected_rows = p_affected_rows,
    database_duration_ms = p_database_duration_ms,
    completed_at = clock_timestamp()
  WHERE attempt_id = p_attempt_id
    AND mutation_id = p_mutation_id
    AND status = 'started';

  IF NOT FOUND THEN
    RAISE EXCEPTION 'mutation audit row is not in started state';
  END IF;
END;
$$;

CREATE FUNCTION semantic.record_mutation_failure(
  p_project text,
  p_principal_subject_hash text,
  p_lsm_schema_version text,
  p_lsm_hash text,
  p_revision_id uuid,
  p_semantic_revision_hash text,
  p_compiler_version text,
  p_config_profile text,
  p_operation text,
  p_model text,
  p_idempotency_key_hash text,
  p_statement_hash text,
  p_compiler_mutation_hash text,
  p_parameter_types jsonb,
  p_lineage jsonb,
  p_policy_context jsonb,
  p_status text,
  p_error_code text,
  p_requested_rows bigint,
  p_validation_duration_ms bigint,
  p_compile_duration_ms bigint,
  p_database_duration_ms bigint
)
RETURNS uuid
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
  v_mutation_id uuid := gen_random_uuid();
BEGIN
  IF p_status NOT IN ('rejected', 'rolled_back', 'indeterminate', 'reconciled') THEN
    RAISE EXCEPTION 'invalid terminal mutation audit status';
  END IF;

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
    validation_duration_ms,
    compile_duration_ms,
    database_duration_ms,
    completed_at
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
    p_status,
    p_error_code,
    p_requested_rows,
    0,
    p_validation_duration_ms,
    p_compile_duration_ms,
    p_database_duration_ms,
    clock_timestamp()
  );

  RETURN v_mutation_id;
END;
$$;

CREATE FUNCTION semantic.lookup_mutation_idempotency(
  p_project text,
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
    AND idempotency_key_hash = p_idempotency_key_hash;
$$;

REVOKE ALL ON
  semantic.mutation_model,
  semantic.mutation_field,
  semantic.mutation_model_role,
  semantic.mutation_idempotency,
  semantic.mutation_audit
FROM PUBLIC;

REVOKE ALL ON FUNCTION semantic.claim_mutation(
  text, text, text, text, uuid, text, text, text, text, text, text, text,
  text, jsonb, jsonb, jsonb, bigint, bigint, bigint
) FROM PUBLIC;
REVOKE ALL ON FUNCTION semantic.finish_mutation(
  uuid, uuid, jsonb, bigint, bigint
) FROM PUBLIC;
REVOKE ALL ON FUNCTION semantic.record_mutation_failure(
  text, text, text, text, uuid, text, text, text, text, text, text, text,
  text, jsonb, jsonb, jsonb, text, text, bigint, bigint, bigint, bigint
) FROM PUBLIC;
REVOKE ALL ON FUNCTION semantic.lookup_mutation_idempotency(text, text)
FROM PUBLIC;

GRANT USAGE ON SCHEMA semantic TO
  postgresem_mutation_runtime,
  postgresem_mutator;

GRANT SELECT ON
  semantic.project,
  semantic.revision,
  semantic.model,
  semantic.field,
  semantic.relationship,
  semantic.relationship_column,
  semantic.metric,
  semantic.term,
  semantic.policy_binding,
  semantic.mutation_model,
  semantic.mutation_field,
  semantic.mutation_model_role
TO postgresem_runtime, postgresem_mutation_runtime;

GRANT EXECUTE ON FUNCTION semantic.claim_mutation(
  text, text, text, text, uuid, text, text, text, text, text, text, text,
  text, jsonb, jsonb, jsonb, bigint, bigint, bigint
) TO postgresem_mutator;
GRANT EXECUTE ON FUNCTION semantic.finish_mutation(
  uuid, uuid, jsonb, bigint, bigint
) TO postgresem_mutator;
GRANT EXECUTE ON FUNCTION semantic.record_mutation_failure(
  text, text, text, text, uuid, text, text, text, text, text, text, text,
  text, jsonb, jsonb, jsonb, text, text, bigint, bigint, bigint, bigint
) TO postgresem_auditor;
GRANT EXECUTE ON FUNCTION semantic.lookup_mutation_idempotency(text, text)
TO postgresem_auditor;

INSERT INTO semantic.schema_migration(version)
VALUES ('0005_governed_mutation');

COMMIT;
