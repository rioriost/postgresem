\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

CREATE FUNCTION semantic.m10_operational_report(p_since timestamptz)
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
    'catalog', jsonb_build_object(
      'user_relations', (
        SELECT count(*)
        FROM pg_catalog.pg_class AS relation
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE relation.relkind IN ('r', 'p', 'v', 'm', 'f')
          AND namespace.nspname NOT IN (
            'pg_catalog',
            'information_schema',
            'semantic'
          )
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'
      ),
      'user_columns', (
        SELECT count(*)
        FROM pg_catalog.pg_attribute AS attribute
        JOIN pg_catalog.pg_class AS relation
          ON relation.oid = attribute.attrelid
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE relation.relkind IN ('r', 'p', 'v', 'm', 'f')
          AND namespace.nspname NOT IN (
            'pg_catalog',
            'information_schema',
            'semantic'
          )
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'
          AND attribute.attnum > 0
          AND NOT attribute.attisdropped
      ),
      'materialized_views', (
        SELECT count(*)
        FROM pg_catalog.pg_class AS relation
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE relation.relkind = 'm'
          AND namespace.nspname NOT IN (
            'pg_catalog',
            'information_schema',
            'semantic'
          )
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'
      ),
      'unpopulated_materialized_views', (
        SELECT count(*)
        FROM pg_catalog.pg_class AS relation
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE relation.relkind = 'm'
          AND NOT relation.relispopulated
          AND namespace.nspname NOT IN (
            'pg_catalog',
            'information_schema',
            'semantic'
          )
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'
      )
    ),
    'semantic', jsonb_build_object(
      'projects', (SELECT count(*) FROM semantic.project),
      'published_revisions', (
        SELECT count(*) FROM semantic.revision WHERE status = 'published'
      ),
      'draft_revisions', (
        SELECT count(*) FROM semantic.revision WHERE status = 'draft'
      ),
      'models', (SELECT count(*) FROM semantic.model),
      'fields', (SELECT count(*) FROM semantic.field),
      'metrics', (SELECT count(*) FROM semantic.metric),
      'relationships', (SELECT count(*) FROM semantic.relationship)
    ),
    'queries', jsonb_build_object(
      'total', (
        SELECT count(*)
        FROM semantic.query_audit
        WHERE started_at >= v_since
      ),
      'succeeded', (
        SELECT count(*)
        FROM semantic.query_audit
        WHERE started_at >= v_since
          AND status = 'succeeded'
      ),
      'failed', (
        SELECT count(*)
        FROM semantic.query_audit
        WHERE started_at >= v_since
          AND status = 'failed'
      ),
      'cancelled', (
        SELECT count(*)
        FROM semantic.query_audit
        WHERE started_at >= v_since
          AND status = 'cancelled'
      ),
      'incomplete', (
        SELECT count(*)
        FROM semantic.query_audit
        WHERE started_at >= v_since
          AND status = 'started'
      ),
      'database_p95_ms', (
        SELECT percentile_cont(0.95) WITHIN GROUP (
          ORDER BY database_duration_ms
        )
        FROM semantic.query_audit
        WHERE started_at >= v_since
          AND database_duration_ms IS NOT NULL
      )
    ),
    'mutations', jsonb_build_object(
      'total', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
      ),
      'committed', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'committed'
      ),
      'rejected', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'rejected'
      ),
      'rolled_back', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'rolled_back'
      ),
      'indeterminate', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'indeterminate'
      ),
      'reconciled', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'reconciled'
      ),
      'incomplete', (
        SELECT count(*)
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'started'
      )
    ),
    'capacity', jsonb_build_object(
      'max_connections', current_setting('max_connections')::integer,
      'database_connections', (
        SELECT count(*)
        FROM pg_catalog.pg_stat_activity
        WHERE datname = current_database()
      ),
      'waiting_locks', (
        SELECT count(*)
        FROM pg_catalog.pg_locks AS lock
        JOIN pg_catalog.pg_stat_activity AS activity
          ON activity.pid = lock.pid
        WHERE activity.datname = current_database()
          AND NOT lock.granted
      )
    ),
    'migrations', jsonb_build_object(
      'applied', (SELECT count(*) FROM semantic.schema_migration),
      'current', (
        SELECT version
        FROM semantic.schema_migration
        ORDER BY version DESC
        LIMIT 1
      )
    ),
    'objectives', jsonb_build_object(
      'query_audit_complete', NOT EXISTS (
        SELECT 1
        FROM semantic.query_audit
        WHERE started_at >= v_since
          AND status = 'started'
      ),
      'mutation_audit_complete', NOT EXISTS (
        SELECT 1
        FROM semantic.mutation_audit
        WHERE started_at >= v_since
          AND status = 'started'
      ),
      'no_waiting_locks', NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_locks AS lock
        JOIN pg_catalog.pg_stat_activity AS activity
          ON activity.pid = lock.pid
        WHERE activity.datname = current_database()
          AND NOT lock.granted
      ),
      'materialized_views_populated', NOT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_class AS relation
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE relation.relkind = 'm'
          AND NOT relation.relispopulated
          AND namespace.nspname NOT IN (
            'pg_catalog',
            'information_schema',
            'semantic'
          )
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'
      )
    )
  )
  INTO v_report;

  RETURN v_report;
END;
$$;

REVOKE ALL ON FUNCTION semantic.m10_operational_report(timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION semantic.m10_operational_report(timestamptz)
  TO postgresem_auditor;

INSERT INTO semantic.schema_migration(version)
VALUES ('0010_m10_operational_report');

COMMIT;
