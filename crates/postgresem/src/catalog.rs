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
        owner.rolinherit AS owner_inherit,
        owner.rolsuper AS owner_superuser,
        owner.rolbypassrls AS owner_bypass_rls,
        ARRAY(
            SELECT candidate.rolname
            FROM pg_catalog.pg_roles AS candidate
            WHERE pg_catalog.pg_has_role(owner.oid, candidate.oid, 'USAGE')
            ORDER BY candidate.rolname
        ) AS owner_effective_roles,
        ARRAY(
            SELECT candidate.rolname
            FROM pg_catalog.pg_roles AS candidate
            WHERE pg_catalog.pg_has_role(owner.oid, candidate.oid, 'SET')
            ORDER BY candidate.rolname
        ) AS owner_settable_roles,
        CASE
            WHEN c.relkind IN ('v', 'm')
            THEN pg_catalog.pg_get_viewdef(c.oid, false)
        END AS view_definition,
        CASE
            WHEN c.relkind = 'v'
            THEN COALESCE(
                (
                    SELECT option_value::boolean
                    FROM pg_catalog.pg_options_to_table(c.reloptions)
                    WHERE option_name = 'security_invoker'
                ),
                false
            )
        END AS view_security_invoker,
        CASE
            WHEN c.relkind = 'v'
            THEN COALESCE(
                (
                    SELECT option_value::boolean
                    FROM pg_catalog.pg_options_to_table(c.reloptions)
                    WHERE option_name = 'security_barrier'
                ),
                false
            )
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

const FUNCTIONS_SQL: &str = r"
    SELECT
        p.oid::bigint AS function_oid,
        n.nspname AS schema_name,
        p.proname AS function_name,
        pg_catalog.pg_get_function_identity_arguments(p.oid)
            AS identity_arguments,
        CASE p.prokind
            WHEN 'a' THEN pg_catalog.jsonb_build_object(
                'kind', aggregate_definition.aggkind,
                'parallel', p.proparallel,
                'direct_arguments', aggregate_definition.aggnumdirectargs,
                'transition_function',
                    aggregate_definition.aggtransfn::oid::regprocedure::text,
                'final_function', CASE aggregate_definition.aggfinalfn
                    WHEN 0 THEN NULL
                    ELSE aggregate_definition.aggfinalfn::oid::regprocedure::text
                END,
                'combine_function', CASE aggregate_definition.aggcombinefn
                    WHEN 0 THEN NULL
                    ELSE aggregate_definition.aggcombinefn::oid::regprocedure::text
                END,
                'serialize_function', CASE aggregate_definition.aggserialfn
                    WHEN 0 THEN NULL
                    ELSE aggregate_definition.aggserialfn::oid::regprocedure::text
                END,
                'deserialize_function', CASE aggregate_definition.aggdeserialfn
                    WHEN 0 THEN NULL
                    ELSE aggregate_definition.aggdeserialfn::oid::regprocedure::text
                END,
                'moving_transition_function',
                    CASE aggregate_definition.aggmtransfn
                        WHEN 0 THEN NULL
                        ELSE aggregate_definition.aggmtransfn::oid::regprocedure::text
                    END,
                'moving_inverse_transition_function',
                    CASE aggregate_definition.aggminvtransfn
                        WHEN 0 THEN NULL
                        ELSE aggregate_definition.aggminvtransfn::oid::regprocedure::text
                    END,
                'moving_final_function',
                    CASE aggregate_definition.aggmfinalfn
                        WHEN 0 THEN NULL
                        ELSE aggregate_definition.aggmfinalfn::oid::regprocedure::text
                    END,
                'final_extra', aggregate_definition.aggfinalextra,
                'moving_final_extra', aggregate_definition.aggmfinalextra,
                'final_modify', aggregate_definition.aggfinalmodify,
                'moving_final_modify', aggregate_definition.aggmfinalmodify,
                'sort_operator', CASE aggregate_definition.aggsortop
                    WHEN 0 THEN NULL
                    ELSE aggregate_definition.aggsortop::regoperator::text
                END,
                'transition_type',
                    pg_catalog.format_type(
                        aggregate_definition.aggtranstype,
                        NULL
                    ),
                'transition_space', aggregate_definition.aggtransspace,
                'moving_transition_type',
                    CASE aggregate_definition.aggmtranstype
                        WHEN 0 THEN NULL
                        ELSE pg_catalog.format_type(
                            aggregate_definition.aggmtranstype,
                            NULL
                        )
                    END,
                'moving_transition_space', aggregate_definition.aggmtransspace,
                'initial_value', aggregate_definition.agginitval,
                'moving_initial_value', aggregate_definition.aggminitval
            )::text
            ELSE pg_catalog.pg_get_functiondef(p.oid)
        END AS function_definition,
        CASE p.prokind
            WHEN 'f' THEN 'function'
            WHEN 'w' THEN 'window_function'
            WHEN 'a' THEN 'aggregate'
        END AS function_kind,
        p.prosecdef AS security_definer,
        owner.rolname AS function_owner,
        owner.rolinherit AS owner_inherit,
        owner.rolsuper AS owner_superuser,
        owner.rolbypassrls AS owner_bypass_rls,
        ARRAY(
            SELECT candidate.rolname
            FROM pg_catalog.pg_roles AS candidate
            WHERE pg_catalog.pg_has_role(owner.oid, candidate.oid, 'USAGE')
            ORDER BY candidate.rolname
        ) AS owner_effective_roles,
        ARRAY(
            SELECT candidate.rolname
            FROM pg_catalog.pg_roles AS candidate
            WHERE pg_catalog.pg_has_role(owner.oid, candidate.oid, 'SET')
            ORDER BY candidate.rolname
        ) AS owner_settable_roles
    FROM pg_catalog.pg_proc AS p
    JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace
    JOIN pg_catalog.pg_roles AS owner ON owner.oid = p.proowner
    LEFT JOIN pg_catalog.pg_aggregate AS aggregate_definition
      ON aggregate_definition.aggfnoid = p.oid
    WHERE p.prokind IN ('f', 'w', 'a')
      AND n.nspname NOT IN ('pg_catalog', 'information_schema')
      AND n.nspname !~ '^pg_toast'
      AND n.nspname !~ '^pg_temp_'
    ORDER BY
        n.nspname,
        p.proname,
        pg_catalog.pg_get_function_identity_arguments(p.oid)
";

const OBJECT_PRIVILEGES_SQL: &str = r#"
    WITH acl_objects AS (
        SELECT
            'database'::text AS object_kind,
            ''::text AS schema_name,
            database_catalog.datname AS object_name,
            ''::text AS identity_arguments,
            database_catalog.datdba AS owner_oid,
            database_catalog.datacl AS object_acl,
            'd'::"char" AS acl_kind
        FROM pg_catalog.pg_database AS database_catalog
        WHERE database_catalog.datname = current_database()

        UNION ALL

        SELECT
            'schema',
            '',
            namespace.nspname,
            '',
            namespace.nspowner,
            namespace.nspacl,
            'n'::"char"
        FROM pg_catalog.pg_namespace AS namespace
        WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'

        UNION ALL

        SELECT
            CASE relation.relkind
                WHEN 'S' THEN 'sequence'
                ELSE 'relation'
            END,
            namespace.nspname,
            relation.relname,
            '',
            relation.relowner,
            relation.relacl,
            CASE relation.relkind
                WHEN 'S' THEN 's'::"char"
                ELSE 'r'::"char"
            END
        FROM pg_catalog.pg_class AS relation
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = relation.relnamespace
        WHERE relation.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
          AND namespace.nspname NOT IN ('pg_catalog', 'information_schema')
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'

        UNION ALL

        SELECT
            CASE routine.prokind
                WHEN 'a' THEN 'aggregate'
                WHEN 'p' THEN 'procedure'
                ELSE 'function'
            END,
            namespace.nspname,
            routine.proname,
            pg_catalog.pg_get_function_identity_arguments(routine.oid),
            routine.proowner,
            routine.proacl,
            'f'::"char"
        FROM pg_catalog.pg_proc AS routine
        JOIN pg_catalog.pg_namespace AS namespace
          ON namespace.oid = routine.pronamespace
        WHERE namespace.nspname NOT IN ('pg_catalog', 'information_schema')
          AND namespace.nspname !~ '^pg_toast'
          AND namespace.nspname !~ '^pg_temp_'
    )
    SELECT
        object.object_kind,
        object.schema_name,
        object.object_name,
        object.identity_arguments,
        owner.rolname AS owner,
        grantor.rolname AS grantor,
        CASE access.grantee
            WHEN 0 THEN 'public'
            ELSE grantee.rolname
        END AS grantee,
        access.privilege_type,
        access.is_grantable
    FROM acl_objects AS object
    JOIN pg_catalog.pg_roles AS owner ON owner.oid = object.owner_oid
    LEFT JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            object.object_acl,
            pg_catalog.acldefault(object.acl_kind, object.owner_oid)
        )
    ) AS access ON true
    LEFT JOIN pg_catalog.pg_roles AS grantor ON grantor.oid = access.grantor
    LEFT JOIN pg_catalog.pg_roles AS grantee ON grantee.oid = access.grantee
    ORDER BY
        object.object_kind,
        object.schema_name,
        object.object_name,
        object.identity_arguments,
        owner,
        grantee,
        grantor,
        access.privilege_type,
        access.is_grantable
"#;

const ROLE_GRAPH_SQL: &str = r"
    SELECT
        member.rolname AS member_role,
        member.rolinherit AS inherit,
        member.rolsuper AS superuser,
        member.rolbypassrls AS bypass_rls,
        granted.rolname AS granted_role,
        grantor.rolname AS grantor_role,
        membership.admin_option,
        CASE
            WHEN membership.roleid IS NULL THEN NULL
            ELSE COALESCE(
                (
                    pg_catalog.to_jsonb(membership)
                    ->> 'inherit_option'
                )::boolean,
                true
            )
        END AS inherit_option,
        CASE
            WHEN membership.roleid IS NULL THEN NULL
            ELSE COALESCE(
                (pg_catalog.to_jsonb(membership) ->> 'set_option')::boolean,
                true
            )
        END AS set_option
    FROM pg_catalog.pg_roles AS member
    LEFT JOIN pg_catalog.pg_auth_members AS membership
      ON membership.member = member.oid
    LEFT JOIN pg_catalog.pg_roles AS granted
      ON granted.oid = membership.roleid
    LEFT JOIN pg_catalog.pg_roles AS grantor
      ON grantor.oid = membership.grantor
    ORDER BY member.rolname, granted.rolname, grantor.rolname
";

const FUNCTION_GRANTS_SQL: &str = r"
    SELECT
        grantor.rolname AS grantor,
        CASE access.grantee
            WHEN 0 THEN 'public'
            ELSE grantee.rolname
        END AS grantee,
        access.is_grantable
    FROM pg_catalog.pg_proc AS p
    CROSS JOIN LATERAL pg_catalog.aclexplode(
        COALESCE(
            p.proacl,
            pg_catalog.acldefault('f', p.proowner)
        )
    ) AS access
    JOIN pg_catalog.pg_roles AS grantor ON grantor.oid = access.grantor
    LEFT JOIN pg_catalog.pg_roles AS grantee ON grantee.oid = access.grantee
    WHERE p.oid = $1::bigint::oid
      AND access.privilege_type = 'EXECUTE'
    ORDER BY grantee, grantor, access.is_grantable
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
    pub role_graph_fingerprint: String,
    pub object_privilege_fingerprint: String,
    pub functions: Vec<CatalogFunction>,
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
    pub owner_authorization: Option<CatalogRoleContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogFunction {
    pub schema: String,
    pub name: String,
    pub identity_arguments: String,
    pub kind: CatalogFunctionKind,
    pub definition_hash: String,
    pub owner: String,
    pub owner_authorization: Option<CatalogRoleContext>,
    pub grants: Vec<CatalogFunctionGrant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogFunctionKind {
    Function,
    WindowFunction,
    Aggregate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogFunctionGrant {
    pub grantor: String,
    pub grantee: String,
    pub grantable: bool,
}

#[derive(Debug, Serialize)]
struct ObjectPrivilegeEvidence {
    object_kind: String,
    schema: String,
    name: String,
    identity_arguments: String,
    owner: String,
    grantor: Option<String>,
    grantee: Option<String>,
    privilege: Option<String>,
    grantable: Option<bool>,
}

#[derive(Debug, Serialize)]
struct RoleGraphEvidence {
    member: String,
    inherit: bool,
    superuser: bool,
    bypass_rls: bool,
    granted_role: Option<String>,
    grantor_role: Option<String>,
    admin_option: Option<bool>,
    inherit_option: Option<bool>,
    set_option: Option<bool>,
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
        .batch_execute(
            "SET LOCAL search_path = pg_catalog; \
             SET LOCAL quote_all_identifiers = off",
        )
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
        role_graph_fingerprint: scan_role_graph_fingerprint(&mut transaction)?,
        object_privilege_fingerprint: scan_object_privilege_fingerprint(&mut transaction)?,
        functions: scan_functions(&mut transaction)?,
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
        owner_authorization: row
            .get::<_, Option<bool>>("view_security_invoker")
            .filter(|security_invoker| !security_invoker)
            .map(|_| role_context_from_owner_row(row)),
    })
}

fn scan_functions(client: &mut impl GenericClient) -> Result<Vec<CatalogFunction>, CatalogError> {
    client
        .query(FUNCTIONS_SQL, &[])
        .map_err(|source| query_error("functions", source))?
        .iter()
        .map(|row| {
            let oid: i64 = row.get("function_oid");
            Ok(CatalogFunction {
                schema: row.get("schema_name"),
                name: row.get("function_name"),
                identity_arguments: row.get("identity_arguments"),
                kind: parse_function_kind(row.get("function_kind"))?,
                definition_hash: hash_expression(
                    row.get::<_, String>("function_definition").as_str(),
                ),
                owner: row.get("function_owner"),
                owner_authorization: row
                    .get::<_, bool>("security_definer")
                    .then(|| role_context_from_owner_row(row)),
                grants: scan_function_grants(client, oid)?,
            })
        })
        .collect()
}

fn scan_object_privilege_fingerprint(
    client: &mut impl GenericClient,
) -> Result<String, CatalogError> {
    let evidence = client
        .query(OBJECT_PRIVILEGES_SQL, &[])
        .map_err(|source| query_error("object privileges", source))?
        .iter()
        .map(|row| ObjectPrivilegeEvidence {
            object_kind: row.get("object_kind"),
            schema: row.get("schema_name"),
            name: row.get("object_name"),
            identity_arguments: row.get("identity_arguments"),
            owner: row.get("owner"),
            grantor: row.get("grantor"),
            grantee: row.get("grantee"),
            privilege: row.get("privilege_type"),
            grantable: row.get("is_grantable"),
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::to_vec(&evidence).map_err(CatalogError::Serialization)?;
    Ok(sha256(canonical))
}

fn scan_role_graph_fingerprint(client: &mut impl GenericClient) -> Result<String, CatalogError> {
    let evidence = client
        .query(ROLE_GRAPH_SQL, &[])
        .map_err(|source| query_error("role graph", source))?
        .iter()
        .map(|row| RoleGraphEvidence {
            member: row.get("member_role"),
            inherit: row.get("inherit"),
            superuser: row.get("superuser"),
            bypass_rls: row.get("bypass_rls"),
            granted_role: row.get("granted_role"),
            grantor_role: row.get("grantor_role"),
            admin_option: row.get("admin_option"),
            inherit_option: row.get("inherit_option"),
            set_option: row.get("set_option"),
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::to_vec(&evidence).map_err(CatalogError::Serialization)?;
    Ok(sha256(canonical))
}

fn scan_function_grants(
    client: &mut impl GenericClient,
    oid: i64,
) -> Result<Vec<CatalogFunctionGrant>, CatalogError> {
    client
        .query(FUNCTION_GRANTS_SQL, &[&oid])
        .map_err(|source| query_error("function grants", source))?
        .iter()
        .map(|row| {
            Ok(CatalogFunctionGrant {
                grantor: row.get("grantor"),
                grantee: row.get("grantee"),
                grantable: row.get("is_grantable"),
            })
        })
        .collect()
}

fn role_context_from_owner_row(row: &Row) -> CatalogRoleContext {
    CatalogRoleContext {
        inherit: row.get("owner_inherit"),
        superuser: row.get("owner_superuser"),
        bypass_rls: row.get("owner_bypass_rls"),
        effective_roles: row.get("owner_effective_roles"),
        settable_roles: row.get("owner_settable_roles"),
    }
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

fn parse_function_kind(value: String) -> Result<CatalogFunctionKind, CatalogError> {
    match value.as_str() {
        "function" => Ok(CatalogFunctionKind::Function),
        "window_function" => Ok(CatalogFunctionKind::WindowFunction),
        "aggregate" => Ok(CatalogFunctionKind::Aggregate),
        _ => Err(CatalogError::UnsupportedValue {
            field: "function kind",
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
        normalize_role_context(&mut self.role_context);
        for function in &mut self.functions {
            if let Some(owner_authorization) = &mut function.owner_authorization {
                normalize_role_context(owner_authorization);
            }
            function.grants.sort_by(|left, right| {
                left.grantee
                    .cmp(&right.grantee)
                    .then(left.grantor.cmp(&right.grantor))
                    .then(left.grantable.cmp(&right.grantable))
            });
            function.grants.dedup();
        }
        self.functions.sort_by(|left, right| {
            left.schema
                .cmp(&right.schema)
                .then(left.name.cmp(&right.name))
                .then(left.identity_arguments.cmp(&right.identity_arguments))
        });
        for relation in &mut self.relations {
            if let Some(view) = &mut relation.view {
                if let Some(owner_authorization) = &mut view.owner_authorization {
                    normalize_role_context(owner_authorization);
                }
            }
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

fn normalize_role_context(context: &mut CatalogRoleContext) {
    context.effective_roles.sort();
    context.effective_roles.dedup();
    context.settable_roles.sort();
    context.settable_roles.dedup();
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
        CatalogColumn, CatalogError, CatalogFunction, CatalogFunctionKind, CatalogRelation,
        CatalogRoleContext, CatalogSnapshot, PolicyCommand, RelationGrantHints, RelationKind,
        RowLevelSecurity, check_constraint, hash_expression, policy,
    };

    const RAW_CHECK: &str = "amount > 0 AND secret_check(amount)";
    const RAW_POLICY: &str = "tenant_id = current_setting('app.secret_tenant')";
    const RAW_FUNCTION: &str = "SELECT current_setting('app.secret_value')";

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
        let mut snapshot = snapshot_with_relation_order(["alpha", "zeta"])?;
        snapshot.functions.push(CatalogFunction {
            schema: "security".to_owned(),
            name: "secret_check".to_owned(),
            identity_arguments: "numeric".to_owned(),
            kind: CatalogFunctionKind::Function,
            definition_hash: hash_expression(RAW_FUNCTION),
            owner: "source_owner".to_owned(),
            owner_authorization: None,
            grants: Vec::new(),
        });
        snapshot.fingerprint.clear();
        let snapshot = snapshot.finalize()?;
        let serialized = serde_json::to_string(&snapshot)?;

        assert!(!serialized.contains(RAW_CHECK));
        assert!(!serialized.contains(RAW_POLICY));
        assert!(!serialized.contains(RAW_FUNCTION));
        assert!(serialized.contains(&hash_expression(RAW_CHECK)));
        assert!(serialized.contains(&hash_expression(RAW_POLICY)));
        assert!(serialized.contains(&hash_expression(RAW_FUNCTION)));
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
            role_graph_fingerprint: "sha256:role-graph".to_owned(),
            object_privilege_fingerprint: "sha256:privileges".to_owned(),
            functions: Vec::new(),
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
