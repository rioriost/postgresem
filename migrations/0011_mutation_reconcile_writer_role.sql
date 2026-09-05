\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

REVOKE ALL ON FUNCTION semantic.lookup_mutation_idempotency(
  text, text, text, text
) FROM PUBLIC, postgresem_auditor;

DROP FUNCTION semantic.lookup_mutation_idempotency(text, text, text, text);

CREATE FUNCTION semantic.lookup_mutation_idempotency(
  p_project text,
  p_authority_hash text,
  p_legacy_authority_hash text,
  p_idempotency_key_hash text,
  p_database_role text
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
  FROM (
    SELECT *
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
      )
    ORDER BY (authority_scheme = 'principal-v1') DESC
    LIMIT 1
  ) AS selected
  -- A role mismatch must not fall back from current to legacy state.
  WHERE database_role = p_database_role;
$$;

REVOKE ALL ON FUNCTION semantic.lookup_mutation_idempotency(
  text, text, text, text, text
) FROM PUBLIC;

GRANT EXECUTE ON FUNCTION semantic.lookup_mutation_idempotency(
  text, text, text, text, text
) TO postgresem_auditor;

INSERT INTO semantic.schema_migration(version)
VALUES ('0011_mutation_reconcile_writer_role');

COMMIT;
