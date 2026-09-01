\set ON_ERROR_STOP on

DO $$
DECLARE
  v_revision_id uuid;
BEGIN
  SELECT revision.revision_id
  INTO STRICT v_revision_id
  FROM semantic.project AS project
  JOIN semantic.revision AS revision
    ON revision.project_id = project.project_id
  WHERE project.semantic_name = 'commerce'
    AND revision.status = 'published'
    AND revision.schema_version = '1'
    AND revision.canonical_hash =
      'sha256:a731347152caed2f8f3dfcecb730aac12c93c839f8cc91e6f81099128f70e58c';

  IF (
    SELECT count(*)
    FROM semantic.revision
    WHERE project_id = (
      SELECT project_id FROM semantic.project WHERE semantic_name = 'commerce'
    )
  ) <> 1 THEN
    RAISE EXCEPTION 'semantic seed is not idempotent';
  END IF;
  IF (SELECT count(*) FROM semantic.model WHERE revision_id = v_revision_id) <> 4 THEN
    RAISE EXCEPTION 'semantic seed model count is incorrect';
  END IF;
  IF (SELECT count(*) FROM semantic.field WHERE revision_id = v_revision_id) <> 22 THEN
    RAISE EXCEPTION 'semantic seed field count is incorrect';
  END IF;
  IF (SELECT count(*) FROM semantic.metric WHERE revision_id = v_revision_id) <> 9 THEN
    RAISE EXCEPTION 'semantic seed metric count is incorrect';
  END IF;
  IF (SELECT count(*) FROM semantic.relationship WHERE revision_id = v_revision_id) <> 1 THEN
    RAISE EXCEPTION 'semantic seed relationship count is incorrect';
  END IF;
  IF (
    SELECT count(*)
    FROM semantic.model
    WHERE revision_id = v_revision_id AND queryable
  ) <> 3 THEN
    RAISE EXCEPTION 'semantic seed queryable state is incorrect';
  END IF;
  IF (
    SELECT count(*)
    FROM semantic.field
    WHERE revision_id = v_revision_id
      AND source_relationship_id IS NOT NULL
  ) <> 2 THEN
    RAISE EXCEPTION 'semantic seed declared relationship field bindings are incorrect';
  END IF;
  IF (
    SELECT count(*)
    FROM semantic.field
    WHERE revision_id = v_revision_id AND hidden
  ) <> 1 OR (
    SELECT count(*)
    FROM semantic.metric
    WHERE revision_id = v_revision_id AND hidden
  ) <> 1 THEN
    RAISE EXCEPTION 'semantic seed hidden object state is incorrect';
  END IF;
  IF EXISTS (
    SELECT 1
    FROM semantic.metric
    WHERE revision_id = v_revision_id
      AND (
        expression ? 'sql'
        OR expression->>'kind' <> 'aggregation'
        OR expression->>'version' <> '1'
      )
  ) THEN
    RAISE EXCEPTION 'semantic seed contains invalid metric metadata';
  END IF;
  IF (SELECT count(*) FROM semantic.mutation_model WHERE revision_id = v_revision_id) <> 2
    OR (SELECT count(*) FROM semantic.mutation_field WHERE revision_id = v_revision_id) <> 10
    OR (SELECT count(*) FROM semantic.mutation_model_role WHERE revision_id = v_revision_id) <> 3
  THEN
    RAISE EXCEPTION 'semantic seed mutation metadata is incorrect';
  END IF;
END;
$$;

SELECT 'semantic seed integration checks passed' AS result;
