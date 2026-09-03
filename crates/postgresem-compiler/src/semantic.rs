use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SUPPORTED_SNAPSHOT_SCHEMA_VERSIONS: [&str; 2] = ["1", "2"];

use crate::{Literal, hash::sha256};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<WritableModel>,
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
    #[serde(default = "nullable_by_default", skip_serializing)]
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritableModel {
    pub insert: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upsert: Option<UpsertPolicy>,
    pub max_rows: u32,
    pub max_request_bytes: u32,
    pub fields: Vec<WritableField>,
    pub returning: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritableField {
    pub field: String,
    pub nullable: bool,
    #[serde(default)]
    pub required_on_insert: bool,
    #[serde(default)]
    pub updatable_on_conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertPolicy {
    pub conflict_fields: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additivity: Option<Additivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregation_anchor: Option<String>,
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
pub enum Additivity {
    Additive,
    SemiAdditive,
    NonAdditive,
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

const fn nullable_by_default() -> bool {
    true
}

#[derive(Debug, Error)]
pub enum SnapshotHashError {
    #[error("failed to serialize semantic snapshot: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl SemanticSnapshot {
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut canonical = self.clone();
        canonical.models.sort_by(|left, right| {
            left.semantic_name
                .cmp(&right.semantic_name)
                .then_with(|| left.source.schema.cmp(&right.source.schema))
                .then_with(|| left.source.relation.cmp(&right.source.relation))
        });
        for model in &mut canonical.models {
            model
                .fields
                .sort_by(|left, right| left.semantic_name.cmp(&right.semantic_name));
            model
                .metrics
                .sort_by(|left, right| left.semantic_name.cmp(&right.semantic_name));
            if let Some(writable) = &mut model.writable {
                writable
                    .fields
                    .sort_by(|left, right| left.field.cmp(&right.field));
                if let Some(upsert) = &mut writable.upsert {
                    upsert.conflict_fields.sort();
                }
            }
            model.relationships.sort_by(|left, right| {
                left.semantic_name
                    .cmp(&right.semantic_name)
                    .then_with(|| left.target_model.cmp(&right.target_model))
            });
        }
        canonical
    }

    pub fn calculate_revision_hash(&self) -> Result<String, SnapshotHashError> {
        let mut canonical = self.normalized();
        canonical.revision_hash.clear();
        let bytes = serde_json::to_vec(&canonical)?;
        Ok(sha256(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Aggregation, Cardinality, DataType, Field, JoinType, Metric, Model, Relation, Relationship,
        SemanticSnapshot,
    };

    #[test]
    fn canonical_hash_sorts_all_semantic_collections() {
        let model = Model {
            semantic_name: "z_model".to_owned(),
            source: Relation {
                schema: "public".to_owned(),
                relation: "z_source".to_owned(),
            },
            timezone: None,
            queryable: true,
            writable: None,
            fields: vec![field("z_field"), field("a_field")],
            metrics: vec![metric("z_metric"), metric("a_metric")],
            relationships: vec![
                relationship("z_relationship"),
                relationship("a_relationship"),
            ],
        };
        let mut reversed = SemanticSnapshot {
            schema_version: "1".to_owned(),
            revision_hash: "ignored".to_owned(),
            models: vec![
                model,
                Model {
                    semantic_name: "a_model".to_owned(),
                    source: Relation {
                        schema: "public".to_owned(),
                        relation: "a_source".to_owned(),
                    },
                    timezone: None,
                    queryable: false,
                    writable: None,
                    fields: vec![],
                    metrics: vec![],
                    relationships: vec![],
                },
            ],
        };
        let expected_hash = reversed
            .calculate_revision_hash()
            .expect("snapshot is serializable");

        reversed.models.reverse();
        reversed.models[1].fields.reverse();
        reversed.models[1].metrics.reverse();
        reversed.models[1].relationships.reverse();

        assert_eq!(
            reversed
                .calculate_revision_hash()
                .expect("snapshot is serializable"),
            expected_hash
        );
        assert_eq!(reversed.normalized().models[0].semantic_name, "a_model");
        assert_eq!(
            reversed.normalized().models[1].fields[0].semantic_name,
            "a_field"
        );
    }

    #[test]
    fn snapshot_v1_hash_is_unchanged_by_optional_v2_fields() {
        let snapshot: SemanticSnapshot = serde_json::from_str(include_str!(
            "../../../fixtures/evals/m0-semantic-snapshot.json"
        ))
        .expect("valid v1 snapshot");
        assert_eq!(
            snapshot
                .calculate_revision_hash()
                .expect("snapshot is serializable"),
            snapshot.revision_hash
        );
    }

    fn field(semantic_name: &str) -> Field {
        Field {
            semantic_name: semantic_name.to_owned(),
            data_type: DataType::Text,
            column: semantic_name.to_owned(),
            relationship: None,
            time_dimension: false,
            entity_key: false,
            visible: true,
            nullable: true,
        }
    }

    fn metric(semantic_name: &str) -> Metric {
        Metric {
            semantic_name: semantic_name.to_owned(),
            data_type: DataType::Integer,
            aggregation: Aggregation::Count,
            field: "a_field".to_owned(),
            filter: None,
            additivity: None,
            aggregation_anchor: None,
            visible: true,
        }
    }

    fn relationship(semantic_name: &str) -> Relationship {
        Relationship {
            semantic_name: semantic_name.to_owned(),
            target_model: "a_model".to_owned(),
            target: Relation {
                schema: "public".to_owned(),
                relation: "a_source".to_owned(),
            },
            cardinality: Cardinality::ManyToOne,
            join_type: JoinType::Left,
            from_column: "a_model_id".to_owned(),
            to_column: "id".to_owned(),
        }
    }
}
