\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

INSERT INTO semantic.project (project_id, semantic_name, display_name)
VALUES (
  '80000000-0000-0000-0000-000000000001',
  'reconciliation_test',
  'Reconciliation test'
);

INSERT INTO semantic.revision (
  revision_id, project_id, revision_number, status, schema_version,
  canonical_hash, compiler_semantic_version
)
VALUES (
  '80000000-0000-0000-0000-000000000002',
  '80000000-0000-0000-0000-000000000001',
  1, 'draft', '1', 'sha256:' || repeat('0', 64), '0.1.0'
);

INSERT INTO semantic.mutation_idempotency (
  project, idempotency_key_hash, authority_hash, lsm_hash, revision_id,
  semantic_revision_hash, mutation_id, status, result, affected_rows,
  committed_at, database_role, authority_scheme
)
SELECT
  'reconciliation_test',
  'sha256:' || repeat(key_digit, 64),
  'sha256:' || repeat(authority_digit, 64),
  'sha256:' || repeat('0', 64),
  '80000000-0000-0000-0000-000000000002'::uuid,
  'sha256:' || repeat('0', 64),
  mutation_id::uuid,
  'committed',
  '["private-result"]'::jsonb,
  1,
  clock_timestamp(),
  database_role,
  authority_scheme
FROM (VALUES
  ('1', '2', '80000000-0000-0000-0000-000000000010', 'postgresem_tenant_a_writer', 'principal-v1'),
  ('1', '3', '80000000-0000-0000-0000-000000000011', 'postgresem_tenant_b_writer', 'legacy-v1'),
  ('4', '3', '80000000-0000-0000-0000-000000000012', 'postgresem_tenant_a_writer', 'legacy-v1')
) AS fixture(key_digit, authority_digit, mutation_id, database_role, authority_scheme);

SET LOCAL ROLE postgresem_auditor;

DO $$
DECLARE
  v_state jsonb;
  v_role text;
BEGIN
  v_state := semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('3', 64), 'sha256:' || repeat('1', 64),
    'postgresem_tenant_a_writer'
  );
  IF v_state ->> 'mutation_id' IS DISTINCT FROM
    '80000000-0000-0000-0000-000000000010'
  THEN
    RAISE EXCEPTION 'same-authority same-role reconciliation failed';
  END IF;
  IF v_state ?| ARRAY['result', 'database_role', 'authority_hash']
    OR v_state::text LIKE '%private-result%'
  THEN
    RAISE EXCEPTION 'reconciliation leaked private state';
  END IF;

  FOREACH v_role IN ARRAY ARRAY['postgresem_tenant_b_writer', '', NULL]
  LOOP
    IF semantic.lookup_mutation_idempotency(
      'reconciliation_test', 'sha256:' || repeat('2', 64),
      'sha256:' || repeat('3', 64), 'sha256:' || repeat('1', 64), v_role
    ) IS NOT NULL THEN
      RAISE EXCEPTION 'role mismatch fell back to legacy state or exposed current state';
    END IF;
  END LOOP;

  v_state := semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('9', 64), 'sha256:' || repeat('1', 64),
    'postgresem_tenant_a_writer'
  );
  IF v_state ->> 'mutation_id' IS DISTINCT FROM
    '80000000-0000-0000-0000-000000000010'
  THEN
    RAISE EXCEPTION 'stable-authority reconciliation failed after legacy hash rotation';
  END IF;

  IF semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('9', 64),
    'sha256:' || repeat('9', 64), 'sha256:' || repeat('1', 64),
    'postgresem_tenant_a_writer'
  ) IS NOT NULL OR semantic.lookup_mutation_idempotency(
    'another_project', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('3', 64), 'sha256:' || repeat('1', 64),
    'postgresem_tenant_a_writer'
  ) IS NOT NULL OR semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('3', 64), 'sha256:' || repeat('9', 64),
    'postgresem_tenant_a_writer'
  ) IS NOT NULL THEN
    RAISE EXCEPTION 'reconciliation crossed project, authority, or key scope';
  END IF;

  v_state := semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('3', 64), 'sha256:' || repeat('4', 64),
    'postgresem_tenant_a_writer'
  );
  IF v_state ->> 'mutation_id' IS DISTINCT FROM
    '80000000-0000-0000-0000-000000000012'
  THEN
    RAISE EXCEPTION 'matching legacy reconciliation failed';
  END IF;
  IF semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('3', 64), 'sha256:' || repeat('4', 64),
    'postgresem_tenant_b_writer'
  ) IS NOT NULL OR semantic.lookup_mutation_idempotency(
    'reconciliation_test', 'sha256:' || repeat('2', 64),
    'sha256:' || repeat('9', 64), 'sha256:' || repeat('4', 64),
    'postgresem_tenant_a_writer'
  ) IS NOT NULL THEN
    RAISE EXCEPTION 'legacy reconciliation ignored role or legacy hash';
  END IF;

  IF to_regprocedure(
    'semantic.lookup_mutation_idempotency(text,text,text,text)'
  ) IS NOT NULL THEN
    RAISE EXCEPTION 'unscoped reconciliation overload remains callable';
  END IF;
  FOREACH v_role IN ARRAY ARRAY[
    'postgresem_runtime', 'postgresem_analyst', 'postgresem_mutation_runtime'
  ]
  LOOP
    IF has_function_privilege(
      v_role,
      'semantic.lookup_mutation_idempotency(text,text,text,text,text)',
      'EXECUTE'
    ) THEN
      RAISE EXCEPTION 'reconciliation was granted beyond the audit boundary';
    END IF;
  END LOOP;
END;
$$;

ROLLBACK;
