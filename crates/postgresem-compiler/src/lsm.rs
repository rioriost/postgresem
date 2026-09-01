use std::{collections::BTreeMap, fmt};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};
use thiserror::Error;

use crate::{Literal, hash::sha256};

const LSM_SCHEMA_VERSION: &str = "1";
const MAX_INPUT_BYTES: usize = 1_048_576;
const MAX_ROWS: usize = 100;
const MAX_FIELDS_PER_ROW: usize = 64;
const MAX_SEMANTIC_NAME_BYTES: usize = 255;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalSemanticMutation {
    pub schema_version: String,
    pub operation: MutationOperation,
    pub model: String,
    pub idempotency_key: String,
    pub rows: Vec<BTreeMap<String, MutationValue>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationOperation {
    Insert,
    Upsert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MutationValue {
    Null,
    Text(String),
    Boolean(bool),
    Integer(i64),
    Numeric(String),
    Date(String),
    Timestamp(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedLsm {
    pub mutation: LogicalSemanticMutation,
    pub canonical_json: String,
    pub hash: String,
    pub idempotency_key_hash: String,
    pub input_bytes: usize,
}

#[derive(Debug, Error)]
pub enum LsmError {
    #[error("LSM input exceeds the maximum size of {MAX_INPUT_BYTES} bytes")]
    InputTooLarge,
    #[error("invalid LSM JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unsupported LSM schema version: {0}")]
    UnsupportedSchemaVersion(String),
    #[error("model must not be empty or exceed {MAX_SEMANTIC_NAME_BYTES} bytes")]
    InvalidModel,
    #[error("idempotency key must not be empty or exceed {MAX_IDEMPOTENCY_KEY_BYTES} bytes")]
    InvalidIdempotencyKey,
    #[error("mutation must contain between 1 and {MAX_ROWS} rows")]
    InvalidRowCount,
    #[error("mutation row must contain between 1 and {MAX_FIELDS_PER_ROW} fields")]
    InvalidFieldCount,
    #[error("semantic field name must not be empty or exceed {MAX_SEMANTIC_NAME_BYTES} bytes")]
    InvalidFieldName,
    #[error("all mutation rows must contain the same semantic fields")]
    InconsistentRowFields,
    #[error("mutation value is not well formed")]
    InvalidValue,
}

impl LsmError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InputTooLarge => "LSM_INPUT_TOO_LARGE",
            Self::InvalidJson(_) => "LSM_INVALID_JSON",
            Self::UnsupportedSchemaVersion(_) => "LSM_UNSUPPORTED_SCHEMA_VERSION",
            Self::InvalidModel => "LSM_INVALID_MODEL",
            Self::InvalidIdempotencyKey => "LSM_INVALID_IDEMPOTENCY_KEY",
            Self::InvalidRowCount => "LSM_INVALID_ROW_COUNT",
            Self::InvalidFieldCount => "LSM_INVALID_FIELD_COUNT",
            Self::InvalidFieldName => "LSM_INVALID_FIELD_NAME",
            Self::InconsistentRowFields => "LSM_INCONSISTENT_ROW_FIELDS",
            Self::InvalidValue => "LSM_INVALID_VALUE",
        }
    }
}

pub fn normalize_lsm(input: &[u8]) -> Result<NormalizedLsm, LsmError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(LsmError::InputTooLarge);
    }
    let unique = serde_json::from_slice::<UniqueValue>(input)?;
    let mutation: LogicalSemanticMutation = serde_json::from_value(unique.0)?;
    validate_mutation(&mutation)?;
    let canonical_json = serde_json::to_string(&mutation)?;
    Ok(NormalizedLsm {
        idempotency_key_hash: sha256(&mutation.idempotency_key),
        hash: sha256(&canonical_json),
        mutation,
        canonical_json,
        input_bytes: input.len(),
    })
}

fn validate_mutation(mutation: &LogicalSemanticMutation) -> Result<(), LsmError> {
    if mutation.schema_version != LSM_SCHEMA_VERSION {
        return Err(LsmError::UnsupportedSchemaVersion(
            mutation.schema_version.clone(),
        ));
    }
    if !valid_bounded_text(&mutation.model, MAX_SEMANTIC_NAME_BYTES) {
        return Err(LsmError::InvalidModel);
    }
    if !valid_bounded_text(&mutation.idempotency_key, MAX_IDEMPOTENCY_KEY_BYTES) {
        return Err(LsmError::InvalidIdempotencyKey);
    }
    if mutation.rows.is_empty() || mutation.rows.len() > MAX_ROWS {
        return Err(LsmError::InvalidRowCount);
    }
    let expected_fields = mutation.rows[0].keys().collect::<Vec<_>>();
    for row in &mutation.rows {
        if row.is_empty() || row.len() > MAX_FIELDS_PER_ROW {
            return Err(LsmError::InvalidFieldCount);
        }
        if row.keys().collect::<Vec<_>>() != expected_fields {
            return Err(LsmError::InconsistentRowFields);
        }
        for (field, value) in row {
            if !valid_bounded_text(field, MAX_SEMANTIC_NAME_BYTES) {
                return Err(LsmError::InvalidFieldName);
            }
            if !value.is_well_formed() {
                return Err(LsmError::InvalidValue);
            }
        }
    }
    Ok(())
}

fn valid_bounded_text(value: &str, maximum_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum_bytes
}

impl MutationValue {
    pub(crate) fn is_well_formed(&self) -> bool {
        self.as_literal()
            .is_none_or(|literal| literal.is_well_formed())
    }

    pub(crate) fn as_literal(&self) -> Option<Literal> {
        match self {
            Self::Null => None,
            Self::Text(value) => Some(Literal::Text(value.clone())),
            Self::Boolean(value) => Some(Literal::Boolean(*value)),
            Self::Integer(value) => Some(Literal::Integer(*value)),
            Self::Numeric(value) => Some(Literal::Numeric(value.clone())),
            Self::Date(value) => Some(Literal::Date(value.clone())),
            Self::Timestamp(value) => Some(Literal::Timestamp(value.clone())),
        }
    }
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

struct UniqueValueVisitor;

impl<'de> Visitor<'de> for UniqueValueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueValue)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_seq<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut result = Vec::new();
        while let Some(value) = values.next_element::<UniqueValue>()? {
            result.push(value.0);
        }
        Ok(UniqueValue(Value::Array(result)))
    }

    fn visit_map<A>(self, mut values: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut result = Map::new();
        while let Some(key) = values.next_key::<String>()? {
            if result.contains_key(&key) {
                return Err(A::Error::custom(format!("duplicate object key: {key}")));
            }
            let value = values.next_value::<UniqueValue>()?;
            result.insert(key, value.0);
        }
        Ok(UniqueValue(Value::Object(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::{LsmError, normalize_lsm};

    const VALID: &str = r#"{
      "schema_version":"1",
      "operation":"insert",
      "model":"orders",
      "idempotency_key":"order-1",
      "rows":[{
        "customer_id":{"type":"integer","value":1},
        "amount":{"type":"numeric","value":"12.50"}
      }]
    }"#;

    #[test]
    fn normalizes_row_field_order_and_hashes_the_key() {
        let reordered = VALID.replace(
            r#""customer_id":{"type":"integer","value":1},
        "amount":{"type":"numeric","value":"12.50"}"#,
            r#""amount":{"value":"12.50","type":"numeric"},
        "customer_id":{"value":1,"type":"integer"}"#,
        );
        let first = normalize_lsm(VALID.as_bytes()).expect("valid LSM");
        let second = normalize_lsm(reordered.as_bytes()).expect("valid reordered LSM");
        assert_eq!(first.canonical_json, second.canonical_json);
        assert_eq!(first.hash, second.hash);
        assert!(first.idempotency_key_hash.starts_with("sha256:"));
    }

    #[test]
    fn rejects_duplicate_keys_at_any_depth() {
        for input in [
            VALID.replace(
                r#""model":"orders""#,
                r#""model":"orders","model":"customers""#,
            ),
            VALID.replace(
                r#""amount":{"type":"numeric","value":"12.50"}"#,
                r#""amount":{"type":"numeric","type":"text","value":"12.50"}"#,
            ),
        ] {
            assert!(matches!(
                normalize_lsm(input.as_bytes()),
                Err(LsmError::InvalidJson(_))
            ));
        }
    }

    #[test]
    fn rejects_unknown_and_unsafe_input() {
        for input in [
            VALID.replace(
                r#""rows":["#,
                r#""sql":"INSERT INTO orders VALUES (1)","rows":["#,
            ),
            VALID.replace(r#""operation":"insert""#, r#""operation":"delete""#),
            VALID.replace(r#""idempotency_key":"order-1""#, r#""idempotency_key":""#),
            VALID.replace(
                r#""amount":{"type":"numeric","value":"12.50"}"#,
                r#""amount":{"type":"numeric","value":"NaN"}"#,
            ),
        ] {
            assert!(normalize_lsm(input.as_bytes()).is_err());
        }
    }

    #[test]
    fn rejects_rows_with_different_field_sets() {
        let input = br#"{
          "schema_version":"1",
          "operation":"insert",
          "model":"orders",
          "idempotency_key":"order-1",
          "rows":[
            {
              "customer_id":{"type":"integer","value":1},
              "amount":{"type":"numeric","value":"12.50"}
            },
            {"customer_id":{"type":"integer","value":2}}
          ]
        }"#;
        assert!(matches!(
            normalize_lsm(input),
            Err(LsmError::InconsistentRowFields)
        ));
    }
}
