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
    for order_by in &query.order_by {
        validate_reference(&order_by.output_reference)?;
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
        }
        Filter::Eq { field, .. }
        | Filter::NotEq { field, .. }
        | Filter::Gt { field, .. }
        | Filter::Gte { field, .. }
        | Filter::Lt { field, .. }
        | Filter::Lte { field, .. }
        | Filter::IsNull { field }
        | Filter::IsNotNull { field } => validate_reference(field)?,
    }

    Ok(())
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
}
