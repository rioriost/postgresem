use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::Literal;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticSnapshot {
    pub schema_version: String,
    pub revision_hash: String,
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub semantic_name: String,
    pub source: Relation,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default = "visible_by_default")]
    pub queryable: bool,
    pub fields: Vec<Field>,
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub schema: String,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub semantic_name: String,
    pub data_type: DataType,
    pub column: String,
    #[serde(default)]
    pub relationship: Option<String>,
    #[serde(default)]
    pub time_dimension: bool,
    #[serde(default)]
    pub entity_key: bool,
    #[serde(default = "visible_by_default")]
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub semantic_name: String,
    pub data_type: DataType,
    pub aggregation: Aggregation,
    pub field: String,
    #[serde(default)]
    pub filter: Option<MetricFilter>,
    #[serde(default = "visible_by_default")]
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricFilter {
    pub field: String,
    pub value: Literal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    pub semantic_name: String,
    pub target_model: String,
    pub target: Relation,
    pub cardinality: Cardinality,
    pub join_type: JoinType,
    pub from_column: String,
    pub to_column: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    Boolean,
    Integer,
    Numeric,
    Text,
    Date,
    Timestamp,
    TimestampTz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregation {
    Count,
    CountDistinct,
    Sum,
    Min,
    Max,
    Avg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    OneToOne,
    ManyToOne,
    OneToMany,
    ManyToMany,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinType {
    Inner,
    Left,
}

const fn visible_by_default() -> bool {
    true
}

#[derive(Debug, Error)]
pub enum SnapshotHashError {
    #[error("failed to serialize semantic snapshot: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl SemanticSnapshot {
    pub fn calculate_revision_hash(&self) -> Result<String, SnapshotHashError> {
        let mut canonical = self.clone();
        canonical.revision_hash.clear();
        let bytes = serde_json::to_vec(&canonical)?;
        Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
}
