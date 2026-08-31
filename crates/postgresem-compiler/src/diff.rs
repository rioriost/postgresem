use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{Field, Metric, Model, Relationship, SemanticSnapshot};

const DIFF_SCHEMA_VERSION: &str = "1";
const SUPPORTED_SNAPSHOT_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    Compatible,
    Breaking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticObjectKind {
    Model,
    Field,
    Metric,
    Relationship,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticChange {
    pub path: String,
    pub object_kind: SemanticObjectKind,
    pub change: ChangeKind,
    pub compatibility: Compatibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffSummary {
    pub total: usize,
    pub compatible: usize,
    pub breaking: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticDiff {
    pub schema_version: String,
    pub from_revision: String,
    pub to_revision: String,
    pub compatibility: Compatibility,
    pub summary: DiffSummary,
    pub changes: Vec<SemanticChange>,
}

impl SemanticDiff {
    #[must_use]
    pub fn has_breaking_changes(&self) -> bool {
        self.compatibility == Compatibility::Breaking
    }
}

#[derive(Debug, Error)]
pub enum DiffError {
    #[error("unsupported semantic snapshot schema version: {0}")]
    UnsupportedSnapshotVersion(String),
    #[error("semantic snapshot contains duplicate {kind} name: {name}")]
    DuplicateObject { kind: &'static str, name: String },
    #[error("failed to serialize semantic diff values")]
    Serialization(#[from] serde_json::Error),
}

pub fn diff_snapshots(
    before: &SemanticSnapshot,
    after: &SemanticSnapshot,
) -> Result<SemanticDiff, DiffError> {
    validate_version(before)?;
    validate_version(after)?;

    let before = before.normalized();
    let after = after.normalized();
    let before_models = named_models(&before.models)?;
    let after_models = named_models(&after.models)?;
    let mut changes = Vec::new();

    for (name, before_model) in &before_models {
        let path = format!("models.{name}");
        let Some(after_model) = after_models.get(name) else {
            changes.push(change(
                path,
                SemanticObjectKind::Model,
                ChangeKind::Removed,
                Compatibility::Breaking,
                Some(*before_model),
                None::<&Model>,
            )?);
            continue;
        };

        if model_metadata(before_model) != model_metadata(after_model) {
            changes.push(SemanticChange {
                path: path.clone(),
                object_kind: SemanticObjectKind::Model,
                change: ChangeKind::Modified,
                compatibility: model_compatibility(before_model, after_model),
                before: Some(model_metadata(before_model)),
                after: Some(model_metadata(after_model)),
            });
        }
        diff_fields(
            &path,
            &before_model.fields,
            &after_model.fields,
            &mut changes,
        )?;
        diff_metrics(
            &path,
            &before_model.metrics,
            &after_model.metrics,
            &mut changes,
        )?;
        diff_relationships(
            &path,
            &before_model.relationships,
            &after_model.relationships,
            &mut changes,
        )?;
    }

    for (name, after_model) in &after_models {
        if !before_models.contains_key(name) {
            changes.push(change(
                format!("models.{name}"),
                SemanticObjectKind::Model,
                ChangeKind::Added,
                Compatibility::Compatible,
                None::<&Model>,
                Some(*after_model),
            )?);
        }
    }

    changes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| change_rank(left.change).cmp(&change_rank(right.change)))
    });
    let breaking = changes
        .iter()
        .filter(|change| change.compatibility == Compatibility::Breaking)
        .count();
    let compatible = changes.len() - breaking;

    Ok(SemanticDiff {
        schema_version: DIFF_SCHEMA_VERSION.to_owned(),
        from_revision: before.revision_hash,
        to_revision: after.revision_hash,
        compatibility: if breaking == 0 {
            Compatibility::Compatible
        } else {
            Compatibility::Breaking
        },
        summary: DiffSummary {
            total: changes.len(),
            compatible,
            breaking,
        },
        changes,
    })
}

fn validate_version(snapshot: &SemanticSnapshot) -> Result<(), DiffError> {
    if snapshot.schema_version != SUPPORTED_SNAPSHOT_VERSION {
        return Err(DiffError::UnsupportedSnapshotVersion(
            snapshot.schema_version.clone(),
        ));
    }
    named_models(&snapshot.models)?;
    for model in &snapshot.models {
        named_objects(&model.fields, "field", |field| &field.semantic_name)?;
        named_objects(&model.metrics, "metric", |metric| &metric.semantic_name)?;
        named_objects(&model.relationships, "relationship", |relationship| {
            &relationship.semantic_name
        })?;
    }
    Ok(())
}

fn named_models(models: &[Model]) -> Result<BTreeMap<&str, &Model>, DiffError> {
    named_objects(models, "model", |model| &model.semantic_name)
}

fn named_objects<'a, T>(
    objects: &'a [T],
    kind: &'static str,
    name: impl Fn(&'a T) -> &'a str,
) -> Result<BTreeMap<&'a str, &'a T>, DiffError> {
    let mut named = BTreeMap::new();
    for object in objects {
        let object_name = name(object);
        if named.insert(object_name, object).is_some() {
            return Err(DiffError::DuplicateObject {
                kind,
                name: object_name.to_owned(),
            });
        }
    }
    Ok(named)
}

fn model_metadata(model: &Model) -> Value {
    json!({
        "source": model.source,
        "timezone": model.timezone,
        "queryable": model.queryable
    })
}

fn model_compatibility(before: &Model, after: &Model) -> Compatibility {
    if before.source != after.source
        || before.timezone != after.timezone
        || (before.queryable && !after.queryable)
    {
        Compatibility::Breaking
    } else {
        Compatibility::Compatible
    }
}

fn diff_fields(
    model_path: &str,
    before: &[Field],
    after: &[Field],
    changes: &mut Vec<SemanticChange>,
) -> Result<(), DiffError> {
    let before = named_objects(before, "field", |field| &field.semantic_name)?;
    let after = named_objects(after, "field", |field| &field.semantic_name)?;
    diff_named(
        model_path,
        "fields",
        SemanticObjectKind::Field,
        &before,
        &after,
        changes,
        |field| field.visible,
        field_compatibility,
    )
}

fn diff_metrics(
    model_path: &str,
    before: &[Metric],
    after: &[Metric],
    changes: &mut Vec<SemanticChange>,
) -> Result<(), DiffError> {
    let before = named_objects(before, "metric", |metric| &metric.semantic_name)?;
    let after = named_objects(after, "metric", |metric| &metric.semantic_name)?;
    diff_named(
        model_path,
        "metrics",
        SemanticObjectKind::Metric,
        &before,
        &after,
        changes,
        |metric| metric.visible,
        metric_compatibility,
    )
}

fn diff_relationships(
    model_path: &str,
    before: &[Relationship],
    after: &[Relationship],
    changes: &mut Vec<SemanticChange>,
) -> Result<(), DiffError> {
    let before = named_objects(before, "relationship", |relationship| {
        &relationship.semantic_name
    })?;
    let after = named_objects(after, "relationship", |relationship| {
        &relationship.semantic_name
    })?;
    diff_named(
        model_path,
        "relationships",
        SemanticObjectKind::Relationship,
        &before,
        &after,
        changes,
        |_| true,
        |_, _| Compatibility::Breaking,
    )
}

#[allow(clippy::too_many_arguments)]
fn diff_named<T>(
    model_path: &str,
    collection: &str,
    object_kind: SemanticObjectKind,
    before: &BTreeMap<&str, &T>,
    after: &BTreeMap<&str, &T>,
    changes: &mut Vec<SemanticChange>,
    visible: impl Fn(&T) -> bool,
    modified_compatibility: impl Fn(&T, &T) -> Compatibility,
) -> Result<(), DiffError>
where
    T: PartialEq + Serialize,
{
    for (name, before_object) in before {
        let path = format!("{model_path}.{collection}.{name}");
        match after.get(name) {
            None => changes.push(change(
                path,
                object_kind,
                ChangeKind::Removed,
                if visible(before_object) {
                    Compatibility::Breaking
                } else {
                    Compatibility::Compatible
                },
                Some(*before_object),
                None::<&T>,
            )?),
            Some(after_object) if *before_object != *after_object => changes.push(change(
                path,
                object_kind,
                ChangeKind::Modified,
                modified_compatibility(before_object, after_object),
                Some(*before_object),
                Some(*after_object),
            )?),
            Some(_) => {}
        }
    }
    for (name, after_object) in after {
        if !before.contains_key(name) {
            changes.push(change(
                format!("{model_path}.{collection}.{name}"),
                object_kind,
                ChangeKind::Added,
                Compatibility::Compatible,
                None::<&T>,
                Some(*after_object),
            )?);
        }
    }
    Ok(())
}

fn field_compatibility(before: &Field, after: &Field) -> Compatibility {
    let only_became_visible = !before.visible
        && after.visible
        && before.data_type == after.data_type
        && before.column == after.column
        && before.relationship == after.relationship
        && before.time_dimension == after.time_dimension
        && before.entity_key == after.entity_key;
    if only_became_visible {
        Compatibility::Compatible
    } else {
        Compatibility::Breaking
    }
}

fn metric_compatibility(before: &Metric, after: &Metric) -> Compatibility {
    let only_became_visible = !before.visible
        && after.visible
        && before.data_type == after.data_type
        && before.aggregation == after.aggregation
        && before.field == after.field
        && before.filter == after.filter;
    if only_became_visible {
        Compatibility::Compatible
    } else {
        Compatibility::Breaking
    }
}

fn change<T: Serialize>(
    path: String,
    object_kind: SemanticObjectKind,
    kind: ChangeKind,
    compatibility: Compatibility,
    before: Option<T>,
    after: Option<T>,
) -> Result<SemanticChange, DiffError> {
    Ok(SemanticChange {
        path,
        object_kind,
        change: kind,
        compatibility,
        before: before.map(serde_json::to_value).transpose()?,
        after: after.map(serde_json::to_value).transpose()?,
    })
}

const fn change_rank(change: ChangeKind) -> u8 {
    match change {
        ChangeKind::Removed => 0,
        ChangeKind::Modified => 1,
        ChangeKind::Added => 2,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Aggregation, Cardinality, DataType, Field, JoinType, Metric, Model, Relation, Relationship,
        SemanticSnapshot,
    };

    use super::{ChangeKind, Compatibility, SemanticObjectKind, diff_snapshots};

    #[test]
    fn diff_is_deterministic_and_classifies_preview_compatibility() {
        let mut before = snapshot();
        let mut after = snapshot();
        after.revision_hash = "sha256:after".to_owned();
        after.models[0].queryable = false;
        after.models[0].fields[0].visible = false;
        after.models[0].fields.push(Field {
            semantic_name: "added".to_owned(),
            data_type: DataType::Text,
            column: "added".to_owned(),
            relationship: None,
            time_dimension: false,
            entity_key: false,
            visible: true,
        });
        after.models[0].metrics.clear();
        after.models[0].relationships.clear();
        after.models.push(Model {
            semantic_name: "added_model".to_owned(),
            source: Relation {
                schema: "public".to_owned(),
                relation: "added_model".to_owned(),
            },
            timezone: None,
            queryable: true,
            fields: vec![],
            metrics: vec![],
            relationships: vec![],
        });
        before.models.reverse();
        after.models.reverse();

        let diff = diff_snapshots(&before, &after).expect("snapshots can be diffed");
        assert_eq!(diff.compatibility, Compatibility::Breaking);
        assert_eq!(diff.summary.total, 6);
        assert_eq!(diff.summary.breaking, 4);
        assert_eq!(diff.summary.compatible, 2);
        assert!(
            diff.changes
                .windows(2)
                .all(|pair| pair[0].path <= pair[1].path)
        );
        assert!(diff.changes.iter().any(|change| {
            change.path == "models.added_model"
                && change.object_kind == SemanticObjectKind::Model
                && change.change == ChangeKind::Added
                && change.compatibility == Compatibility::Compatible
        }));
    }

    #[test]
    fn hidden_object_removal_and_visibility_addition_are_compatible() {
        let mut before = snapshot();
        before.models[0].fields[0].visible = false;
        before.models[0].metrics[0].visible = false;
        let mut after = before.clone();
        after.revision_hash = "sha256:after".to_owned();
        after.models[0].fields[0].visible = true;
        after.models[0].metrics.clear();

        let diff = diff_snapshots(&before, &after).expect("snapshots can be diffed");
        assert_eq!(diff.compatibility, Compatibility::Compatible);
        assert_eq!(diff.summary.compatible, 2);
    }

    #[test]
    fn duplicate_names_are_rejected_even_on_added_models() {
        let before = SemanticSnapshot {
            schema_version: "1".to_owned(),
            revision_hash: "sha256:before".to_owned(),
            models: vec![],
        };
        let mut after = snapshot();
        let duplicate = after.models[0].fields[0].clone();
        after.models[0].fields.push(duplicate);

        let error = diff_snapshots(&before, &after).expect_err("duplicate field must fail");
        assert!(error.to_string().contains("duplicate field name"));
    }

    fn snapshot() -> SemanticSnapshot {
        SemanticSnapshot {
            schema_version: "1".to_owned(),
            revision_hash: "sha256:before".to_owned(),
            models: vec![Model {
                semantic_name: "orders".to_owned(),
                source: Relation {
                    schema: "sales".to_owned(),
                    relation: "orders".to_owned(),
                },
                timezone: Some("UTC".to_owned()),
                queryable: true,
                fields: vec![Field {
                    semantic_name: "order_id".to_owned(),
                    data_type: DataType::Integer,
                    column: "order_id".to_owned(),
                    relationship: None,
                    time_dimension: false,
                    entity_key: true,
                    visible: true,
                }],
                metrics: vec![Metric {
                    semantic_name: "order_count".to_owned(),
                    data_type: DataType::Integer,
                    aggregation: Aggregation::Count,
                    field: "order_id".to_owned(),
                    filter: None,
                    visible: true,
                }],
                relationships: vec![Relationship {
                    semantic_name: "customer".to_owned(),
                    target_model: "customers".to_owned(),
                    target: Relation {
                        schema: "sales".to_owned(),
                        relation: "customers".to_owned(),
                    },
                    cardinality: Cardinality::ManyToOne,
                    join_type: JoinType::Left,
                    from_column: "customer_id".to_owned(),
                    to_column: "customer_id".to_owned(),
                }],
            }],
        }
    }
}
