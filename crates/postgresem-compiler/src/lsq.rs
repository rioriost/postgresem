use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const LSQ_SCHEMA_VERSION: &str = "1";
const MAX_LIMIT: u32 = 10_000;
const MAX_FILTER_DEPTH: usize = 16;
const MAX_FILTER_NODES: usize = 128;
const MAX_IN_VALUES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalSemanticQuery {
    pub schema_version: String,
    pub model: String,
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
    #[serde(default)]
    pub metrics: Vec<MetricReference>,
    #[serde(default)]
    pub filters: Option<Filter>,
    #[serde(default)]
    pub order_by: Vec<OrderBy>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub field: String,
    pub time_grain: Option<TimeGrain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricReference {
    pub metric: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderBy {
    #[serde(rename = "ref")]
    pub output_reference: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeGrain {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Filter {
    And { args: Vec<Filter> },
    Or { args: Vec<Filter> },
    Not { arg: Box<Filter> },
    Eq { field: String, value: Literal },
    NotEq { field: String, value: Literal },
    Gt { field: String, value: Literal },
    Gte { field: String, value: Literal },
    Lt { field: String, value: Literal },
    Lte { field: String, value: Literal },
    In { field: String, values: Vec<Literal> },
    IsNull { field: String },
    IsNotNull { field: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Literal {
    Text(String),
    Boolean(bool),
    Integer(i64),
    Numeric(String),
    Date(String),
    Timestamp(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedLsq {
    pub query: LogicalSemanticQuery,
    pub canonical_json: String,
    pub hash: String,
}

#[derive(Debug, Error)]
pub enum LsqError {
    #[error("invalid LSQ JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unsupported LSQ schema version: {0}")]
    UnsupportedSchemaVersion(String),
    #[error("model must not be empty")]
    EmptyModel,
    #[error("at least one dimension or metric is required")]
    EmptyProjection,
    #[error("semantic reference must not be empty")]
    EmptyReference,
    #[error("duplicate semantic reference: {0}")]
    DuplicateReference(String),
    #[error("duplicate order reference: {0}")]
    DuplicateOrderReference(String),
    #[error("literal value is not valid for type: {0}")]
    InvalidLiteralValue(&'static str),
    #[error("limit must be between 1 and {MAX_LIMIT}")]
    InvalidLimit,
    #[error("filter exceeds maximum depth of {MAX_FILTER_DEPTH}")]
    FilterTooDeep,
    #[error("filter exceeds maximum node count of {MAX_FILTER_NODES}")]
    FilterTooLarge,
    #[error("logical filter must contain at least one argument")]
    EmptyLogicalFilter,
    #[error("IN filter must contain between 1 and {MAX_IN_VALUES} values")]
    InvalidInFilterSize,
}

impl LsqError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidJson(_) => "LSQ_INVALID_JSON",
            Self::UnsupportedSchemaVersion(_) => "LSQ_UNSUPPORTED_SCHEMA_VERSION",
            Self::EmptyModel => "LSQ_EMPTY_MODEL",
            Self::EmptyProjection => "LSQ_EMPTY_PROJECTION",
            Self::EmptyReference => "LSQ_EMPTY_REFERENCE",
            Self::DuplicateReference(_) => "LSQ_DUPLICATE_REFERENCE",
            Self::DuplicateOrderReference(_) => "LSQ_DUPLICATE_ORDER_REFERENCE",
            Self::InvalidLiteralValue(_) => "LSQ_INVALID_LITERAL_VALUE",
            Self::InvalidLimit => "LSQ_INVALID_LIMIT",
            Self::FilterTooDeep => "LSQ_FILTER_TOO_DEEP",
            Self::FilterTooLarge => "LSQ_FILTER_TOO_LARGE",
            Self::EmptyLogicalFilter => "LSQ_EMPTY_LOGICAL_FILTER",
            Self::InvalidInFilterSize => "LSQ_INVALID_IN_FILTER_SIZE",
        }
    }
}

pub fn normalize_lsq(input: &[u8]) -> Result<NormalizedLsq, LsqError> {
    let query: LogicalSemanticQuery = serde_json::from_slice(input)?;
    validate_query(&query)?;

    let canonical_json = serde_json::to_string(&query)?;
    let hash = format!("sha256:{:x}", Sha256::digest(canonical_json.as_bytes()));

    Ok(NormalizedLsq {
        query,
        canonical_json,
        hash,
    })
}

fn validate_query(query: &LogicalSemanticQuery) -> Result<(), LsqError> {
    if query.schema_version != LSQ_SCHEMA_VERSION {
        return Err(LsqError::UnsupportedSchemaVersion(
            query.schema_version.clone(),
        ));
    }
    if query.model.trim().is_empty() {
        return Err(LsqError::EmptyModel);
    }
    if query.dimensions.is_empty() && query.metrics.is_empty() {
        return Err(LsqError::EmptyProjection);
    }
    if query
        .limit
        .is_some_and(|limit| limit == 0 || limit > MAX_LIMIT)
    {
        return Err(LsqError::InvalidLimit);
    }

    let mut references = HashSet::new();
    for reference in query
        .dimensions
        .iter()
        .map(|dimension| &dimension.field)
        .chain(query.metrics.iter().map(|metric| &metric.metric))
    {
        validate_reference(reference)?;
        if !references.insert(reference) {
            return Err(LsqError::DuplicateReference(reference.clone()));
        }
    }
    let mut order_references = HashSet::new();
    for order_by in &query.order_by {
        validate_reference(&order_by.output_reference)?;
        if !order_references.insert(&order_by.output_reference) {
            return Err(LsqError::DuplicateOrderReference(
                order_by.output_reference.clone(),
            ));
        }
    }
    if let Some(filter) = &query.filters {
        let mut node_count = 0;
        validate_filter(filter, 1, &mut node_count)?;
    }

    Ok(())
}

fn validate_reference(reference: &str) -> Result<(), LsqError> {
    if reference.trim().is_empty() {
        return Err(LsqError::EmptyReference);
    }
    Ok(())
}

fn validate_filter(filter: &Filter, depth: usize, node_count: &mut usize) -> Result<(), LsqError> {
    if depth > MAX_FILTER_DEPTH {
        return Err(LsqError::FilterTooDeep);
    }
    *node_count += 1;
    if *node_count > MAX_FILTER_NODES {
        return Err(LsqError::FilterTooLarge);
    }

    match filter {
        Filter::And { args } | Filter::Or { args } => {
            if args.is_empty() {
                return Err(LsqError::EmptyLogicalFilter);
            }
            for arg in args {
                validate_filter(arg, depth + 1, node_count)?;
            }
        }
        Filter::Not { arg } => validate_filter(arg, depth + 1, node_count)?,
        Filter::In { field, values } => {
            validate_reference(field)?;
            if values.is_empty() || values.len() > MAX_IN_VALUES {
                return Err(LsqError::InvalidInFilterSize);
            }
            for value in values {
                validate_literal_value(value)?;
            }
        }
        Filter::Eq { field, value }
        | Filter::NotEq { field, value }
        | Filter::Gt { field, value }
        | Filter::Gte { field, value }
        | Filter::Lt { field, value }
        | Filter::Lte { field, value } => {
            validate_reference(field)?;
            validate_literal_value(value)?;
        }
        Filter::IsNull { field } | Filter::IsNotNull { field } => validate_reference(field)?,
    }

    Ok(())
}

fn validate_literal_value(literal: &Literal) -> Result<(), LsqError> {
    if literal.is_well_formed() {
        Ok(())
    } else {
        Err(LsqError::InvalidLiteralValue(literal.type_name()))
    }
}

impl Literal {
    pub(crate) fn is_well_formed(&self) -> bool {
        match self {
            Self::Text(_) | Self::Boolean(_) | Self::Integer(_) => true,
            Self::Numeric(value) => valid_numeric(value),
            Self::Date(value) => valid_date(value),
            Self::Timestamp(value) => valid_timestamp(value),
        }
    }

    const fn type_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Boolean(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::Numeric(_) => "numeric",
            Self::Date(_) => "date",
            Self::Timestamp(_) => "timestamp",
        }
    }
}

fn valid_numeric(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let start = usize::from(bytes[0] == b'-');
    if start == bytes.len() {
        return false;
    }
    let unsigned = &bytes[start..];
    let integer_end = unsigned
        .iter()
        .position(|byte| *byte == b'.')
        .unwrap_or(unsigned.len());
    let integer = &unsigned[..integer_end];
    if integer.is_empty()
        || !integer.iter().all(u8::is_ascii_digit)
        || (integer.len() > 1 && integer[0] == b'0')
    {
        return false;
    }
    if integer_end == unsigned.len() {
        return true;
    }
    let fraction = &unsigned[integer_end + 1..];
    !fraction.is_empty() && fraction.iter().all(u8::is_ascii_digit)
}

fn valid_date(value: &str) -> bool {
    valid_date_bytes(value.as_bytes())
}

fn valid_date_bytes(bytes: &[u8]) -> bool {
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let Some(year) = parse_digits(&bytes[0..4]) else {
        return false;
    };
    let Some(month) = parse_digits(&bytes[5..7]) else {
        return false;
    };
    let Some(day) = parse_digits(&bytes[8..10]) else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    let days = match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day)
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.get(10) != Some(&b'T') || !valid_date_bytes(&bytes[..10]) {
        return false;
    }
    if bytes.get(13) != Some(&b':') || bytes.get(16) != Some(&b':') {
        return false;
    }
    let (Some(hour), Some(minute), Some(second)) = (
        parse_digits(&bytes[11..13]),
        parse_digits(&bytes[14..16]),
        parse_digits(&bytes[17..19]),
    ) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 59 {
        return false;
    }

    let mut position = 19;
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        let fraction_start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == fraction_start {
            return false;
        }
    }
    if bytes.get(position) == Some(&b'Z') {
        return position + 1 == bytes.len();
    }
    if !matches!(bytes.get(position), Some(b'+') | Some(b'-'))
        || bytes.len() != position + 6
        || bytes.get(position + 3) != Some(&b':')
    {
        return false;
    }
    let (Some(offset_hour), Some(offset_minute)) = (
        parse_digits(&bytes[position + 1..position + 3]),
        parse_digits(&bytes[position + 4..position + 6]),
    ) else {
        return false;
    };
    offset_hour <= 23 && offset_minute <= 59
}

fn parse_digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes.iter().try_fold(0_u32, |value, byte| {
        value.checked_mul(10)?.checked_add(u32::from(*byte - b'0'))
    })
}

const fn is_leap_year(year: u32) -> bool {
    divisible_by(year, 4) && (!divisible_by(year, 100) || divisible_by(year, 400))
}

const fn divisible_by(value: u32, divisor: u32) -> bool {
    value / divisor * divisor == value
}

#[cfg(test)]
mod tests {
    use super::{LsqError, normalize_lsq};

    const VALID_QUERY: &str = r#"{
        "schema_version": "1",
        "model": "orders",
        "dimensions": [{"field": "ordered_at", "time_grain": "month"}],
        "metrics": [{"metric": "revenue"}],
        "filters": {
            "op": "gte",
            "field": "ordered_at",
            "value": {"type": "date", "value": "2026-01-01"}
        },
        "order_by": [{"ref": "revenue", "direction": "desc"}],
        "limit": 100
    }"#;

    #[test]
    fn normalizes_equivalent_object_key_order() {
        let reordered = r#"{
            "limit": 100,
            "metrics": [{"metric": "revenue"}],
            "schema_version": "1",
            "order_by": [{"direction": "desc", "ref": "revenue"}],
            "model": "orders",
            "filters": {
                "value": {"value": "2026-01-01", "type": "date"},
                "field": "ordered_at",
                "op": "gte"
            },
            "dimensions": [{"time_grain": "month", "field": "ordered_at"}]
        }"#;

        let first = normalize_lsq(VALID_QUERY.as_bytes()).expect("valid LSQ");
        let second = normalize_lsq(reordered.as_bytes()).expect("valid reordered LSQ");

        assert_eq!(first.canonical_json, second.canonical_json);
        assert_eq!(first.hash, second.hash);
    }

    #[test]
    fn rejects_unknown_top_level_property() {
        let input = VALID_QUERY.replace("\"limit\": 100", "\"limit\": 100, \"sql\": \"select 1\"");

        assert!(matches!(
            normalize_lsq(input.as_bytes()),
            Err(LsqError::InvalidJson(_))
        ));
    }

    #[test]
    fn rejects_request_selected_database_role() {
        let input = VALID_QUERY.replace(
            "\"limit\": 100",
            "\"limit\": 100, \"database_role\": \"postgresem_tenant_a\"",
        );

        assert!(matches!(
            normalize_lsq(input.as_bytes()),
            Err(LsqError::InvalidJson(_))
        ));
    }

    #[test]
    fn rejects_duplicate_projection_reference() {
        let input = VALID_QUERY.replace(
            r#""metrics": [{"metric": "revenue"}]"#,
            r#""metrics": [{"metric": "revenue"}, {"metric": "revenue"}]"#,
        );

        assert!(matches!(
            normalize_lsq(input.as_bytes()),
            Err(LsqError::DuplicateReference(reference)) if reference == "revenue"
        ));
    }

    #[test]
    fn rejects_empty_in_filter() {
        let input = VALID_QUERY.replace(
            r#""op": "gte",
            "field": "ordered_at",
            "value": {"type": "date", "value": "2026-01-01"}"#,
            r#""op": "in", "field": "status", "values": []"#,
        );

        assert!(matches!(
            normalize_lsq(input.as_bytes()),
            Err(LsqError::InvalidInFilterSize)
        ));
    }

    #[test]
    fn rejects_malformed_typed_literals() {
        for invalid_literal in [
            r#"{"type": "date", "value": "2026-02-30"}"#,
            r#"{"type": "numeric", "value": "NaN"}"#,
            r#"{"type": "timestamp", "value": "2026-01-01 00:00:00"}"#,
        ] {
            let input = VALID_QUERY.replace(
                r#"{"type": "date", "value": "2026-01-01"}"#,
                invalid_literal,
            );
            assert!(matches!(
                normalize_lsq(input.as_bytes()),
                Err(LsqError::InvalidLiteralValue(_))
            ));
        }
    }

    #[test]
    fn rejects_duplicate_order_reference() {
        let input = VALID_QUERY.replace(
            r#""order_by": [{"ref": "revenue", "direction": "desc"}]"#,
            r#""order_by": [
                {"ref": "revenue", "direction": "desc"},
                {"ref": "revenue", "direction": "asc"}
            ]"#,
        );

        assert!(matches!(
            normalize_lsq(input.as_bytes()),
            Err(LsqError::DuplicateOrderReference(reference)) if reference == "revenue"
        ));
    }
}
