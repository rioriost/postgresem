\set ON_ERROR_STOP on

BEGIN;
SET LOCAL ROLE postgresem_owner;

ALTER TABLE semantic.model
  ADD COLUMN queryable boolean NOT NULL DEFAULT true;

ALTER TABLE semantic.field
  ADD COLUMN source_relationship_id uuid;

ALTER TABLE semantic.field
  ADD CONSTRAINT field_source_relationship_revision_fkey
  FOREIGN KEY (source_relationship_id, revision_id)
  REFERENCES semantic.relationship(relationship_id, revision_id);

ALTER TABLE semantic.field
  ADD CONSTRAINT field_relationship_source_is_column
  CHECK (
    source_relationship_id IS NULL
    OR source_column IS NOT NULL
  );

INSERT INTO semantic.schema_migration(version)
VALUES ('0002_published_snapshot_v1');

COMMIT;
