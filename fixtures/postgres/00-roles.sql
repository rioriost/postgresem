\set ON_ERROR_STOP on
\getenv runtime_password POSTGRESEM_RUNTIME_PASSWORD
\getenv audit_writer_password POSTGRESEM_AUDIT_WRITER_PASSWORD
\getenv mutation_runtime_password POSTGRESEM_MUTATION_RUNTIME_PASSWORD

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

SELECT format(
  'CREATE ROLE postgresem_audit_writer LOGIN PASSWORD %L',
  :'audit_writer_password'
)
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_audit_writer')
\gexec

SELECT 'CREATE ROLE postgresem_mutator NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_mutator')
\gexec

SELECT format(
  'CREATE ROLE postgresem_mutation_runtime LOGIN NOINHERIT PASSWORD %L',
  :'mutation_runtime_password'
)
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_mutation_runtime')
\gexec

SELECT 'CREATE ROLE postgresem_analyst NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_analyst')
\gexec

SELECT 'CREATE ROLE postgresem_source_owner NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_source_owner')
\gexec

SELECT 'CREATE ROLE postgresem_test_superuser NOLOGIN SUPERUSER'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_test_superuser')
\gexec

SELECT 'CREATE ROLE postgresem_test_bypassrls NOLOGIN BYPASSRLS'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_test_bypassrls')
\gexec

SELECT 'CREATE ROLE postgresem_tenant_a NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_tenant_a')
\gexec

SELECT 'CREATE ROLE postgresem_tenant_b NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_tenant_b')
\gexec

SELECT 'CREATE ROLE postgresem_order_writer NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_order_writer')
\gexec

SELECT 'CREATE ROLE postgresem_tenant_a_writer NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_tenant_a_writer')
\gexec

SELECT 'CREATE ROLE postgresem_tenant_b_writer NOLOGIN'
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'postgresem_tenant_b_writer')
\gexec

GRANT postgresem_auditor TO postgresem_audit_writer;
GRANT postgresem_mutator TO
  postgresem_order_writer,
  postgresem_tenant_a_writer,
  postgresem_tenant_b_writer;
GRANT
  postgresem_analyst,
  postgresem_tenant_a,
  postgresem_tenant_b
TO postgresem_runtime;
GRANT
  postgresem_order_writer,
  postgresem_tenant_a_writer,
  postgresem_tenant_b_writer
TO postgresem_mutation_runtime;

REVOKE
  postgresem_source_owner,
  postgresem_test_superuser,
  postgresem_test_bypassrls
FROM postgresem_runtime;

REVOKE
  postgresem_source_owner,
  postgresem_test_superuser,
  postgresem_test_bypassrls,
  postgresem_analyst,
  postgresem_tenant_a,
  postgresem_tenant_b
FROM postgresem_mutation_runtime;
