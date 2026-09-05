use std::collections::{BTreeMap, BTreeSet};

use postgresem_compiler::{
    Aggregation, Cardinality, DataType, Field, JoinType, Metric, Model, Relation, Relationship,
    SemanticSnapshot,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    catalog::{CatalogConstraint, CatalogError, CatalogRelation, CatalogSnapshot},
    catalog_types::{portable_identifier, postgres_data_type as catalog_postgres_data_type},
    hash::sha256,
};

const OSI_VERSION: &str = "0.1.1";
pub(crate) const IMPORT_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OsiImportReport {
    pub schema_version: String,
    pub source_format: String,
    pub source_version: String,
    pub source_hash: String,
    pub catalog_fingerprint: String,
    pub semantic_model: String,
    pub warnings: Vec<OsiImportWarning>,
    pub snapshot: SemanticSnapshot,
}

fn ensure_unique_names<'a>(
    kind: &'static str,
    names: impl Iterator<Item = &'a str>,
) -> Result<(), OsiImportError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(OsiImportError::DuplicateName {
                kind,
                name: name.to_owned(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OsiImportWarning {
    pub code: &'static str,
    pub path: String,
    pub message: &'static str,
}

#[derive(Debug, Error)]
pub enum OsiImportError {
    #[error("OSI document is not valid strict YAML")]
    Parse(#[source] serde_yaml_ng::Error),
    #[error("OSI document must contain exactly one semantic model or select one explicitly")]
    AmbiguousSemanticModel,
    #[error("selected OSI semantic model is not available")]
    SemanticModelNotFound,
    #[error("OSI identifier at {path} is not portable: {value}")]
    InvalidIdentifier { path: String, value: String },
    #[error("duplicate OSI {kind} name: {name}")]
    DuplicateName { kind: &'static str, name: String },
    #[error("OSI custom extensions are not supported at {0}")]
    UnsupportedExtension(String),
    #[error("OSI unique_keys are not representable at {0}")]
    UnsupportedUniqueKeys(String),
    #[error("OSI source at {path} must be schema.relation or database.schema.relation")]
    InvalidSource { path: String },
    #[error("OSI source database at {path} does not match catalog evidence")]
    DatabaseMismatch { path: String },
    #[error("OSI source relation at {path} is not present in catalog evidence")]
    RelationNotFound { path: String },
    #[error("OSI source relation at {path} is not selectable by the catalog role")]
    RelationNotSelectable { path: String },
    #[error("OSI expression at {path} is outside the supported closed subset")]
    UnsupportedExpression { path: String },
    #[error("PostgreSQL type at {path} is not supported: {value}")]
    UnsupportedPostgresType { path: String, value: String },
    #[error("OSI field at {path} is not present or selectable in catalog evidence")]
    ColumnNotAvailable { path: String },
    #[error("OSI time dimension at {0} lacks a safe PostgreSQL time contract")]
    InvalidTimeDimension(String),
    #[error("OSI primary key at {0} does not match the validated PostgreSQL primary key")]
    PrimaryKeyMismatch(String),
    #[error("OSI primary key at {0} is composite and cannot be represented safely")]
    CompositePrimaryKey(String),
    #[error("OSI relationship at {0} must contain exactly one column pair")]
    CompositeRelationship(String),
    #[error("OSI relationship at {path} references an unavailable dataset or field")]
    InvalidRelationship { path: String },
    #[error("OSI metric at {0} must be one supported single-field aggregate expression")]
    UnsupportedMetricExpression(String),
    #[error("OSI metric at {0} uses count on a non-entity field")]
    CountRequiresEntityKey(String),
    #[error("catalog evidence is invalid")]
    InvalidCatalog(#[source] CatalogError),
    #[error("failed to calculate imported snapshot revision hash")]
    SnapshotHash(#[source] postgresem_compiler::SnapshotHashError),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiDocument {
    semantic_model: Vec<OsiSemanticModel>,
    #[serde(default)]
    custom_extensions: Vec<OsiExtension>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiSemanticModel {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    ai_context: Option<serde_yaml_ng::Value>,
    datasets: Vec<OsiDataset>,
    #[serde(default)]
    relationships: Vec<OsiRelationship>,
    #[serde(default)]
    metrics: Vec<OsiMetric>,
    #[serde(default)]
    custom_extensions: Vec<OsiExtension>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiDataset {
    name: String,
    source: String,
    #[serde(default)]
    primary_key: Vec<String>,
    #[serde(default)]
    unique_keys: Vec<Vec<String>>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    ai_context: Option<serde_yaml_ng::Value>,
    #[serde(default)]
    fields: Vec<OsiField>,
    #[serde(default)]
    custom_extensions: Vec<OsiExtension>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiRelationship {
    name: String,
    from: String,
    to: String,
    from_columns: Vec<String>,
    to_columns: Vec<String>,
    #[serde(default)]
    ai_context: Option<serde_yaml_ng::Value>,
    #[serde(default)]
    custom_extensions: Vec<OsiExtension>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiField {
    name: String,
    expression: OsiExpression,
    #[serde(default)]
    dimension: Option<OsiDimension>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    ai_context: Option<serde_yaml_ng::Value>,
    #[serde(default)]
    custom_extensions: Vec<OsiExtension>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiMetric {
    name: String,
    expression: OsiExpression,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    ai_context: Option<serde_yaml_ng::Value>,
    #[serde(default)]
    custom_extensions: Vec<OsiExtension>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiExpression {
    dialects: Vec<OsiDialectExpression>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiDialectExpression {
    dialect: OsiDialect,
    expression: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum OsiDialect {
    AnsiSql,
    Snowflake,
    Mdx,
    Tableau,
    Databricks,
    Maql,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiDimension {
    #[serde(default)]
    is_time: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OsiExtension {
    vendor_name: String,
    data: serde_yaml_ng::Value,
}

struct ImportedModel {
    model: Model,
    fields: BTreeMap<String, ImportedField>,
    foreign_keys: Vec<ImportedForeignKey>,
}

struct ImportedField {
    column: String,
    data_type: DataType,
    entity_key: bool,
    time_dimension: bool,
    nullable: bool,
}

struct ImportedForeignKey {
    columns: Vec<String>,
    target: Relation,
    referenced_columns: Vec<String>,
}

pub fn import(
    input: &[u8],
    catalog: &CatalogSnapshot,
    selected_model: Option<&str>,
) -> Result<OsiImportReport, OsiImportError> {
    let document: OsiDocument = serde_yaml_ng::from_slice(input).map_err(OsiImportError::Parse)?;
    reject_extensions("/", &document.custom_extensions)?;
    ensure_unique_names(
        "semantic model",
        document
            .semantic_model
            .iter()
            .map(|model| model.name.as_str()),
    )?;
    let semantic_model = select_semantic_model(document.semantic_model, selected_model)?;
    validate_identifier("/semantic_model/name", &semantic_model.name)?;
    reject_extensions(
        &format!("/semantic_model/{}", semantic_model.name),
        &semantic_model.custom_extensions,
    )?;

    let catalog = catalog
        .validated_normalized()
        .map_err(OsiImportError::InvalidCatalog)?;
    let relation_index = catalog
        .relations
        .iter()
        .map(|relation| ((relation.schema.as_str(), relation.name.as_str()), relation))
        .collect::<BTreeMap<_, _>>();
    let mut warnings = Vec::new();
    warn_context(
        &format!("/semantic_model/{}", semantic_model.name),
        semantic_model.description.as_ref(),
        semantic_model.ai_context.as_ref(),
        &mut warnings,
    );

    let mut imported = BTreeMap::new();
    for dataset in semantic_model.datasets {
        import_dataset(
            dataset,
            &catalog.current_database,
            &relation_index,
            &mut imported,
            &mut warnings,
        )?;
    }
    ensure_unique_names(
        "relationship",
        semantic_model
            .relationships
            .iter()
            .map(|relationship| relationship.name.as_str()),
    )?;
    ensure_unique_names(
        "metric",
        semantic_model
            .metrics
            .iter()
            .map(|metric| metric.name.as_str()),
    )?;
    for relationship in semantic_model.relationships {
        import_relationship(relationship, &mut imported, &mut warnings)?;
    }
    for metric in semantic_model.metrics {
        import_metric(metric, &mut imported, &mut warnings)?;
    }

    let mut snapshot = SemanticSnapshot {
        schema_version: "1".to_owned(),
        revision_hash: String::new(),
        models: imported.into_values().map(|entry| entry.model).collect(),
    }
    .normalized();
    snapshot.revision_hash = snapshot
        .calculate_revision_hash()
        .map_err(OsiImportError::SnapshotHash)?;
    warnings.sort_by(|left, right| left.path.cmp(&right.path).then(left.code.cmp(right.code)));

    Ok(OsiImportReport {
        schema_version: IMPORT_SCHEMA_VERSION.to_owned(),
        source_format: "apache-ossie".to_owned(),
        source_version: OSI_VERSION.to_owned(),
        source_hash: sha256(input),
        catalog_fingerprint: catalog.fingerprint,
        semantic_model: semantic_model.name,
        warnings,
        snapshot,
    })
}

fn select_semantic_model(
    models: Vec<OsiSemanticModel>,
    selected: Option<&str>,
) -> Result<OsiSemanticModel, OsiImportError> {
    if let Some(selected) = selected {
        return models
            .into_iter()
            .find(|model| model.name == selected)
            .ok_or(OsiImportError::SemanticModelNotFound);
    }
    let mut models = models.into_iter();
    let model = models
        .next()
        .ok_or(OsiImportError::AmbiguousSemanticModel)?;
    if models.next().is_some() {
        return Err(OsiImportError::AmbiguousSemanticModel);
    }
    Ok(model)
}

fn import_dataset(
    dataset: OsiDataset,
    current_database: &str,
    catalog: &BTreeMap<(&str, &str), &CatalogRelation>,
    imported: &mut BTreeMap<String, ImportedModel>,
    warnings: &mut Vec<OsiImportWarning>,
) -> Result<(), OsiImportError> {
    let path = format!("/datasets/{}", dataset.name);
    validate_identifier(&format!("{path}/name"), &dataset.name)?;
    if imported.contains_key(&dataset.name) {
        return Err(OsiImportError::DuplicateName {
            kind: "dataset",
            name: dataset.name,
        });
    }
    reject_extensions(&path, &dataset.custom_extensions)?;
    if !dataset.unique_keys.is_empty() {
        return Err(OsiImportError::UnsupportedUniqueKeys(path));
    }
    warn_context(
        &path,
        dataset.description.as_ref(),
        dataset.ai_context.as_ref(),
        warnings,
    );

    let (schema, relation_name) = parse_source(&dataset.source, current_database, &path)?;
    let relation = catalog
        .get(&(schema.as_str(), relation_name.as_str()))
        .copied()
        .ok_or_else(|| OsiImportError::RelationNotFound { path: path.clone() })?;
    if !relation.grants.schema_usage
        || (!relation.grants.table_select && !relation.grants.any_column_select)
    {
        return Err(OsiImportError::RelationNotSelectable { path });
    }

    let catalog_primary_key = catalog_primary_key(relation);
    if !dataset.primary_key.is_empty() && dataset.primary_key != catalog_primary_key {
        return Err(OsiImportError::PrimaryKeyMismatch(path));
    }
    let effective_primary_key = if dataset.primary_key.is_empty() {
        if !catalog_primary_key.is_empty() {
            warnings.push(OsiImportWarning {
                code: "OSI_PRIMARY_KEY_DERIVED_FROM_CATALOG",
                path: path.clone(),
                message: "primary key was omitted and derived from validated PostgreSQL evidence",
            });
        }
        catalog_primary_key
    } else {
        dataset.primary_key
    };
    if effective_primary_key.len() > 1 {
        return Err(OsiImportError::CompositePrimaryKey(path));
    }
    let entity_keys = effective_primary_key.iter().collect::<BTreeSet<_>>();
    let catalog_columns = relation
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect::<BTreeMap<_, _>>();
    let mut field_names = BTreeSet::new();
    let mut fields = Vec::new();
    let mut imported_fields = BTreeMap::new();
    for field in dataset.fields {
        let field_path = format!("{path}/fields/{}", field.name);
        validate_identifier(&format!("{field_path}/name"), &field.name)?;
        if !field_names.insert(field.name.clone()) {
            return Err(OsiImportError::DuplicateName {
                kind: "field",
                name: field.name,
            });
        }
        reject_extensions(&field_path, &field.custom_extensions)?;
        warn_context(
            &field_path,
            field.description.as_ref().or(field.label.as_ref()),
            field.ai_context.as_ref(),
            warnings,
        );
        let column_name = direct_ansi_expression(&field.expression, &field_path)?;
        let column = catalog_columns.get(column_name.as_str()).ok_or_else(|| {
            OsiImportError::ColumnNotAvailable {
                path: field_path.clone(),
            }
        })?;
        if !column.select_grant {
            return Err(OsiImportError::ColumnNotAvailable { path: field_path });
        }
        let data_type = postgres_data_type(&column.data_type, &field_path)?;
        let is_time = field
            .dimension
            .and_then(|dimension| dimension.is_time)
            .unwrap_or(false);
        if is_time && !matches!(data_type, DataType::Date | DataType::Timestamp) {
            return Err(OsiImportError::InvalidTimeDimension(field_path));
        }
        let entity_key = entity_keys.contains(&column_name);
        fields.push(Field {
            semantic_name: field.name.clone(),
            data_type,
            column: column_name.clone(),
            relationship: None,
            time_dimension: is_time,
            entity_key,
            visible: true,
            nullable: column.nullable,
        });
        imported_fields.insert(
            field.name,
            ImportedField {
                column: column_name,
                data_type,
                entity_key,
                time_dimension: is_time,
                nullable: column.nullable,
            },
        );
    }
    for key in &effective_primary_key {
        if !imported_fields
            .values()
            .any(|field| field.column == *key && field.entity_key)
        {
            return Err(OsiImportError::PrimaryKeyMismatch(path));
        }
    }

    imported.insert(
        dataset.name.clone(),
        ImportedModel {
            model: Model {
                semantic_name: dataset.name,
                source: Relation {
                    schema,
                    relation: relation_name,
                },
                timezone: None,
                queryable: true,
                writable: None,
                fields,
                metrics: Vec::new(),
                relationships: Vec::new(),
            },
            fields: imported_fields,
            foreign_keys: catalog_foreign_keys(relation),
        },
    );
    Ok(())
}

fn import_relationship(
    relationship: OsiRelationship,
    imported: &mut BTreeMap<String, ImportedModel>,
    warnings: &mut Vec<OsiImportWarning>,
) -> Result<(), OsiImportError> {
    let path = format!("/relationships/{}", relationship.name);
    validate_identifier(&format!("{path}/name"), &relationship.name)?;
    reject_extensions(&path, &relationship.custom_extensions)?;
    if relationship.ai_context.is_some() {
        warnings.push(dropped_context_warning(path.clone()));
    }
    if relationship.from_columns.len() != 1 || relationship.to_columns.len() != 1 {
        return Err(OsiImportError::CompositeRelationship(path));
    }
    let from_field_name = &relationship.from_columns[0];
    let to_field_name = &relationship.to_columns[0];
    let to_model = imported
        .get(&relationship.to)
        .ok_or_else(|| OsiImportError::InvalidRelationship { path: path.clone() })?;
    let target = to_model.model.source.clone();
    let projected_fields = to_model
        .fields
        .iter()
        .map(|(name, field)| {
            (
                format!("{}_{}", relationship.name, name),
                field.column.clone(),
                field.data_type,
                field.time_dimension,
                field.nullable,
            )
        })
        .collect::<Vec<_>>();
    let to_column = to_model
        .fields
        .values()
        .find(|field| field.column == *to_field_name)
        .filter(|field| field.entity_key)
        .map(|field| field.column.clone())
        .ok_or_else(|| OsiImportError::InvalidRelationship { path: path.clone() })?;
    let from_model = imported
        .get_mut(&relationship.from)
        .ok_or_else(|| OsiImportError::InvalidRelationship { path: path.clone() })?;
    if from_model
        .model
        .relationships
        .iter()
        .any(|existing| existing.semantic_name == relationship.name)
    {
        return Err(OsiImportError::DuplicateName {
            kind: "relationship",
            name: relationship.name,
        });
    }
    let from_column = from_model
        .fields
        .values()
        .find(|field| field.column == *from_field_name)
        .map(|field| field.column.clone())
        .ok_or_else(|| OsiImportError::InvalidRelationship { path })?;
    if !from_model.foreign_keys.iter().any(|foreign_key| {
        foreign_key.columns.len() == 1
            && foreign_key.columns[0] == from_column
            && foreign_key.target == target
            && foreign_key.referenced_columns.len() == 1
            && foreign_key.referenced_columns[0] == to_column
    }) {
        return Err(OsiImportError::InvalidRelationship {
            path: format!("/relationships/{}", relationship.name),
        });
    }
    from_model.model.relationships.push(Relationship {
        semantic_name: relationship.name.clone(),
        target_model: relationship.to,
        target,
        cardinality: Cardinality::ManyToOne,
        join_type: JoinType::Left,
        from_column,
        to_column,
    });
    for (projected_name, column, data_type, time_dimension, nullable) in projected_fields {
        if from_model
            .model
            .fields
            .iter()
            .any(|field| field.semantic_name == projected_name)
        {
            return Err(OsiImportError::InvalidRelationship {
                path: format!("/relationships/{}", relationship.name),
            });
        }
        from_model.model.fields.push(Field {
            semantic_name: projected_name,
            data_type,
            column,
            relationship: Some(relationship.name.clone()),
            time_dimension,
            entity_key: false,
            visible: true,
            nullable,
        });
    }
    Ok(())
}

fn import_metric(
    metric: OsiMetric,
    imported: &mut BTreeMap<String, ImportedModel>,
    warnings: &mut Vec<OsiImportWarning>,
) -> Result<(), OsiImportError> {
    let path = format!("/metrics/{}", metric.name);
    validate_identifier(&format!("{path}/name"), &metric.name)?;
    reject_extensions(&path, &metric.custom_extensions)?;
    warn_context(
        &path,
        metric.description.as_ref(),
        metric.ai_context.as_ref(),
        warnings,
    );
    let expression = ansi_expression(&metric.expression, &path)?;
    let parsed = parse_metric_expression(expression)
        .ok_or_else(|| OsiImportError::UnsupportedMetricExpression(path.clone()))?;
    let imported_model = imported
        .get_mut(parsed.dataset)
        .ok_or_else(|| OsiImportError::UnsupportedMetricExpression(path.clone()))?;
    if imported_model
        .model
        .metrics
        .iter()
        .any(|existing| existing.semantic_name == metric.name)
    {
        return Err(OsiImportError::DuplicateName {
            kind: "metric",
            name: metric.name,
        });
    }
    let field = imported_model
        .fields
        .get(parsed.field)
        .ok_or_else(|| OsiImportError::UnsupportedMetricExpression(path.clone()))?;
    if matches!(
        parsed.aggregation,
        Aggregation::Count | Aggregation::CountDistinct
    ) && !field.entity_key
    {
        return Err(OsiImportError::CountRequiresEntityKey(path));
    }
    if !aggregate_supports_type(parsed.aggregation, field.data_type) {
        return Err(OsiImportError::UnsupportedMetricExpression(path));
    }
    let expected_type = expected_metric_type(parsed.aggregation, field.data_type);
    imported_model.model.metrics.push(Metric {
        semantic_name: metric.name,
        data_type: expected_type,
        aggregation: parsed.aggregation,
        field: parsed.field.to_owned(),
        filter: None,
        additivity: None,
        aggregation_anchor: None,
        visible: true,
    });
    Ok(())
}

struct ParsedMetric<'a> {
    aggregation: Aggregation,
    dataset: &'a str,
    field: &'a str,
}

fn parse_metric_expression(expression: &str) -> Option<ParsedMetric<'_>> {
    let expression = expression.trim();
    let open = expression.find('(')?;
    if !expression.ends_with(')') {
        return None;
    }
    let function = expression[..open].trim();
    let mut argument = expression[open + 1..expression.len() - 1].trim();
    let mut distinct = false;
    if argument
        .as_bytes()
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"distinct"))
    {
        if !argument
            .as_bytes()
            .get(8)
            .is_some_and(u8::is_ascii_whitespace)
        {
            return None;
        }
        argument = argument.get(8..)?.trim();
        distinct = true;
    }
    let (dataset, field) = argument.split_once('.')?;
    if field.contains('.') || !portable_identifier(dataset) || !portable_identifier(field) {
        return None;
    }
    let aggregation = if function.eq_ignore_ascii_case("count") {
        if distinct {
            Aggregation::CountDistinct
        } else {
            Aggregation::Count
        }
    } else if distinct {
        return None;
    } else if function.eq_ignore_ascii_case("sum") {
        Aggregation::Sum
    } else if function.eq_ignore_ascii_case("min") {
        Aggregation::Min
    } else if function.eq_ignore_ascii_case("max") {
        Aggregation::Max
    } else if function.eq_ignore_ascii_case("avg") {
        Aggregation::Avg
    } else {
        return None;
    };
    Some(ParsedMetric {
        aggregation,
        dataset,
        field,
    })
}

fn direct_ansi_expression(
    expression: &OsiExpression,
    path: &str,
) -> Result<String, OsiImportError> {
    let value = ansi_expression(expression, path)?.trim();
    if !portable_identifier(value) {
        return Err(OsiImportError::UnsupportedExpression {
            path: path.to_owned(),
        });
    }
    Ok(value.to_owned())
}

fn ansi_expression<'a>(
    expression: &'a OsiExpression,
    path: &str,
) -> Result<&'a str, OsiImportError> {
    if expression.dialects.len() != 1 || expression.dialects[0].dialect != OsiDialect::AnsiSql {
        return Err(OsiImportError::UnsupportedExpression {
            path: path.to_owned(),
        });
    }
    Ok(&expression.dialects[0].expression)
}

fn parse_source(
    source: &str,
    current_database: &str,
    path: &str,
) -> Result<(String, String), OsiImportError> {
    let parts = source.split('.').collect::<Vec<_>>();
    let (database, schema, relation) = match parts.as_slice() {
        [schema, relation] => (None, *schema, *relation),
        [database, schema, relation] => (Some(*database), *schema, *relation),
        _ => {
            return Err(OsiImportError::InvalidSource {
                path: path.to_owned(),
            });
        }
    };
    if !portable_identifier(schema) || !portable_identifier(relation) {
        return Err(OsiImportError::InvalidSource {
            path: path.to_owned(),
        });
    }
    if database.is_some_and(|database| database != current_database) {
        return Err(OsiImportError::DatabaseMismatch {
            path: path.to_owned(),
        });
    }
    Ok((schema.to_owned(), relation.to_owned()))
}

fn catalog_primary_key(relation: &CatalogRelation) -> Vec<String> {
    relation
        .constraints
        .iter()
        .find_map(|constraint| match constraint {
            CatalogConstraint::PrimaryKey {
                columns,
                enforced: true,
                period: false,
                validated: true,
                ..
            } => Some(columns.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn catalog_foreign_keys(relation: &CatalogRelation) -> Vec<ImportedForeignKey> {
    relation
        .constraints
        .iter()
        .filter_map(|constraint| match constraint {
            CatalogConstraint::ForeignKey {
                columns,
                referenced_relation,
                referenced_columns,
                enforced: true,
                period: false,
                validated: true,
                ..
            } => Some(ImportedForeignKey {
                columns: columns.clone(),
                target: Relation {
                    schema: referenced_relation.schema.clone(),
                    relation: referenced_relation.name.clone(),
                },
                referenced_columns: referenced_columns.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn postgres_data_type(value: &str, path: &str) -> Result<DataType, OsiImportError> {
    catalog_postgres_data_type(value).ok_or_else(|| OsiImportError::UnsupportedPostgresType {
        path: path.to_owned(),
        value: value.to_ascii_lowercase(),
    })
}

const fn expected_metric_type(aggregation: Aggregation, field_type: DataType) -> DataType {
    match aggregation {
        Aggregation::Count | Aggregation::CountDistinct => DataType::Integer,
        Aggregation::Avg => DataType::Numeric,
        Aggregation::Sum if matches!(field_type, DataType::Integer) => DataType::Numeric,
        Aggregation::Sum | Aggregation::Min | Aggregation::Max => field_type,
    }
}

const fn aggregate_supports_type(aggregation: Aggregation, field_type: DataType) -> bool {
    match aggregation {
        Aggregation::Count | Aggregation::CountDistinct => true,
        Aggregation::Sum | Aggregation::Avg => {
            matches!(field_type, DataType::Integer | DataType::Numeric)
        }
        Aggregation::Min | Aggregation::Max => !matches!(field_type, DataType::Boolean),
    }
}

fn reject_extensions(path: &str, extensions: &[OsiExtension]) -> Result<(), OsiImportError> {
    if let Some(extension) = extensions.first() {
        let _ = (&extension.vendor_name, &extension.data);
        return Err(OsiImportError::UnsupportedExtension(path.to_owned()));
    }
    Ok(())
}

fn warn_context(
    path: &str,
    description: Option<&String>,
    ai_context: Option<&serde_yaml_ng::Value>,
    warnings: &mut Vec<OsiImportWarning>,
) {
    if description.is_some() || ai_context.is_some() {
        warnings.push(dropped_context_warning(path.to_owned()));
    }
}

fn dropped_context_warning(path: String) -> OsiImportWarning {
    OsiImportWarning {
        code: "OSI_CONTEXT_REQUIRES_REVIEW",
        path,
        message: "description or AI context is not part of the executable snapshot projection",
    }
}

fn validate_identifier(path: &str, value: &str) -> Result<(), OsiImportError> {
    if portable_identifier(value) {
        Ok(())
    } else {
        Err(OsiImportError::InvalidIdentifier {
            path: path.to_owned(),
            value: value.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use postgresem_compiler::{CompilerOptions, compile_lsq, normalize_lsq};

    use super::{OsiImportError, import};
    use crate::catalog::{
        CatalogColumn, CatalogConstraint, CatalogRelation, CatalogRoleContext, CatalogSnapshot,
        RelationGrantHints, RelationKind, RowLevelSecurity,
    };

    const VALID_OSI: &str = r#"
semantic_model:
  - name: commerce
    datasets:
      - name: orders
        source: app.commerce.orders
        primary_key: [order_id]
        fields:
          - name: order_id
            expression:
              dialects:
                - dialect: ANSI_SQL
                  expression: order_id
          - name: amount
            expression:
              dialects:
                - dialect: ANSI_SQL
                  expression: amount
    metrics:
      - name: order_count
        expression:
          dialects:
            - dialect: ANSI_SQL
              expression: COUNT(orders.order_id)
      - name: revenue
        expression:
          dialects:
            - dialect: ANSI_SQL
              expression: SUM(orders.amount)
"#;

    #[test]
    fn imports_typed_direct_fields_and_supported_metrics() {
        let report = import(
            VALID_OSI.as_bytes(),
            &catalog().expect("valid catalog"),
            None,
        )
        .expect("valid OSI import");

        assert_eq!(report.semantic_model, "commerce");
        assert_eq!(report.snapshot.models.len(), 1);
        assert_eq!(report.snapshot.models[0].fields.len(), 2);
        assert_eq!(report.snapshot.models[0].metrics.len(), 2);
        assert!(report.snapshot.models[0].writable.is_none());
        assert!(report.snapshot.revision_hash.starts_with("sha256:"));
    }

    #[test]
    fn rejects_raw_sql_unsupported_catalog_types_and_duplicate_yaml_keys() {
        let raw_sql = VALID_OSI.replace("expression: amount", "expression: amount * 2");
        assert!(matches!(
            import(raw_sql.as_bytes(), &catalog().expect("valid catalog"), None),
            Err(OsiImportError::UnsupportedExpression { .. })
        ));

        let mut unsupported_catalog = catalog().expect("valid catalog");
        unsupported_catalog.relations[0].columns[1].data_type = "double precision".to_owned();
        unsupported_catalog = unsupported_catalog
            .finalize()
            .expect("valid unsupported-type catalog");
        assert!(matches!(
            import(VALID_OSI.as_bytes(), &unsupported_catalog, None),
            Err(OsiImportError::UnsupportedPostgresType { .. })
        ));

        let non_ascii_metric = VALID_OSI.replace("SUM(orders.amount)", "COUNT(aaaaaaaé)");
        assert!(matches!(
            import(
                non_ascii_metric.as_bytes(),
                &catalog().expect("valid catalog"),
                None
            ),
            Err(OsiImportError::UnsupportedMetricExpression(_))
        ));

        let invalid_time = VALID_OSI.replace(
            "            expression:\n              dialects:\n                - dialect: ANSI_SQL\n                  expression: amount",
            "            expression:\n              dialects:\n                - dialect: ANSI_SQL\n                  expression: amount\n            dimension:\n              is_time: true",
        );
        assert!(matches!(
            import(
                invalid_time.as_bytes(),
                &catalog().expect("valid catalog"),
                None
            ),
            Err(OsiImportError::InvalidTimeDimension(_))
        ));

        let duplicate = VALID_OSI.replace(
            "  - name: commerce",
            "  - name: commerce\n    name: duplicate",
        );
        assert!(matches!(
            import(
                duplicate.as_bytes(),
                &catalog().expect("valid catalog"),
                None
            ),
            Err(OsiImportError::Parse(_))
        ));
    }

    #[test]
    fn rejects_catalog_tampering_and_unrepresentable_semantics() {
        let mut tampered = catalog().expect("valid catalog");
        tampered.relations[0].columns[0].data_type = "text".to_owned();
        assert!(matches!(
            import(VALID_OSI.as_bytes(), &tampered, None),
            Err(OsiImportError::InvalidCatalog(_))
        ));

        let unique_keys = VALID_OSI.replace(
            "        primary_key: [order_id]",
            "        primary_key: [order_id]\n        unique_keys: [[amount]]",
        );
        assert!(matches!(
            import(
                unique_keys.as_bytes(),
                &catalog().expect("valid catalog"),
                None
            ),
            Err(OsiImportError::UnsupportedUniqueKeys(_))
        ));

        let composite_primary_key = VALID_OSI.replace(
            "        primary_key: [order_id]",
            "        primary_key: [order_id, amount]",
        );
        let mut composite_catalog = catalog().expect("valid catalog");
        composite_catalog.relations[0].constraints[0] = CatalogConstraint::PrimaryKey {
            name: "orders_pkey".to_owned(),
            columns: vec!["order_id".to_owned(), "amount".to_owned()],
            enforced: true,
            period: false,
            deferrable: false,
            initially_deferred: false,
            validated: true,
        };
        composite_catalog = composite_catalog
            .finalize()
            .expect("valid composite catalog");
        assert!(matches!(
            import(composite_primary_key.as_bytes(), &composite_catalog, None),
            Err(OsiImportError::CompositePrimaryKey(_))
        ));

        let mut temporal_catalog = catalog().expect("valid catalog");
        if let CatalogConstraint::PrimaryKey {
            enforced, period, ..
        } = &mut temporal_catalog.relations[0].constraints[0]
        {
            *enforced = false;
            *period = true;
        }
        temporal_catalog = temporal_catalog.finalize().expect("valid temporal catalog");
        assert!(matches!(
            import(VALID_OSI.as_bytes(), &temporal_catalog, None),
            Err(OsiImportError::PrimaryKeyMismatch(_))
        ));

        let mut text_amount = catalog().expect("valid catalog");
        text_amount.relations[0].columns[1].data_type = "text".to_owned();
        text_amount = text_amount.finalize().expect("valid text catalog");
        assert!(matches!(
            import(VALID_OSI.as_bytes(), &text_amount, None),
            Err(OsiImportError::UnsupportedMetricExpression(_))
        ));
    }

    #[test]
    fn committed_fixture_imports_and_compiles() {
        let catalog: CatalogSnapshot = serde_json::from_slice(
            &std::fs::read(repo_path(
                "fixtures/interoperability/osi-commerce-catalog.json",
            ))
            .expect("catalog fixture exists"),
        )
        .expect("valid catalog fixture");
        let report = import(
            &std::fs::read(repo_path("fixtures/interoperability/osi-commerce.yaml"))
                .expect("OSI fixture exists"),
            &catalog,
            None,
        )
        .expect("fixture imports");
        let query = normalize_lsq(
            &std::fs::read(repo_path("examples/semantic_demo/requests/orders-revenue.json"))
                .expect("query fixture exists"),
        )
        .expect("valid LSQ");

        let compiled = compile_lsq(&query, &report.snapshot, CompilerOptions::default())
            .expect("imported snapshot compiles");
        assert_eq!(compiled.output_schema[0].name, "revenue");

        let related_query = normalize_lsq(
            br#"{
                "schema_version": "1",
                "model": "orders",
                "dimensions": [{"field": "orders_to_customers_region"}]
            }"#,
        )
        .expect("valid related LSQ");
        let related = compile_lsq(&related_query, &report.snapshot, CompilerOptions::default())
            .expect("imported relationship compiles");
        assert_eq!(
            related.lineage.relationships,
            ["orders_to_customers".to_owned()]
        );
    }

    fn catalog() -> Result<CatalogSnapshot, crate::catalog::CatalogError> {
        CatalogSnapshot {
            schema_version: "2".to_owned(),
            server_version_num: 180_000,
            current_database: "app".to_owned(),
            current_role: "postgresem_introspector".to_owned(),
            role_context: CatalogRoleContext {
                inherit: true,
                superuser: false,
                bypass_rls: false,
                effective_roles: vec!["postgresem_introspector".to_owned()],
                settable_roles: Vec::new(),
            },
            role_graph_fingerprint: "sha256:role-graph".to_owned(),
            object_privilege_fingerprint: "sha256:privileges".to_owned(),
            functions: Vec::new(),
            relations: vec![CatalogRelation {
                schema: "commerce".to_owned(),
                name: "orders".to_owned(),
                kind: RelationKind::Table,
                owner: "postgresem_source_owner".to_owned(),
                view: None,
                comment: Some("Orders".to_owned()),
                grants: RelationGrantHints {
                    schema_usage: true,
                    table_select: true,
                    any_column_select: true,
                },
                rls: RowLevelSecurity {
                    enabled: false,
                    forced: false,
                },
                columns: vec![
                    CatalogColumn {
                        name: "order_id".to_owned(),
                        ordinal: 1,
                        data_type: "bigint".to_owned(),
                        nullable: false,
                        comment: None,
                        select_grant: true,
                    },
                    CatalogColumn {
                        name: "amount".to_owned(),
                        ordinal: 2,
                        data_type: "numeric(12,2)".to_owned(),
                        nullable: false,
                        comment: None,
                        select_grant: true,
                    },
                ],
                constraints: vec![CatalogConstraint::PrimaryKey {
                    name: "orders_pkey".to_owned(),
                    columns: vec!["order_id".to_owned()],
                    enforced: true,
                    period: false,
                    deferrable: false,
                    initially_deferred: false,
                    validated: true,
                }],
                policies: Vec::new(),
            }],
            fingerprint: String::new(),
        }
        .finalize()
    }

    fn repo_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }
}
