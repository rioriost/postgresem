#!/bin/sh
set -eu

psql --no-psqlrc -v ON_ERROR_STOP=1 -f /semantic/commerce.sql
