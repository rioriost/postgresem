use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use thiserror::Error;

use crate::{
    Aggregation, Cardinality, DataType, Field, Filter, JoinType, Literal, Metric, Model,
    NormalizedLsq, OrderBy, Relationship, SemanticSnapshot, SortDirection, TimeGrain, hash::sha256,
};

pub const COMPILER_SEMANTIC_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerOptions {
    pub default_limit: u32,
    pub hard_limit: u32,
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            default_limit: 100,
            hard_limit: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompiledQuery {
    pub sql: String,
    pub parameters: Vec<CompiledParameter>,
    pub output_schema: Vec<OutputColumn>,
    pub lineage: Lineage,
    pub query_hash: String,
    pub sql_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompiledParameter {
    pub position: usize,
    pub data_type: DataType,
    pub value: Literal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputColumn {
    pub name: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lineage {
    pub models: Vec<String>,
    pub metrics: Vec<String>,
    pub relationships: Vec<String>,
    pub source_columns: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompileError {
    #[error("unsupported semantic snapshot schema version: {0}")]
    UnsupportedSnapshotVersion(String),
    #[error("semantic revision hash is invalid")]
    InvalidRevisionHash,
    #[error("semantic model is not available")]
    UnknownModel,
    #[error("semantic field is not available: {0}")]
    UnknownField(String),
    #[error("semantic metric is not available: {0}")]
    UnknownMetric(String),
    #[error("semantic field is not visible: {0}")]
    HiddenField(String),
    #[error("semantic metric is not visible: {0}")]
    HiddenMetric(String),
    #[error("time grain is not valid for field: {0}")]
    InvalidTimeGrain(String),
    #[error("literal type is not compatible with field: {0}")]
    LiteralTypeMismatch(String),
    #[error("order reference is not projected: {0}")]
    UnknownOrderReference(String),
    #[error("relationship is not defined: {0}")]
    UnknownRelationship(String),
    #[error("relationship cardinality is unsafe for MVP: {0}")]
    UnsafeRelationship(String),
    #[error("metric source field is invalid: {0}")]
    InvalidMetricField(String),
    #[error("metric cannot aggregate a joined field in MVP: {0}")]
    JoinedMetricField(String),
    #[error("count_distinct requires an entity key: {0}")]
    CountDistinctRequiresEntityKey(String),
    #[error("count requires an entity key: {0}")]
    CountRequiresEntityKey(String),
    #[error("metric output type is inconsistent with its aggregation: {0}")]
    InvalidMetricType(String),
    #[error("relationship target model is invalid: {0}")]
    InvalidRelationshipTarget(String),
    #[error("relationship target is not an entity key: {0}")]
    RelationshipTargetNotEntityKey(String),
    #[error("model timezone is required for timestamp with time zone field: {0}")]
    MissingTimezone(String),
    #[error("failed to serialize compiler hash inputs")]
    HashSerialization,
    #[error("compiler limit configuration is invalid")]
    InvalidLimitConfiguration,
    #[error("query limit exceeds compiler hard limit")]
    LimitExceeded,
}

impl CompileError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSnapshotVersion(_) => "SEMANTIC_UNSUPPORTED_SNAPSHOT_VERSION",
            Self::InvalidRevisionHash => "SEMANTIC_INVALID_REVISION_HASH",
            Self::UnknownModel => "SEMANTIC_MODEL_NOT_AVAILABLE",
            Self::UnknownField(_) => "SEMANTIC_FIELD_NOT_AVAILABLE",
            Self::UnknownMetric(_) => "SEMANTIC_METRIC_NOT_AVAILABLE",
            Self::HiddenField(_) => "SEMANTIC_FIELD_NOT_AVAILABLE",
            Self::HiddenMetric(_) => "SEMANTIC_METRIC_NOT_AVAILABLE",
            Self::InvalidTimeGrain(_) => "SEMANTIC_INVALID_TIME_GRAIN",
            Self::LiteralTypeMismatch(_) => "SEMANTIC_LITERAL_TYPE_MISMATCH",
            Self::UnknownOrderReference(_) => "SEMANTIC_ORDER_REFERENCE_NOT_PROJECTED",
            Self::UnknownRelationship(_) => "SEMANTIC_RELATIONSHIP_NOT_AVAILABLE",
            Self::UnsafeRelationship(_) => "SEMANTIC_UNSAFE_RELATIONSHIP",
            Self::InvalidMetricField(_) => "SEMANTIC_INVALID_METRIC_FIELD",
            Self::JoinedMetricField(_) => "SEMANTIC_JOINED_METRIC_FIELD",
            Self::CountDistinctRequiresEntityKey(_) => {
                "SEMANTIC_COUNT_DISTINCT_REQUIRES_ENTITY_KEY"
            }
            Self::CountRequiresEntityKey(_) => "SEMANTIC_COUNT_REQUIRES_ENTITY_KEY",
            Self::InvalidMetricType(_) => "SEMANTIC_INVALID_METRIC_TYPE",
            Self::InvalidRelationshipTarget(_) => "SEMANTIC_INVALID_RELATIONSHIP_TARGET",
            Self::RelationshipTargetNotEntityKey(_) => {
                "SEMANTIC_RELATIONSHIP_TARGET_NOT_ENTITY_KEY"
            }
            Self::MissingTimezone(_) => "SEMANTIC_MISSING_TIMEZONE",
            Self::HashSerialization => "COMPILER_HASH_SERIALIZATION_FAILED",
            Self::InvalidLimitConfiguration => "COMPILER_INVALID_LIMIT_CONFIGURATION",
            Self::LimitExceeded => "COMPILER_LIMIT_EXCEEDED",
        }
    }
}

#[derive(Debug, Clone)]
struct ValidatedQuery {
    model: Model,
    dimensions: Vec<ResolvedDimension>,
    metrics: Vec<ResolvedMetric>,
    filter: Option<ResolvedFilter>,
    order_by: Vec<OrderBy>,
    relationships: BTreeMap<String, Relationship>,
    limit: u32,
}

#[derive(Debug, Clone)]
struct ResolvedDimension {
    field: Field,
    time_grain: Option<TimeGrain>,
}

#[derive(Debug, Clone)]
struct ResolvedMetric {
    metric: Metric,
    field: Field,
    filter: Option<(Field, Literal)>,
}

#[derive(Debug, Clone)]
enum ResolvedFilter {
    And(Vec<ResolvedFilter>),
    Or(Vec<ResolvedFilter>),
    Not(Box<ResolvedFilter>),
    Comparison {
        operator: ComparisonOperator,
        field: Field,
        value: Literal,
    },
    In {
        field: Field,
        values: Vec<Literal>,
    },
    IsNull(Field),
    IsNotNull(Field),
}

#[derive(Debug, Clone, Copy)]
enum ComparisonOperator {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
}

pub fn compile_lsq(
    normalized: &NormalizedLsq,
    snapshot: &SemanticSnapshot,
    options: CompilerOptions,
) -> Result<CompiledQuery, CompileError> {
    let validated = validate(normalized, snapshot, options)?;
    render(normalized, snapshot, validated)
}

fn validate(
    normalized: &NormalizedLsq,
    snapshot: &SemanticSnapshot,
    options: CompilerOptions,
) -> Result<ValidatedQuery, CompileError> {
    if snapshot.schema_version != "1" {
        return Err(CompileError::UnsupportedSnapshotVersion(
            snapshot.schema_version.clone(),
        ));
    }
    if !is_sha256(&snapshot.revision_hash)
        || snapshot
            .calculate_revision_hash()
            .map_err(|_| CompileError::HashSerialization)?
            != snapshot.revision_hash
    {
        return Err(CompileError::InvalidRevisionHash);
    }
    if options.default_limit == 0
        || options.hard_limit == 0
        || options.default_limit > options.hard_limit
    {
        return Err(CompileError::InvalidLimitConfiguration);
    }

    let query = &normalized.query;
    let model = snapshot
        .models
        .iter()
        .find(|model| model.semantic_name == query.model && model.queryable)
        .cloned()
        .ok_or(CompileError::UnknownModel)?;
    let limit = query.limit.unwrap_or(options.default_limit);
    if limit > options.hard_limit {
        return Err(CompileError::LimitExceeded);
    }

    let mut required_relationships = BTreeSet::new();
    let dimensions = query
        .dimensions
        .iter()
        .map(|dimension| {
            let field = resolve_field(&model, &dimension.field)?;
            if !field.visible {
                return Err(CompileError::HiddenField(dimension.field.clone()));
            }
            if dimension.time_grain.is_some()
                && (!field.time_dimension
                    || !matches!(
                        field.data_type,
                        DataType::Date | DataType::Timestamp | DataType::TimestampTz
                    ))
            {
                return Err(CompileError::InvalidTimeGrain(dimension.field.clone()));
            }
            record_relationship(&field, &mut required_relationships);
            Ok(ResolvedDimension {
                field,
                time_grain: dimension.time_grain,
            })
        })
        .collect::<Result<Vec<_>, CompileError>>()?;

    let metrics = query
        .metrics
        .iter()
        .map(|reference| {
            let metric = model
                .metrics
                .iter()
                .find(|metric| metric.semantic_name == reference.metric)
                .cloned()
                .ok_or_else(|| CompileError::UnknownMetric(reference.metric.clone()))?;
            if !metric.visible {
                return Err(CompileError::HiddenMetric(reference.metric.clone()));
            }
            let field = resolve_field(&model, &metric.field)
                .map_err(|_| CompileError::InvalidMetricField(metric.semantic_name.clone()))?;
            if field.relationship.is_some() {
                return Err(CompileError::JoinedMetricField(
                    metric.semantic_name.clone(),
                ));
            }
            if metric.aggregation == Aggregation::Count && !field.entity_key {
                return Err(CompileError::CountRequiresEntityKey(
                    metric.semantic_name.clone(),
                ));
            }
            if metric.aggregation == Aggregation::CountDistinct && !field.entity_key {
                return Err(CompileError::CountDistinctRequiresEntityKey(
                    metric.semantic_name.clone(),
                ));
            }
            if metric.data_type != expected_metric_type(metric.aggregation, field.data_type) {
                return Err(CompileError::InvalidMetricType(
                    metric.semantic_name.clone(),
                ));
            }
            record_relationship(&field, &mut required_relationships);

            let filter = metric
                .filter
                .as_ref()
                .map(|filter| {
                    let filter_field = resolve_field(&model, &filter.field).map_err(|_| {
                        CompileError::InvalidMetricField(metric.semantic_name.clone())
                    })?;
                    validate_literal(&filter_field, &filter.value)?;
                    record_relationship(&filter_field, &mut required_relationships);
                    Ok((filter_field, filter.value.clone()))
                })
                .transpose()?;

            Ok(ResolvedMetric {
                metric,
                field,
                filter,
            })
        })
        .collect::<Result<Vec<_>, CompileError>>()?;

    let filter = query
        .filters
        .as_ref()
        .map(|filter| resolve_filter(&model, filter, &mut required_relationships))
        .transpose()?;

    let projected = query
        .dimensions
        .iter()
        .map(|dimension| dimension.field.as_str())
        .chain(query.metrics.iter().map(|metric| metric.metric.as_str()))
        .collect::<BTreeSet<_>>();
    for order_by in &query.order_by {
        if !projected.contains(order_by.output_reference.as_str()) {
            return Err(CompileError::UnknownOrderReference(
                order_by.output_reference.clone(),
            ));
        }
    }

    let relationships = required_relationships
        .into_iter()
        .map(|name| {
            let relationship = model
                .relationships
                .iter()
                .find(|relationship| relationship.semantic_name == name)
                .cloned()
                .ok_or_else(|| CompileError::UnknownRelationship(name.clone()))?;
            if !matches!(
                relationship.cardinality,
                Cardinality::ManyToOne | Cardinality::OneToOne
            ) {
                return Err(CompileError::UnsafeRelationship(name));
            }
            let target_model = snapshot
                .models
                .iter()
                .find(|model| model.semantic_name == relationship.target_model)
                .ok_or_else(|| {
                    CompileError::InvalidRelationshipTarget(relationship.semantic_name.clone())
                })?;
            if target_model.source != relationship.target {
                return Err(CompileError::InvalidRelationshipTarget(
                    relationship.semantic_name.clone(),
                ));
            }
            if !target_model.fields.iter().any(|field| {
                field.column == relationship.to_column
                    && field.relationship.is_none()
                    && field.entity_key
            }) {
                return Err(CompileError::RelationshipTargetNotEntityKey(
                    relationship.semantic_name.clone(),
                ));
            }
            Ok((relationship.semantic_name.clone(), relationship))
        })
        .collect::<Result<BTreeMap<_, _>, CompileError>>()?;

    Ok(ValidatedQuery {
        model,
        dimensions,
        metrics,
        filter,
        order_by: query.order_by.clone(),
        relationships,
        limit,
    })
}

fn resolve_field(model: &Model, name: &str) -> Result<Field, CompileError> {
    model
        .fields
        .iter()
        .find(|field| field.semantic_name == name)
        .cloned()
        .ok_or_else(|| CompileError::UnknownField(name.to_owned()))
}

fn record_relationship(field: &Field, relationships: &mut BTreeSet<String>) {
    if let Some(relationship) = &field.relationship {
        relationships.insert(relationship.clone());
    }
}

fn resolve_filter(
    model: &Model,
    filter: &Filter,
    relationships: &mut BTreeSet<String>,
) -> Result<ResolvedFilter, CompileError> {
    match filter {
        Filter::And { args } => Ok(ResolvedFilter::And(
            args.iter()
                .map(|arg| resolve_filter(model, arg, relationships))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Filter::Or { args } => Ok(ResolvedFilter::Or(
            args.iter()
                .map(|arg| resolve_filter(model, arg, relationships))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Filter::Not { arg } => Ok(ResolvedFilter::Not(Box::new(resolve_filter(
            model,
            arg,
            relationships,
        )?))),
        Filter::Eq { field, value } => {
            resolve_comparison(model, field, value, ComparisonOperator::Eq, relationships)
        }
        Filter::NotEq { field, value } => resolve_comparison(
            model,
            field,
            value,
            ComparisonOperator::NotEq,
            relationships,
        ),
        Filter::Gt { field, value } => {
            resolve_comparison(model, field, value, ComparisonOperator::Gt, relationships)
        }
        Filter::Gte { field, value } => {
            resolve_comparison(model, field, value, ComparisonOperator::Gte, relationships)
        }
        Filter::Lt { field, value } => {
            resolve_comparison(model, field, value, ComparisonOperator::Lt, relationships)
        }
        Filter::Lte { field, value } => {
            resolve_comparison(model, field, value, ComparisonOperator::Lte, relationships)
        }
        Filter::In { field, values } => {
            let resolved = visible_field(model, field)?;
            for value in values {
                validate_literal(&resolved, value)?;
            }
            record_relationship(&resolved, relationships);
            Ok(ResolvedFilter::In {
                field: resolved,
                values: values.clone(),
            })
        }
        Filter::IsNull { field } => {
            let resolved = visible_field(model, field)?;
            record_relationship(&resolved, relationships);
            Ok(ResolvedFilter::IsNull(resolved))
        }
        Filter::IsNotNull { field } => {
            let resolved = visible_field(model, field)?;
            record_relationship(&resolved, relationships);
            Ok(ResolvedFilter::IsNotNull(resolved))
        }
    }
}

fn resolve_comparison(
    model: &Model,
    field: &str,
    value: &Literal,
    operator: ComparisonOperator,
    relationships: &mut BTreeSet<String>,
) -> Result<ResolvedFilter, CompileError> {
    let resolved = visible_field(model, field)?;
    validate_literal(&resolved, value)?;
    record_relationship(&resolved, relationships);
    Ok(ResolvedFilter::Comparison {
        operator,
        field: resolved,
        value: value.clone(),
    })
}

fn visible_field(model: &Model, name: &str) -> Result<Field, CompileError> {
    let field = resolve_field(model, name)?;
    if !field.visible {
        return Err(CompileError::HiddenField(name.to_owned()));
    }
    Ok(field)
}

fn validate_literal(field: &Field, literal: &Literal) -> Result<(), CompileError> {
    let compatible = matches!(
        (field.data_type, literal),
        (DataType::Boolean, Literal::Boolean(_))
            | (DataType::Integer, Literal::Integer(_))
            | (DataType::Numeric, Literal::Integer(_) | Literal::Numeric(_))
            | (DataType::Text, Literal::Text(_))
            | (DataType::Date, Literal::Date(_))
            | (DataType::Timestamp, Literal::Timestamp(_))
            | (
                DataType::TimestampTz,
                Literal::Date(_) | Literal::Timestamp(_)
            )
    );
    if compatible {
        Ok(())
    } else {
        Err(CompileError::LiteralTypeMismatch(
            field.semantic_name.clone(),
        ))
    }
}

fn render(
    normalized: &NormalizedLsq,
    snapshot: &SemanticSnapshot,
    validated: ValidatedQuery,
) -> Result<CompiledQuery, CompileError> {
    let relationship_aliases = validated
        .relationships
        .keys()
        .enumerate()
        .map(|(index, name)| (name.clone(), format!("t{}", index + 1)))
        .collect::<BTreeMap<_, _>>();
    let mut parameters = Vec::new();
    let mut source_columns = BTreeSet::new();
    let mut select = Vec::new();
    let mut output_schema = Vec::new();

    for dimension in &validated.dimensions {
        let expression = render_dimension(
            &validated.model,
            dimension,
            &relationship_aliases,
            &mut parameters,
            &mut source_columns,
        )?;
        select.push(format!(
            "{expression} AS {}",
            quote_identifier(&dimension.field.semantic_name)
        ));
        output_schema.push(OutputColumn {
            name: dimension.field.semantic_name.clone(),
            data_type: if dimension.time_grain.is_some() {
                DataType::Date
            } else {
                dimension.field.data_type
            },
        });
    }

    for metric in &validated.metrics {
        let expression = render_metric(
            &validated.model,
            metric,
            &relationship_aliases,
            &mut parameters,
            &mut source_columns,
        )?;
        select.push(format!(
            "{expression} AS {}",
            quote_identifier(&metric.metric.semantic_name)
        ));
        output_schema.push(OutputColumn {
            name: metric.metric.semantic_name.clone(),
            data_type: metric.metric.data_type,
        });
    }

    let mut sql = format!(
        "SELECT {}\nFROM {}.{} AS t0",
        select.join(", "),
        quote_identifier(&validated.model.source.schema),
        quote_identifier(&validated.model.source.relation)
    );

    for (name, relationship) in &validated.relationships {
        let alias = relationship_aliases
            .get(name)
            .ok_or_else(|| CompileError::UnknownRelationship(name.clone()))?;
        let join = match relationship.join_type {
            JoinType::Inner => "INNER JOIN",
            JoinType::Left => "LEFT JOIN",
        };
        sql.push_str(&format!(
            "\n{join} {}.{} AS {alias} ON t0.{} = {alias}.{}",
            quote_identifier(&relationship.target.schema),
            quote_identifier(&relationship.target.relation),
            quote_identifier(&relationship.from_column),
            quote_identifier(&relationship.to_column)
        ));
        source_columns.insert(source_column(
            &validated.model.source,
            &relationship.from_column,
        ));
        source_columns.insert(source_column(&relationship.target, &relationship.to_column));
    }

    if let Some(filter) = &validated.filter {
        let predicate = render_filter(
            &validated.model,
            filter,
            &relationship_aliases,
            &mut parameters,
            &mut source_columns,
        )?;
        sql.push_str("\nWHERE ");
        sql.push_str(&predicate);
    }

    if !validated.dimensions.is_empty() {
        let positions = (1..=validated.dimensions.len())
            .map(|position| position.to_string())
            .collect::<Vec<_>>();
        sql.push_str(&format!("\nGROUP BY {}", positions.join(", ")));
    }

    if !validated.order_by.is_empty() {
        let order_by = validated
            .order_by
            .iter()
            .map(|order| {
                let direction = match order.direction {
                    SortDirection::Asc => "ASC",
                    SortDirection::Desc => "DESC",
                };
                format!("{} {direction}", quote_identifier(&order.output_reference))
            })
            .collect::<Vec<_>>();
        sql.push_str(&format!("\nORDER BY {}", order_by.join(", ")));
    }

    let limit_position = push_parameter(
        &mut parameters,
        DataType::Integer,
        Literal::Integer(i64::from(validated.limit)),
    );
    sql.push_str(&format!("\nLIMIT ${limit_position}::text::bigint"));

    let sql_hash = hash(&sql);
    let parameter_hash_input =
        serde_json::to_string(&parameters).map_err(|_| CompileError::HashSerialization)?;
    let query_hash = hash(&format!(
        "{}|{}|{}|{}|{}",
        normalized.hash,
        snapshot.revision_hash,
        COMPILER_SEMANTIC_VERSION,
        sql_hash,
        parameter_hash_input
    ));
    let mut models = vec![validated.model.semantic_name.clone()];
    models.extend(
        validated
            .relationships
            .values()
            .map(|relationship| relationship.target_model.clone()),
    );
    models.sort();
    models.dedup();

    Ok(CompiledQuery {
        sql,
        parameters,
        output_schema,
        lineage: Lineage {
            models,
            metrics: validated
                .metrics
                .iter()
                .map(|metric| metric.metric.semantic_name.clone())
                .collect(),
            relationships: validated.relationships.keys().cloned().collect(),
            source_columns: source_columns.into_iter().collect(),
        },
        query_hash,
        sql_hash,
    })
}

fn render_dimension(
    model: &Model,
    dimension: &ResolvedDimension,
    aliases: &BTreeMap<String, String>,
    parameters: &mut Vec<CompiledParameter>,
    source_columns: &mut BTreeSet<String>,
) -> Result<String, CompileError> {
    let field = render_field(model, &dimension.field, aliases, source_columns)?;
    Ok(match dimension.time_grain {
        Some(grain) if dimension.field.data_type == DataType::TimestampTz => {
            let timezone = model.timezone.as_ref().ok_or_else(|| {
                CompileError::MissingTimezone(dimension.field.semantic_name.clone())
            })?;
            let position =
                push_parameter(parameters, DataType::Text, Literal::Text(timezone.clone()));
            format!(
                "date_trunc('{}', timezone(${position}::text, {field}))::date",
                time_grain(grain)
            )
        }
        Some(grain) => format!("date_trunc('{}', {field})::date", time_grain(grain)),
        None => field,
    })
}

fn render_metric(
    model: &Model,
    metric: &ResolvedMetric,
    aliases: &BTreeMap<String, String>,
    parameters: &mut Vec<CompiledParameter>,
    source_columns: &mut BTreeSet<String>,
) -> Result<String, CompileError> {
    let field = render_field(model, &metric.field, aliases, source_columns)?;
    let aggregate = match metric.metric.aggregation {
        Aggregation::Count => format!("count({field})"),
        Aggregation::CountDistinct => format!("count(DISTINCT {field})"),
        Aggregation::Sum => format!("sum({field})"),
        Aggregation::Min => format!("min({field})"),
        Aggregation::Max => format!("max({field})"),
        Aggregation::Avg => format!("avg({field})"),
    };

    if let Some((filter_field, value)) = &metric.filter {
        let filter_expression = render_field(model, filter_field, aliases, source_columns)?;
        let position = push_parameter(parameters, filter_field.data_type, value.clone());
        Ok(format!(
            "{aggregate} FILTER (WHERE {filter_expression} = {})",
            render_parameter(position, filter_field.data_type)
        ))
    } else {
        Ok(aggregate)
    }
}

fn render_filter(
    model: &Model,
    filter: &ResolvedFilter,
    aliases: &BTreeMap<String, String>,
    parameters: &mut Vec<CompiledParameter>,
    source_columns: &mut BTreeSet<String>,
) -> Result<String, CompileError> {
    match filter {
        ResolvedFilter::And(args) => {
            render_logical(model, args, "AND", aliases, parameters, source_columns)
        }
        ResolvedFilter::Or(args) => {
            render_logical(model, args, "OR", aliases, parameters, source_columns)
        }
        ResolvedFilter::Not(arg) => Ok(format!(
            "NOT ({})",
            render_filter(model, arg, aliases, parameters, source_columns)?
        )),
        ResolvedFilter::Comparison {
            operator,
            field,
            value,
        } => {
            let field_type = field.data_type;
            let field_name = field.semantic_name.clone();
            let rendered_field = render_field(model, field, aliases, source_columns)?;
            if field_type == DataType::TimestampTz && matches!(value, Literal::Date(_)) {
                let timezone = model
                    .timezone
                    .as_ref()
                    .ok_or(CompileError::MissingTimezone(field_name))?;
                let timezone_position =
                    push_parameter(parameters, DataType::Text, Literal::Text(timezone.clone()));
                let value_position = push_literal(parameters, value.clone());
                Ok(format!(
                    "{rendered_field} {} timezone(${timezone_position}::text, ${value_position}::text::date::timestamp)",
                    comparison_operator(*operator)
                ))
            } else {
                let position = push_parameter(parameters, field_type, value.clone());
                Ok(format!(
                    "{rendered_field} {} {}",
                    comparison_operator(*operator),
                    render_parameter(position, field_type)
                ))
            }
        }
        ResolvedFilter::In { field, values } => {
            let field_type = field.data_type;
            let field = render_field(model, field, aliases, source_columns)?;
            let placeholders = values
                .iter()
                .map(|value| {
                    let position = push_parameter(parameters, field_type, value.clone());
                    render_parameter(position, field_type)
                })
                .collect::<Vec<_>>();
            Ok(format!("{field} IN ({})", placeholders.join(", ")))
        }
        ResolvedFilter::IsNull(field) => Ok(format!(
            "{} IS NULL",
            render_field(model, field, aliases, source_columns)?
        )),
        ResolvedFilter::IsNotNull(field) => Ok(format!(
            "{} IS NOT NULL",
            render_field(model, field, aliases, source_columns)?
        )),
    }
}

fn render_logical(
    model: &Model,
    args: &[ResolvedFilter],
    operator: &str,
    aliases: &BTreeMap<String, String>,
    parameters: &mut Vec<CompiledParameter>,
    source_columns: &mut BTreeSet<String>,
) -> Result<String, CompileError> {
    let rendered = args
        .iter()
        .map(|arg| render_filter(model, arg, aliases, parameters, source_columns))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format!("({})", rendered.join(&format!(" {operator} "))))
}

fn render_field(
    model: &Model,
    field: &Field,
    aliases: &BTreeMap<String, String>,
    source_columns: &mut BTreeSet<String>,
) -> Result<String, CompileError> {
    let (alias, relation) = if let Some(relationship_name) = &field.relationship {
        let relationship = model
            .relationships
            .iter()
            .find(|relationship| relationship.semantic_name == *relationship_name)
            .ok_or_else(|| CompileError::UnknownRelationship(relationship_name.clone()))?;
        let alias = aliases
            .get(relationship_name)
            .ok_or_else(|| CompileError::UnknownRelationship(relationship_name.clone()))?;
        (alias.as_str(), &relationship.target)
    } else {
        ("t0", &model.source)
    };
    source_columns.insert(source_column(relation, &field.column));
    Ok(format!("{alias}.{}", quote_identifier(&field.column)))
}

fn push_literal(parameters: &mut Vec<CompiledParameter>, value: Literal) -> usize {
    push_parameter(parameters, literal_type(&value), value)
}

fn push_parameter(
    parameters: &mut Vec<CompiledParameter>,
    data_type: DataType,
    value: Literal,
) -> usize {
    if let Some(existing) = parameters
        .iter()
        .find(|parameter| parameter.data_type == data_type && parameter.value == value)
    {
        return existing.position;
    }
    let position = parameters.len() + 1;
    parameters.push(CompiledParameter {
        position,
        data_type,
        value,
    });
    position
}

const fn literal_type(literal: &Literal) -> DataType {
    match literal {
        Literal::Text(_) => DataType::Text,
        Literal::Boolean(_) => DataType::Boolean,
        Literal::Integer(_) => DataType::Integer,
        Literal::Numeric(_) => DataType::Numeric,
        Literal::Date(_) => DataType::Date,
        Literal::Timestamp(_) => DataType::TimestampTz,
    }
}

const fn postgres_type(data_type: DataType) -> &'static str {
    match data_type {
        DataType::Boolean => "boolean",
        DataType::Integer => "bigint",
        DataType::Numeric => "numeric",
        DataType::Text => "text",
        DataType::Date => "date",
        DataType::Timestamp => "timestamp",
        DataType::TimestampTz => "timestamptz",
    }
}

fn render_parameter(position: usize, data_type: DataType) -> String {
    if data_type == DataType::Text {
        format!("${position}::text")
    } else {
        format!("${position}::text::{}", postgres_type(data_type))
    }
}

const fn comparison_operator(operator: ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Eq => "=",
        ComparisonOperator::NotEq => "<>",
        ComparisonOperator::Gt => ">",
        ComparisonOperator::Gte => ">=",
        ComparisonOperator::Lt => "<",
        ComparisonOperator::Lte => "<=",
    }
}

const fn time_grain(grain: TimeGrain) -> &'static str {
    match grain {
        TimeGrain::Day => "day",
        TimeGrain::Week => "week",
        TimeGrain::Month => "month",
        TimeGrain::Quarter => "quarter",
        TimeGrain::Year => "year",
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn source_column(relation: &crate::Relation, column: &str) -> String {
    format!("{}.{}.{}", relation.schema, relation.relation, column)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hash(value: &str) -> String {
    sha256(value)
}

fn expected_metric_type(aggregation: Aggregation, field_type: DataType) -> DataType {
    match aggregation {
        Aggregation::Count | Aggregation::CountDistinct => DataType::Integer,
        Aggregation::Avg => DataType::Numeric,
        Aggregation::Sum if field_type == DataType::Integer => DataType::Numeric,
        Aggregation::Sum | Aggregation::Min | Aggregation::Max => field_type,
    }
}

#[cfg(test)]
mod tests {
    use crate::{CompilerOptions, SemanticSnapshot, compile_lsq, normalize_lsq};

    fn snapshot() -> SemanticSnapshot {
        serde_json::from_str(include_str!(
            "../../../fixtures/evals/m0-semantic-snapshot.json"
        ))
        .expect("valid semantic snapshot fixture")
    }

    #[test]
    fn compiles_revenue_by_month_with_stable_sql() {
        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"orders",
                "dimensions":[{"field":"ordered_at","time_grain":"month"}],
                "metrics":[{"metric":"revenue"}],
                "filters":{"op":"gte","field":"ordered_at","value":{"type":"date","value":"2026-01-01"}},
                "order_by":[{"ref":"revenue","direction":"desc"}],
                "limit":100
            }"#,
        )
        .expect("valid LSQ");

        let compiled =
            compile_lsq(&query, &snapshot(), CompilerOptions::default()).expect("compiles");

        assert_eq!(
            compiled.sql,
            concat!(
                "SELECT date_trunc('month', timezone($1::text, t0.\"ordered_at\"))::date AS \"ordered_at\", ",
                "sum(t0.\"amount\") FILTER (WHERE t0.\"status\" = $2::text) AS \"revenue\"\n",
                "FROM \"commerce\".\"orders\" AS t0\n",
                "WHERE t0.\"ordered_at\" >= timezone($1::text, $3::text::date::timestamp)\n",
                "GROUP BY 1\n",
                "ORDER BY \"revenue\" DESC\n",
                "LIMIT $4::text::bigint"
            )
        );
        assert_eq!(compiled.parameters.len(), 4);
        assert_eq!(
            compiled.sql_hash,
            "sha256:1c9dedaf4401b65e64a3244ae7257d6521f049ec9fa488df9c01fd0fe335ef54"
        );
        assert_eq!(
            compiled.lineage.source_columns,
            [
                "commerce.orders.amount",
                "commerce.orders.ordered_at",
                "commerce.orders.status"
            ]
        );
    }

    #[test]
    fn adds_a_stable_many_to_one_join() {
        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"orders",
                "dimensions":[{"field":"customer_region"}],
                "metrics":[{"metric":"revenue"}]
            }"#,
        )
        .expect("valid LSQ");

        let compiled =
            compile_lsq(&query, &snapshot(), CompilerOptions::default()).expect("compiles");

        assert!(compiled.sql.contains(
            "LEFT JOIN \"commerce\".\"customer\" AS t1 ON t0.\"customer_id\" = t1.\"customer_id\""
        ));
        assert_eq!(compiled.lineage.relationships, ["customer"]);
    }

    #[test]
    fn groups_dimension_only_queries() {
        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"orders",
                "dimensions":[{"field":"status"}]
            }"#,
        )
        .expect("valid LSQ");

        let compiled =
            compile_lsq(&query, &snapshot(), CompilerOptions::default()).expect("compiles");

        assert!(compiled.sql.contains("\nGROUP BY 1\n"));
    }

    #[test]
    fn renders_all_bind_values_through_text_into_target_types() {
        for (filter, expected) in [
            (
                r#"{"op":"gte","field":"amount","value":{"type":"integer","value":1}}"#,
                "$1::text::numeric",
            ),
            (
                r#"{"op":"gte","field":"ordered_at","value":{"type":"timestamp","value":"2026-01-01T00:00:00Z"}}"#,
                "$1::text::timestamptz",
            ),
            (
                r#"{"op":"eq","field":"customer_id","value":{"type":"integer","value":1}}"#,
                "$1::text::bigint",
            ),
        ] {
            let input = format!(
                r#"{{
                    "schema_version":"1",
                    "model":"orders",
                    "metrics":[{{"metric":"order_count"}}],
                    "filters":{filter}
                }}"#
            );
            let query = normalize_lsq(input.as_bytes()).expect("valid LSQ");
            let compiled = compile_lsq(&query, &snapshot(), CompilerOptions::default())
                .expect("query compiles");
            assert!(compiled.sql.contains(expected));
            assert!(compiled.sql.contains("LIMIT $2::text::bigint"));
        }

        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"subscriptions",
                "metrics":[{"metric":"subscription_count"}],
                "filters":{"op":"eq","field":"active","value":{"type":"boolean","value":true}}
            }"#,
        )
        .expect("valid LSQ");
        let compiled =
            compile_lsq(&query, &snapshot(), CompilerOptions::default()).expect("query compiles");
        assert!(compiled.sql.contains("$1::text::boolean"));

        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"subscriptions",
                "metrics":[{"metric":"subscription_count"}],
                "filters":{"op":"gte","field":"started_on","value":{"type":"date","value":"2026-01-01"}}
            }"#,
        )
        .expect("valid LSQ");
        let compiled =
            compile_lsq(&query, &snapshot(), CompilerOptions::default()).expect("query compiles");
        assert!(compiled.sql.contains("$1::text::date"));
    }

    #[test]
    fn query_hash_includes_resolved_limit_parameter() {
        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"orders",
                "metrics":[{"metric":"order_count"}]
            }"#,
        )
        .expect("valid LSQ");
        let snapshot = snapshot();
        let first = compile_lsq(
            &query,
            &snapshot,
            CompilerOptions {
                default_limit: 100,
                hard_limit: 1_000,
            },
        )
        .expect("compiles");
        let second = compile_lsq(
            &query,
            &snapshot,
            CompilerOptions {
                default_limit: 200,
                hard_limit: 1_000,
            },
        )
        .expect("compiles");

        assert_eq!(first.sql, second.sql);
        assert_ne!(first.query_hash, second.query_hash);
    }

    #[test]
    fn rejects_snapshot_content_that_does_not_match_revision_hash() {
        let query = normalize_lsq(
            br#"{
                "schema_version":"1",
                "model":"orders",
                "metrics":[{"metric":"order_count"}]
            }"#,
        )
        .expect("valid LSQ");
        let mut snapshot = snapshot();
        snapshot.models[0].source.relation = "tampered_orders".to_owned();

        let error = compile_lsq(&query, &snapshot, CompilerOptions::default())
            .expect_err("snapshot hash drift must fail closed");

        assert_eq!(error, super::CompileError::InvalidRevisionHash);
    }
}
