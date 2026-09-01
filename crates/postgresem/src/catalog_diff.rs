use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::catalog::{
    CatalogColumn, CatalogConstraint, CatalogError, CatalogRelation, CatalogSnapshot,
    RowLevelSecurityPolicy,
};

const CATALOG_DIFF_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogCompatibility {
    Compatible,
    ReviewRequired,
    Breaking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogObjectKind {
    Context,
    Relation,
    Column,
    Constraint,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogDiffSummary {
    pub total: usize,
    pub compatible: usize,
    pub review_required: usize,
    pub breaking: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogChange {
    pub path: String,
    pub object_kind: CatalogObjectKind,
    pub change: CatalogChangeKind,
    pub compatibility: CatalogCompatibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogDiff {
    pub schema_version: String,
    pub from_fingerprint: String,
    pub to_fingerprint: String,
    pub compatibility: CatalogCompatibility,
    pub summary: CatalogDiffSummary,
    pub changes: Vec<CatalogChange>,
}

impl CatalogDiff {
    #[must_use]
    pub fn has_breaking_changes(&self) -> bool {
        self.summary.breaking > 0
    }
}

#[derive(Debug, Error)]
pub enum CatalogDiffError {
    #[error("invalid source catalog snapshot")]
    InvalidSource(#[source] CatalogError),
    #[error("invalid target catalog snapshot")]
    InvalidTarget(#[source] CatalogError),
    #[error("catalog snapshots were captured from different databases or roles")]
    ContextMismatch,
    #[error("failed to serialize catalog diff value")]
    Serialization(#[source] serde_json::Error),
}

pub fn diff_catalogs(
    before: &CatalogSnapshot,
    after: &CatalogSnapshot,
) -> Result<CatalogDiff, CatalogDiffError> {
    let before = before
        .validated_normalized()
        .map_err(CatalogDiffError::InvalidSource)?;
    let after = after
        .validated_normalized()
        .map_err(CatalogDiffError::InvalidTarget)?;

    if before.current_database != after.current_database
        || before.current_role != after.current_role
    {
        return Err(CatalogDiffError::ContextMismatch);
    }

    let mut changes = Vec::new();
    if before.server_version_num != after.server_version_num {
        push_modified(
            &mut changes,
            "/server_version_num",
            CatalogObjectKind::Context,
            CatalogCompatibility::ReviewRequired,
            &before.server_version_num,
            &after.server_version_num,
        )?;
    }
    if before.role_context != after.role_context {
        push_modified(
            &mut changes,
            "/role_context",
            CatalogObjectKind::Context,
            CatalogCompatibility::Breaking,
            &before.role_context,
            &after.role_context,
        )?;
    }

    let before_relations = relation_map(&before.relations);
    let after_relations = relation_map(&after.relations);
    for key in before_relations.keys().chain(after_relations.keys()) {
        match (before_relations.get(key), after_relations.get(key)) {
            (Some(left), Some(right)) => diff_relation(left, right, &mut changes)?,
            (Some(left), None) => push_change(
                &mut changes,
                relation_path(left),
                CatalogObjectKind::Relation,
                CatalogChangeKind::Removed,
                CatalogCompatibility::Breaking,
                Some(left),
                None::<&CatalogRelation>,
            )?,
            (None, Some(right)) => push_change(
                &mut changes,
                relation_path(right),
                CatalogObjectKind::Relation,
                CatalogChangeKind::Added,
                CatalogCompatibility::Compatible,
                None::<&CatalogRelation>,
                Some(right),
            )?,
            (None, None) => {}
        }
    }

    changes.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(change_order(left.change).cmp(&change_order(right.change)))
    });
    changes.dedup();
    let summary = summarize(&changes);
    let compatibility = if summary.breaking > 0 {
        CatalogCompatibility::Breaking
    } else if summary.review_required > 0 {
        CatalogCompatibility::ReviewRequired
    } else {
        CatalogCompatibility::Compatible
    };

    Ok(CatalogDiff {
        schema_version: CATALOG_DIFF_SCHEMA_VERSION.to_owned(),
        from_fingerprint: before.fingerprint,
        to_fingerprint: after.fingerprint,
        compatibility,
        summary,
        changes,
    })
}

fn relation_map(relations: &[CatalogRelation]) -> BTreeMap<(&str, &str), &CatalogRelation> {
    relations
        .iter()
        .map(|relation| ((relation.schema.as_str(), relation.name.as_str()), relation))
        .collect()
}

fn diff_relation(
    before: &CatalogRelation,
    after: &CatalogRelation,
    changes: &mut Vec<CatalogChange>,
) -> Result<(), CatalogDiffError> {
    let base = relation_path(before);
    if before.kind != after.kind {
        push_modified(
            changes,
            &format!("{base}/kind"),
            CatalogObjectKind::Relation,
            CatalogCompatibility::Breaking,
            &before.kind,
            &after.kind,
        )?;
    }
    if before.owner != after.owner {
        push_modified(
            changes,
            &format!("{base}/owner"),
            CatalogObjectKind::Relation,
            CatalogCompatibility::Breaking,
            &before.owner,
            &after.owner,
        )?;
    }
    if before.comment != after.comment {
        push_modified(
            changes,
            &format!("{base}/comment"),
            CatalogObjectKind::Relation,
            CatalogCompatibility::ReviewRequired,
            &before.comment,
            &after.comment,
        )?;
    }
    if before.grants != after.grants {
        push_modified(
            changes,
            &format!("{base}/grants"),
            CatalogObjectKind::Relation,
            CatalogCompatibility::Breaking,
            &before.grants,
            &after.grants,
        )?;
    }
    if before.rls != after.rls {
        push_modified(
            changes,
            &format!("{base}/rls"),
            CatalogObjectKind::Relation,
            CatalogCompatibility::Breaking,
            &before.rls,
            &after.rls,
        )?;
    }

    diff_columns(&base, &before.columns, &after.columns, changes)?;
    diff_constraints(&base, &before.constraints, &after.constraints, changes)?;
    diff_policies(&base, &before.policies, &after.policies, changes)
}

fn diff_columns(
    base: &str,
    before: &[CatalogColumn],
    after: &[CatalogColumn],
    changes: &mut Vec<CatalogChange>,
) -> Result<(), CatalogDiffError> {
    let left = before
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect::<BTreeMap<_, _>>();
    let right = after
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect::<BTreeMap<_, _>>();
    for name in left.keys().chain(right.keys()) {
        let path = format!("{base}/columns/{}", path_segment(name));
        match (left.get(name), right.get(name)) {
            (Some(before), Some(after)) => {
                if *before != *after {
                    let compatibility = if before.data_type != after.data_type
                        || before.nullable != after.nullable
                        || before.select_grant != after.select_grant
                        || before.ordinal != after.ordinal
                    {
                        CatalogCompatibility::Breaking
                    } else {
                        CatalogCompatibility::ReviewRequired
                    };
                    push_modified(
                        changes,
                        &path,
                        CatalogObjectKind::Column,
                        compatibility,
                        *before,
                        *after,
                    )?;
                }
            }
            (Some(before), None) => push_change(
                changes,
                path,
                CatalogObjectKind::Column,
                CatalogChangeKind::Removed,
                CatalogCompatibility::Breaking,
                Some(*before),
                None::<&CatalogColumn>,
            )?,
            (None, Some(after)) => push_change(
                changes,
                path,
                CatalogObjectKind::Column,
                CatalogChangeKind::Added,
                CatalogCompatibility::Compatible,
                None::<&CatalogColumn>,
                Some(*after),
            )?,
            (None, None) => {}
        }
    }
    Ok(())
}

fn diff_constraints(
    base: &str,
    before: &[CatalogConstraint],
    after: &[CatalogConstraint],
    changes: &mut Vec<CatalogChange>,
) -> Result<(), CatalogDiffError> {
    let left = before
        .iter()
        .map(|constraint| (constraint_key(constraint), constraint))
        .collect::<BTreeMap<_, _>>();
    let right = after
        .iter()
        .map(|constraint| (constraint_key(constraint), constraint))
        .collect::<BTreeMap<_, _>>();
    for key in left.keys().chain(right.keys()) {
        let path = format!("{base}/constraints/{}", path_segment(key));
        match (left.get(key), right.get(key)) {
            (Some(before), Some(after)) if *before != *after => push_modified(
                changes,
                &path,
                CatalogObjectKind::Constraint,
                CatalogCompatibility::Breaking,
                *before,
                *after,
            )?,
            (Some(before), None) => push_change(
                changes,
                path,
                CatalogObjectKind::Constraint,
                CatalogChangeKind::Removed,
                CatalogCompatibility::Breaking,
                Some(*before),
                None::<&CatalogConstraint>,
            )?,
            (None, Some(after)) => push_change(
                changes,
                path,
                CatalogObjectKind::Constraint,
                CatalogChangeKind::Added,
                CatalogCompatibility::ReviewRequired,
                None::<&CatalogConstraint>,
                Some(*after),
            )?,
            _ => {}
        }
    }
    Ok(())
}

fn diff_policies(
    base: &str,
    before: &[RowLevelSecurityPolicy],
    after: &[RowLevelSecurityPolicy],
    changes: &mut Vec<CatalogChange>,
) -> Result<(), CatalogDiffError> {
    let left = before
        .iter()
        .map(|policy| (policy.name.as_str(), policy))
        .collect::<BTreeMap<_, _>>();
    let right = after
        .iter()
        .map(|policy| (policy.name.as_str(), policy))
        .collect::<BTreeMap<_, _>>();
    for name in left.keys().chain(right.keys()) {
        let path = format!("{base}/policies/{}", path_segment(name));
        match (left.get(name), right.get(name)) {
            (Some(before), Some(after)) if *before != *after => push_modified(
                changes,
                &path,
                CatalogObjectKind::Policy,
                CatalogCompatibility::Breaking,
                *before,
                *after,
            )?,
            (Some(before), None) => push_change(
                changes,
                path,
                CatalogObjectKind::Policy,
                CatalogChangeKind::Removed,
                CatalogCompatibility::Breaking,
                Some(*before),
                None::<&RowLevelSecurityPolicy>,
            )?,
            (None, Some(after)) => push_change(
                changes,
                path,
                CatalogObjectKind::Policy,
                CatalogChangeKind::Added,
                CatalogCompatibility::Breaking,
                None::<&RowLevelSecurityPolicy>,
                Some(*after),
            )?,
            _ => {}
        }
    }
    Ok(())
}

fn push_modified<T: Serialize>(
    changes: &mut Vec<CatalogChange>,
    path: &str,
    object_kind: CatalogObjectKind,
    compatibility: CatalogCompatibility,
    before: &T,
    after: &T,
) -> Result<(), CatalogDiffError> {
    push_change(
        changes,
        path.to_owned(),
        object_kind,
        CatalogChangeKind::Modified,
        compatibility,
        Some(before),
        Some(after),
    )
}

fn push_change<B: Serialize, A: Serialize>(
    changes: &mut Vec<CatalogChange>,
    path: String,
    object_kind: CatalogObjectKind,
    change: CatalogChangeKind,
    compatibility: CatalogCompatibility,
    before: Option<B>,
    after: Option<A>,
) -> Result<(), CatalogDiffError> {
    changes.push(CatalogChange {
        path,
        object_kind,
        change,
        compatibility,
        before: before
            .map(serde_json::to_value)
            .transpose()
            .map_err(CatalogDiffError::Serialization)?,
        after: after
            .map(serde_json::to_value)
            .transpose()
            .map_err(CatalogDiffError::Serialization)?,
    });
    Ok(())
}

fn relation_path(relation: &CatalogRelation) -> String {
    format!(
        "/relations/{}/{}",
        path_segment(&relation.schema),
        path_segment(&relation.name)
    )
}

fn path_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn constraint_key(constraint: &CatalogConstraint) -> String {
    match constraint {
        CatalogConstraint::PrimaryKey { name, .. } => format!("primary_key:{name}"),
        CatalogConstraint::Unique { name, .. } => format!("unique:{name}"),
        CatalogConstraint::ForeignKey { name, .. } => format!("foreign_key:{name}"),
        CatalogConstraint::Check { name, .. } => format!("check:{name}"),
    }
}

fn summarize(changes: &[CatalogChange]) -> CatalogDiffSummary {
    CatalogDiffSummary {
        total: changes.len(),
        compatible: changes
            .iter()
            .filter(|change| change.compatibility == CatalogCompatibility::Compatible)
            .count(),
        review_required: changes
            .iter()
            .filter(|change| change.compatibility == CatalogCompatibility::ReviewRequired)
            .count(),
        breaking: changes
            .iter()
            .filter(|change| change.compatibility == CatalogCompatibility::Breaking)
            .count(),
    }
}

const fn change_order(change: CatalogChangeKind) -> u8 {
    match change {
        CatalogChangeKind::Removed => 0,
        CatalogChangeKind::Modified => 1,
        CatalogChangeKind::Added => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogCompatibility, CatalogDiffError, diff_catalogs};
    use crate::catalog::{
        CatalogColumn, CatalogRelation, CatalogRoleContext, CatalogSnapshot, RelationGrantHints,
        RelationKind, RowLevelSecurity,
    };

    #[test]
    fn classifies_additions_comments_and_security_drift() {
        let before = snapshot(relation()).expect("valid source snapshot");
        let mut changed = relation();
        changed.comment = Some("renamed meaning".to_owned());
        changed.rls.enabled = true;
        changed.columns.push(CatalogColumn {
            name: "created_at".to_owned(),
            ordinal: 2,
            data_type: "timestamp with time zone".to_owned(),
            nullable: false,
            comment: None,
            select_grant: true,
        });
        let after = snapshot(changed).expect("valid target snapshot");

        let diff = diff_catalogs(&before, &after).expect("snapshots are comparable");

        assert_eq!(diff.compatibility, CatalogCompatibility::Breaking);
        assert_eq!(diff.summary.total, 3);
        assert_eq!(diff.summary.compatible, 1);
        assert_eq!(diff.summary.review_required, 1);
        assert_eq!(diff.summary.breaking, 1);
        assert!(diff.has_breaking_changes());
    }

    #[test]
    fn rejects_tampered_and_cross_context_snapshots() {
        let source = snapshot(relation()).expect("valid source snapshot");
        let mut tampered = source.clone();
        tampered.relations[0].name = "other".to_owned();
        assert!(matches!(
            diff_catalogs(&source, &tampered),
            Err(CatalogDiffError::InvalidTarget(_))
        ));

        let mut other_role = CatalogSnapshot {
            current_role: "other".to_owned(),
            fingerprint: String::new(),
            ..source.clone()
        };
        other_role = other_role.finalize().expect("valid other-role snapshot");
        assert!(matches!(
            diff_catalogs(&source, &other_role),
            Err(CatalogDiffError::ContextMismatch)
        ));
    }

    #[test]
    fn classifies_role_context_and_owner_changes_as_breaking() {
        let before = snapshot(relation()).expect("valid source snapshot");
        let mut after = before.clone();
        after.role_context.bypass_rls = true;
        after
            .role_context
            .effective_roles
            .push("rls_policy_role".to_owned());
        after.relations[0].owner = "postgresem_introspector".to_owned();
        after.fingerprint.clear();
        let after = after.finalize().expect("valid target snapshot");

        let diff = diff_catalogs(&before, &after).expect("snapshots are comparable");

        assert_eq!(diff.compatibility, CatalogCompatibility::Breaking);
        assert_eq!(diff.summary.breaking, 2);
        assert!(
            diff.changes
                .iter()
                .any(|change| change.path == "/role_context")
        );
        assert!(
            diff.changes
                .iter()
                .any(|change| change.path.ends_with("/owner"))
        );
    }

    fn snapshot(
        relation: CatalogRelation,
    ) -> Result<CatalogSnapshot, crate::catalog::CatalogError> {
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
            relations: vec![relation],
            fingerprint: String::new(),
        }
        .finalize()
    }

    fn relation() -> CatalogRelation {
        CatalogRelation {
            schema: "commerce".to_owned(),
            name: "orders".to_owned(),
            kind: RelationKind::Table,
            owner: "postgresem_source_owner".to_owned(),
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
            columns: vec![CatalogColumn {
                name: "order_id".to_owned(),
                ordinal: 1,
                data_type: "bigint".to_owned(),
                nullable: false,
                comment: Some("Order ID".to_owned()),
                select_grant: true,
            }],
            constraints: Vec::new(),
            policies: Vec::new(),
        }
    }
}
