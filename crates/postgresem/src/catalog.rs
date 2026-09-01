use std::{cmp::Ordering, env};

use postgres::{Client, GenericClient, IsolationLevel, Row};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{database, hash::sha256};

const CATALOG_SNAPSHOT_SCHEMA_VERSION: &str = "2";

const RELATIONS_SQL: &str = r"
    SELECT
        n.nspname AS schema_name,
        c.relname AS relation_name,
        CASE c.relkind
            WHEN 'r' THEN 'table'
            WHEN 'p' THEN 'partitioned_table'
            WHEN 'v' THEN 'view'
            WHEN 'm' THEN 'materialized_view'
            WHEN 'f' THEN 'foreign_table'
        END AS relation_kind,
        owner.rolname AS relation_owner,
        CASE
            WHEN c.relkind IN ('v', 'm')
            THEN pg_catalog.pg_get_viewdef(c.oid, false)
        END AS view_definition,
        CASE
            WHEN c.relkind = 'v'
            THEN COALESCE('security_invoker=true' = ANY(c.reloptions), false)
        END AS view_security_invoker,
        CASE
            WHEN c.relkind = 'v'
            THEN COALESCE('security_barrier=true' = ANY(c.reloptions), false)
        END AS view_security_barrier,
        obj_description(c.oid, 'pg_class') AS relation_comment,
        c.relrowsecurity AS rls_enabled,
        c.relforcerowsecurity AS rls_forced,
        has_schema_privilege(n.oid, 'USAGE') AS schema_usage,
        has_table_privilege(c.oid, 'SELECT') AS table_select,
        has_any_column_privilege(c.oid, 'SELECT') AS any_column_select
    FROM pg_catalog.pg_class AS c
    JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
    JOIN pg_catalog.pg_roles AS owner ON owner.oid = c.relowner
    WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f')
      AND n.nspname NOT IN ('pg_catalog', 'information_schema')
      AND n.nspname <> 'semantic'
      AND n.nspname !~ '^pg_toast'
      AND n.nspname !~ '^pg_temp_'
    ORDER BY n.nspname, c.relname, c.relkind
";

const ROLE_CONTEXT_SQL: &str = r"
    SELECT
        current_role_attributes.rolinherit AS inherit,
        current_role_attributes.rolsuper AS superuser,
        current_role_attributes.rolbypassrls AS bypass_rls,
        ARRAY(
            SELECT candidate.rolname
            FROM pg_catalog.pg_roles AS candidate
            WHERE pg_catalog.pg_has_role(current_user, candidate.oid, 'USAGE')
            ORDER BY candidate.rolname
        ) AS effective_roles,
        ARRAY(
            SELECT candidate.rolname
            FROM pg_catalog.pg_roles AS candidate
            WHERE pg_catalog.pg_has_role(current_user, candidate.oid, 'SET')
            ORDER BY candidate.rolname
        ) AS settable_roles
    FROM pg_catalog.pg_roles AS current_role_attributes
    WHERE current_role_attributes.rolname = current_user
";

const COLUMNS_SQL: &str = r"
    SELECT
        a.attname AS column_name,
        a.attnum::integer AS ordinal,
        pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
        NOT a.attnotnull AS nullable,
        col_description(c.oid, a.attnum) AS column_comment,
        has_column_privilege(c.oid, a.attnum, 'SELECT') AS column_select
    FROM pg_catalog.pg_class AS c
    JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
    JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid
    WHERE n.nspname = $1
      AND c.relname = $2
      AND a.attnum > 0
      AND NOT a.attisdropped
    ORDER BY a.attnum
";

const CONSTRAINTS_SQL: &str = r"
    SELECT
        con.conname AS constraint_name,
        CASE con.contype
            WHEN 'p' THEN 'primary_key'
            WHEN 'u' THEN 'unique'
            WHEN 'f' THEN 'foreign_key'
            WHEN 'c' THEN 'check'
        END AS constraint_kind,
        ARRAY(
            SELECT att.attname
            FROM unnest(con.conkey) WITH ORDINALITY AS key(attnum, ordinal)
            JOIN pg_catalog.pg_attribute AS att
              ON att.attrelid = con.conrelid
             AND att.attnum = key.attnum
            ORDER BY key.ordinal
        ) AS columns,
        referenced_namespace.nspname AS referenced_schema,
        referenced_class.relname AS referenced_relation,
        ARRAY(
            SELECT att.attname
            FROM unnest(con.confkey) WITH ORDINALITY AS key(attnum, ordinal)
            JOIN pg_catalog.pg_attribute AS att
              ON att.attrelid = con.confrelid
             AND att.attnum = key.attnum
            ORDER BY key.ordinal
        ) AS referenced_columns,
        ARRAY(
            SELECT att.attname
            FROM unnest(con.confdelsetcols) WITH ORDINALITY
                AS key(attnum, ordinal)
            JOIN pg_catalog.pg_attribute AS att
              ON att.attrelid = con.conrelid
             AND att.attnum = key.attnum
            ORDER BY key.ordinal
        ) AS delete_set_columns,
        CASE
            WHEN con.contype = 'c'
            THEN pg_catalog.pg_get_expr(con.conbin, con.conrelid, false)
        END AS check_expression,
        con.convalidated AS validated,
        con.connoinherit AS no_inherit,
        con.condeferrable AS deferrable,
        con.condeferred AS initially_deferred,
        COALESCE(backing_index.indnullsnotdistinct, false)
            AS nulls_not_distinct,
        COALESCE(
            (pg_catalog.to_jsonb(con) ->> 'conenforced')::boolean,
            true
        ) AS enforced,
        COALESCE(
            (pg_catalog.to_jsonb(con) ->> 'conperiod')::boolean,
            false
        ) AS period,
        CASE con.confmatchtype
            WHEN 'f' THEN 'full'
            WHEN 'p' THEN 'partial'
            WHEN 's' THEN 'simple'
        END AS match_type,
        CASE con.confupdtype
            WHEN 'a' THEN 'no_action'
            WHEN 'r' THEN 'restrict'
            WHEN 'c' THEN 'cascade'
            WHEN 'n' THEN 'set_null'
            WHEN 'd' THEN 'set_default'
        END AS on_update,
        CASE con.confdeltype
            WHEN 'a' THEN 'no_action'
            WHEN 'r' THEN 'restrict'
            WHEN 'c' THEN 'cascade'
            WHEN 'n' THEN 'set_null'
            WHEN 'd' THEN 'set_default'
        END AS on_delete
    FROM pg_catalog.pg_constraint AS con
    JOIN pg_catalog.pg_class AS c ON c.oid = con.conrelid
    JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
    LEFT JOIN pg_catalog.pg_class AS referenced_class
      ON referenced_class.oid = con.confrelid
    LEFT JOIN pg_catalog.pg_namespace AS referenced_namespace
      ON referenced_namespace.oid = referenced_class.relnamespace
    LEFT JOIN pg_catalog.pg_index AS backing_index
      ON backing_index.indexrelid = con.conindid
    WHERE n.nspname = $1
      AND c.relname = $2
      AND con.contype IN ('p', 'u', 'f', 'c')
    ORDER BY constraint_kind, con.conname
";

const POLICIES_SQL: &str = r"
    SELECT
        policy.polname AS policy_name,
        CASE policy.polcmd
            WHEN 'r' THEN 'select'
            WHEN 'a' THEN 'insert'
            WHEN 'w' THEN 'update'
            WHEN 'd' THEN 'delete'
            WHEN '*' THEN 'all'
        END AS command,
        policy.polpermissive AS permissive,
        ARRAY(
            SELECT CASE role_oid
                WHEN 0 THEN 'public'
                ELSE role.rolname
            END
            FROM unnest(policy.polroles) AS policy_role(role_oid)
            LEFT JOIN pg_catalog.pg_roles AS role ON role.oid = role_oid
            ORDER BY CASE role_oid
                WHEN 0 THEN 'public'
                ELSE role.rolname
            END
        ) AS roles,
        pg_catalog.pg_get_expr(policy.polqual, policy.polrelid, false)
            AS using_expression,
        pg_catalog.pg_get_expr(policy.polwithcheck, policy.polrelid, false)
            AS with_check_expression
    FROM pg_catalog.pg_policy AS policy
    JOIN pg_catalog.pg_class AS c ON c.oid = policy.polrelid
    JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace
    WHERE n.nspname = $1
      AND c.relname = $2
    ORDER BY policy.polname
";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSnapshot {
    pub schema_version: String,
    pub server_version_num: u32,
    pub current_database: String,
    pub current_role: String,
    pub role_context: CatalogRoleContext,
    pub relations: Vec<CatalogRelation>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRoleContext {
    pub inherit: bool,
    pub superuser: bool,
    pub bypass_rls: bool,
    pub effective_roles: Vec<String>,
    pub settable_roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRelation {
    pub schema: String,
    pub name: String,
    pub kind: RelationKind,
    pub owner: String,
    pub view: Option<CatalogView>,
    pub comment: Option<String>,
    pub grants: RelationGrantHints,
    pub rls: RowLevelSecurity,
    pub columns: Vec<CatalogColumn>,
    pub constraints: Vec<CatalogConstraint>,
    pub policies: Vec<RowLevelSecurityPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogView {
    pub definition_hash: String,
    pub security_invoker: bool,
    pub security_barrier: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Table,
    PartitionedTable,
    View,
    MaterializedView,
    ForeignTable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationGrantHints {
    pub schema_usage: bool,
    pub table_select: bool,
    pub any_column_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowLevelSecurity {
    pub enabled: bool,
    pub forced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogColumn {
    pub name: String,
    pub ordinal: u32,
    pub data_type: String,
    pub nullable: bool,
    pub comment: Option<String>,
    pub select_grant: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogConstraint {
    PrimaryKey {
        name: String,
        columns: Vec<String>,
        enforced: bool,
        period: bool,
        deferrable: bool,
        initially_deferred: bool,
        validated: bool,
    },
    Unique {
        name: String,
        columns: Vec<String>,
        nulls_not_distinct: bool,
        enforced: bool,
        period: bool,
        deferrable: bool,
        initially_deferred: bool,
        validated: bool,
    },
    ForeignKey {
        name: String,
        columns: Vec<String>,
        referenced_relation: RelationReference,
        referenced_columns: Vec<String>,
        delete_set_columns: Vec<String>,
        match_type: ForeignKeyMatch,
        on_update: ForeignKeyAction,
        on_delete: ForeignKeyAction,
        enforced: bool,
        period: bool,
        deferrable: bool,
        initially_deferred: bool,
        validated: bool,
    },
    Check {
        name: String,
        columns: Vec<String>,
        expression_hash: String,
        no_inherit: bool,
        enforced: bool,
        validated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationReference {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignKeyMatch {
    Full,
    Partial,
    Simple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeignKeyAction {
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowLevelSecurityPolicy {
    pub name: String,
    pub command: PolicyCommand,
    pub permissive: bool,
    pub roles: Vec<String>,
    pub using_expression_hash: Option<String>,
    pub with_check_expression_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyCommand {
    All,
    Select,
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("unsupported catalog snapshot schema version: {0}")]
    UnsupportedSchemaVersion(String),
    #[error("catalog snapshot fingerprint does not match its canonical contents")]
    InvalidFingerprint,
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
    #[error("failed to start read-only catalog transaction")]
    StartTransaction(#[source] postgres::Error),
    #[error("failed to read catalog {operation}")]
    Query {
        operation: &'static str,
        #[source]
        source: postgres::Error,
    },
    #[error("catalog returned unsupported {field}: {value}")]
    UnsupportedValue { field: &'static str, value: String },
    #[error("catalog returned invalid server_version_num: {0}")]
    InvalidServerVersion(i32),
    #[error("catalog returned invalid column ordinal: {0}")]
    InvalidColumnOrdinal(i32),
    #[error("catalog constraint is missing referenced relation metadata")]
    MissingReferencedRelation,
    #[error("catalog constraint is missing a CHECK expression")]
    MissingCheckExpression,
    #[error("failed to commit catalog transaction")]
    Commit(#[source] postgres::Error),
    #[error("failed to serialize catalog snapshot")]
    Serialization(#[source] serde_json::Error),
}

pub fn scan_from_env(variable: &str) -> Result<CatalogSnapshot, CatalogError> {
    let database_url = env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => CatalogError::MissingConnectionUrl(variable.to_owned()),
        env::VarError::NotUnicode(_) => CatalogError::InvalidConnectionUrl(variable.to_owned()),
    })?;
    let mut client =
        database::connect(&database_url, None).map_err(|source| CatalogError::Connect {
            variable: variable.to_owned(),
            source,
        })?;
    scan(&mut client)
}

fn scan(client: &mut Client) -> Result<CatalogSnapshot, CatalogError> {
    let mut transaction = client
        .build_transaction()
        .isolation_level(IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .map_err(CatalogError::StartTransaction)?;
    transaction
        .batch_execute("SET LOCAL search_path = pg_catalog")
        .map_err(|source| query_error("search path", source))?;

    let metadata = transaction
        .query_one(
            "SELECT current_setting('server_version_num')::integer, \
             current_database(), current_role",
            &[],
        )
        .map_err(|source| query_error("metadata", source))?;
    let server_version: i32 = metadata.get(0);
    let server_version_num = u32::try_from(server_version)
        .map_err(|_| CatalogError::InvalidServerVersion(server_version))?;
    let role_context = transaction
        .query_one(ROLE_CONTEXT_SQL, &[])
        .map_err(|source| query_error("role context", source))?;

    let mut snapshot = CatalogSnapshot {
        schema_version: CATALOG_SNAPSHOT_SCHEMA_VERSION.to_owned(),
        server_version_num,
        current_database: metadata.get(1),
        current_role: metadata.get(2),
        role_context: CatalogRoleContext {
            inherit: role_context.get("inherit"),
            superuser: role_context.get("superuser"),
            bypass_rls: role_context.get("bypass_rls"),
            effective_roles: role_context.get("effective_roles"),
            settable_roles: role_context.get("settable_roles"),
        },
        relations: scan_relations(&mut transaction)?,
        fingerprint: String::new(),
    };
    snapshot.normalize();
    snapshot.fingerprint = snapshot.calculate_fingerprint()?;

    transaction.commit().map_err(CatalogError::Commit)?;
    Ok(snapshot)
}

fn scan_relations(client: &mut impl GenericClient) -> Result<Vec<CatalogRelation>, CatalogError> {
    let rows = client
        .query(RELATIONS_SQL, &[])
        .map_err(|source| query_error("relations", source))?;
    rows.iter()
        .map(|row| {
            let schema: String = row.get("schema_name");
            let name: String = row.get("relation_name");
            Ok(CatalogRelation {
                columns: scan_columns(client, &schema, &name)?,
                constraints: scan_constraints(client, &schema, &name)?,
                policies: scan_policies(client, &schema, &name)?,
                schema,
                name,
                kind: parse_relation_kind(row.get("relation_kind"))?,
                owner: row.get("relation_owner"),
                view: view_from_row(row),
                comment: row.get("relation_comment"),
                grants: RelationGrantHints {
                    schema_usage: row.get("schema_usage"),
                    table_select: row.get("table_select"),
                    any_column_select: row.get("any_column_select"),
                },
                rls: RowLevelSecurity {
                    enabled: row.get("rls_enabled"),
                    forced: row.get("rls_forced"),
                },
            })
        })
        .collect()
}

fn view_from_row(row: &Row) -> Option<CatalogView> {
    let definition: Option<String> = row.get("view_definition");
    definition.map(|definition| CatalogView {
        definition_hash: hash_expression(&definition),
        security_invoker: row
            .get::<_, Option<bool>>("view_security_invoker")
            .unwrap_or(false),
        security_barrier: row
            .get::<_, Option<bool>>("view_security_barrier")
            .unwrap_or(false),
    })
}

fn scan_columns(
    client: &mut impl GenericClient,
    schema: &str,
    relation: &str,
) -> Result<Vec<CatalogColumn>, CatalogError> {
    client
        .query(COLUMNS_SQL, &[&schema, &relation])
        .map_err(|source| query_error("columns", source))?
        .iter()
        .map(|row| {
            let ordinal: i32 = row.get("ordinal");
            Ok(CatalogColumn {
                name: row.get("column_name"),
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| CatalogError::InvalidColumnOrdinal(ordinal))?,
                data_type: row.get("data_type"),
                nullable: row.get("nullable"),
                comment: row.get("column_comment"),
                select_grant: row.get("column_select"),
            })
        })
        .collect()
}

fn scan_constraints(
    client: &mut impl GenericClient,
    schema: &str,
    relation: &str,
) -> Result<Vec<CatalogConstraint>, CatalogError> {
    client
        .query(CONSTRAINTS_SQL, &[&schema, &relation])
        .map_err(|source| query_error("constraints", source))?
        .iter()
        .map(constraint_from_row)
        .collect()
}

fn constraint_from_row(row: &Row) -> Result<CatalogConstraint, CatalogError> {
    let kind: String = row.get("constraint_kind");
    let name = row.get("constraint_name");
    let columns = row.get("columns");
    let validated = row.get("validated");
    let deferrable = row.get("deferrable");
    let initially_deferred = row.get("initially_deferred");
    let enforced = row.get("enforced");
    let period = row.get("period");

    match kind.as_str() {
        "primary_key" => Ok(CatalogConstraint::PrimaryKey {
            name,
            columns,
            enforced,
            period,
            deferrable,
            initially_deferred,
            validated,
        }),
        "unique" => Ok(CatalogConstraint::Unique {
            name,
            columns,
            nulls_not_distinct: row.get("nulls_not_distinct"),
            enforced,
            period,
            deferrable,
            initially_deferred,
            validated,
        }),
        "foreign_key" => {
            let referenced_schema: Option<String> = row.get("referenced_schema");
            let referenced_name: Option<String> = row.get("referenced_relation");
            Ok(CatalogConstraint::ForeignKey {
                name,
                columns,
                referenced_relation: RelationReference {
                    schema: referenced_schema.ok_or(CatalogError::MissingReferencedRelation)?,
                    name: referenced_name.ok_or(CatalogError::MissingReferencedRelation)?,
                },
                referenced_columns: row.get("referenced_columns"),
                delete_set_columns: row.get("delete_set_columns"),
                match_type: parse_foreign_key_match(row.get("match_type"))?,
                on_update: parse_foreign_key_action(row.get("on_update"))?,
                on_delete: parse_foreign_key_action(row.get("on_delete"))?,
                enforced,
                period,
                deferrable,
                initially_deferred,
                validated,
            })
        }
        "check" => {
            let expression: Option<String> = row.get("check_expression");
            Ok(check_constraint(
                name,
                columns,
                expression
                    .as_deref()
                    .ok_or(CatalogError::MissingCheckExpression)?,
                row.get("no_inherit"),
                enforced,
                validated,
            ))
        }
        _ => Err(CatalogError::UnsupportedValue {
            field: "constraint kind",
            value: kind,
        }),
    }
}

fn scan_policies(
    client: &mut impl GenericClient,
    schema: &str,
    relation: &str,
) -> Result<Vec<RowLevelSecurityPolicy>, CatalogError> {
    client
        .query(POLICIES_SQL, &[&schema, &relation])
        .map_err(|source| query_error("RLS policies", source))?
        .iter()
        .map(policy_from_row)
        .collect()
}

fn policy_from_row(row: &Row) -> Result<RowLevelSecurityPolicy, CatalogError> {
    let using_expression: Option<String> = row.get("using_expression");
    let with_check_expression: Option<String> = row.get("with_check_expression");
    let mut roles: Vec<String> = row.get("roles");
    roles.sort();

    Ok(policy(
        row.get("policy_name"),
        parse_policy_command(row.get("command"))?,
        row.get("permissive"),
        roles,
        using_expression.as_deref(),
        with_check_expression.as_deref(),
    ))
}

fn parse_relation_kind(value: String) -> Result<RelationKind, CatalogError> {
    match value.as_str() {
        "table" => Ok(RelationKind::Table),
        "partitioned_table" => Ok(RelationKind::PartitionedTable),
        "view" => Ok(RelationKind::View),
        "materialized_view" => Ok(RelationKind::MaterializedView),
        "foreign_table" => Ok(RelationKind::ForeignTable),
        _ => Err(CatalogError::UnsupportedValue {
            field: "relation kind",
            value,
        }),
    }
}

fn parse_foreign_key_match(value: String) -> Result<ForeignKeyMatch, CatalogError> {
    match value.as_str() {
        "full" => Ok(ForeignKeyMatch::Full),
        "partial" => Ok(ForeignKeyMatch::Partial),
        "simple" => Ok(ForeignKeyMatch::Simple),
        _ => Err(CatalogError::UnsupportedValue {
            field: "foreign key match type",
            value,
        }),
    }
}

fn parse_foreign_key_action(value: String) -> Result<ForeignKeyAction, CatalogError> {
    match value.as_str() {
        "no_action" => Ok(ForeignKeyAction::NoAction),
        "restrict" => Ok(ForeignKeyAction::Restrict),
        "cascade" => Ok(ForeignKeyAction::Cascade),
        "set_null" => Ok(ForeignKeyAction::SetNull),
        "set_default" => Ok(ForeignKeyAction::SetDefault),
        _ => Err(CatalogError::UnsupportedValue {
            field: "foreign key action",
            value,
        }),
    }
}

fn parse_policy_command(value: String) -> Result<PolicyCommand, CatalogError> {
    match value.as_str() {
        "all" => Ok(PolicyCommand::All),
        "select" => Ok(PolicyCommand::Select),
        "insert" => Ok(PolicyCommand::Insert),
        "update" => Ok(PolicyCommand::Update),
        "delete" => Ok(PolicyCommand::Delete),
        _ => Err(CatalogError::UnsupportedValue {
            field: "policy command",
            value,
        }),
    }
}

fn query_error(operation: &'static str, source: postgres::Error) -> CatalogError {
    CatalogError::Query { operation, source }
}

fn hash_expression(expression: &str) -> String {
    sha256(expression)
}

fn check_constraint(
    name: String,
    columns: Vec<String>,
    expression: &str,
    no_inherit: bool,
    enforced: bool,
    validated: bool,
) -> CatalogConstraint {
    CatalogConstraint::Check {
        name,
        columns,
        expression_hash: hash_expression(expression),
        no_inherit,
        enforced,
        validated,
    }
}

fn policy(
    name: String,
    command: PolicyCommand,
    permissive: bool,
    mut roles: Vec<String>,
    using_expression: Option<&str>,
    with_check_expression: Option<&str>,
) -> RowLevelSecurityPolicy {
    roles.sort();
    RowLevelSecurityPolicy {
        name,
        command,
        permissive,
        roles,
        using_expression_hash: using_expression.map(hash_expression),
        with_check_expression_hash: with_check_expression.map(hash_expression),
    }
}

impl CatalogSnapshot {
    fn normalize(&mut self) {
        self.role_context.effective_roles.sort();
        self.role_context.effective_roles.dedup();
        self.role_context.settable_roles.sort();
        self.role_context.settable_roles.dedup();
        for relation in &mut self.relations {
            relation.columns.sort_by(|left, right| {
                left.ordinal
                    .cmp(&right.ordinal)
                    .then(left.name.cmp(&right.name))
            });
            relation.constraints.sort_by(constraint_order);
            for policy in &mut relation.policies {
                policy.roles.sort();
            }
            relation.policies.sort_by(|left, right| {
                left.name.cmp(&right.name).then(
                    policy_command_order(left.command).cmp(&policy_command_order(right.command)),
                )
            });
        }
        self.relations.sort_by(|left, right| {
            left.schema
                .cmp(&right.schema)
                .then(left.name.cmp(&right.name))
                .then(left.kind.cmp(&right.kind))
        });
    }

    fn calculate_fingerprint(&self) -> Result<String, CatalogError> {
        let mut canonical = self.clone();
        canonical.fingerprint.clear();
        let bytes = serde_json::to_vec(&canonical).map_err(CatalogError::Serialization)?;
        Ok(sha256(bytes))
    }

    #[cfg(test)]
    pub(crate) fn finalize(mut self) -> Result<Self, CatalogError> {
        self.normalize();
        self.fingerprint = self.calculate_fingerprint()?;
        Ok(self)
    }

    pub(crate) fn validated_normalized(&self) -> Result<Self, CatalogError> {
        if self.schema_version != CATALOG_SNAPSHOT_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        let mut canonical = self.clone();
        canonical.normalize();
        if canonical.calculate_fingerprint()? != self.fingerprint {
            return Err(CatalogError::InvalidFingerprint);
        }
        Ok(canonical)
    }
}

fn constraint_order(left: &CatalogConstraint, right: &CatalogConstraint) -> Ordering {
    constraint_sort_key(left).cmp(&constraint_sort_key(right))
}

fn constraint_sort_key(constraint: &CatalogConstraint) -> (u8, &str) {
    match constraint {
        CatalogConstraint::PrimaryKey { name, .. } => (0, name),
        CatalogConstraint::Unique { name, .. } => (1, name),
        CatalogConstraint::ForeignKey { name, .. } => (2, name),
        CatalogConstraint::Check { name, .. } => (3, name),
    }
}

const fn policy_command_order(command: PolicyCommand) -> u8 {
    match command {
        PolicyCommand::All => 0,
        PolicyCommand::Select => 1,
        PolicyCommand::Insert => 2,
        PolicyCommand::Update => 3,
        PolicyCommand::Delete => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CatalogColumn, CatalogError, CatalogRelation, CatalogRoleContext, CatalogSnapshot,
        PolicyCommand, RelationGrantHints, RelationKind, RowLevelSecurity, check_constraint,
        hash_expression, policy,
    };

    const RAW_CHECK: &str = "amount > 0 AND secret_check(amount)";
    const RAW_POLICY: &str = "tenant_id = current_setting('app.secret_tenant')";

    #[test]
    fn fingerprint_is_deterministic_after_normalization() -> Result<(), CatalogError> {
        let first = snapshot_with_relation_order(["zeta", "alpha"])?;
        let second = snapshot_with_relation_order(["alpha", "zeta"])?;

        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.relations, second.relations);
        Ok(())
    }

    #[test]
    fn serialized_snapshot_excludes_raw_check_and_policy_expressions()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = snapshot_with_relation_order(["alpha", "zeta"])?;
        let serialized = serde_json::to_string(&snapshot)?;

        assert!(!serialized.contains(RAW_CHECK));
        assert!(!serialized.contains(RAW_POLICY));
        assert!(serialized.contains(&hash_expression(RAW_CHECK)));
        assert!(serialized.contains(&hash_expression(RAW_POLICY)));
        Ok(())
    }

    #[test]
    fn rejects_catalog_snapshots_without_authorization_context() -> Result<(), CatalogError> {
        let mut snapshot = snapshot_with_relation_order(["alpha", "zeta"])?;
        snapshot.schema_version = "1".to_owned();
        snapshot.fingerprint.clear();
        snapshot = snapshot.finalize()?;

        assert!(matches!(
            snapshot.validated_normalized(),
            Err(CatalogError::UnsupportedSchemaVersion(version)) if version == "1"
        ));
        Ok(())
    }

    fn snapshot_with_relation_order(names: [&str; 2]) -> Result<CatalogSnapshot, CatalogError> {
        let relations = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| relation(name, index == 0))
            .collect::<Vec<CatalogRelation>>();
        CatalogSnapshot {
            schema_version: "2".to_owned(),
            server_version_num: 180_000,
            current_database: "app".to_owned(),
            current_role: "postgresem_introspector".to_owned(),
            role_context: CatalogRoleContext {
                inherit: true,
                superuser: false,
                bypass_rls: false,
                effective_roles: vec![
                    "postgresem_introspector".to_owned(),
                    "postgresem_reader".to_owned(),
                ],
                settable_roles: vec!["postgresem_reader".to_owned()],
            },
            relations,
            fingerprint: String::new(),
        }
        .finalize()
    }

    fn relation(name: &str, reverse_nested_arrays: bool) -> CatalogRelation {
        let mut relation = CatalogRelation {
            schema: "public".to_owned(),
            name: name.to_owned(),
            kind: RelationKind::Table,
            owner: "source_owner".to_owned(),
            view: None,
            comment: Some("business relation".to_owned()),
            grants: RelationGrantHints {
                schema_usage: true,
                table_select: true,
                any_column_select: true,
            },
            rls: RowLevelSecurity {
                enabled: true,
                forced: true,
            },
            columns: vec![
                CatalogColumn {
                    name: "amount".to_owned(),
                    ordinal: 2,
                    data_type: "numeric".to_owned(),
                    nullable: false,
                    comment: None,
                    select_grant: true,
                },
                CatalogColumn {
                    name: "tenant_id".to_owned(),
                    ordinal: 1,
                    data_type: "uuid".to_owned(),
                    nullable: false,
                    comment: None,
                    select_grant: true,
                },
            ],
            constraints: vec![check_constraint(
                "positive_amount".to_owned(),
                vec!["amount".to_owned()],
                RAW_CHECK,
                false,
                true,
                true,
            )],
            policies: vec![policy(
                "tenant_isolation".to_owned(),
                PolicyCommand::All,
                true,
                vec!["tenant_b".to_owned(), "tenant_a".to_owned()],
                Some(RAW_POLICY),
                None,
            )],
        };
        if reverse_nested_arrays {
            relation.columns.reverse();
            relation.policies[0].roles.reverse();
        }
        relation
    }
}
