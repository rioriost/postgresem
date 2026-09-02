use std::{
    collections::{BTreeMap, BTreeSet},
    env,
};

use postgres::{Client, IsolationLevel, Row};
use postgresem_compiler::{
    Additivity, Aggregation, Cardinality, DataType, Field, JoinType, Metric, MetricFilter, Model,
    MutationCapabilities, Relation, Relationship, SemanticSnapshot, UpsertPolicy, WritableField,
    WritableModel,
};
use serde::Deserialize;
use thiserror::Error;

use crate::database;

const REVISION_SQL: &str = r"
    SELECT
        revision.revision_id::text AS revision_id,
        revision.schema_version,
        revision.canonical_hash
    FROM semantic.project AS project
    JOIN semantic.revision AS revision
      ON revision.project_id = project.project_id
    WHERE project.semantic_name = $1
      AND revision.status = 'published'
";

const MODELS_SQL: &str = r"
    SELECT
        model_id::text AS model_id,
        semantic_name,
        source_database,
        current_database() AS current_database,
        source_schema,
        source_relation,
        default_timezone,
        queryable
    FROM semantic.model
    WHERE revision_id = $1::text::uuid
    ORDER BY semantic_name
";

const FIELDS_SQL: &str = r"
    SELECT
        field.model_id::text AS model_id,
        field.semantic_name,
        field.field_kind,
        field.logical_type,
        field.source_column,
        field.expression::text AS expression,
        field.source_relationship_id::text AS source_relationship_id,
        source_relationship.semantic_name AS declared_relationship_name,
        source_relationship.from_model_id::text AS relationship_from_model_id,
        field.hidden,
        field.nullable
    FROM semantic.field AS field
    LEFT JOIN semantic.relationship AS source_relationship
      ON source_relationship.relationship_id = field.source_relationship_id
     AND source_relationship.revision_id = field.revision_id
    WHERE field.revision_id = $1::text::uuid
    ORDER BY field.model_id, field.semantic_name
";

const MUTATION_MODELS_SQL: &str = r"
    SELECT
        model_id::text AS model_id,
        insert_enabled,
        upsert_enabled,
        max_rows,
        max_request_bytes
    FROM semantic.mutation_model
    WHERE revision_id = $1::text::uuid
    ORDER BY model_id
";

const MUTATION_SCHEMA_AVAILABLE_SQL: &str = r"
    SELECT count(*) = 3 AS available
    FROM pg_catalog.pg_class AS relation
    JOIN pg_catalog.pg_namespace AS namespace
      ON namespace.oid = relation.relnamespace
    WHERE namespace.nspname = 'semantic'
      AND relation.relname IN (
        'mutation_model',
        'mutation_field',
        'mutation_model_role'
      )
      AND relation.relkind IN ('r', 'p')
";

const MUTATION_FIELDS_SQL: &str = r"
    SELECT
        mutation_field.model_id::text AS model_id,
        field.semantic_name,
        mutation_field.insertable,
        mutation_field.required_on_insert,
        mutation_field.updatable_on_conflict,
        mutation_field.conflict_key_ordinal::integer AS conflict_key_ordinal,
        mutation_field.returning_ordinal::integer AS returning_ordinal
    FROM semantic.mutation_field
    JOIN semantic.field
      ON field.field_id = mutation_field.field_id
     AND field.revision_id = mutation_field.revision_id
    WHERE mutation_field.revision_id = $1::text::uuid
    ORDER BY mutation_field.model_id, field.semantic_name
";

const MUTATION_CAPABILITIES_SQL: &str = r"
    SELECT model.semantic_name
    FROM semantic.mutation_model_role
    JOIN semantic.model
      ON model.model_id = mutation_model_role.model_id
     AND model.revision_id = mutation_model_role.revision_id
    WHERE mutation_model_role.revision_id = $1::text::uuid
      AND mutation_model_role.database_role = $2::name
    ORDER BY model.semantic_name
";

const METRICS_SQL: &str = r"
    SELECT
        metric.model_id::text AS model_id,
        metric.semantic_name,
        metric.result_type,
        metric.expression::text AS expression,
        metric.metric_filter::text AS metric_filter,
        metric.additivity,
        anchor.semantic_name AS aggregation_anchor,
        metric.hidden
    FROM semantic.metric AS metric
    LEFT JOIN semantic.field AS anchor
      ON anchor.field_id = metric.aggregation_anchor_field_id
     AND anchor.revision_id = metric.revision_id
    WHERE metric.revision_id = $1::text::uuid
    ORDER BY metric.model_id, metric.semantic_name
";

const LEGACY_METRICS_SQL: &str = r"
    SELECT
        model_id::text AS model_id,
        semantic_name,
        result_type,
        expression::text AS expression,
        metric_filter::text AS metric_filter,
        additivity,
        NULL::text AS aggregation_anchor,
        hidden
    FROM semantic.metric
    WHERE revision_id = $1::text::uuid
    ORDER BY model_id, semantic_name
";

const AGGREGATION_ANCHOR_SCHEMA_AVAILABLE_SQL: &str = r"
    SELECT EXISTS (
        SELECT 1
        FROM pg_catalog.pg_attribute AS attribute
        JOIN pg_catalog.pg_class AS relation
          ON relation.oid = attribute.attrelid
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE namespace.nspname = 'semantic'
          AND relation.relname = 'metric'
          AND attribute.attname = 'aggregation_anchor_field_id'
          AND NOT attribute.attisdropped
    ) AS available
";

const RELATIONSHIPS_SQL: &str = r"
    SELECT
        relationship.from_model_id::text AS from_model_id,
        relationship.semantic_name,
        target_model.semantic_name AS target_model,
        target_model.source_schema AS target_schema,
        target_model.source_relation AS target_relation,
        relationship.cardinality,
        relationship.join_type,
        relationship.allowed_direction,
        relationship.priority,
        relationship_column.ordinal::integer AS ordinal,
        from_field.source_column AS from_column,
        to_field.source_column AS to_column
    FROM semantic.relationship AS relationship
    JOIN semantic.model AS target_model
      ON target_model.model_id = relationship.to_model_id
     AND target_model.revision_id = relationship.revision_id
    LEFT JOIN semantic.relationship_column
      ON relationship_column.relationship_id = relationship.relationship_id
     AND relationship_column.revision_id = relationship.revision_id
    LEFT JOIN semantic.field AS from_field
      ON from_field.field_id = relationship_column.from_field_id
     AND from_field.revision_id = relationship_column.revision_id
    LEFT JOIN semantic.field AS to_field
      ON to_field.field_id = relationship_column.to_field_id
     AND to_field.revision_id = relationship_column.revision_id
    WHERE relationship.revision_id = $1::text::uuid
    ORDER BY relationship.from_model_id, relationship.semantic_name, relationship_column.ordinal
";

#[derive(Debug, Error)]
pub enum PublishedModelError {
    #[error("connection URL environment variable {0} is not set")]
    MissingConnectionUrl(String),
    #[error("connection URL environment variable {0} is not valid Unicode")]
    InvalidConnectionUrl(String),
    #[error("failed to connect using connection URL environment variable {variable}")]
    Connect {
        variable: String,
        #[source]
        source: database::ConnectError,
    },
    #[error("failed to start read-only semantic model transaction")]
    StartTransaction(#[source] postgres::Error),
    #[error("failed to read published semantic model {operation}: {source}")]
    Query {
        operation: &'static str,
        #[source]
        source: postgres::Error,
    },
    #[error("semantic project has no published revision: {0}")]
    ProjectNotFound(String),
    #[error("published semantic revision has unsupported schema version: {0}")]
    UnsupportedSchemaVersion(String),
    #[error("published semantic model references an unsupported source database: {0}")]
    UnsupportedSourceDatabase(String),
    #[error("published semantic model references an unknown model id")]
    UnknownModel,
    #[error("published semantic field {model}.{field} has unsupported logical type: {value}")]
    UnsupportedLogicalType {
        model: String,
        field: String,
        value: String,
    },
    #[error("published semantic field {model}.{field} has unsupported field kind: {value}")]
    UnsupportedFieldKind {
        model: String,
        field: String,
        value: String,
    },
    #[error("published semantic field {model}.{field} does not have a column source")]
    MissingFieldColumn { model: String, field: String },
    #[error("published semantic field {model}.{field} has an unsupported expression source")]
    UnsupportedFieldExpression { model: String, field: String },
    #[error("published semantic field {model}.{field} has an invalid relationship source binding")]
    InvalidFieldRelationship { model: String, field: String },
    #[error("published semantic metric {model}.{metric} has invalid aggregation metadata")]
    InvalidMetricExpression {
        model: String,
        metric: String,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "published semantic metric {model}.{metric} has unsupported expression version: {version}"
    )]
    UnsupportedMetricExpressionVersion {
        model: String,
        metric: String,
        version: String,
    },
    #[error("published semantic metric {model}.{metric} has an invalid filter")]
    InvalidMetricFilter {
        model: String,
        metric: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("published semantic metric {model}.{metric} has unsupported additivity: {value}")]
    UnsupportedAdditivity {
        model: String,
        metric: String,
        value: String,
    },
    #[error("published semantic metric {model}.{metric} has invalid aggregation anchor metadata")]
    InvalidAggregationAnchor { model: String, metric: String },
    #[error(
        "published semantic relationship {model}.{relationship} has unsupported cardinality: {value}"
    )]
    UnsupportedCardinality {
        model: String,
        relationship: String,
        value: String,
    },
    #[error(
        "published semantic relationship {model}.{relationship} has unsupported join type: {value}"
    )]
    UnsupportedJoinType {
        model: String,
        relationship: String,
        value: String,
    },
    #[error(
        "published semantic relationship {model}.{relationship} has unsupported routing options"
    )]
    UnsupportedRelationshipOptions { model: String, relationship: String },
    #[error(
        "published semantic relationship {model}.{relationship} must have exactly one column pair"
    )]
    InvalidRelationshipColumns { model: String, relationship: String },
    #[error("published writable semantic model metadata is invalid")]
    InvalidWritableMetadata,
    #[error("published semantic revision hash mismatch: expected {expected}, calculated {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("failed to calculate published semantic revision hash")]
    HashCalculation(#[from] postgresem_compiler::SnapshotHashError),
    #[error("failed to commit read-only semantic model transaction")]
    Commit(#[source] postgres::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedModel {
    pub project: String,
    pub revision_id: String,
    pub snapshot: SemanticSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedMutationModel {
    pub published: PublishedModel,
    pub capabilities: MutationCapabilities,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregationExpression {
    version: String,
    kind: AggregationExpressionKind,
    aggregation: Aggregation,
    field: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AggregationExpressionKind {
    Aggregation,
}

pub fn load_from_env(
    variable: &str,
    project: &str,
) -> Result<SemanticSnapshot, PublishedModelError> {
    Ok(load_published_from_env(variable, project)?.snapshot)
}

pub fn load_published_from_env(
    variable: &str,
    project: &str,
) -> Result<PublishedModel, PublishedModelError> {
    let database_url = env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => PublishedModelError::MissingConnectionUrl(variable.to_owned()),
        env::VarError::NotUnicode(_) => {
            PublishedModelError::InvalidConnectionUrl(variable.to_owned())
        }
    })?;
    let mut client =
        database::connect(&database_url, None).map_err(|source| PublishedModelError::Connect {
            variable: variable.to_owned(),
            source,
        })?;
    load_published(&mut client, project)
}

pub fn load_published(
    client: &mut Client,
    project: &str,
) -> Result<PublishedModel, PublishedModelError> {
    Ok(load_published_internal(client, project, None)?.0)
}

pub fn load_published_for_mutation(
    client: &mut Client,
    project: &str,
    database_role: &str,
) -> Result<PublishedMutationModel, PublishedModelError> {
    let (published, writable_models) =
        load_published_internal(client, project, Some(database_role))?;
    Ok(PublishedMutationModel {
        published,
        capabilities: MutationCapabilities {
            profile: format!("database-role:{database_role}"),
            writable_models,
        },
    })
}

fn load_published_internal(
    client: &mut Client,
    project: &str,
    database_role: Option<&str>,
) -> Result<(PublishedModel, BTreeSet<String>), PublishedModelError> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .map_err(PublishedModelError::StartTransaction)?;

    let revision = transaction
        .query_opt(REVISION_SQL, &[&project])
        .map_err(|source| query_error("revision", source))?
        .ok_or_else(|| PublishedModelError::ProjectNotFound(project.to_owned()))?;
    let revision_id: String = revision.get("revision_id");
    let schema_version: String = revision.get("schema_version");
    let expected_hash: String = revision.get("canonical_hash");
    if !matches!(schema_version.as_str(), "1" | "2") {
        return Err(PublishedModelError::UnsupportedSchemaVersion(
            schema_version,
        ));
    }

    let mut models = load_models(
        &transaction
            .query(MODELS_SQL, &[&revision_id])
            .map_err(|source| query_error("models", source))?,
    )?;
    load_fields(
        &transaction
            .query(FIELDS_SQL, &[&revision_id])
            .map_err(|source| query_error("fields", source))?,
        &mut models,
    )?;
    let aggregation_anchor_schema_available: bool = transaction
        .query_one(AGGREGATION_ANCHOR_SCHEMA_AVAILABLE_SQL, &[])
        .map_err(|source| query_error("aggregation anchor schema availability", source))?
        .get("available");
    if schema_version == "2" && !aggregation_anchor_schema_available {
        return Err(PublishedModelError::UnsupportedSchemaVersion(
            schema_version,
        ));
    }
    let metrics_sql = if aggregation_anchor_schema_available {
        METRICS_SQL
    } else {
        LEGACY_METRICS_SQL
    };
    load_metrics(
        &transaction
            .query(metrics_sql, &[&revision_id])
            .map_err(|source| query_error("metrics", source))?,
        &mut models,
        &schema_version,
    )?;
    load_relationships(
        &transaction
            .query(RELATIONSHIPS_SQL, &[&revision_id])
            .map_err(|source| query_error("relationships", source))?,
        &mut models,
    )?;
    let mutation_schema_available: bool = transaction
        .query_one(MUTATION_SCHEMA_AVAILABLE_SQL, &[])
        .map_err(|source| query_error("mutation schema availability", source))?
        .get("available");
    if mutation_schema_available {
        load_writable_models(
            &transaction
                .query(MUTATION_MODELS_SQL, &[&revision_id])
                .map_err(|source| query_error("mutation models", source))?,
            &transaction
                .query(MUTATION_FIELDS_SQL, &[&revision_id])
                .map_err(|source| query_error("mutation fields", source))?,
            &mut models,
        )?;
    }
    let writable_models = if mutation_schema_available {
        if let Some(database_role) = database_role {
            transaction
                .query(MUTATION_CAPABILITIES_SQL, &[&revision_id, &database_role])
                .map_err(|source| query_error("mutation capabilities", source))?
                .into_iter()
                .map(|row| row.get("semantic_name"))
                .collect()
        } else {
            BTreeSet::new()
        }
    } else {
        BTreeSet::new()
    };

    let snapshot = SemanticSnapshot {
        schema_version,
        revision_hash: expected_hash.clone(),
        models: models.into_values().collect(),
    }
    .normalized();
    verify_hash(&snapshot, &expected_hash)?;

    transaction.commit().map_err(PublishedModelError::Commit)?;
    Ok((
        PublishedModel {
            project: project.to_owned(),
            revision_id,
            snapshot,
        },
        writable_models,
    ))
}

struct LoadedWritable {
    policy: WritableModel,
    conflict_fields: Vec<(i32, String)>,
    returning: Vec<(i32, String)>,
}

fn load_writable_models(
    model_rows: &[Row],
    field_rows: &[Row],
    models: &mut BTreeMap<String, Model>,
) -> Result<(), PublishedModelError> {
    let mut writable = BTreeMap::new();
    for row in model_rows {
        let model_id: String = row.get("model_id");
        if !models.contains_key(&model_id) || writable.contains_key(&model_id) {
            return Err(PublishedModelError::InvalidWritableMetadata);
        }
        let upsert_enabled: bool = row.get("upsert_enabled");
        writable.insert(
            model_id,
            LoadedWritable {
                policy: WritableModel {
                    insert: row.get("insert_enabled"),
                    upsert: upsert_enabled.then(|| UpsertPolicy {
                        conflict_fields: vec![],
                    }),
                    max_rows: u32::try_from(row.get::<_, i32>("max_rows"))
                        .map_err(|_| PublishedModelError::InvalidWritableMetadata)?,
                    max_request_bytes: u32::try_from(row.get::<_, i32>("max_request_bytes"))
                        .map_err(|_| PublishedModelError::InvalidWritableMetadata)?,
                    fields: vec![],
                    returning: vec![],
                },
                conflict_fields: vec![],
                returning: vec![],
            },
        );
    }

    for row in field_rows {
        let model_id: String = row.get("model_id");
        let loaded = writable
            .get_mut(&model_id)
            .ok_or(PublishedModelError::InvalidWritableMetadata)?;
        let field: String = row.get("semantic_name");
        if row.get("insertable") {
            let nullable = models
                .get(&model_id)
                .and_then(|model| {
                    model
                        .fields
                        .iter()
                        .find(|model_field| model_field.semantic_name == field)
                })
                .ok_or(PublishedModelError::InvalidWritableMetadata)?
                .nullable;
            loaded.policy.fields.push(WritableField {
                field: field.clone(),
                nullable,
                required_on_insert: row.get("required_on_insert"),
                updatable_on_conflict: row.get("updatable_on_conflict"),
            });
        }
        if let Some(ordinal) = row.get("conflict_key_ordinal") {
            loaded.conflict_fields.push((ordinal, field.clone()));
        }
        if let Some(ordinal) = row.get("returning_ordinal") {
            loaded.returning.push((ordinal, field));
        }
    }

    for (model_id, mut loaded) in writable {
        loaded.conflict_fields.sort_by_key(|(ordinal, _)| *ordinal);
        loaded.returning.sort_by_key(|(ordinal, _)| *ordinal);
        if let Some(upsert) = &mut loaded.policy.upsert {
            upsert.conflict_fields = loaded
                .conflict_fields
                .into_iter()
                .map(|(_, field)| field)
                .collect();
        } else if !loaded.conflict_fields.is_empty() {
            return Err(PublishedModelError::InvalidWritableMetadata);
        }
        loaded.policy.returning = loaded
            .returning
            .into_iter()
            .map(|(_, field)| field)
            .collect();
        models
            .get_mut(&model_id)
            .ok_or(PublishedModelError::InvalidWritableMetadata)?
            .writable = Some(loaded.policy);
    }
    Ok(())
}

fn load_models(rows: &[Row]) -> Result<BTreeMap<String, Model>, PublishedModelError> {
    let mut models = BTreeMap::new();
    for row in rows {
        let source_database: String = row.get("source_database");
        if source_database != row.get::<_, String>("current_database") {
            return Err(PublishedModelError::UnsupportedSourceDatabase(
                source_database,
            ));
        }
        models.insert(
            row.get("model_id"),
            Model {
                semantic_name: row.get("semantic_name"),
                source: Relation {
                    schema: row.get("source_schema"),
                    relation: row.get("source_relation"),
                },
                timezone: row.get("default_timezone"),
                queryable: row.get("queryable"),
                writable: None,
                fields: vec![],
                metrics: vec![],
                relationships: vec![],
            },
        );
    }
    Ok(models)
}

fn load_fields(
    rows: &[Row],
    models: &mut BTreeMap<String, Model>,
) -> Result<(), PublishedModelError> {
    for row in rows {
        let model_id: String = row.get("model_id");
        let model = models
            .get_mut(&model_id)
            .ok_or(PublishedModelError::UnknownModel)?;
        let field: String = row.get("semantic_name");
        let expression: Option<String> = row.get("expression");
        if expression.is_some() {
            return Err(PublishedModelError::UnsupportedFieldExpression {
                model: model.semantic_name.clone(),
                field,
            });
        }
        let column = row
            .get::<_, Option<String>>("source_column")
            .ok_or_else(|| PublishedModelError::MissingFieldColumn {
                model: model.semantic_name.clone(),
                field: field.clone(),
            })?;
        let relationship = parse_field_relationship(row, &model_id, &model.semantic_name, &field)?;
        let (time_dimension, entity_key) = parse_field_kind(
            &row.get::<_, String>("field_kind"),
            &model.semantic_name,
            &field,
        )?;
        model.fields.push(Field {
            semantic_name: field.clone(),
            data_type: parse_data_type(
                &row.get::<_, String>("logical_type"),
                &model.semantic_name,
                &field,
            )?,
            column,
            relationship,
            time_dimension,
            entity_key,
            visible: !row.get::<_, bool>("hidden"),
            nullable: row.get("nullable"),
        });
    }
    Ok(())
}

fn load_metrics(
    rows: &[Row],
    models: &mut BTreeMap<String, Model>,
    schema_version: &str,
) -> Result<(), PublishedModelError> {
    for row in rows {
        let model_id: String = row.get("model_id");
        let model = models
            .get_mut(&model_id)
            .ok_or(PublishedModelError::UnknownModel)?;
        let metric: String = row.get("semantic_name");
        let expression = parse_metric_expression(
            &row.get::<_, String>("expression"),
            &model.semantic_name,
            &metric,
        )?;
        let filter = row
            .get::<_, Option<String>>("metric_filter")
            .map(|value| parse_metric_filter(&value, &model.semantic_name, &metric))
            .transpose()?;
        let additivity_value: String = row.get("additivity");
        let additivity = (schema_version == "2")
            .then(|| parse_additivity(&additivity_value, &model.semantic_name, &metric))
            .transpose()?;
        let aggregation_anchor: Option<String> = row.get("aggregation_anchor");
        if schema_version == "1" && aggregation_anchor.is_some() {
            return Err(PublishedModelError::InvalidAggregationAnchor {
                model: model.semantic_name.clone(),
                metric,
            });
        }
        model.metrics.push(Metric {
            semantic_name: metric.clone(),
            data_type: parse_data_type(
                &row.get::<_, String>("result_type"),
                &model.semantic_name,
                &metric,
            )?,
            aggregation: expression.aggregation,
            field: expression.field,
            filter,
            additivity,
            aggregation_anchor,
            visible: !row.get::<_, bool>("hidden"),
        });
    }

    fn parse_additivity(
        value: &str,
        model: &str,
        metric: &str,
    ) -> Result<Additivity, PublishedModelError> {
        match value {
            "additive" => Ok(Additivity::Additive),
            "semi_additive" => Ok(Additivity::SemiAdditive),
            "non_additive" => Ok(Additivity::NonAdditive),
            _ => Err(PublishedModelError::UnsupportedAdditivity {
                model: model.to_owned(),
                metric: metric.to_owned(),
                value: value.to_owned(),
            }),
        }
    }
    Ok(())
}

fn load_relationships(
    rows: &[Row],
    models: &mut BTreeMap<String, Model>,
) -> Result<(), PublishedModelError> {
    let mut previous_key: Option<(String, String)> = None;
    for row in rows {
        let model_id: String = row.get("from_model_id");
        let model = models
            .get_mut(&model_id)
            .ok_or(PublishedModelError::UnknownModel)?;
        let relationship: String = row.get("semantic_name");
        let key = (model_id, relationship.clone());
        let ordinal: Option<i32> = row.get("ordinal");
        let from_column: Option<String> = row.get("from_column");
        let to_column: Option<String> = row.get("to_column");
        if previous_key.as_ref() == Some(&key)
            || ordinal != Some(1)
            || from_column.is_none()
            || to_column.is_none()
        {
            return Err(PublishedModelError::InvalidRelationshipColumns {
                model: model.semantic_name.clone(),
                relationship,
            });
        }
        previous_key = Some(key);
        let allowed_direction: String = row.get("allowed_direction");
        let priority: i32 = row.get("priority");
        if allowed_direction != "forward" || priority != 0 {
            return Err(PublishedModelError::UnsupportedRelationshipOptions {
                model: model.semantic_name.clone(),
                relationship,
            });
        }
        model.relationships.push(Relationship {
            semantic_name: relationship.clone(),
            target_model: row.get("target_model"),
            target: Relation {
                schema: row.get("target_schema"),
                relation: row.get("target_relation"),
            },
            cardinality: parse_cardinality(
                &row.get::<_, String>("cardinality"),
                &model.semantic_name,
                &relationship,
            )?,
            join_type: parse_join_type(
                &row.get::<_, String>("join_type"),
                &model.semantic_name,
                &relationship,
            )?,
            from_column: from_column.ok_or_else(|| {
                PublishedModelError::InvalidRelationshipColumns {
                    model: model.semantic_name.clone(),
                    relationship: relationship.clone(),
                }
            })?,
            to_column: to_column.ok_or_else(|| {
                PublishedModelError::InvalidRelationshipColumns {
                    model: model.semantic_name.clone(),
                    relationship: relationship.clone(),
                }
            })?,
        });
    }
    Ok(())
}

fn parse_field_relationship(
    row: &Row,
    model_id: &str,
    model: &str,
    field: &str,
) -> Result<Option<String>, PublishedModelError> {
    let relationship_id: Option<String> = row.get("source_relationship_id");
    match relationship_id {
        Some(_) => {
            let declared_name: Option<String> = row.get("declared_relationship_name");
            let from_model_id: Option<String> = row.get("relationship_from_model_id");
            if from_model_id.as_deref() != Some(model_id) {
                return Err(PublishedModelError::InvalidFieldRelationship {
                    model: model.to_owned(),
                    field: field.to_owned(),
                });
            }
            declared_name
                .map(Some)
                .ok_or_else(|| PublishedModelError::InvalidFieldRelationship {
                    model: model.to_owned(),
                    field: field.to_owned(),
                })
        }
        None => Ok(None),
    }
}

fn parse_field_kind(
    value: &str,
    model: &str,
    field: &str,
) -> Result<(bool, bool), PublishedModelError> {
    match value {
        "dimension" => Ok((false, false)),
        "entity_key" => Ok((false, true)),
        "time_dimension" => Ok((true, false)),
        _ => Err(PublishedModelError::UnsupportedFieldKind {
            model: model.to_owned(),
            field: field.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn parse_data_type(value: &str, model: &str, field: &str) -> Result<DataType, PublishedModelError> {
    match value {
        "boolean" => Ok(DataType::Boolean),
        "integer" => Ok(DataType::Integer),
        "numeric" => Ok(DataType::Numeric),
        "text" => Ok(DataType::Text),
        "date" => Ok(DataType::Date),
        "timestamp" => Ok(DataType::Timestamp),
        "timestamp_tz" => Ok(DataType::TimestampTz),
        _ => Err(PublishedModelError::UnsupportedLogicalType {
            model: model.to_owned(),
            field: field.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn parse_metric_expression(
    value: &str,
    model: &str,
    metric: &str,
) -> Result<AggregationExpression, PublishedModelError> {
    let expression: AggregationExpression = serde_json::from_str(value).map_err(|source| {
        PublishedModelError::InvalidMetricExpression {
            model: model.to_owned(),
            metric: metric.to_owned(),
            source,
        }
    })?;
    if expression.version != "1" {
        return Err(PublishedModelError::UnsupportedMetricExpressionVersion {
            model: model.to_owned(),
            metric: metric.to_owned(),
            version: expression.version,
        });
    }
    let AggregationExpressionKind::Aggregation = expression.kind;
    Ok(expression)
}

fn parse_metric_filter(
    value: &str,
    model: &str,
    metric: &str,
) -> Result<MetricFilter, PublishedModelError> {
    serde_json::from_str(value).map_err(|source| PublishedModelError::InvalidMetricFilter {
        model: model.to_owned(),
        metric: metric.to_owned(),
        source,
    })
}

fn parse_cardinality(
    value: &str,
    model: &str,
    relationship: &str,
) -> Result<Cardinality, PublishedModelError> {
    match value {
        "one_to_one" => Ok(Cardinality::OneToOne),
        "many_to_one" => Ok(Cardinality::ManyToOne),
        "one_to_many" => Ok(Cardinality::OneToMany),
        "many_to_many" => Ok(Cardinality::ManyToMany),
        _ => Err(PublishedModelError::UnsupportedCardinality {
            model: model.to_owned(),
            relationship: relationship.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn parse_join_type(
    value: &str,
    model: &str,
    relationship: &str,
) -> Result<JoinType, PublishedModelError> {
    match value {
        "inner" => Ok(JoinType::Inner),
        "left" => Ok(JoinType::Left),
        _ => Err(PublishedModelError::UnsupportedJoinType {
            model: model.to_owned(),
            relationship: relationship.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn verify_hash(snapshot: &SemanticSnapshot, expected: &str) -> Result<(), PublishedModelError> {
    let actual = snapshot.calculate_revision_hash()?;
    if actual != expected {
        return Err(PublishedModelError::HashMismatch {
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

fn query_error(operation: &'static str, source: postgres::Error) -> PublishedModelError {
    PublishedModelError::Query { operation, source }
}

#[cfg(test)]
mod tests {
    use postgresem_compiler::SemanticSnapshot;

    use super::{
        PublishedModelError, parse_cardinality, parse_data_type, parse_join_type,
        parse_metric_expression, parse_metric_filter, verify_hash,
    };

    #[test]
    fn parsers_reject_unknown_database_values() {
        assert!(matches!(
            parse_data_type("money", "orders", "amount"),
            Err(PublishedModelError::UnsupportedLogicalType { .. })
        ));
        assert!(matches!(
            parse_cardinality("sometimes", "orders", "customer"),
            Err(PublishedModelError::UnsupportedCardinality { .. })
        ));
        assert!(matches!(
            parse_join_type("right", "orders", "customer"),
            Err(PublishedModelError::UnsupportedJoinType { .. })
        ));
    }

    #[test]
    fn aggregation_metadata_is_strict_and_versioned() {
        assert!(matches!(
            parse_metric_expression(
                r#"{"version":"1","kind":"aggregation","aggregation":"sum","field":"amount","sql":"amount"}"#,
                "orders",
                "revenue"
            ),
            Err(PublishedModelError::InvalidMetricExpression { .. })
        ));
        assert!(matches!(
            parse_metric_expression(
                r#"{"version":"2","kind":"aggregation","aggregation":"sum","field":"amount"}"#,
                "orders",
                "revenue"
            ),
            Err(PublishedModelError::UnsupportedMetricExpressionVersion { .. })
        ));
        assert!(matches!(
            parse_metric_filter(
                r#"{"field":"status","value":{"type":"raw_sql","value":"status = 'paid'"}}"#,
                "orders",
                "revenue"
            ),
            Err(PublishedModelError::InvalidMetricFilter { .. })
        ));
    }

    #[test]
    fn revision_hash_mismatch_fails_closed() {
        let snapshot = SemanticSnapshot {
            schema_version: "1".to_owned(),
            revision_hash:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            models: vec![],
        };
        assert!(matches!(
            verify_hash(&snapshot, &snapshot.revision_hash),
            Err(PublishedModelError::HashMismatch { .. })
        ));
    }
}
