\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

ALTER TABLE semantic.metric
  ADD COLUMN aggregation_anchor_field_id uuid;

ALTER TABLE semantic.metric
  ADD CONSTRAINT metric_aggregation_anchor_revision_fkey
  FOREIGN KEY (aggregation_anchor_field_id, revision_id)
  REFERENCES semantic.field(field_id, revision_id);

CREATE FUNCTION semantic.validate_metric_aggregation_anchor()
RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog
AS $$
DECLARE
  v_schema_version text;
  v_model_id uuid;
  v_field_kind text;
  v_source_relationship_id uuid;
BEGIN
  IF NEW.aggregation_anchor_field_id IS NULL THEN
    RETURN NEW;
  END IF;

  SELECT revision.schema_version
  INTO v_schema_version
  FROM semantic.revision AS revision
  WHERE revision.revision_id = NEW.revision_id;

  SELECT field.model_id, field.field_kind, field.source_relationship_id
  INTO v_model_id, v_field_kind, v_source_relationship_id
  FROM semantic.field AS field
  WHERE field.field_id = NEW.aggregation_anchor_field_id
    AND field.revision_id = NEW.revision_id;

  IF v_schema_version IS DISTINCT FROM '2'
    OR v_model_id IS DISTINCT FROM NEW.model_id
    OR v_field_kind IS DISTINCT FROM 'entity_key'
    OR v_source_relationship_id IS NOT NULL
  THEN
    RAISE EXCEPTION 'metric aggregation anchor must be a direct entity key in the same schema-v2 model'
      USING ERRCODE = 'check_violation';
  END IF;

  RETURN NEW;
END;
$$;

CREATE TRIGGER metric_aggregation_anchor_is_valid
BEFORE INSERT OR UPDATE ON semantic.metric
FOR EACH ROW EXECUTE FUNCTION semantic.validate_metric_aggregation_anchor();

REVOKE ALL ON FUNCTION semantic.validate_metric_aggregation_anchor()
  FROM PUBLIC;

INSERT INTO semantic.schema_migration(version)
VALUES ('0006_fanout_aggregation_anchor');

COMMIT;
