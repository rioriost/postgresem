\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM semantic.metric AS metric
    LEFT JOIN semantic.revision AS revision
      ON revision.revision_id = metric.revision_id
    LEFT JOIN semantic.field AS anchor
      ON anchor.field_id = metric.aggregation_anchor_field_id
     AND anchor.revision_id = metric.revision_id
    WHERE metric.aggregation_anchor_field_id IS NOT NULL
      AND (
        revision.schema_version IS DISTINCT FROM '2'
        OR anchor.field_id IS NULL
        OR anchor.model_id IS DISTINCT FROM metric.model_id
        OR anchor.field_kind IS DISTINCT FROM 'entity_key'
        OR anchor.source_relationship_id IS NOT NULL
      )
  ) THEN
    RAISE EXCEPTION 'existing metric aggregation anchor violates the schema-v2 direct entity-key contract'
      USING ERRCODE = 'check_violation';
  END IF;
END;
$$;

ALTER TABLE semantic.field
  ADD COLUMN aggregation_anchor_eligible boolean
  GENERATED ALWAYS AS (
    field_kind = 'entity_key' AND source_relationship_id IS NULL
  ) STORED;

ALTER TABLE semantic.metric
  ADD COLUMN aggregation_anchor_required_eligible boolean
  GENERATED ALWAYS AS (
    CASE WHEN aggregation_anchor_field_id IS NULL THEN NULL ELSE true END
  ) STORED,
  ADD COLUMN aggregation_anchor_required_schema_version text
  GENERATED ALWAYS AS (
    CASE WHEN aggregation_anchor_field_id IS NULL THEN NULL ELSE '2' END
  ) STORED;

ALTER TABLE semantic.field
  ADD CONSTRAINT field_anchor_identity_unique
  UNIQUE (field_id, revision_id, model_id, aggregation_anchor_eligible);

ALTER TABLE semantic.revision
  ADD CONSTRAINT revision_anchor_schema_version_unique
  UNIQUE (revision_id, schema_version);

ALTER TABLE semantic.metric
  DROP CONSTRAINT metric_aggregation_anchor_revision_fkey,
  ADD CONSTRAINT metric_aggregation_anchor_model_fkey
  FOREIGN KEY (
    aggregation_anchor_field_id,
    revision_id,
    model_id,
    aggregation_anchor_required_eligible
  )
  REFERENCES semantic.field(
    field_id,
    revision_id,
    model_id,
    aggregation_anchor_eligible
  ),
  ADD CONSTRAINT metric_aggregation_anchor_schema_fkey
  FOREIGN KEY (revision_id, aggregation_anchor_required_schema_version)
  REFERENCES semantic.revision(revision_id, schema_version);

CREATE OR REPLACE FUNCTION semantic.require_draft_revision()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
  target_revision_id uuid;
  target_status text;
BEGIN
  IF TG_OP = 'UPDATE'
    AND NEW.revision_id IS DISTINCT FROM OLD.revision_id
  THEN
    RAISE EXCEPTION 'semantic child rows cannot move between revisions'
      USING ERRCODE = 'check_violation';
  END IF;

  IF TG_OP = 'DELETE' THEN
    target_revision_id := OLD.revision_id;
  ELSE
    target_revision_id := NEW.revision_id;
  END IF;

  SELECT revision.status
  INTO target_status
  FROM semantic.revision AS revision
  WHERE revision.revision_id = target_revision_id
  FOR SHARE;

  IF target_status IS DISTINCT FROM 'draft' THEN
    RAISE EXCEPTION 'semantic revision % is not mutable', target_revision_id;
  END IF;

  IF TG_OP = 'DELETE' THEN
    RETURN OLD;
  END IF;
  RETURN NEW;
END;
$$;

REVOKE ALL ON FUNCTION semantic.require_draft_revision()
  FROM PUBLIC;

INSERT INTO semantic.schema_migration(version)
VALUES ('0007_fanout_anchor_invariants');

COMMIT;
