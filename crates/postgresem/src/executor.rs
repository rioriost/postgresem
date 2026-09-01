use std::{
    collections::BTreeSet,
    env,
    time::{Duration, Instant},
};

use fallible_iterator::FallibleIterator;
use postgres::{Client, IsolationLevel, Transaction, error::SqlState, types::ToSql};
use postgresem_compiler::{
    COMPILER_SEMANTIC_VERSION, CompiledParameter, CompiledQuery, CompilerOptions, DataType,
    Lineage, Literal, NormalizedLsq, OutputColumn, SemanticSnapshot, compile_lsq, normalize_lsq,
};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    database,
    hash::sha256,
    published_model::{self, PublishedModel},
};

const DEFAULT_MAX_RESULT_BYTES: usize = 1_048_576;
const DEFAULT_STATEMENT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_LOCK_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_IDLE_TRANSACTION_TIMEOUT_MS: u64 = 5_000;
const RESULT_TRUNCATION_WARNING: &str =
    "result is incomplete because it exceeded the byte limit; narrow the query";

pub struct ExecutorConfig {
    database_url: String,
    database_url_variable: String,
    database_password: Option<String>,
    audit_database_url: String,
    audit_database_url_variable: String,
    audit_database_password: Option<String>,
    database_role: String,
    max_result_bytes: usize,
    statement_timeout: Duration,
    lock_timeout: Duration,
    idle_transaction_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    principal_subject: String,
    config_profile: String,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub schema_version: String,
    pub query_id: String,
    pub semantic_revision: String,
    pub columns: Vec<OutputColumn>,
    pub rows: Vec<Value>,
    pub truncated: bool,
    pub lineage: Lineage,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExecuteError {
    #[error("environment variable name is invalid: {0}")]
    InvalidEnvironmentVariableName(String),
    #[error("required environment variable {0} is not set")]
    MissingEnvironmentVariable(String),
    #[error("required environment variable {0} is not valid Unicode")]
    InvalidEnvironmentVariable(String),
    #[error("environment variable {variable} must be a positive integer")]
    InvalidIntegerConfiguration { variable: &'static str },
    #[error("configured database role is not a safe unquoted PostgreSQL identifier")]
    InvalidDatabaseRole,
    #[error("execution context must have a non-empty principal subject and config profile")]
    InvalidExecutionContext,
    #[error("failed to connect using runtime URL environment variable {0}")]
    RuntimeConnect(String),
    #[error("failed to connect using audit URL environment variable {0}")]
    AuditConnect(String),
    #[error(transparent)]
    PublishedModel(#[from] published_model::PublishedModelError),
    #[error(transparent)]
    Lsq(#[from] postgresem_compiler::LsqError),
    #[error(transparent)]
    Compile(#[from] postgresem_compiler::CompileError),
    #[error("mandatory started audit record failed")]
    StartedAudit(#[source] postgres::Error),
    #[error("failed to serialize mandatory audit metadata")]
    AuditSerialization(#[source] serde_json::Error),
    #[error("terminal audit update failed")]
    TerminalAudit(#[source] postgres::Error),
    #[error("failed to start read-only source transaction")]
    StartSourceTransaction(#[source] postgres::Error),
    #[error("configured database role does not exist")]
    DatabaseRoleNotFound,
    #[error("runtime login is not an allowed member of the configured database role")]
    DatabaseRoleMembership,
    #[error("configured database role is superuser or has BYPASSRLS")]
    UnsafeDatabaseRole,
    #[error("compiled lineage references an unavailable physical source relation")]
    SourceRelationNotFound,
    #[error("configured database role owns a physical source relation used by the query")]
    SourceRelationOwner,
    #[error("failed to apply guarded transaction configuration")]
    TransactionConfiguration(#[source] postgres::Error),
    #[error("source query was cancelled")]
    SourceCancelled(#[source] postgres::Error),
    #[error("source query execution failed")]
    SourceExecution(#[source] postgres::Error),
    #[error("source transaction commit failed")]
    SourceCommit(#[source] postgres::Error),
    #[error("source row did not have the generated JSON array shape")]
    InvalidRowShape,
    #[error("failed to serialize a source result row")]
    RowSerialization(#[source] serde_json::Error),
}

impl ExecutorConfig {
    pub fn from_environment(
        database_url_variable: &str,
        audit_database_url_variable: &str,
        database_role_variable: &str,
    ) -> Result<Self, ExecuteError> {
        Self::from_environment_with_passwords(
            database_url_variable,
            None,
            audit_database_url_variable,
            None,
            database_role_variable,
        )
    }

    pub fn from_environment_with_passwords(
        database_url_variable: &str,
        database_password_variable: Option<&str>,
        audit_database_url_variable: &str,
        audit_database_password_variable: Option<&str>,
        database_role_variable: &str,
    ) -> Result<Self, ExecuteError> {
        for variable in [
            Some(database_url_variable),
            database_password_variable,
            Some(audit_database_url_variable),
            audit_database_password_variable,
            Some(database_role_variable),
        ]
        .into_iter()
        .flatten()
        {
            if !valid_environment_variable_name(variable) {
                return Err(ExecuteError::InvalidEnvironmentVariableName(
                    variable.to_owned(),
                ));
            }
        }

        let database_url = required_environment(database_url_variable)?;
        let database_password = database_password_variable
            .map(required_environment)
            .transpose()?;
        let audit_database_url = required_environment(audit_database_url_variable)?;
        let audit_database_password = audit_database_password_variable
            .map(required_environment)
            .transpose()?;
        let database_role = required_environment(database_role_variable)?;
        if !valid_database_role(&database_role) {
            return Err(ExecuteError::InvalidDatabaseRole);
        }

        Ok(Self {
            database_url,
            database_url_variable: database_url_variable.to_owned(),
            database_password,
            audit_database_url,
            audit_database_url_variable: audit_database_url_variable.to_owned(),
            audit_database_password,
            database_role,
            max_result_bytes: positive_integer_environment(
                "POSTGRESEM_MAX_RESULT_BYTES",
                DEFAULT_MAX_RESULT_BYTES,
            )?,
            statement_timeout: Duration::from_millis(positive_integer_environment(
                "POSTGRESEM_STATEMENT_TIMEOUT_MS",
                DEFAULT_STATEMENT_TIMEOUT_MS,
            )?),
            lock_timeout: Duration::from_millis(positive_integer_environment(
                "POSTGRESEM_LOCK_TIMEOUT_MS",
                DEFAULT_LOCK_TIMEOUT_MS,
            )?),
            idle_transaction_timeout: Duration::from_millis(positive_integer_environment(
                "POSTGRESEM_IDLE_IN_TRANSACTION_SESSION_TIMEOUT_MS",
                DEFAULT_IDLE_TRANSACTION_TIMEOUT_MS,
            )?),
        })
    }

    #[must_use]
    pub fn database_role(&self) -> &str {
        &self.database_role
    }

    #[must_use]
    pub const fn max_result_bytes(&self) -> usize {
        self.max_result_bytes
    }

    pub(crate) fn connect_runtime(&self) -> Result<Client, ExecuteError> {
        connect(
            &self.database_url,
            self.database_password.as_deref(),
            || ExecuteError::RuntimeConnect(self.database_url_variable.clone()),
        )
    }

    fn connect_audit(&self) -> Result<Client, ExecuteError> {
        connect(
            &self.audit_database_url,
            self.audit_database_password.as_deref(),
            || ExecuteError::AuditConnect(self.audit_database_url_variable.clone()),
        )
    }
}

impl ExecutionContext {
    pub fn new(
        principal_subject: impl Into<String>,
        config_profile: impl Into<String>,
    ) -> Result<Self, ExecuteError> {
        let principal_subject = principal_subject.into();
        let config_profile = config_profile.into();
        if principal_subject.trim().is_empty() || config_profile.trim().is_empty() {
            return Err(ExecuteError::InvalidExecutionContext);
        }
        Ok(Self {
            principal_subject,
            config_profile,
        })
    }

    pub(crate) fn principal_subject(&self) -> &str {
        &self.principal_subject
    }

    pub(crate) fn config_profile(&self) -> &str {
        &self.config_profile
    }
}

pub fn execute(
    input: &[u8],
    project: &str,
    config: &ExecutorConfig,
    context: &ExecutionContext,
) -> Result<QueryResult, ExecuteError> {
    let mut runtime = config.connect_runtime()?;
    let published = published_model::load_published(&mut runtime, project)?;

    let validation_started = Instant::now();
    let normalized = normalize_lsq(input)?;
    let validation_duration = validation_started.elapsed();

    let compile_started = Instant::now();
    let compiled = compile_lsq(&normalized, &published.snapshot, CompilerOptions::default())?;
    let compile_duration = compile_started.elapsed();

    let mut audit = config.connect_audit()?;
    let query_id = write_started_audit(
        &mut audit,
        &published,
        &normalized,
        &compiled,
        config,
        context,
        validation_duration,
        compile_duration,
    )?;

    let database_started = Instant::now();
    match execute_compiled(&mut runtime, &published.snapshot, &compiled, config) {
        Ok(execution) => {
            let database_duration = database_started.elapsed();
            finish_audit(
                &mut audit,
                &query_id,
                "succeeded",
                None,
                database_duration,
                execution.serialization_duration,
                execution.rows.len(),
                execution.byte_count,
                execution.truncated,
            )?;
            Ok(QueryResult {
                schema_version: normalized.query.schema_version,
                query_id,
                semantic_revision: published.snapshot.revision_hash,
                columns: compiled.output_schema,
                rows: execution.rows,
                truncated: execution.truncated,
                lineage: compiled.lineage,
                warnings: result_warnings(execution.truncated),
            })
        }

        Err(error) => {
            let status = if matches!(error, ExecuteError::SourceCancelled(_)) {
                "cancelled"
            } else {
                "failed"
            };
            let code = execution_error_code(&error);
            finish_audit(
                &mut audit,
                &query_id,
                status,
                Some(code),
                database_started.elapsed(),
                Duration::ZERO,
                0,
                0,
                false,
            )?;
            Err(error)
        }
    }
}

fn result_warnings(truncated: bool) -> Vec<String> {
    if truncated {
        vec![RESULT_TRUNCATION_WARNING.to_owned()]
    } else {
        vec![]
    }
}

struct ExecutionRows {
    rows: Vec<Value>,
    truncated: bool,
    byte_count: usize,
    serialization_duration: Duration,
}

fn execute_compiled(
    client: &mut Client,
    snapshot: &SemanticSnapshot,
    compiled: &CompiledQuery,
    config: &ExecutorConfig,
) -> Result<ExecutionRows, ExecuteError> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::ReadCommitted)
        .read_only(true)
        .start()
        .map_err(ExecuteError::StartSourceTransaction)?;

    fix_search_path(&mut transaction)?;
    let is_role_member = verify_role(&mut transaction, &config.database_role)?;
    verify_relation_ownership(&mut transaction, snapshot, compiled, &config.database_role)?;
    if !is_role_member {
        return Err(ExecuteError::DatabaseRoleMembership);
    }
    apply_transaction_configuration(&mut transaction, config)?;

    let sql = result_wrapper_sql(compiled);
    let parameter_values = compiled
        .parameters
        .iter()
        .map(parameter_text)
        .collect::<Vec<_>>();
    let parameters = parameter_values
        .iter()
        .map(|value| value as &(dyn ToSql + Sync))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut byte_count = 2_usize;
    let mut truncated = false;
    let mut serialization_duration = Duration::ZERO;
    {
        let mut source_rows = transaction
            .query_raw(&sql, parameters)
            .map_err(source_query_error)?;
        while let Some(source_row) = source_rows.next().map_err(source_query_error)? {
            let serialization_started = Instant::now();
            let value: Value = source_row.get("row_data");
            if !value.is_array() {
                return Err(ExecuteError::InvalidRowShape);
            }
            let row_bytes = serde_json::to_vec(&value).map_err(ExecuteError::RowSerialization)?;
            let separator_bytes = usize::from(!rows.is_empty());
            if byte_count
                .saturating_add(separator_bytes)
                .saturating_add(row_bytes.len())
                > config.max_result_bytes
            {
                truncated = true;
                serialization_duration += serialization_started.elapsed();
                break;
            }
            byte_count += separator_bytes + row_bytes.len();
            rows.push(value);
            serialization_duration += serialization_started.elapsed();
        }
    }

    transaction.commit().map_err(ExecuteError::SourceCommit)?;
    Ok(ExecutionRows {
        rows,
        truncated,
        byte_count,
        serialization_duration,
    })
}

fn verify_role(
    transaction: &mut Transaction<'_>,
    database_role: &str,
) -> Result<bool, ExecuteError> {
    let row = transaction
        .query_opt(
            r"
            SELECT
                role.rolsuper,
                role.rolbypassrls,
                EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_roles AS inherited_role
                    WHERE (
                        inherited_role.rolsuper
                        OR inherited_role.rolbypassrls
                    )
                      AND pg_catalog.pg_has_role(
                        role.oid,
                        inherited_role.oid,
                        'MEMBER'
                      )
                ) AS can_assume_unsafe_role,
                pg_has_role(session_user, role.oid, 'MEMBER') AS is_member
            FROM pg_catalog.pg_roles AS role
            WHERE role.rolname = $1
            ",
            &[&database_role],
        )
        .map_err(ExecuteError::TransactionConfiguration)?
        .ok_or(ExecuteError::DatabaseRoleNotFound)?;
    if row.get::<_, bool>("rolsuper")
        || row.get::<_, bool>("rolbypassrls")
        || row.get::<_, bool>("can_assume_unsafe_role")
    {
        return Err(ExecuteError::UnsafeDatabaseRole);
    }
    Ok(row.get("is_member"))
}

fn verify_relation_ownership(
    transaction: &mut Transaction<'_>,
    snapshot: &SemanticSnapshot,
    compiled: &CompiledQuery,
    database_role: &str,
) -> Result<(), ExecuteError> {
    let mut relations = BTreeSet::new();
    for name in &compiled.lineage.models {
        let model = snapshot
            .models
            .iter()
            .find(|model| &model.semantic_name == name)
            .ok_or(ExecuteError::SourceRelationNotFound)?;
        relations.insert((model.source.schema.clone(), model.source.relation.clone()));
    }

    for (schema, relation) in relations {
        let row = transaction
            .query_opt(
                r"
                SELECT pg_catalog.pg_has_role(
                  mapped_role.oid,
                  owner.oid,
                  'MEMBER'
                ) AS can_assume_owner
                FROM pg_catalog.pg_class AS relation
                JOIN pg_catalog.pg_namespace AS namespace
                  ON namespace.oid = relation.relnamespace
                JOIN pg_catalog.pg_roles AS owner
                  ON owner.oid = relation.relowner
                JOIN pg_catalog.pg_roles AS mapped_role
                  ON mapped_role.rolname = $1
                WHERE namespace.nspname = $2
                  AND relation.relname = $3
                  AND relation.relkind IN ('r', 'p', 'v', 'm', 'f')
                ",
                &[&database_role, &schema, &relation],
            )
            .map_err(ExecuteError::TransactionConfiguration)?
            .ok_or(ExecuteError::SourceRelationNotFound)?;
        if row.get::<_, bool>("can_assume_owner") {
            return Err(ExecuteError::SourceRelationOwner);
        }
    }
    Ok(())
}

fn apply_transaction_configuration(
    transaction: &mut Transaction<'_>,
    config: &ExecutorConfig,
) -> Result<(), ExecuteError> {
    transaction
        .batch_execute(&format!(
            "SET LOCAL ROLE {}",
            quote_identifier(&config.database_role)
        ))
        .map_err(ExecuteError::TransactionConfiguration)?;
    for (name, value) in [
        ("statement_timeout", config.statement_timeout),
        ("lock_timeout", config.lock_timeout),
        (
            "idle_in_transaction_session_timeout",
            config.idle_transaction_timeout,
        ),
    ] {
        let milliseconds = format!("{}ms", value.as_millis());
        transaction
            .query_one(
                "SELECT pg_catalog.set_config($1, $2, true)",
                &[&name, &milliseconds],
            )
            .map_err(ExecuteError::TransactionConfiguration)?;
    }
    transaction
        .query_one("SELECT pg_catalog.set_config('TimeZone', 'UTC', true)", &[])
        .map_err(ExecuteError::TransactionConfiguration)?;
    Ok(())
}

fn fix_search_path(transaction: &mut Transaction<'_>) -> Result<(), ExecuteError> {
    transaction
        .query_one(
            "SELECT pg_catalog.set_config('search_path', 'pg_catalog', true)",
            &[],
        )
        .map_err(ExecuteError::TransactionConfiguration)?;
    Ok(())
}

fn result_wrapper_sql(compiled: &CompiledQuery) -> String {
    let row_values = compiled
        .output_schema
        .iter()
        .map(|column| {
            let identifier = format!("compiled_result.{}", quote_identifier(&column.name));
            if column.data_type == DataType::Numeric {
                format!("{identifier}::text")
            } else {
                identifier
            }
        })
        .collect::<Vec<_>>();
    format!(
        "SELECT jsonb_build_array({}) AS row_data\nFROM (\n{}\n) AS compiled_result",
        row_values.join(", "),
        compiled.sql
    )
}

fn parameter_text(parameter: &CompiledParameter) -> String {
    match &parameter.value {
        Literal::Text(value)
        | Literal::Numeric(value)
        | Literal::Date(value)
        | Literal::Timestamp(value) => value.clone(),
        Literal::Boolean(value) => value.to_string(),
        Literal::Integer(value) => value.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_started_audit(
    audit: &mut Client,
    published: &PublishedModel,
    normalized: &NormalizedLsq,
    compiled: &CompiledQuery,
    config: &ExecutorConfig,
    context: &ExecutionContext,
    validation_duration: Duration,
    compile_duration: Duration,
) -> Result<String, ExecuteError> {
    let parameter_types = Value::Array(
        compiled
            .parameters
            .iter()
            .map(|parameter| serde_json::to_value(parameter.data_type))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ExecuteError::AuditSerialization)?,
    );
    let lineage =
        serde_json::to_value(&compiled.lineage).map_err(ExecuteError::AuditSerialization)?;
    let policy_context = json!({
        "database_role": config.database_role,
        "project": published.project,
    });
    let principal_hash = hash(&context.principal_subject);
    let validation_ms = duration_milliseconds(validation_duration);
    let compile_ms = duration_milliseconds(compile_duration);
    let row = audit
        .query_one(
            r"
            SELECT semantic.start_query_audit(
                $1, $2, $3, $4::text::uuid, $5, $6, $7, $8, $9, $10, $11,
                $12, $13, $14
            )::text AS query_id
            ",
            &[
                &principal_hash,
                &normalized.query.schema_version,
                &normalized.hash,
                &published.revision_id,
                &published.snapshot.revision_hash,
                &COMPILER_SEMANTIC_VERSION,
                &context.config_profile,
                &compiled.sql_hash,
                &compiled.query_hash,
                &parameter_types,
                &lineage,
                &policy_context,
                &validation_ms,
                &compile_ms,
            ],
        )
        .map_err(ExecuteError::StartedAudit)?;
    Ok(row.get("query_id"))
}

#[allow(clippy::too_many_arguments)]
fn finish_audit(
    audit: &mut Client,
    query_id: &str,
    status: &str,
    error_code: Option<&str>,
    database_duration: Duration,
    serialization_duration: Duration,
    row_count: usize,
    byte_count: usize,
    truncated: bool,
) -> Result<(), ExecuteError> {
    let database_ms = duration_milliseconds(database_duration);
    let serialization_ms = duration_milliseconds(serialization_duration);
    let row_count = i64::try_from(row_count).unwrap_or(i64::MAX);
    let byte_count = i64::try_from(byte_count).unwrap_or(i64::MAX);
    audit
        .query_one(
            r"
            SELECT semantic.finish_query_audit(
                $1::text::uuid, $2, $3, $4, $5, $6, $7, $8
            )
            ",
            &[
                &query_id,
                &status,
                &error_code,
                &database_ms,
                &serialization_ms,
                &row_count,
                &byte_count,
                &truncated,
            ],
        )
        .map_err(ExecuteError::TerminalAudit)?;
    Ok(())
}

fn execution_error_code(error: &ExecuteError) -> &'static str {
    match error {
        ExecuteError::DatabaseRoleNotFound => "EXECUTOR_DATABASE_ROLE_NOT_FOUND",
        ExecuteError::DatabaseRoleMembership => "EXECUTOR_DATABASE_ROLE_MEMBERSHIP_DENIED",
        ExecuteError::UnsafeDatabaseRole => "EXECUTOR_UNSAFE_DATABASE_ROLE",
        ExecuteError::SourceRelationNotFound => "EXECUTOR_SOURCE_RELATION_NOT_FOUND",
        ExecuteError::SourceRelationOwner => "EXECUTOR_SOURCE_RELATION_OWNER",
        ExecuteError::SourceCancelled(_) => "EXECUTOR_QUERY_CANCELLED",
        ExecuteError::SourceExecution(_) => "EXECUTOR_SOURCE_QUERY_FAILED",
        ExecuteError::SourceCommit(_) => "EXECUTOR_SOURCE_COMMIT_FAILED",
        ExecuteError::InvalidRowShape => "EXECUTOR_INVALID_ROW_SHAPE",
        ExecuteError::RowSerialization(_) => "EXECUTOR_ROW_SERIALIZATION_FAILED",
        _ => "EXECUTOR_GUARD_CONFIGURATION_FAILED",
    }
}

fn source_query_error(source: postgres::Error) -> ExecuteError {
    if source.code() == Some(&SqlState::QUERY_CANCELED) {
        ExecuteError::SourceCancelled(source)
    } else {
        ExecuteError::SourceExecution(source)
    }
}

fn required_environment(variable: &str) -> Result<String, ExecuteError> {
    env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => ExecuteError::MissingEnvironmentVariable(variable.to_owned()),
        env::VarError::NotUnicode(_) => {
            ExecuteError::InvalidEnvironmentVariable(variable.to_owned())
        }
    })
}

fn connect(
    conninfo: &str,
    password: Option<&str>,
    error: impl Fn() -> ExecuteError,
) -> Result<Client, ExecuteError> {
    database::connect(conninfo, password).map_err(|_| error())
}

fn positive_integer_environment<T>(variable: &'static str, default: T) -> Result<T, ExecuteError>
where
    T: std::str::FromStr + PartialOrd + From<u8> + Copy,
{
    match env::var(variable) {
        Ok(value) => value
            .parse::<T>()
            .ok()
            .filter(|parsed| *parsed > T::from(0))
            .ok_or(ExecuteError::InvalidIntegerConfiguration { variable }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => {
            Err(ExecuteError::InvalidIntegerConfiguration { variable })
        }
    }
}

fn valid_environment_variable_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_database_role(value: &str) -> bool {
    value.len() <= 63 && valid_environment_variable_name(value)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn duration_milliseconds(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn hash(value: &str) -> String {
    sha256(value)
}

#[cfg(test)]
mod tests {
    use postgresem_compiler::{CompiledQuery, DataType, Lineage, OutputColumn};

    use crate::database::connection_config;

    use super::{
        ExecuteError, ExecutionContext, ExecutorConfig, RESULT_TRUNCATION_WARNING, result_warnings,
        result_wrapper_sql, valid_database_role, valid_environment_variable_name,
    };

    #[test]
    fn configuration_names_and_roles_are_strict_identifiers() {
        assert!(valid_environment_variable_name("POSTGRESEM_DATABASE_URL"));
        assert!(!valid_environment_variable_name(
            "postgresql://localhost/app"
        ));
        assert!(!valid_environment_variable_name("ROLE-NAME"));
        assert!(valid_database_role("postgresem_analyst"));
        assert!(!valid_database_role("analyst\"; RESET ROLE; --"));
        assert!(matches!(
            ExecutorConfig::from_environment_with_passwords(
                "DATABASE_URL",
                Some("INVALID-NAME"),
                "POSTGRESEM_AUDIT_DATABASE_URL",
                Some("POSTGRESEM_AUDIT_WRITER_PASSWORD"),
                "POSTGRESEM_DB_ROLE",
            ),
            Err(ExecuteError::InvalidEnvironmentVariableName(variable))
                if variable == "INVALID-NAME"
        ));
    }

    #[test]
    fn connection_config_applies_passwords_without_conninfo_quoting() {
        let special_password = br#"quote' and backslash\ password"#;
        let configured = connection_config(
            "host=localhost dbname=postgresem user=runtime",
            Some(std::str::from_utf8(special_password).expect("ASCII password")),
        )
        .expect("passwordless conninfo parses");
        assert_eq!(configured.get_password(), Some(special_password.as_slice()));

        let complete_url =
            connection_config("postgresql://runtime:embedded@localhost/postgresem", None)
                .expect("complete URL parses");
        assert_eq!(complete_url.get_password(), Some(b"embedded".as_slice()));
    }

    #[test]
    fn result_wrapper_converts_only_numeric_columns_to_text() {
        let compiled = CompiledQuery {
            sql: "SELECT 1::bigint AS \"count\", 1.5::numeric AS \"amount\"".to_owned(),
            parameters: vec![],
            output_schema: vec![
                OutputColumn {
                    name: "count".to_owned(),
                    data_type: DataType::Integer,
                },
                OutputColumn {
                    name: "amount".to_owned(),
                    data_type: DataType::Numeric,
                },
            ],
            lineage: Lineage {
                models: vec![],
                metrics: vec![],
                relationships: vec![],
                source_columns: vec![],
            },
            query_hash: String::new(),
            sql_hash: String::new(),
        };

        assert_eq!(
            result_wrapper_sql(&compiled),
            concat!(
                "SELECT jsonb_build_array(compiled_result.\"count\", ",
                "compiled_result.\"amount\"::text) AS row_data\n",
                "FROM (\n",
                "SELECT 1::bigint AS \"count\", 1.5::numeric AS \"amount\"\n",
                ") AS compiled_result"
            )
        );
    }

    #[test]
    fn execution_context_requires_fixed_non_empty_identity_values() {
        assert!(ExecutionContext::new("mcp:stdio", "mcp-stdio").is_ok());
        assert!(ExecutionContext::new("", "mcp-stdio").is_err());
        assert!(ExecutionContext::new("mcp:stdio", " ").is_err());
    }

    #[test]
    fn byte_truncation_has_a_stable_incomplete_result_warning() {
        assert!(result_warnings(false).is_empty());
        assert_eq!(result_warnings(true), [RESULT_TRUNCATION_WARNING]);
    }
}
