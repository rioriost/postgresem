\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

CREATE FUNCTION semantic.beta_operational_report(p_since timestamptz)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog
AS $$
DECLARE
  v_now timestamptz := clock_timestamp();
  v_since timestamptz;
  v_report jsonb;
BEGIN
  IF p_since IS NULL
    OR p_since > v_now
    OR p_since < v_now - interval '365 days'
  THEN
    RAISE EXCEPTION 'report start time must be within the previous 365 days';
  END IF;
  v_since := date_trunc('hour', p_since);

  SELECT jsonb_build_object(
    'schema_version', '1',
    'window_start', v_since,
    'generated_at', v_now,
    'queries', jsonb_build_object(
      'total', count(*),
      'succeeded', count(*) FILTER (WHERE status = 'succeeded'),
      'failed', count(*) FILTER (WHERE status = 'failed'),
      'cancelled', count(*) FILTER (WHERE status = 'cancelled'),
      'incomplete', count(*) FILTER (WHERE status = 'started'),
      'truncated', count(*) FILTER (WHERE truncated),
      'active_principals',
        CASE
          WHEN count(*) >= 10 THEN count(DISTINCT principal_subject_hash)
          ELSE NULL
        END,
      'semantic_revisions', count(DISTINCT semantic_revision_hash)
    ),
    'latency_ms', jsonb_build_object(
      'validation_compile_p50',
        percentile_cont(0.50) WITHIN GROUP (
          ORDER BY validation_duration_ms + compile_duration_ms
        ) FILTER (
          WHERE validation_duration_ms IS NOT NULL
            AND compile_duration_ms IS NOT NULL
        ),
      'validation_compile_p95',
        percentile_cont(0.95) WITHIN GROUP (
          ORDER BY validation_duration_ms + compile_duration_ms
        ) FILTER (
          WHERE validation_duration_ms IS NOT NULL
            AND compile_duration_ms IS NOT NULL
        ),
      'database_p95',
        percentile_cont(0.95) WITHIN GROUP (
          ORDER BY database_duration_ms
        ) FILTER (WHERE database_duration_ms IS NOT NULL)
    ),
    'objectives', jsonb_build_object(
      'audit_complete',
        count(*) FILTER (WHERE status = 'started') = 0,
      'validation_compile_p95_under_50_ms',
        CASE
          WHEN count(*) FILTER (
            WHERE validation_duration_ms IS NOT NULL
              AND compile_duration_ms IS NOT NULL
          ) = 0
          THEN NULL
          ELSE percentile_cont(0.95) WITHIN GROUP (
            ORDER BY validation_duration_ms + compile_duration_ms
          ) FILTER (
            WHERE validation_duration_ms IS NOT NULL
              AND compile_duration_ms IS NOT NULL
          ) < 50
        END
    ),
    'error_codes',
      CASE
        WHEN count(*) < 10 THEN '{}'::jsonb
        ELSE coalesce(
          (
            SELECT jsonb_object_agg(code, occurrences ORDER BY code)
            FROM (
              SELECT error_code AS code, count(*) AS occurrences
              FROM semantic.query_audit
              WHERE started_at >= v_since
                AND error_code IS NOT NULL
              GROUP BY error_code
            ) errors
          ),
          '{}'::jsonb
        )
      END
  )
  INTO v_report
  FROM semantic.query_audit
  WHERE started_at >= v_since;

  RETURN v_report;
END;
$$;

REVOKE ALL ON FUNCTION semantic.beta_operational_report(timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION semantic.beta_operational_report(timestamptz)
  TO postgresem_auditor;

INSERT INTO semantic.schema_migration(version)
VALUES ('0004_beta_operational_report');

COMMIT;
