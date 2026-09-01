use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use thiserror::Error;

use crate::{
    DataType, Field, MutationOperation, MutationValue, NormalizedLsm, OutputColumn,
    SemanticSnapshot, WritableField, WritableModel, hash::sha256,
};

pub const MUTATION_COMPILER_SEMANTIC_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationCapabilities {
    pub profile: String,
    pub writable_models: BTreeSet<String>,
}

impl MutationCapabilities {
    #[must_use]
    pub fn allows(&self, model: &str) -> bool {
        self.writable_models.contains(model)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationCompilerOptions {
    pub hard_row_limit: u32,
    pub hard_request_byte_limit: u32,
}

impl Default for MutationCompilerOptions {
    fn default() -> Self {
        Self {
            hard_row_limit: 100,
            hard_request_byte_limit: 1_048_576,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompiledMutation {
    pub statement: String,
    pub parameters: Vec<MutationParameter>,
    pub returning_schema: Vec<OutputColumn>,
    pub lineage: MutationLineage,
    pub mutation_hash: String,
    pub statement_hash: String,
    pub expected_rows: usize,
    pub operation: MutationOperation,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationParameter {
    pub position: usize,
    pub data_type: DataType,
    pub value: MutationValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationLineage {
    pub model: String,
    pub fields: Vec<String>,
    pub returning_fields: Vec<String>,
    pub source_columns: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MutationCompileError {
    #[error("unsupported semantic snapshot schema version: {0}")]
    UnsupportedSnapshotVersion(String),
    #[error("semantic revision hash is invalid")]
    InvalidRevisionHash,
    #[error("mutation compiler limit configuration is invalid")]
    InvalidLimitConfiguration,
    #[error("semantic model is not writable")]
    ModelNotWritable,
    #[error("requested mutation operation is not enabled")]
    OperationNotEnabled,
    #[error("mutation exceeds the published model row limit")]
    RowLimitExceeded,
    #[error("mutation exceeds the published model byte limit")]
    RequestByteLimitExceeded,
    #[error("writable model metadata is invalid")]
    InvalidWritableModel,
    #[error("semantic mutation field is not writable: {0}")]
    FieldNotWritable(String),
    #[error("required semantic mutation field is missing: {0}")]
    RequiredFieldMissing(String),
    #[error("semantic mutation field type is invalid: {0}")]
    FieldTypeMismatch(String),
    #[error("semantic mutation field does not accept null: {0}")]
    NullNotAllowed(String),
    #[error("upsert conflict field is missing: {0}")]
    ConflictFieldMissing(String),
    #[error("upsert does not contain an approved mutable field")]
    NoUpdatableField,
    #[error("failed to serialize mutation compiler hash inputs")]
    HashSerialization,
}

impl MutationCompileError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSnapshotVersion(_) => "SEMANTIC_UNSUPPORTED_SNAPSHOT_VERSION",
            Self::InvalidRevisionHash => "SEMANTIC_INVALID_REVISION_HASH",
            Self::InvalidLimitConfiguration => "MUTATION_INVALID_LIMIT_CONFIGURATION",
            Self::ModelNotWritable => "MUTATION_MODEL_NOT_WRITABLE",
            Self::OperationNotEnabled => "MUTATION_OPERATION_NOT_ENABLED",
            Self::RowLimitExceeded => "MUTATION_ROW_LIMIT_EXCEEDED",
            Self::RequestByteLimitExceeded => "MUTATION_REQUEST_BYTES_EXCEEDED",
            Self::InvalidWritableModel => "MUTATION_INVALID_WRITABLE_MODEL",
            Self::FieldNotWritable(_) => "MUTATION_FIELD_NOT_WRITABLE",
            Self::RequiredFieldMissing(_) => "MUTATION_REQUIRED_FIELD_MISSING",
            Self::FieldTypeMismatch(_) => "MUTATION_FIELD_TYPE_MISMATCH",
            Self::NullNotAllowed(_) => "MUTATION_NULL_NOT_ALLOWED",
            Self::ConflictFieldMissing(_) => "MUTATION_CONFLICT_FIELD_MISSING",
            Self::NoUpdatableField => "MUTATION_NO_UPDATABLE_FIELD",
            Self::HashSerialization => "MUTATION_HASH_SERIALIZATION_FAILED",
        }
    }
}

pub fn compile_lsm(
    normalized: &NormalizedLsm,
    snapshot: &SemanticSnapshot,
    capabilities: &MutationCapabilities,
    options: MutationCompilerOptions,
) -> Result<CompiledMutation, MutationCompileError> {
    validate_snapshot(snapshot)?;
    if options.hard_row_limit == 0 || options.hard_request_byte_limit == 0 {
        return Err(MutationCompileError::InvalidLimitConfiguration);
    }
    if capabilities.profile.trim().is_empty() || !capabilities.allows(&normalized.mutation.model) {
        return Err(MutationCompileError::ModelNotWritable);
    }
    let model = snapshot
        .models
        .iter()
        .find(|model| model.semantic_name == normalized.mutation.model)
        .ok_or(MutationCompileError::ModelNotWritable)?;
    let writable = model
        .writable
        .as_ref()
        .ok_or(MutationCompileError::ModelNotWritable)?;
    validate_writable_model(model.fields.as_slice(), writable)?;
    if writable.max_rows == 0
        || writable.max_rows > options.hard_row_limit
        || writable.max_request_bytes == 0
        || writable.max_request_bytes > options.hard_request_byte_limit
    {
        return Err(MutationCompileError::InvalidWritableModel);
    }
    if normalized.mutation.rows.len() > writable.max_rows as usize {
        return Err(MutationCompileError::RowLimitExceeded);
    }
    if normalized.input_bytes > writable.max_request_bytes as usize {
        return Err(MutationCompileError::RequestByteLimitExceeded);
    }

    match normalized.mutation.operation {
        MutationOperation::Insert if !writable.insert => {
            return Err(MutationCompileError::OperationNotEnabled);
        }
        MutationOperation::Upsert if writable.upsert.is_none() => {
            return Err(MutationCompileError::OperationNotEnabled);
        }
        _ => {}
    }

    let field_by_name = model
        .fields
        .iter()
        .map(|field| (field.semantic_name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let writable_by_name = writable
        .fields
        .iter()
        .map(|field| (field.field.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let input_names = normalized.mutation.rows[0]
        .keys()
        .cloned()
        .collect::<Vec<_>>();

    for name in &input_names {
        let policy = writable_by_name
            .get(name.as_str())
            .ok_or_else(|| MutationCompileError::FieldNotWritable(name.clone()))?;
        let field = field_by_name
            .get(name.as_str())
            .ok_or(MutationCompileError::InvalidWritableModel)?;
        if field.relationship.is_some() {
            return Err(MutationCompileError::InvalidWritableModel);
        }
        for row in &normalized.mutation.rows {
            validate_value(field, policy, &row[name])?;
        }
        if normalized.mutation.operation == MutationOperation::Upsert
            && !policy.updatable_on_conflict
            && !writable
                .upsert
                .as_ref()
                .is_some_and(|upsert| upsert.conflict_fields.contains(name))
        {
            return Err(MutationCompileError::FieldNotWritable(name.clone()));
        }
    }

    let update_names = if let Some(upsert) = &writable.upsert {
        if normalized.mutation.operation == MutationOperation::Upsert {
            for field in &upsert.conflict_fields {
                if !input_names.contains(field) {
                    return Err(MutationCompileError::ConflictFieldMissing(field.clone()));
                }
            }
            let names = input_names
                .iter()
                .filter(|name| {
                    writable_by_name
                        .get(name.as_str())
                        .is_some_and(|field| field.updatable_on_conflict)
                })
                .cloned()
                .collect::<Vec<_>>();
            if names.is_empty() {
                return Err(MutationCompileError::NoUpdatableField);
            }
            names
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    for field in writable
        .fields
        .iter()
        .filter(|field| field.required_on_insert)
    {
        if !input_names.contains(&field.field) {
            return Err(MutationCompileError::RequiredFieldMissing(
                field.field.clone(),
            ));
        }
    }

    render(
        normalized,
        snapshot,
        model.source.schema.as_str(),
        model.source.relation.as_str(),
        writable,
        &field_by_name,
        input_names,
        update_names,
        &capabilities.profile,
    )
}

fn validate_snapshot(snapshot: &SemanticSnapshot) -> Result<(), MutationCompileError> {
    if snapshot.schema_version != "1" {
        return Err(MutationCompileError::UnsupportedSnapshotVersion(
            snapshot.schema_version.clone(),
        ));
    }
    if !is_sha256(&snapshot.revision_hash)
        || snapshot
            .calculate_revision_hash()
            .map_err(|_| MutationCompileError::HashSerialization)?
            != snapshot.revision_hash
    {
        return Err(MutationCompileError::InvalidRevisionHash);
    }
    Ok(())
}

fn validate_writable_model(
    model_fields: &[Field],
    writable: &WritableModel,
) -> Result<(), MutationCompileError> {
    if (!writable.insert && writable.upsert.is_none())
        || writable.fields.is_empty()
        || writable.returning.is_empty()
    {
        return Err(MutationCompileError::InvalidWritableModel);
    }
    let model_fields = model_fields
        .iter()
        .map(|field| (field.semantic_name.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let mut names = BTreeSet::new();
    for field in &writable.fields {
        if !names.insert(&field.field)
            || field.field.trim().is_empty()
            || !model_fields
                .get(field.field.as_str())
                .is_some_and(|model_field| model_field.relationship.is_none())
        {
            return Err(MutationCompileError::InvalidWritableModel);
        }
    }
    let mut returning = BTreeSet::new();
    for field in &writable.returning {
        if !returning.insert(field)
            || !model_fields
                .get(field.as_str())
                .is_some_and(|model_field| model_field.relationship.is_none())
        {
            return Err(MutationCompileError::InvalidWritableModel);
        }
    }
    if let Some(upsert) = &writable.upsert {
        if upsert.conflict_fields.is_empty() {
            return Err(MutationCompileError::InvalidWritableModel);
        }
        let mut conflict = BTreeSet::new();
        for name in &upsert.conflict_fields {
            let Some(policy) = writable.fields.iter().find(|field| field.field == *name) else {
                return Err(MutationCompileError::InvalidWritableModel);
            };
            if !conflict.insert(name) || policy.updatable_on_conflict {
                return Err(MutationCompileError::InvalidWritableModel);
            }
        }
    }
    Ok(())
}

fn validate_value(
    field: &Field,
    writable_field: &WritableField,
    value: &MutationValue,
) -> Result<(), MutationCompileError> {
    if *value == MutationValue::Null {
        return if writable_field.nullable {
            Ok(())
        } else {
            Err(MutationCompileError::NullNotAllowed(
                field.semantic_name.clone(),
            ))
        };
    }
    let compatible = matches!(
        (field.data_type, value),
        (DataType::Text, MutationValue::Text(_))
            | (DataType::Boolean, MutationValue::Boolean(_))
            | (DataType::Integer, MutationValue::Integer(_))
            | (DataType::Numeric, MutationValue::Numeric(_))
            | (DataType::Date, MutationValue::Date(_))
            | (
                DataType::Timestamp | DataType::TimestampTz,
                MutationValue::Timestamp(_)
            )
    );
    if compatible {
        Ok(())
    } else {
        Err(MutationCompileError::FieldTypeMismatch(
            field.semantic_name.clone(),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn render(
    normalized: &NormalizedLsm,
    snapshot: &SemanticSnapshot,
    schema: &str,
    relation: &str,
    writable: &WritableModel,
    fields: &BTreeMap<&str, &Field>,
    input_names: Vec<String>,
    update_names: Vec<String>,
    capability_profile: &str,
) -> Result<CompiledMutation, MutationCompileError> {
    let columns = input_names
        .iter()
        .map(|name| quote_identifier(&fields[name.as_str()].column))
        .collect::<Vec<_>>();
    let mut parameters = Vec::new();
    let mut value_rows = Vec::new();
    for row in &normalized.mutation.rows {
        let mut values = Vec::new();
        for name in &input_names {
            let field = fields[name.as_str()];
            let value = row[name].clone();
            let position = parameters.len() + 1;
            parameters.push(MutationParameter {
                position,
                data_type: field.data_type,
                value,
            });
            values.push(parameter_sql(position, field.data_type));
        }
        value_rows.push(format!("  ({})", values.join(", ")));
    }

    let mut statement = format!(
        "INSERT INTO {}.{} ({})\nVALUES\n{}",
        quote_identifier(schema),
        quote_identifier(relation),
        columns.join(", "),
        value_rows.join(",\n")
    );
    if normalized.mutation.operation == MutationOperation::Upsert {
        let conflict = writable
            .upsert
            .as_ref()
            .ok_or(MutationCompileError::InvalidWritableModel)?
            .conflict_fields
            .iter()
            .map(|name| quote_identifier(&fields[name.as_str()].column))
            .collect::<Vec<_>>();
        let updates = update_names
            .iter()
            .map(|name| {
                let column = quote_identifier(&fields[name.as_str()].column);
                format!("  {column} = EXCLUDED.{column}")
            })
            .collect::<Vec<_>>();
        statement.push_str(&format!(
            "\nON CONFLICT ({}) DO UPDATE SET\n{}",
            conflict.join(", "),
            updates.join(",\n")
        ));
    }

    let returning_schema = writable
        .returning
        .iter()
        .map(|name| OutputColumn {
            name: name.clone(),
            data_type: fields[name.as_str()].data_type,
        })
        .collect::<Vec<_>>();
    let returning = writable
        .returning
        .iter()
        .map(|name| {
            format!(
                "  {} AS {}",
                quote_identifier(&fields[name.as_str()].column),
                quote_identifier(name)
            )
        })
        .collect::<Vec<_>>();
    statement.push_str(&format!("\nRETURNING\n{}", returning.join(",\n")));

    let statement_hash = sha256(&statement);
    let parameter_hash_input =
        serde_json::to_string(&parameters).map_err(|_| MutationCompileError::HashSerialization)?;
    let mutation_hash = sha256(format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        normalized.hash,
        snapshot.revision_hash,
        MUTATION_COMPILER_SEMANTIC_VERSION,
        capability_profile,
        statement_hash,
        parameter_hash_input
    ));
    let source_columns = input_names
        .iter()
        .chain(writable.returning.iter())
        .map(|name| format!("{schema}.{relation}.{}", fields[name.as_str()].column))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(CompiledMutation {
        statement,
        parameters,
        returning_schema,
        lineage: MutationLineage {
            model: normalized.mutation.model.clone(),
            fields: input_names,
            returning_fields: writable.returning.clone(),
            source_columns,
        },
        mutation_hash,
        statement_hash,
        expected_rows: normalized.mutation.rows.len(),
        operation: normalized.mutation.operation,
        model: normalized.mutation.model.clone(),
    })
}

fn parameter_sql(position: usize, data_type: DataType) -> String {
    let target_type = match data_type {
        DataType::Boolean => "boolean",
        DataType::Integer => "bigint",
        DataType::Numeric => "numeric",
        DataType::Text => return format!("${position}::text"),
        DataType::Date => "date",
        DataType::Timestamp => "timestamp",
        DataType::TimestampTz => "timestamptz",
    };
    format!("${position}::text::{target_type}")
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use crate::{
        DataType, Field, Model, MutationCapabilities, Relation, SemanticSnapshot, UpsertPolicy,
        WritableField, WritableModel, normalize_lsm,
    };

    use super::{MutationCompileError, MutationCompilerOptions, compile_lsm};

    fn snapshot() -> SemanticSnapshot {
        let mut snapshot = SemanticSnapshot {
            schema_version: "1".to_owned(),
            revision_hash: String::new(),
            models: vec![Model {
                semantic_name: "orders".to_owned(),
                source: Relation {
                    schema: "commerce".to_owned(),
                    relation: "orders".to_owned(),
                },
                timezone: Some("UTC".to_owned()),
                queryable: true,
                writable: Some(WritableModel {
                    insert: true,
                    upsert: Some(UpsertPolicy {
                        conflict_fields: vec!["external_id".to_owned()],
                    }),
                    max_rows: 10,
                    max_request_bytes: 4096,
                    fields: vec![
                        WritableField {
                            field: "external_id".to_owned(),
                            nullable: false,
                            required_on_insert: true,
                            updatable_on_conflict: false,
                        },
                        WritableField {
                            field: "amount".to_owned(),
                            nullable: false,
                            required_on_insert: true,
                            updatable_on_conflict: true,
                        },
                        WritableField {
                            field: "note".to_owned(),
                            nullable: true,
                            required_on_insert: false,
                            updatable_on_conflict: true,
                        },
                    ],
                    returning: vec!["order_id".to_owned(), "external_id".to_owned()],
                }),
                fields: vec![
                    field("order_id", DataType::Integer, false),
                    field("external_id", DataType::Text, false),
                    field("amount", DataType::Numeric, false),
                    field("note", DataType::Text, true),
                ],
                metrics: vec![],
                relationships: vec![],
            }],
        };
        snapshot.revision_hash = snapshot
            .calculate_revision_hash()
            .expect("snapshot is serializable");
        snapshot
    }

    fn field(name: &str, data_type: DataType, nullable: bool) -> Field {
        Field {
            semantic_name: name.to_owned(),
            data_type,
            column: name.to_owned(),
            relationship: None,
            time_dimension: false,
            entity_key: name == "order_id",
            visible: true,
            nullable,
        }
    }

    fn mutation(operation: &str, rows: &str) -> Vec<u8> {
        format!(
            r#"{{
              "schema_version":"1",
              "operation":"{operation}",
              "model":"orders",
              "idempotency_key":"request-1",
              "rows":{rows}
            }}"#
        )
        .into_bytes()
    }

    fn capabilities() -> MutationCapabilities {
        MutationCapabilities {
            profile: "test-writer".to_owned(),
            writable_models: ["orders".to_owned()].into_iter().collect(),
        }
    }

    #[test]
    fn compiles_deterministic_bounded_insert() {
        let normalized = normalize_lsm(&mutation(
            "insert",
            r#"[{"amount":{"type":"numeric","value":"12.50"},"external_id":{"type":"text","value":"a"}}]"#,
        ))
        .expect("valid LSM");
        let compiled = compile_lsm(
            &normalized,
            &snapshot(),
            &capabilities(),
            MutationCompilerOptions::default(),
        )
        .expect("compiles");

        assert!(
            compiled
                .statement
                .starts_with("INSERT INTO \"commerce\".\"orders\" (\"amount\", \"external_id\")")
        );
        assert!(!compiled.statement.contains("ON CONFLICT"));
        assert!(compiled.statement.contains("$1::text::numeric"));
        assert_eq!(compiled.expected_rows, 1);
        assert_eq!(compiled.returning_schema[0].name, "order_id");
    }

    #[test]
    fn exported_snapshot_round_trip_preserves_writable_nullability() {
        let exported = serde_json::to_vec(&snapshot()).expect("snapshot serializes");
        let imported: SemanticSnapshot =
            serde_json::from_slice(&exported).expect("snapshot deserializes");
        let normalized = normalize_lsm(&mutation(
            "insert",
            r#"[{"amount":{"type":"null"},"external_id":{"type":"text","value":"a"}}]"#,
        ))
        .expect("valid LSM");

        let error = compile_lsm(
            &normalized,
            &imported,
            &capabilities(),
            MutationCompilerOptions::default(),
        )
        .expect_err("writable projection must preserve non-nullability");
        assert_eq!(error.code(), "MUTATION_NULL_NOT_ALLOWED");
    }

    #[test]
    fn compiles_only_the_published_upsert_target_and_fields() {
        let normalized = normalize_lsm(&mutation(
            "upsert",
            r#"[{"external_id":{"type":"text","value":"a"},"amount":{"type":"numeric","value":"20.00"}}]"#,
        ))
        .expect("valid LSM");
        let compiled = compile_lsm(
            &normalized,
            &snapshot(),
            &capabilities(),
            MutationCompilerOptions::default(),
        )
        .expect("compiles");

        assert!(compiled.statement.contains(
            "ON CONFLICT (\"external_id\") DO UPDATE SET\n  \"amount\" = EXCLUDED.\"amount\""
        ));
        assert!(!compiled.statement.contains("order_id = EXCLUDED"));
    }

    #[test]
    fn rejects_unknown_generated_missing_and_null_fields() {
        for (rows, expected) in [
            (
                r#"[{"external_id":{"type":"text","value":"a"},"amount":{"type":"numeric","value":"1"},"order_id":{"type":"integer","value":4}}]"#,
                "MUTATION_FIELD_NOT_WRITABLE",
            ),
            (
                r#"[{"external_id":{"type":"text","value":"a"}}]"#,
                "MUTATION_REQUIRED_FIELD_MISSING",
            ),
            (
                r#"[{"external_id":{"type":"text","value":"a"},"amount":{"type":"null"}}]"#,
                "MUTATION_NULL_NOT_ALLOWED",
            ),
        ] {
            let normalized = normalize_lsm(&mutation("insert", rows)).expect("valid LSM");
            let error = compile_lsm(
                &normalized,
                &snapshot(),
                &capabilities(),
                MutationCompilerOptions::default(),
            )
            .expect_err("must reject unsafe mutation");
            assert_eq!(error.code(), expected);
        }
    }

    #[test]
    fn rejects_partial_conflict_key_and_immutable_upsert_field() {
        let partial = normalize_lsm(&mutation(
            "upsert",
            r#"[{"amount":{"type":"numeric","value":"1"}}]"#,
        ))
        .expect("valid LSM");
        assert!(matches!(
            compile_lsm(
                &partial,
                &snapshot(),
                &capabilities(),
                MutationCompilerOptions::default()
            ),
            Err(MutationCompileError::ConflictFieldMissing(_))
        ));

        let only_key = normalize_lsm(&mutation(
            "upsert",
            r#"[{"external_id":{"type":"text","value":"a"}}]"#,
        ))
        .expect("valid LSM");
        assert!(matches!(
            compile_lsm(
                &only_key,
                &snapshot(),
                &capabilities(),
                MutationCompilerOptions::default()
            ),
            Err(MutationCompileError::RequiredFieldMissing(_))
                | Err(MutationCompileError::NoUpdatableField)
        ));
    }
}
