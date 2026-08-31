\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

CREATE TABLE semantic.project (
  project_id uuid PRIMARY KEY,
  semantic_name text NOT NULL UNIQUE CHECK (btrim(semantic_name) <> ''),
  display_name text NOT NULL CHECK (btrim(display_name) <> ''),
  description text,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE semantic.revision (
  revision_id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES semantic.project(project_id),
  revision_number bigint NOT NULL CHECK (revision_number > 0),
  parent_revision_id uuid REFERENCES semantic.revision(revision_id),
  status text NOT NULL CHECK (status IN ('draft', 'published', 'retired')),
  schema_version text NOT NULL CHECK (btrim(schema_version) <> ''),
  canonical_hash text NOT NULL CHECK (canonical_hash ~ '^sha256:[0-9a-f]{64}$'),
  compiler_semantic_version text NOT NULL CHECK (btrim(compiler_semantic_version) <> ''),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  published_at timestamptz,
  retired_at timestamptz,
  UNIQUE (revision_id, project_id),
  UNIQUE (project_id, revision_number),
  UNIQUE (project_id, canonical_hash),
  CHECK (
    (status = 'draft' AND published_at IS NULL AND retired_at IS NULL)
    OR (status = 'published' AND published_at IS NOT NULL AND retired_at IS NULL)
    OR (status = 'retired' AND published_at IS NOT NULL AND retired_at IS NOT NULL)
  )
);

CREATE UNIQUE INDEX one_published_revision_per_project
  ON semantic.revision(project_id)
  WHERE status = 'published';

CREATE TABLE semantic.model (
  model_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  semantic_name text NOT NULL CHECK (btrim(semantic_name) <> ''),
  display_name text NOT NULL CHECK (btrim(display_name) <> ''),
  description text,
  model_kind text NOT NULL CHECK (model_kind IN ('fact', 'dimension')),
  source_database text NOT NULL CHECK (btrim(source_database) <> ''),
  source_schema text NOT NULL CHECK (btrim(source_schema) <> ''),
  source_relation text NOT NULL CHECK (btrim(source_relation) <> ''),
  source_relation_kind text NOT NULL
    CHECK (source_relation_kind IN ('table', 'view', 'materialized_view')),
  default_timezone text,
  UNIQUE (model_id, revision_id),
  UNIQUE (revision_id, semantic_name)
);

CREATE TABLE semantic.field (
  field_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  model_id uuid NOT NULL,
  semantic_name text NOT NULL CHECK (btrim(semantic_name) <> ''),
  display_name text NOT NULL CHECK (btrim(display_name) <> ''),
  description text,
  field_kind text NOT NULL
    CHECK (field_kind IN ('dimension', 'entity_key', 'time_dimension', 'calculated')),
  logical_type text NOT NULL CHECK (btrim(logical_type) <> ''),
  source_column text,
  expression jsonb,
  nullable boolean NOT NULL,
  hidden boolean NOT NULL DEFAULT false,
  UNIQUE (field_id, revision_id),
  UNIQUE (model_id, semantic_name),
  CHECK ((source_column IS NOT NULL)::integer + (expression IS NOT NULL)::integer = 1),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE
);

CREATE TABLE semantic.relationship (
  relationship_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  semantic_name text NOT NULL CHECK (btrim(semantic_name) <> ''),
  from_model_id uuid NOT NULL,
  to_model_id uuid NOT NULL,
  cardinality text NOT NULL
    CHECK (cardinality IN ('one_to_one', 'many_to_one', 'one_to_many', 'many_to_many')),
  join_type text NOT NULL CHECK (join_type IN ('inner', 'left')),
  allowed_direction text NOT NULL CHECK (allowed_direction IN ('forward', 'both')),
  priority integer NOT NULL DEFAULT 0,
  UNIQUE (relationship_id, revision_id),
  UNIQUE (revision_id, semantic_name),
  CHECK (from_model_id <> to_model_id),
  FOREIGN KEY (from_model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE,
  FOREIGN KEY (to_model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE
);

CREATE TABLE semantic.relationship_column (
  relationship_id uuid NOT NULL,
  revision_id uuid NOT NULL,
  ordinal smallint NOT NULL CHECK (ordinal > 0),
  from_field_id uuid NOT NULL,
  to_field_id uuid NOT NULL,
  PRIMARY KEY (relationship_id, ordinal),
  UNIQUE (relationship_id, from_field_id, to_field_id),
  FOREIGN KEY (relationship_id, revision_id)
    REFERENCES semantic.relationship(relationship_id, revision_id) ON DELETE CASCADE,
  FOREIGN KEY (from_field_id, revision_id)
    REFERENCES semantic.field(field_id, revision_id),
  FOREIGN KEY (to_field_id, revision_id)
    REFERENCES semantic.field(field_id, revision_id)
);

CREATE TABLE semantic.metric (
  metric_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  model_id uuid NOT NULL,
  semantic_name text NOT NULL CHECK (btrim(semantic_name) <> ''),
  display_name text NOT NULL CHECK (btrim(display_name) <> ''),
  description text,
  result_type text NOT NULL CHECK (btrim(result_type) <> ''),
  expression jsonb NOT NULL,
  metric_filter jsonb,
  additivity text NOT NULL
    CHECK (additivity IN ('additive', 'semi_additive', 'non_additive')),
  hidden boolean NOT NULL DEFAULT false,
  UNIQUE (metric_id, revision_id),
  UNIQUE (model_id, semantic_name),
  CHECK (expression ? 'version'),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE
);

CREATE TABLE semantic.term (
  term_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  model_id uuid,
  field_id uuid,
  metric_id uuid,
  locale text NOT NULL DEFAULT 'und',
  term text NOT NULL CHECK (btrim(term) <> ''),
  term_kind text NOT NULL CHECK (term_kind IN ('display_name', 'synonym')),
  UNIQUE (revision_id, locale, term),
  CHECK (num_nonnulls(model_id, field_id, metric_id) = 1),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE,
  FOREIGN KEY (field_id, revision_id)
    REFERENCES semantic.field(field_id, revision_id) ON DELETE CASCADE,
  FOREIGN KEY (metric_id, revision_id)
    REFERENCES semantic.metric(metric_id, revision_id) ON DELETE CASCADE
);

CREATE TABLE semantic.policy_binding (
  policy_binding_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  model_id uuid,
  field_id uuid,
  database_role name,
  source_schema name,
  source_relation name,
  policy_name name,
  visibility text NOT NULL CHECK (visibility IN ('discover', 'query')),
  details jsonb NOT NULL DEFAULT '{}'::jsonb,
  CHECK (num_nonnulls(model_id, field_id) = 1),
  FOREIGN KEY (model_id, revision_id)
    REFERENCES semantic.model(model_id, revision_id) ON DELETE CASCADE,
  FOREIGN KEY (field_id, revision_id)
    REFERENCES semantic.field(field_id, revision_id) ON DELETE CASCADE
);

CREATE TABLE semantic.source_snapshot (
  source_snapshot_id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES semantic.project(project_id) ON DELETE CASCADE,
  source_database text NOT NULL,
  source_schema text NOT NULL,
  source_relation text NOT NULL,
  source_column text,
  object_kind text NOT NULL,
  fingerprint text NOT NULL CHECK (fingerprint ~ '^sha256:[0-9a-f]{64}$'),
  normalized_definition jsonb NOT NULL,
  captured_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  UNIQUE NULLS NOT DISTINCT (
    project_id,
    source_database,
    source_schema,
    source_relation,
    source_column,
    fingerprint
  )
);

CREATE TABLE semantic.import_run (
  import_run_id uuid PRIMARY KEY,
  project_id uuid NOT NULL REFERENCES semantic.project(project_id) ON DELETE CASCADE,
  status text NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
  started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  completed_at timestamptz,
  source_snapshot_count bigint NOT NULL DEFAULT 0 CHECK (source_snapshot_count >= 0),
  CHECK (
    (status = 'running' AND completed_at IS NULL)
    OR (status IN ('succeeded', 'failed') AND completed_at IS NOT NULL)
  )
);

CREATE TABLE semantic.import_issue (
  import_issue_id uuid PRIMARY KEY,
  import_run_id uuid NOT NULL REFERENCES semantic.import_run(import_run_id) ON DELETE CASCADE,
  severity text NOT NULL CHECK (severity IN ('info', 'warning', 'breaking')),
  issue_code text NOT NULL CHECK (btrim(issue_code) <> ''),
  object_path text NOT NULL CHECK (btrim(object_path) <> ''),
  message text NOT NULL CHECK (btrim(message) <> ''),
  evidence jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE semantic.lineage_edge (
  lineage_edge_id uuid PRIMARY KEY,
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id) ON DELETE CASCADE,
  source_kind text NOT NULL,
  source_id uuid NOT NULL,
  target_kind text NOT NULL,
  target_id uuid NOT NULL,
  edge_kind text NOT NULL,
  definition_hash text NOT NULL CHECK (definition_hash ~ '^sha256:[0-9a-f]{64}$'),
  UNIQUE (revision_id, source_kind, source_id, target_kind, target_id, edge_kind)
);

CREATE TABLE semantic.query_audit (
  query_id uuid PRIMARY KEY,
  request_id text,
  principal_subject_hash text NOT NULL CHECK (btrim(principal_subject_hash) <> ''),
  lsq_schema_version text NOT NULL,
  canonical_lsq_hash text NOT NULL CHECK (canonical_lsq_hash ~ '^sha256:[0-9a-f]{64}$'),
  revision_id uuid NOT NULL REFERENCES semantic.revision(revision_id),
  semantic_revision_hash text NOT NULL CHECK (semantic_revision_hash ~ '^sha256:[0-9a-f]{64}$'),
  compiler_version text NOT NULL,
  config_profile text NOT NULL,
  generated_sql_hash text CHECK (generated_sql_hash ~ '^sha256:[0-9a-f]{64}$'),
  parameter_types jsonb NOT NULL DEFAULT '[]'::jsonb,
  lineage jsonb NOT NULL DEFAULT '{}'::jsonb,
  policy_context jsonb NOT NULL DEFAULT '{}'::jsonb,
  status text NOT NULL CHECK (status IN ('started', 'succeeded', 'failed', 'cancelled')),
  error_code text,
  validation_duration_ms bigint CHECK (validation_duration_ms >= 0),
  compile_duration_ms bigint CHECK (compile_duration_ms >= 0),
  queue_duration_ms bigint CHECK (queue_duration_ms >= 0),
  database_duration_ms bigint CHECK (database_duration_ms >= 0),
  serialization_duration_ms bigint CHECK (serialization_duration_ms >= 0),
  row_count bigint CHECK (row_count >= 0),
  byte_count bigint CHECK (byte_count >= 0),
  truncated boolean NOT NULL DEFAULT false,
  started_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  completed_at timestamptz,
  CHECK (
    (status = 'started' AND completed_at IS NULL)
    OR (status IN ('succeeded', 'failed', 'cancelled') AND completed_at IS NOT NULL)
  )
);

CREATE FUNCTION semantic.enforce_revision_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD.status = 'draft' AND NEW.status IN ('draft', 'published') THEN
    RETURN NEW;
  END IF;
  IF OLD.status = 'published'
    AND NEW.status = 'retired'
    AND NEW.revision_id = OLD.revision_id
    AND NEW.project_id = OLD.project_id
    AND NEW.revision_number = OLD.revision_number
    AND NEW.parent_revision_id IS NOT DISTINCT FROM OLD.parent_revision_id
    AND NEW.schema_version = OLD.schema_version
    AND NEW.canonical_hash = OLD.canonical_hash
    AND NEW.compiler_semantic_version = OLD.compiler_semantic_version
    AND NEW.created_at = OLD.created_at
    AND NEW.published_at = OLD.published_at
    AND NEW.retired_at IS NOT NULL
  THEN
    RETURN NEW;
  END IF;
  RAISE EXCEPTION 'invalid semantic revision transition: % -> %', OLD.status, NEW.status;
END;
$$;

CREATE TRIGGER revision_transition
BEFORE UPDATE ON semantic.revision
FOR EACH ROW EXECUTE FUNCTION semantic.enforce_revision_transition();

CREATE FUNCTION semantic.require_draft_revision()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  target_revision_id uuid;
  target_status text;
BEGIN
  IF TG_OP = 'DELETE' THEN
    target_revision_id := OLD.revision_id;
  ELSE
    target_revision_id := NEW.revision_id;
  END IF;

  SELECT status INTO target_status
  FROM semantic.revision
  WHERE revision_id = target_revision_id;

  IF target_status IS DISTINCT FROM 'draft' THEN
    RAISE EXCEPTION 'semantic revision % is not mutable', target_revision_id;
  END IF;

  IF TG_OP = 'DELETE' THEN
    RETURN OLD;
  END IF;
  RETURN NEW;
END;
$$;

CREATE TRIGGER model_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.model
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER field_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.field
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER relationship_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.relationship
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER relationship_column_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.relationship_column
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER metric_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.metric
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER term_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.term
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER policy_binding_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.policy_binding
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

CREATE TRIGGER lineage_edge_requires_draft
BEFORE INSERT OR UPDATE OR DELETE ON semantic.lineage_edge
FOR EACH ROW EXECUTE FUNCTION semantic.require_draft_revision();

REVOKE ALL ON SCHEMA semantic FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA semantic FROM PUBLIC;
REVOKE ALL ON ALL FUNCTIONS IN SCHEMA semantic FROM PUBLIC;

GRANT USAGE ON SCHEMA semantic TO
  postgresem_runtime,
  postgresem_editor,
  postgresem_publisher,
  postgresem_introspector,
  postgresem_auditor;

GRANT SELECT ON
  semantic.project,
  semantic.revision,
  semantic.model,
  semantic.field,
  semantic.relationship,
  semantic.relationship_column,
  semantic.metric,
  semantic.term,
  semantic.policy_binding
TO postgresem_runtime;

GRANT INSERT, UPDATE ON semantic.query_audit TO postgresem_auditor;
GRANT SELECT, INSERT, UPDATE ON
  semantic.source_snapshot,
  semantic.import_run,
  semantic.import_issue
TO postgresem_introspector;

INSERT INTO semantic.schema_migration(version)
VALUES ('0001_semantic_schema');

COMMIT;
