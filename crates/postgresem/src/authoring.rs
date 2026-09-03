use std::collections::BTreeSet;

use postgresem_compiler::{
    Additivity, Aggregation, DataType, Field, Metric, Model, Relation, SemanticSnapshot,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    catalog::{CatalogConstraint, CatalogError, CatalogRelation, CatalogSnapshot, RelationKind},
    catalog_types::{portable_identifier, postgres_data_type},
};

const AUTHORING_SCHEMA_VERSION: &str = "1";
const MAX_SCAFFOLD_MODELS: usize = 1_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaffoldRequest {
    pub schema_version: String,
    pub schema: String,
    #[serde(default)]
    pub relations: Vec<String>,
    #[serde(default)]
    pub relation_prefix: Option<String>,
    pub max_models: usize,
    #[serde(default)]
    pub timezone: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScaffoldReport {
    pub schema_version: String,
    pub catalog_fingerprint: String,
    pub source_database: String,
    pub source_schema: String,
    pub selected_relations: usize,
    pub omitted_unselectable_columns: usize,
    pub snapshot: SemanticSnapshot,
}

#[derive(Debug, Error)]
pub enum ScaffoldError {
    #[error("authoring request is not valid strict JSON")]
    Parse(#[source] serde_json::Error),
    #[error("unsupported authoring request schema version: {0}")]
    UnsupportedSchemaVersion(String),
    #[error("authoring request must select either explicit relations or one non-empty prefix")]
    AmbiguousSelection,
    #[error("authoring max_models must be between 1 and 1000")]
    InvalidModelLimit,
    #[error("authoring identifier is not portable: {0}")]
    InvalidIdentifier(String),
    #[error("authoring timezone must be the canonical value UTC")]
    InvalidTimezone,
    #[error("authoring relation selection contains duplicates")]
    DuplicateRelation,
    #[error("authoring relation is not available in catalog evidence: {0}")]
    RelationNotFound(String),
    #[error("authoring selection matched no relations")]
    EmptySelection,
    #[error("authoring selection exceeds max_models")]
    ModelLimitExceeded,
    #[error("authoring relation is not selectable by the catalog role: {0}")]
    RelationNotSelectable(String),
    #[error("foreign tables cannot be scaffolded as executable semantic models: {0}")]
    UnsupportedRelationKind(String),
    #[error("PostgreSQL type is outside the scaffold subset at {path}: {value}")]
    UnsupportedPostgresType { path: String, value: String },
    #[error("temporal field requires an explicit authoring timezone at {0}")]
    MissingTimezone(String),
    #[error("catalog evidence is invalid")]
    InvalidCatalog(#[source] CatalogError),
    #[error("failed to calculate scaffold revision hash")]
    SnapshotHash(#[from] postgresem_compiler::SnapshotHashError),
}

pub fn scaffold(
    request: &[u8],
    catalog: &CatalogSnapshot,
) -> Result<ScaffoldReport, ScaffoldError> {
    let request: ScaffoldRequest = serde_json::from_slice(request).map_err(ScaffoldError::Parse)?;
    validate_request(&request)?;
    let catalog = catalog
        .validated_normalized()
        .map_err(ScaffoldError::InvalidCatalog)?;
    let selected = select_relations(&request, &catalog)?;
    if selected.len() > request.max_models {
        return Err(ScaffoldError::ModelLimitExceeded);
    }

    let mut omitted_unselectable_columns = 0;
    let mut models = Vec::with_capacity(selected.len());
    for relation in selected {
        models.push(scaffold_model(
            relation,
            request.timezone.as_deref(),
            &mut omitted_unselectable_columns,
        )?);
    }
    let mut snapshot = SemanticSnapshot {
        schema_version: "2".to_owned(),
        revision_hash: String::new(),
        models,
    };
    snapshot.revision_hash = snapshot.calculate_revision_hash()?;

    Ok(ScaffoldReport {
        schema_version: AUTHORING_SCHEMA_VERSION.to_owned(),
        catalog_fingerprint: catalog.fingerprint,
        source_database: catalog.current_database,
        source_schema: request.schema,
        selected_relations: snapshot.models.len(),
        omitted_unselectable_columns,
        snapshot,
    })
}

fn validate_request(request: &ScaffoldRequest) -> Result<(), ScaffoldError> {
    if request.schema_version != AUTHORING_SCHEMA_VERSION {
        return Err(ScaffoldError::UnsupportedSchemaVersion(
            request.schema_version.clone(),
        ));
    }
    if !portable_identifier(&request.schema) {
        return Err(ScaffoldError::InvalidIdentifier(request.schema.clone()));
    }
    if request.max_models == 0 || request.max_models > MAX_SCAFFOLD_MODELS {
        return Err(ScaffoldError::InvalidModelLimit);
    }
    let has_relations = !request.relations.is_empty();
    let has_prefix = request
        .relation_prefix
        .as_deref()
        .is_some_and(|prefix| !prefix.is_empty());
    if has_relations == has_prefix {
        return Err(ScaffoldError::AmbiguousSelection);
    }
    if request.relation_prefix.as_deref() == Some("") {
        return Err(ScaffoldError::AmbiguousSelection);
    }
    let mut names = BTreeSet::new();
    for relation in &request.relations {
        if !portable_identifier(relation) {
            return Err(ScaffoldError::InvalidIdentifier(relation.clone()));
        }
        if !names.insert(relation) {
            return Err(ScaffoldError::DuplicateRelation);
        }
    }
    if let Some(prefix) = request.relation_prefix.as_deref() {
        if !portable_identifier(prefix) {
            return Err(ScaffoldError::InvalidIdentifier(prefix.to_owned()));
        }
    }
    if request
        .timezone
        .as_deref()
        .is_some_and(|timezone| timezone != "UTC")
    {
        return Err(ScaffoldError::InvalidTimezone);
    }
    Ok(())
}

fn select_relations<'a>(
    request: &ScaffoldRequest,
    catalog: &'a CatalogSnapshot,
) -> Result<Vec<&'a CatalogRelation>, ScaffoldError> {
    let in_schema = catalog
        .relations
        .iter()
        .filter(|relation| relation.schema == request.schema)
        .collect::<Vec<_>>();
    let selected = if let Some(prefix) = request.relation_prefix.as_deref() {
        in_schema
            .into_iter()
            .filter(|relation| relation.name.starts_with(prefix))
            .collect::<Vec<_>>()
    } else {
        let names = request
            .relations
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let selected = in_schema
            .into_iter()
            .filter(|relation| names.contains(relation.name.as_str()))
            .collect::<Vec<_>>();
        for name in &request.relations {
            if !selected.iter().any(|relation| relation.name == *name) {
                return Err(ScaffoldError::RelationNotFound(name.clone()));
            }
        }
        selected
    };
    if selected.is_empty() {
        return Err(ScaffoldError::EmptySelection);
    }
    Ok(selected)
}

fn scaffold_model(
    relation: &CatalogRelation,
    timezone: Option<&str>,
    omitted_unselectable_columns: &mut usize,
) -> Result<Model, ScaffoldError> {
    if !portable_identifier(&relation.name) {
        return Err(ScaffoldError::InvalidIdentifier(relation.name.clone()));
    }
    if matches!(relation.kind, RelationKind::ForeignTable) {
        return Err(ScaffoldError::UnsupportedRelationKind(
            relation.name.clone(),
        ));
    }
    if !relation.grants.schema_usage || !relation.grants.any_column_select {
        return Err(ScaffoldError::RelationNotSelectable(relation.name.clone()));
    }
    let primary_key = relation
        .constraints
        .iter()
        .find_map(|constraint| match constraint {
            CatalogConstraint::PrimaryKey {
                columns,
                enforced: true,
                validated: true,
                ..
            } => Some(columns.iter().cloned().collect::<BTreeSet<_>>()),
            _ => None,
        })
        .unwrap_or_default();

    let mut fields = Vec::new();
    for column in &relation.columns {
        if !column.select_grant {
            *omitted_unselectable_columns += 1;
            continue;
        }
        if !portable_identifier(&column.name) {
            return Err(ScaffoldError::InvalidIdentifier(column.name.clone()));
        }
        let path = format!(
            "/relations/{}/{}/columns/{}",
            relation.schema, relation.name, column.name
        );
        let data_type = postgres_data_type(&column.data_type).ok_or_else(|| {
            ScaffoldError::UnsupportedPostgresType {
                path: path.clone(),
                value: column.data_type.clone(),
            }
        })?;
        if matches!(data_type, DataType::Timestamp | DataType::TimestampTz) && timezone.is_none() {
            return Err(ScaffoldError::MissingTimezone(path));
        }
        fields.push(Field {
            semantic_name: column.name.clone(),
            data_type,
            column: column.name.clone(),
            relationship: None,
            time_dimension: matches!(
                data_type,
                DataType::Date | DataType::Timestamp | DataType::TimestampTz
            ),
            entity_key: primary_key.len() == 1 && primary_key.contains(&column.name),
            visible: true,
            nullable: column.nullable,
        });
    }
    if fields.is_empty() {
        return Err(ScaffoldError::RelationNotSelectable(relation.name.clone()));
    }

    let metrics = if primary_key.len() == 1 {
        let key = primary_key
            .first()
            .filter(|key| {
                fields
                    .iter()
                    .any(|field| field.column.as_str() == key.as_str())
            })
            .cloned();
        key.map(|key| {
            vec![Metric {
                semantic_name: "row_count".to_owned(),
                data_type: DataType::Integer,
                aggregation: Aggregation::Count,
                field: key.clone(),
                filter: None,
                additivity: Some(Additivity::Additive),
                aggregation_anchor: Some(key),
                visible: true,
            }]
        })
        .unwrap_or_default()
    } else {
        vec![]
    };

    Ok(Model {
        semantic_name: relation.name.clone(),
        source: Relation {
            schema: relation.schema.clone(),
            relation: relation.name.clone(),
        },
        timezone: timezone.map(str::to_owned),
        queryable: true,
        writable: None,
        fields,
        metrics,
        relationships: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::catalog::{
        CatalogColumn, CatalogConstraint, CatalogRelation, CatalogRoleContext, CatalogSnapshot,
        RelationGrantHints, RelationKind, RowLevelSecurity,
    };

    use super::{ScaffoldError, scaffold};

    #[test]
    fn scaffolds_a_deterministic_catalog_bound_snapshot() {
        let catalog = catalog("bigint").expect("catalog");
        let request = br#"{
            "schema_version": "1",
            "schema": "app",
            "relations": ["orders"],
            "max_models": 10,
            "timezone": "UTC"
        }"#;
        let report = scaffold(request, &catalog).expect("scaffold");
        assert_eq!(report.selected_relations, 1);
        assert_eq!(
            report.snapshot.models[0].metrics[0].semantic_name,
            "row_count"
        );
        assert_eq!(
            report.snapshot.revision_hash,
            report
                .snapshot
                .calculate_revision_hash()
                .expect("revision hash")
        );
    }

    #[test]
    fn rejects_ambiguous_unsafe_and_unsupported_authoring_input() {
        let unsupported_catalog = catalog("jsonb").expect("catalog");
        for request in [
            br#"{"schema_version":"1","schema":"app","relations":[],"max_models":10}"#.as_slice(),
            br#"{"schema_version":"1","schema":"app","relations":["orders"],"relation_prefix":"ord","max_models":10}"#.as_slice(),
            br#"{"schema_version":"1","schema":"app","relations":["Order Items"],"max_models":10}"#.as_slice(),
        ] {
            assert!(scaffold(request, &unsupported_catalog).is_err());
        }
        assert!(matches!(
            scaffold(
                br#"{"schema_version":"1","schema":"app","relations":["orders"],"max_models":10}"#,
                &unsupported_catalog
            ),
            Err(ScaffoldError::UnsupportedPostgresType { .. })
        ));

        let mut tampered = catalog("bigint").expect("catalog");
        tampered.relations[0].columns[0].data_type = "text".to_owned();
        assert!(matches!(
            scaffold(
                br#"{"schema_version":"1","schema":"app","relations":["orders"],"max_models":10}"#,
                &tampered
            ),
            Err(ScaffoldError::InvalidCatalog(_))
        ));

        assert!(matches!(
            scaffold(
                br#"{"schema_version":"1","schema":"app","relations":["orders"],"max_models":1001}"#,
                &catalog("bigint").expect("catalog")
            ),
            Err(ScaffoldError::InvalidModelLimit)
        ));
    }

    #[test]
    fn requires_timezone_for_temporal_scaffolds() {
        for data_type in ["timestamp without time zone", "timestamp with time zone"] {
            let temporal_catalog = catalog(data_type).expect("catalog");
            assert!(matches!(
                scaffold(
                    br#"{"schema_version":"1","schema":"app","relations":["orders"],"max_models":10}"#,
                    &temporal_catalog
                ),
                Err(ScaffoldError::MissingTimezone(_))
            ));
            let report = scaffold(
                br#"{"schema_version":"1","schema":"app","relations":["orders"],"max_models":10,"timezone":"UTC"}"#,
                &temporal_catalog,
            )
            .expect("temporal scaffold");
            assert_eq!(report.snapshot.models[0].timezone.as_deref(), Some("UTC"));
            assert!(report.snapshot.models[0].fields[0].time_dimension);
        }
        for timezone in ["", " UTC ", "Not/AZone"] {
            let request = format!(
                r#"{{"schema_version":"1","schema":"app","relations":["orders"],"max_models":10,"timezone":"{timezone}"}}"#
            );
            assert!(matches!(
                scaffold(
                    request.as_bytes(),
                    &catalog("timestamp with time zone").expect("catalog")
                ),
                Err(ScaffoldError::InvalidTimezone)
            ));
        }
    }

    #[test]
    fn does_not_treat_composite_key_components_as_entity_keys() {
        let mut composite = catalog("bigint").expect("catalog");
        composite.relations[0].columns.push(CatalogColumn {
            name: "tenant_id".to_owned(),
            ordinal: 2,
            data_type: "bigint".to_owned(),
            nullable: false,
            comment: None,
            select_grant: true,
        });
        composite.relations[0].constraints[0] = CatalogConstraint::PrimaryKey {
            name: "orders_pkey".to_owned(),
            columns: vec!["order_id".to_owned(), "tenant_id".to_owned()],
            enforced: true,
            period: false,
            deferrable: false,
            initially_deferred: false,
            validated: true,
        };
        composite.fingerprint.clear();
        let composite = composite.finalize().expect("composite catalog");
        let report = scaffold(
            br#"{"schema_version":"1","schema":"app","relations":["orders"],"max_models":10}"#,
            &composite,
        )
        .expect("composite scaffold");
        assert!(
            report.snapshot.models[0]
                .fields
                .iter()
                .all(|field| !field.entity_key)
        );
        assert!(report.snapshot.models[0].metrics.is_empty());
    }

    fn catalog(data_type: &str) -> Result<CatalogSnapshot, crate::catalog::CatalogError> {
        CatalogSnapshot {
            schema_version: "2".to_owned(),
            server_version_num: 180000,
            current_database: "app".to_owned(),
            current_role: "reader".to_owned(),
            role_context: CatalogRoleContext {
                inherit: true,
                superuser: false,
                bypass_rls: false,
                effective_roles: vec!["reader".to_owned()],
                settable_roles: vec!["reader".to_owned()],
            },
            role_graph_fingerprint:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_owned(),
            object_privilege_fingerprint:
                "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_owned(),
            functions: vec![],
            relations: vec![CatalogRelation {
                schema: "app".to_owned(),
                name: "orders".to_owned(),
                kind: RelationKind::Table,
                owner: "owner".to_owned(),
                view: None,
                comment: None,
                grants: RelationGrantHints {
                    schema_usage: true,
                    table_select: true,
                    any_column_select: true,
                },
                rls: RowLevelSecurity {
                    enabled: true,
                    forced: true,
                },
                columns: vec![CatalogColumn {
                    name: "order_id".to_owned(),
                    ordinal: 1,
                    data_type: data_type.to_owned(),
                    nullable: false,
                    comment: None,
                    select_grant: true,
                }],
                constraints: vec![CatalogConstraint::PrimaryKey {
                    name: "orders_pkey".to_owned(),
                    columns: vec!["order_id".to_owned()],
                    enforced: true,
                    period: false,
                    deferrable: false,
                    initially_deferred: false,
                    validated: true,
                }],
                policies: vec![],
            }],
            fingerprint: String::new(),
        }
        .finalize()
    }
}
