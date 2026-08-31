\set ON_ERROR_STOP on
\getenv runtime_password POSTGRESEM_RUNTIME_PASSWORD

SELECT 'CREATE ROLE postgresem_owner NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_owner')
\gexec

SELECT format(
  'CREATE ROLE postgresem_runtime LOGIN NOINHERIT PASSWORD %L',
  :'runtime_password'
)
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_runtime')
\gexec

SELECT 'CREATE ROLE postgresem_editor NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_editor')
\gexec

SELECT 'CREATE ROLE postgresem_publisher NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_publisher')
\gexec

SELECT 'CREATE ROLE postgresem_introspector NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_introspector')
\gexec

SELECT 'CREATE ROLE postgresem_auditor NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_auditor')
\gexec

SELECT 'CREATE ROLE postgresem_tenant_a NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_tenant_a')
\gexec

SELECT 'CREATE ROLE postgresem_tenant_b NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_tenant_b')
\gexec

GRANT postgresem_tenant_a, postgresem_tenant_b TO postgresem_runtime;

