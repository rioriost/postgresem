# Backup, restore, and N-1 upgrade

Postgresem uses forward-only migrations. The beta rollback mechanism is a
validated pre-upgrade backup restored under the original database name plus the
previous binary. Down migrations are not supported.

## Ownership boundary

The repository provides a local Apple Container reference workflow. Production
backup, encryption, retention, replication, cluster roles, RPO, RTO, and
disaster recovery remain owned by the PostgreSQL platform operator.

The Semantic Snapshot records `source_database`. Restoring the same content
under a different database name intentionally fails closed during query
execution. Restore validation must preserve the original database name or
publish a new semantic revision after an intentional rename.

## Local reference backup

With the quickstart stack running:

```sh
scripts/backup.sh
scripts/verify-backup.sh backups/postgresem-<timestamp>
```

The backup directory is created with restrictive permissions and contains:

- `database.dump`: custom-format dump of the complete local pilot database;
- `globals.sql`: cluster role metadata, including password verifiers;
- `migrations.txt`: applied migration versions;
- `published-revisions.txt`: published canonical hashes;
- `MANIFEST`: format, timestamp, database name, and checksums.

Never commit or attach these files to an issue. `globals.sql` is sensitive.
Store all backup files encrypted with access and retention controls appropriate
for the source data and audit metadata.

## Restore validation

`make test-recovery` runs only against the isolated repository fixture cluster.
It:

1. builds the N-1 schema through `0003_guarded_execution_audit`;
2. runs the current binary and a guarded query against N-1;
3. applies `0004_beta_operational_report`;
4. creates a full custom-format database dump;
5. temporarily moves the fixture source database aside;
6. restores the dump under the original database name;
7. verifies the published semantic hash;
8. runs a guarded query and the beta operational report;
9. restores the original fixture database.

Do not copy this rename procedure into a shared or production cluster. Restore
production backups into an isolated cluster or recovery environment that can
use the original database name, validate there, then use the platform's
controlled cutover process.

## Upgrade sequence

1. Stop new gateway requests.
2. Confirm no `started` audit rows remain.
3. Create and independently verify the platform backup.
4. Record the binary version, migration list, and published revision hashes.
5. Apply database migrations.
6. Deploy the current binary.
7. Run model export/diff, a guarded canary query, and `postgresem report beta`.
8. Resume traffic only after all checks pass.

If validation fails, contain the deployment, preserve logs and audit evidence,
restore the pre-upgrade backup under the original name, deploy the previous
binary, and repeat the canary checks.

