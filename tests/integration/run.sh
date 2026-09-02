#!/bin/sh
set -eu

psql --no-psqlrc -v ON_ERROR_STOP=1 -f /tests/semantic_schema.sql
/bin/sh /tests/anchor_concurrency.sh
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /tests/semantic_seed.sql
psql --no-psqlrc -v ON_ERROR_STOP=1 -f /tests/known_answers.sql
echo "all database integration checks passed"
