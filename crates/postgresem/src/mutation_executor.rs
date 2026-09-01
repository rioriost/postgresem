use std::{
    env,
    time::{Duration, Instant},
};

use fallible_iterator::FallibleIterator;
use postgres::{Client, IsolationLevel, Transaction, error::SqlState, types::ToSql};
use postgresem_compiler::{
    CompiledMutation, DataType, MUTATION_COMPILER_SEMANTIC_VERSION, MutationCompileError,
    MutationCompilerOptions, MutationLineage, MutationOperation, MutationParameter, MutationValue,
    NormalizedLsm, OutputColumn, compile_lsm, normalize_lsm,
};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    database,
    executor::ExecutionContext,
    hash::sha256,
    published_model::{self, PublishedModel, PublishedMutationModel},
};

const DEFAULT_MAX_RESULT_BYTES: usize = 1_048_576;
const DEFAULT_STATEMENT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_LOCK_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_IDLE_TRANSACTION_TIMEOUT_MS: u64 = 5_000;

pub struct MutationExecutorConfig {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationResult {
    pub schema_version: String,
    pub mutation_id: String,
    pub semantic_revision: String,
    pub operation: MutationOperation,
    pub model: String,
    pub columns: Vec<OutputColumn>,
    pub rows: Vec<Value>,
    pub affected_rows: usize,
    pub replayed: bool,
    pub lineage: MutationLineage,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum MutationExecuteError {
    #[error("environment variable name is invalid: {0}")]
    InvalidEnvironmentVariableName(String),
    #[error("required environment variable {0} is not set")]
    MissingEnvironmentVariable(String),
    #[error("required environment variable {0} is not valid Unicode")]
    InvalidEnvironmentVariable(String),
    #[error("environment variable {variable} must be a positive integer")]
    InvalidIntegerConfiguration { variable: &'static str },
    #[error("configured mutation database role is not a safe unquoted PostgreSQL identifier")]
    InvalidDatabaseRole,
    #[error("failed to connect using mutation URL environment variable {0}")]
    MutationConnect(String),
    #[error("failed to connect using audit URL environment variable {0}")]
    AuditConnect(String),
    #[error(transparent)]
    PublishedModel(#[from] published_model::PublishedModelError),
    #[error(transparent)]
    Lsm(#[from] postgresem_compiler::LsmError),
    #[error(transparent)]
    Compile(#[from] MutationCompileError),
    #[error("failed to record mandatory mutation failure audit")]
    FailureAudit(#[source] postgres::Error),
    #[error("failed to start guarded mutation transaction")]
    StartTransaction(#[source] postgres::Error),
    #[error("configured mutation database role does not exist")]
    DatabaseRoleNotFound,
    #[error("mutation login is not an allowed member of the configured database role")]
    DatabaseRoleMembership,
    #[error("configured mutation database role is superuser or has BYPASSRLS")]
    UnsafeDatabaseRole,
    #[error("writable semantic model references an unavailable table")]
    TargetRelationNotFound,
    #[error("configured mutation database role owns the target table")]
    TargetRelationOwner,
    #[error("writable semantic model target is not a PostgreSQL table")]
    UnsafeTargetRelation,
    #[error("failed to apply guarded mutation transaction configuration")]
    TransactionConfiguration(#[source] postgres::Error),
    #[error("failed to claim mutation idempotency and audit state")]
    Claim(#[source] postgres::Error),
    #[error("idempotency key was already used for a different mutation")]
    IdempotencyConflict,
    #[error("mutation was cancelled")]
    Cancelled(#[source] postgres::Error),
    #[error("PostgreSQL rejected the mutation")]
    Execution(#[source] postgres::Error),
    #[error("mutation returned an invalid row shape")]
    InvalidRowShape,
    #[error("mutation result exceeded the configured byte limit")]
    ResultByteLimitExceeded,
    #[error("mutation affected an unexpected number of rows")]
    AffectedRowMismatch,
    #[error("failed to serialize mutation result or audit metadata")]
    Serialization(#[source] serde_json::Error),
    #[error("failed to atomically finish mutation audit")]
    FinishAudit(#[source] postgres::Error),
    #[error("mutation commit outcome is indeterminate; retry with the same idempotency key")]
    CommitIndeterminate(#[source] postgres::Error),
    #[error("stored idempotent mutation result is invalid")]
    InvalidReplayResult,
    #[error("failed to reconcile mutation idempotency state")]
    Reconciliation(#[source] postgres::Error),
    #[error("idempotency key must not be empty or exceed 256 bytes")]
    InvalidIdempotencyKey,
}

impl MutationExecutorConfig {
    pub fn from_environment(
        database_url_variable: &str,
        audit_database_url_variable: &str,
        database_role_variable: &str,
    ) -> Result<Self, MutationExecuteError> {
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
    ) -> Result<Self, MutationExecuteError> {
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
                return Err(MutationExecuteError::InvalidEnvironmentVariableName(
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
            return Err(MutationExecuteError::InvalidDatabaseRole);
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
                "POSTGRESEM_MAX_MUTATION_RESULT_BYTES",
                DEFAULT_MAX_RESULT_BYTES,
            )?,
            statement_timeout: Duration::from_millis(positive_integer_environment(
                "POSTGRESEM_MUTATION_STATEMENT_TIMEOUT_MS",
                DEFAULT_STATEMENT_TIMEOUT_MS,
            )?),
            lock_timeout: Duration::from_millis(positive_integer_environment(
                "POSTGRESEM_MUTATION_LOCK_TIMEOUT_MS",
                DEFAULT_LOCK_TIMEOUT_MS,
            )?),
            idle_transaction_timeout: Duration::from_millis(positive_integer_environment(
                "POSTGRESEM_MUTATION_IDLE_IN_TRANSACTION_SESSION_TIMEOUT_MS",
                DEFAULT_IDLE_TRANSACTION_TIMEOUT_MS,
            )?),
        })
    }

    #[must_use]
    pub fn database_role(&self) -> &str {
        &self.database_role
    }

    pub(crate) fn connect_mutation(&self) -> Result<Client, MutationExecuteError> {
        database::connect(&self.database_url, self.database_password.as_deref())
            .map_err(|_| MutationExecuteError::MutationConnect(self.database_url_variable.clone()))
    }

    fn connect_audit(&self) -> Result<Client, MutationExecuteError> {
        database::connect(
            &self.audit_database_url,
            self.audit_database_password.as_deref(),
        )
        .map_err(|_| MutationExecuteError::AuditConnect(self.audit_database_url_variable.clone()))
    }
}

pub fn execute(
    input: &[u8],
    project: &str,
    config: &MutationExecutorConfig,
    context: &ExecutionContext,
) -> Result<MutationResult, MutationExecuteError> {
    let mut audit = config.connect_audit()?;
    let raw_hash = sha256(input);
    let validation_started = Instant::now();
    let normalized = match normalize_lsm(input) {
        Ok(normalized) => normalized,
        Err(error) => {
            record_failure(
                &mut audit,
                project,
                context,
                config,
                FailureMetadata::raw(raw_hash),
                "rejected",
                error.code(),
                validation_started.elapsed(),
                Duration::ZERO,
                Duration::ZERO,
            )?;
            return Err(error.into());
        }
    };
    let validation_duration = validation_started.elapsed();

    let mut mutation = match config.connect_mutation() {
        Ok(client) => client,
        Err(error) => {
            record_failure(
                &mut audit,
                project,
                context,
                config,
                FailureMetadata::normalized(&normalized),
                "rejected",
                mutation_error_code(&error),
                validation_duration,
                Duration::ZERO,
                Duration::ZERO,
            )?;
            return Err(error);
        }
    };
    let published = match published_model::load_published_for_mutation(
        &mut mutation,
        project,
        &config.database_role,
    ) {
        Ok(published) => published,
        Err(error) => {
            record_failure(
                &mut audit,
                project,
                context,
                config,
                FailureMetadata::normalized(&normalized),
                "rejected",
                "MUTATION_SEMANTIC_SNAPSHOT_UNAVAILABLE",
                validation_duration,
                Duration::ZERO,
                Duration::ZERO,
            )?;
            return Err(error.into());
        }
    };

    let compile_started = Instant::now();
    let compiled = match compile_lsm(
        &normalized,
        &published.published.snapshot,
        &published.capabilities,
        MutationCompilerOptions::default(),
    ) {
        Ok(compiled) => compiled,
        Err(error) => {
            let compile_duration = compile_started.elapsed();
            record_failure(
                &mut audit,
                project,
                context,
                config,
                FailureMetadata::published(&normalized, &published),
                "rejected",
                error.code(),
                validation_duration,
                compile_duration,
                Duration::ZERO,
            )?;
            return Err(error.into());
        }
    };
    let compile_duration = compile_started.elapsed();
    let database_started = Instant::now();
    match execute_compiled(
        &mut mutation,
        project,
        &published,
        &normalized,
        &compiled,
        config,
        context,
        validation_duration,
        compile_duration,
    ) {
        Ok(result) => Ok(result),
        Err(error) => {
            let status = if matches!(error, MutationExecuteError::CommitIndeterminate(_)) {
                "indeterminate"
            } else if mutation_was_attempted(&error) {
                "rolled_back"
            } else {
                "rejected"
            };
            if !matches!(error, MutationExecuteError::IdempotencyConflict) {
                record_failure(
                    &mut audit,
                    project,
                    context,
                    config,
                    FailureMetadata::compiled(&normalized, &published, &compiled)?,
                    status,
                    mutation_error_code(&error),
                    validation_duration,
                    compile_duration,
                    database_started.elapsed(),
                )?;
            }
            Err(error)
        }
    }
}

pub fn reconcile(
    project: &str,
    idempotency_key: &str,
    config: &MutationExecutorConfig,
) -> Result<Option<Value>, MutationExecuteError> {
    if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
        return Err(MutationExecuteError::InvalidIdempotencyKey);
    }
    let mut audit = config.connect_audit()?;
    let row = audit
        .query_one(
            "SELECT semantic.lookup_mutation_idempotency($1, $2) AS state",
            &[&project, &sha256(idempotency_key)],
        )
        .map_err(MutationExecuteError::Reconciliation)?;
    Ok(row.get("state"))
}

#[allow(clippy::too_many_arguments)]
fn execute_compiled(
    client: &mut Client,
    project: &str,
    published: &PublishedMutationModel,
    normalized: &NormalizedLsm,
    compiled: &CompiledMutation,
    config: &MutationExecutorConfig,
    context: &ExecutionContext,
    validation_duration: Duration,
    compile_duration: Duration,
) -> Result<MutationResult, MutationExecuteError> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::ReadCommitted)
        .start()
        .map_err(MutationExecuteError::StartTransaction)?;

    fix_search_path(&mut transaction)?;
    let is_member = verify_role(&mut transaction, &config.database_role)?;
    verify_target_relation(
        &mut transaction,
        &published.published,
        compiled,
        &config.database_role,
    )?;
    if !is_member {
        return Err(MutationExecuteError::DatabaseRoleMembership);
    }
    apply_transaction_configuration(&mut transaction, config)?;

    let claim = claim_mutation(
        &mut transaction,
        project,
        published,
        normalized,
        compiled,
        config,
        context,
        validation_duration,
        compile_duration,
    )?;
    match claim.disposition.as_str() {
        "conflict" => {
            transaction
                .commit()
                .map_err(MutationExecuteError::CommitIndeterminate)?;
            return Err(MutationExecuteError::IdempotencyConflict);
        }
        "replay" => {
            let rows = replay_rows(claim.result)?;
            transaction
                .commit()
                .map_err(MutationExecuteError::CommitIndeterminate)?;
            return Ok(result(
                normalized,
                published,
                compiled,
                claim.mutation_id,
                rows,
                usize::try_from(claim.affected_rows.unwrap_or_default())
                    .map_err(|_| MutationExecuteError::InvalidReplayResult)?,
                true,
            ));
        }
        "execute" => {}
        _ => return Err(MutationExecuteError::InvalidReplayResult),
    }

    let database_started = Instant::now();
    let rows = run_statement(&mut transaction, compiled, config.max_result_bytes)?;
    if rows.len() != compiled.expected_rows {
        return Err(MutationExecuteError::AffectedRowMismatch);
    }
    let result_json = Value::Array(rows.clone());
    let affected_rows =
        i64::try_from(rows.len()).map_err(|_| MutationExecuteError::AffectedRowMismatch)?;
    finish_mutation(
        &mut transaction,
        &claim.mutation_id,
        claim
            .attempt_id
            .as_deref()
            .ok_or(MutationExecuteError::InvalidReplayResult)?,
        &result_json,
        affected_rows,
        database_started.elapsed(),
    )?;
    transaction
        .commit()
        .map_err(MutationExecuteError::CommitIndeterminate)?;
    Ok(result(
        normalized,
        published,
        compiled,
        claim.mutation_id,
        rows,
        usize::try_from(affected_rows).map_err(|_| MutationExecuteError::AffectedRowMismatch)?,
        false,
    ))
}

fn result(
    normalized: &NormalizedLsm,
    published: &PublishedMutationModel,
    compiled: &CompiledMutation,
    mutation_id: String,
    rows: Vec<Value>,
    affected_rows: usize,
    replayed: bool,
) -> MutationResult {
    MutationResult {
        schema_version: normalized.mutation.schema_version.clone(),
        mutation_id,
        semantic_revision: published.published.snapshot.revision_hash.clone(),
        operation: compiled.operation,
        model: compiled.model.clone(),
        columns: compiled.returning_schema.clone(),
        rows,
        affected_rows,
        replayed,
        lineage: compiled.lineage.clone(),
        warnings: vec![],
    }
}

struct Claim {
    disposition: String,
    mutation_id: String,
    attempt_id: Option<String>,
    affected_rows: Option<i64>,
    result: Option<Value>,
}

#[allow(clippy::too_many_arguments)]
fn claim_mutation(
    transaction: &mut Transaction<'_>,
    project: &str,
    published: &PublishedMutationModel,
    normalized: &NormalizedLsm,
    compiled: &CompiledMutation,
    config: &MutationExecutorConfig,
    context: &ExecutionContext,
    validation_duration: Duration,
    compile_duration: Duration,
) -> Result<Claim, MutationExecuteError> {
    let parameter_types = parameter_types(&compiled.parameters)?;
    let lineage =
        serde_json::to_value(&compiled.lineage).map_err(MutationExecuteError::Serialization)?;
    let policy_context = json!({
        "database_role": config.database_role,
        "project": project,
        "capability_profile": published.capabilities.profile,
    });
    let operation = operation_name(compiled.operation);
    let principal_subject_hash = sha256(context.principal_subject());
    let authority_hash = sha256(
        &serde_json::to_string(&[
            principal_subject_hash.as_str(),
            context.config_profile(),
            config.database_role(),
        ])
        .map_err(MutationExecuteError::Serialization)?,
    );
    let requested_rows = i64::try_from(compiled.expected_rows)
        .map_err(|_| MutationExecuteError::AffectedRowMismatch)?;
    let validation_ms = duration_milliseconds(validation_duration);
    let compile_ms = duration_milliseconds(compile_duration);
    let row = transaction
        .query_one(
            r"
            SELECT
              disposition,
              mutation_id::text AS mutation_id,
              attempt_id::text AS attempt_id,
              affected_rows,
              result
            FROM semantic.claim_mutation(
              $1, $2, $3, $4, $5, $6::text::uuid, $7, $8, $9, $10, $11,
              $12, $13, $14, $15, $16, $17, $18, $19, $20
            )
            ",
            &[
                &project,
                &normalized.idempotency_key_hash,
                &authority_hash,
                &normalized.mutation.schema_version,
                &normalized.hash,
                &published.published.revision_id,
                &published.published.snapshot.revision_hash,
                &principal_subject_hash,
                &MUTATION_COMPILER_SEMANTIC_VERSION,
                &context.config_profile(),
                &operation,
                &compiled.model,
                &compiled.statement_hash,
                &compiled.mutation_hash,
                &parameter_types,
                &lineage,
                &policy_context,
                &requested_rows,
                &validation_ms,
                &compile_ms,
            ],
        )
        .map_err(MutationExecuteError::Claim)?;
    Ok(Claim {
        disposition: row.get("disposition"),
        mutation_id: row.get("mutation_id"),
        attempt_id: row.get("attempt_id"),
        affected_rows: row.get("affected_rows"),
        result: row.get("result"),
    })
}

fn run_statement(
    transaction: &mut Transaction<'_>,
    compiled: &CompiledMutation,
    max_result_bytes: usize,
) -> Result<Vec<Value>, MutationExecuteError> {
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
    {
        let mut source_rows = transaction
            .query_raw(&sql, parameters)
            .map_err(mutation_query_error)?;
        while let Some(source_row) = source_rows.next().map_err(mutation_query_error)? {
            let value: Value = source_row.get("row_data");
            if !value.is_array() {
                return Err(MutationExecuteError::InvalidRowShape);
            }
            let row_bytes =
                serde_json::to_vec(&value).map_err(MutationExecuteError::Serialization)?;
            let separator_bytes = usize::from(!rows.is_empty());
            if byte_count
                .saturating_add(separator_bytes)
                .saturating_add(row_bytes.len())
                > max_result_bytes
            {
                return Err(MutationExecuteError::ResultByteLimitExceeded);
            }
            byte_count += separator_bytes + row_bytes.len();
            rows.push(value);
        }
    }
    Ok(rows)
}

fn replay_rows(result: Option<Value>) -> Result<Vec<Value>, MutationExecuteError> {
    let Value::Array(rows) = result.ok_or(MutationExecuteError::InvalidReplayResult)? else {
        return Err(MutationExecuteError::InvalidReplayResult);
    };
    if rows.iter().all(Value::is_array) {
        Ok(rows)
    } else {
        Err(MutationExecuteError::InvalidReplayResult)
    }
}

fn verify_role(
    transaction: &mut Transaction<'_>,
    database_role: &str,
) -> Result<bool, MutationExecuteError> {
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
        .map_err(MutationExecuteError::TransactionConfiguration)?
        .ok_or(MutationExecuteError::DatabaseRoleNotFound)?;
    if row.get::<_, bool>("rolsuper")
        || row.get::<_, bool>("rolbypassrls")
        || row.get::<_, bool>("can_assume_unsafe_role")
    {
        return Err(MutationExecuteError::UnsafeDatabaseRole);
    }
    Ok(row.get("is_member"))
}

fn verify_target_relation(
    transaction: &mut Transaction<'_>,
    published: &PublishedModel,
    compiled: &CompiledMutation,
    database_role: &str,
) -> Result<(), MutationExecuteError> {
    let model = published
        .snapshot
        .models
        .iter()
        .find(|model| model.semantic_name == compiled.model)
        .ok_or(MutationExecuteError::TargetRelationNotFound)?;
    let row = transaction
        .query_opt(
            r"
            SELECT
              pg_catalog.pg_has_role(
                mapped_role.oid,
                owner.oid,
                'MEMBER'
              ) AS can_assume_owner,
              relation.relkind::text AS relation_kind
            FROM pg_catalog.pg_class AS relation
            JOIN pg_catalog.pg_namespace AS namespace
              ON namespace.oid = relation.relnamespace
            JOIN pg_catalog.pg_roles AS owner
              ON owner.oid = relation.relowner
            JOIN pg_catalog.pg_roles AS mapped_role
              ON mapped_role.rolname = $1
            WHERE namespace.nspname = $2
              AND relation.relname = $3
            ",
            &[&database_role, &model.source.schema, &model.source.relation],
        )
        .map_err(MutationExecuteError::TransactionConfiguration)?
        .ok_or(MutationExecuteError::TargetRelationNotFound)?;
    if row.get::<_, bool>("can_assume_owner") {
        return Err(MutationExecuteError::TargetRelationOwner);
    }
    if !matches!(row.get::<_, String>("relation_kind").as_str(), "r" | "p") {
        return Err(MutationExecuteError::UnsafeTargetRelation);
    }
    Ok(())
}

fn apply_transaction_configuration(
    transaction: &mut Transaction<'_>,
    config: &MutationExecutorConfig,
) -> Result<(), MutationExecuteError> {
    transaction
        .batch_execute(&format!(
            "SET LOCAL ROLE {}",
            quote_identifier(&config.database_role)
        ))
        .map_err(MutationExecuteError::TransactionConfiguration)?;
    for (name, value) in [
        ("statement_timeout", config.statement_timeout),
        ("lock_timeout", config.lock_timeout),
        (
            "idle_in_transaction_session_timeout",
            config.idle_transaction_timeout,
        ),
    ] {
        transaction
            .query_one(
                "SELECT pg_catalog.set_config($1, $2, true)",
                &[&name, &format!("{}ms", value.as_millis())],
            )
            .map_err(MutationExecuteError::TransactionConfiguration)?;
    }
    transaction
        .query_one("SELECT pg_catalog.set_config('TimeZone', 'UTC', true)", &[])
        .map_err(MutationExecuteError::TransactionConfiguration)?;
    Ok(())
}

fn fix_search_path(transaction: &mut Transaction<'_>) -> Result<(), MutationExecuteError> {
    transaction
        .query_one(
            "SELECT pg_catalog.set_config('search_path', 'pg_catalog', true)",
            &[],
        )
        .map_err(MutationExecuteError::TransactionConfiguration)?;
    Ok(())
}

fn result_wrapper_sql(compiled: &CompiledMutation) -> String {
    let row_values = compiled
        .returning_schema
        .iter()
        .map(|column| {
            let identifier = format!("mutation_result.{}", quote_identifier(&column.name));
            if column.data_type == DataType::Numeric {
                format!("{identifier}::text")
            } else {
                identifier
            }
        })
        .collect::<Vec<_>>();
    format!(
        "WITH mutation_result AS (\n{}\n)\nSELECT jsonb_build_array({}) AS row_data\nFROM mutation_result",
        compiled.statement,
        row_values.join(", ")
    )
}

fn parameter_text(parameter: &MutationParameter) -> Option<String> {
    match &parameter.value {
        MutationValue::Null => None,
        MutationValue::Text(value)
        | MutationValue::Numeric(value)
        | MutationValue::Date(value)
        | MutationValue::Timestamp(value) => Some(value.clone()),
        MutationValue::Boolean(value) => Some(value.to_string()),
        MutationValue::Integer(value) => Some(value.to_string()),
    }
}

fn finish_mutation(
    transaction: &mut Transaction<'_>,
    mutation_id: &str,
    attempt_id: &str,
    result: &Value,
    affected_rows: i64,
    database_duration: Duration,
) -> Result<(), MutationExecuteError> {
    transaction
        .query_one(
            r"
            SELECT semantic.finish_mutation(
              $1::text::uuid, $2::text::uuid, $3, $4, $5
            )
            ",
            &[
                &mutation_id,
                &attempt_id,
                &result,
                &affected_rows,
                &duration_milliseconds(database_duration),
            ],
        )
        .map_err(MutationExecuteError::FinishAudit)?;
    Ok(())
}

struct FailureMetadata {
    lsm_schema_version: Option<String>,
    lsm_hash: String,
    revision_id: Option<String>,
    semantic_revision_hash: Option<String>,
    operation: Option<String>,
    model: Option<String>,
    idempotency_key_hash: Option<String>,
    statement_hash: Option<String>,
    compiler_mutation_hash: Option<String>,
    parameter_types: Value,
    lineage: Value,
    requested_rows: i64,
}

impl FailureMetadata {
    fn raw(lsm_hash: String) -> Self {
        Self {
            lsm_schema_version: None,
            lsm_hash,
            revision_id: None,
            semantic_revision_hash: None,
            operation: None,
            model: None,
            idempotency_key_hash: None,
            statement_hash: None,
            compiler_mutation_hash: None,
            parameter_types: json!([]),
            lineage: json!({}),
            requested_rows: 0,
        }
    }

    fn normalized(normalized: &NormalizedLsm) -> Self {
        Self {
            lsm_schema_version: Some(normalized.mutation.schema_version.clone()),
            lsm_hash: normalized.hash.clone(),
            revision_id: None,
            semantic_revision_hash: None,
            operation: Some(operation_name(normalized.mutation.operation).to_owned()),
            model: Some(normalized.mutation.model.clone()),
            idempotency_key_hash: Some(normalized.idempotency_key_hash.clone()),
            statement_hash: None,
            compiler_mutation_hash: None,
            parameter_types: json!([]),
            lineage: json!({
                "model": normalized.mutation.model,
                "fields": normalized.mutation.rows[0].keys().collect::<Vec<_>>(),
            }),
            requested_rows: i64::try_from(normalized.mutation.rows.len()).unwrap_or(i64::MAX),
        }
    }

    fn published(normalized: &NormalizedLsm, published: &PublishedMutationModel) -> Self {
        let mut metadata = Self::normalized(normalized);
        metadata.revision_id = Some(published.published.revision_id.clone());
        metadata.semantic_revision_hash = Some(published.published.snapshot.revision_hash.clone());
        metadata
    }

    fn compiled(
        normalized: &NormalizedLsm,
        published: &PublishedMutationModel,
        compiled: &CompiledMutation,
    ) -> Result<Self, MutationExecuteError> {
        let mut metadata = Self::published(normalized, published);
        metadata.statement_hash = Some(compiled.statement_hash.clone());
        metadata.compiler_mutation_hash = Some(compiled.mutation_hash.clone());
        metadata.parameter_types = parameter_types(&compiled.parameters)?;
        metadata.lineage =
            serde_json::to_value(&compiled.lineage).map_err(MutationExecuteError::Serialization)?;
        Ok(metadata)
    }
}

#[allow(clippy::too_many_arguments)]
fn record_failure(
    audit: &mut Client,
    project: &str,
    context: &ExecutionContext,
    config: &MutationExecutorConfig,
    metadata: FailureMetadata,
    status: &str,
    error_code: &str,
    validation_duration: Duration,
    compile_duration: Duration,
    database_duration: Duration,
) -> Result<(), MutationExecuteError> {
    let policy_context = json!({
        "database_role": config.database_role,
        "project": project,
    });
    audit
        .query_one(
            r"
            SELECT semantic.record_mutation_failure(
              $1, $2, $3, $4, $5::text::uuid, $6, $7, $8, $9, $10, $11,
              $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22
            )::text
            ",
            &[
                &project,
                &sha256(context.principal_subject()),
                &metadata.lsm_schema_version,
                &metadata.lsm_hash,
                &metadata.revision_id,
                &metadata.semantic_revision_hash,
                &MUTATION_COMPILER_SEMANTIC_VERSION,
                &context.config_profile(),
                &metadata.operation,
                &metadata.model,
                &metadata.idempotency_key_hash,
                &metadata.statement_hash,
                &metadata.compiler_mutation_hash,
                &metadata.parameter_types,
                &metadata.lineage,
                &policy_context,
                &status,
                &error_code,
                &metadata.requested_rows,
                &duration_milliseconds(validation_duration),
                &duration_milliseconds(compile_duration),
                &duration_milliseconds(database_duration),
            ],
        )
        .map_err(MutationExecuteError::FailureAudit)?;
    Ok(())
}

fn parameter_types(parameters: &[MutationParameter]) -> Result<Value, MutationExecuteError> {
    Ok(Value::Array(
        parameters
            .iter()
            .map(|parameter| serde_json::to_value(parameter.data_type))
            .collect::<Result<Vec<_>, _>>()
            .map_err(MutationExecuteError::Serialization)?,
    ))
}

fn mutation_query_error(source: postgres::Error) -> MutationExecuteError {
    if source.code() == Some(&SqlState::QUERY_CANCELED) {
        MutationExecuteError::Cancelled(source)
    } else {
        MutationExecuteError::Execution(source)
    }
}

fn mutation_was_attempted(error: &MutationExecuteError) -> bool {
    matches!(
        error,
        MutationExecuteError::Cancelled(_)
            | MutationExecuteError::Execution(_)
            | MutationExecuteError::InvalidRowShape
            | MutationExecuteError::ResultByteLimitExceeded
            | MutationExecuteError::AffectedRowMismatch
            | MutationExecuteError::Serialization(_)
            | MutationExecuteError::FinishAudit(_)
    )
}

pub const fn mutation_error_code(error: &MutationExecuteError) -> &'static str {
    match error {
        MutationExecuteError::Lsm(error) => error.code(),
        MutationExecuteError::Compile(error) => error.code(),
        MutationExecuteError::DatabaseRoleNotFound => "MUTATION_DATABASE_ROLE_NOT_FOUND",
        MutationExecuteError::DatabaseRoleMembership => "MUTATION_DATABASE_ROLE_MEMBERSHIP_DENIED",
        MutationExecuteError::UnsafeDatabaseRole => "MUTATION_UNSAFE_DATABASE_ROLE",
        MutationExecuteError::TargetRelationNotFound => "MUTATION_TARGET_RELATION_NOT_FOUND",
        MutationExecuteError::TargetRelationOwner => "MUTATION_TARGET_RELATION_OWNER",
        MutationExecuteError::UnsafeTargetRelation => "MUTATION_UNSAFE_TARGET_RELATION",
        MutationExecuteError::IdempotencyConflict => "MUTATION_IDEMPOTENCY_CONFLICT",
        MutationExecuteError::Cancelled(_) => "MUTATION_CANCELLED",
        MutationExecuteError::Execution(_) => "MUTATION_DATABASE_REJECTED",
        MutationExecuteError::InvalidRowShape => "MUTATION_INVALID_ROW_SHAPE",
        MutationExecuteError::ResultByteLimitExceeded => "MUTATION_RESULT_BYTES_EXCEEDED",
        MutationExecuteError::AffectedRowMismatch => "MUTATION_AFFECTED_ROWS_MISMATCH",
        MutationExecuteError::FinishAudit(_) => "MUTATION_ATOMIC_AUDIT_FAILED",
        MutationExecuteError::CommitIndeterminate(_) => "MUTATION_COMMIT_INDETERMINATE",
        MutationExecuteError::InvalidReplayResult => "MUTATION_INVALID_REPLAY_RESULT",
        MutationExecuteError::Reconciliation(_) => "MUTATION_RECONCILIATION_FAILED",
        MutationExecuteError::InvalidIdempotencyKey => "MUTATION_INVALID_IDEMPOTENCY_KEY",
        _ => "MUTATION_GUARD_CONFIGURATION_FAILED",
    }
}

const fn operation_name(operation: MutationOperation) -> &'static str {
    match operation {
        MutationOperation::Insert => "insert",
        MutationOperation::Upsert => "upsert",
    }
}

fn required_environment(variable: &str) -> Result<String, MutationExecuteError> {
    env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => {
            MutationExecuteError::MissingEnvironmentVariable(variable.to_owned())
        }
        env::VarError::NotUnicode(_) => {
            MutationExecuteError::InvalidEnvironmentVariable(variable.to_owned())
        }
    })
}

fn positive_integer_environment<T>(
    variable: &'static str,
    default: T,
) -> Result<T, MutationExecuteError>
where
    T: std::str::FromStr + PartialOrd + From<u8> + Copy,
{
    match env::var(variable) {
        Ok(value) => value
            .parse::<T>()
            .ok()
            .filter(|parsed| *parsed > T::from(0))
            .ok_or(MutationExecuteError::InvalidIntegerConfiguration { variable }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => {
            Err(MutationExecuteError::InvalidIntegerConfiguration { variable })
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

#[cfg(test)]
mod tests {
    use postgresem_compiler::{
        CompiledMutation, DataType, MutationLineage, MutationOperation, OutputColumn,
    };

    use super::{
        MutationExecuteError, mutation_error_code, result_wrapper_sql, valid_database_role,
        valid_environment_variable_name,
    };

    #[test]
    fn configuration_names_and_roles_are_strict_identifiers() {
        assert!(valid_environment_variable_name(
            "POSTGRESEM_MUTATION_DATABASE_URL"
        ));
        assert!(!valid_environment_variable_name("ROLE-NAME"));
        assert!(valid_database_role("postgresem_order_writer"));
        assert!(!valid_database_role("writer\"; RESET ROLE; --"));
    }

    #[test]
    fn result_wrapper_never_accepts_caller_sql() {
        let compiled = CompiledMutation {
            statement: "INSERT INTO \"commerce\".\"orders\" (\"amount\") VALUES ($1::text::numeric) RETURNING \"amount\" AS \"amount\"".to_owned(),
            parameters: vec![],
            returning_schema: vec![OutputColumn {
                name: "amount".to_owned(),
                data_type: DataType::Numeric,
            }],
            lineage: MutationLineage {
                model: "orders".to_owned(),
                fields: vec!["amount".to_owned()],
                returning_fields: vec!["amount".to_owned()],
                source_columns: vec!["commerce.orders.amount".to_owned()],
            },
            mutation_hash: "sha256:test".to_owned(),
            statement_hash: "sha256:test".to_owned(),
            expected_rows: 1,
            operation: MutationOperation::Insert,
            model: "orders".to_owned(),
        };
        let sql = result_wrapper_sql(&compiled);
        assert!(sql.starts_with("WITH mutation_result AS ("));
        assert!(sql.contains("mutation_result.\"amount\"::text"));
    }

    #[test]
    fn public_error_codes_do_not_expose_database_details() {
        assert_eq!(
            mutation_error_code(&MutationExecuteError::IdempotencyConflict),
            "MUTATION_IDEMPOTENCY_CONFLICT"
        );
        assert_eq!(
            mutation_error_code(&MutationExecuteError::AffectedRowMismatch),
            "MUTATION_AFFECTED_ROWS_MISMATCH"
        );
    }
}
