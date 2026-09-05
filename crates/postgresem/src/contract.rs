use serde::Serialize;

use postgresem_compiler::{
    COMPILER_SEMANTIC_VERSION, LSM_SCHEMA_VERSION, LSQ_SCHEMA_VERSION,
    MUTATION_COMPILER_SEMANTIC_VERSION, SUPPORTED_SNAPSHOT_SCHEMA_VERSIONS,
};

use crate::{
    authoring::AUTHORING_SCHEMA_VERSION,
    benchmark::BENCHMARK_SCHEMA_VERSION,
    catalog::CATALOG_SNAPSHOT_SCHEMA_VERSION,
    catalog_diff::CATALOG_DIFF_SCHEMA_VERSION,
    mcp::{MCP_PROTOCOL_VERSION, TOOL_SCHEMA_VERSION},
    mcp_http::MODERN_PROTOCOL_VERSION,
    osi::IMPORT_SCHEMA_VERSION,
};

const ERROR_TAXONOMY_VERSION: &str = "1";
const FIRST_MIGRATION: &str = "0001_semantic_schema";
const CURRENT_MIGRATION: &str = "0011_mutation_reconcile_writer_role";

#[derive(Debug, PartialEq, Serialize)]
pub struct ContractManifest {
    schema_version: &'static str,
    release: &'static str,
    contract_status: &'static str,
    contracts: Contracts,
    protocols: Protocols,
    cli_commands: [&'static str; 10],
    mcp_tools: McpTools,
    deprecated: [DeprecatedSurface; 1],
    deferred: [&'static str; 7],
}

#[derive(Debug, PartialEq, Serialize)]
struct Contracts {
    lsq: [&'static str; 1],
    lsm: [&'static str; 1],
    semantic_snapshot_load: [&'static str; 2],
    semantic_snapshot_author: &'static str,
    compiler_semantics: &'static str,
    mutation_compiler_semantics: &'static str,
    catalog_snapshot: &'static str,
    catalog_diff: &'static str,
    authoring_scaffold: &'static str,
    osi_import: &'static str,
    benchmark: &'static str,
    mcp_tool_schema: &'static str,
    error_taxonomy: &'static str,
    database_migrations: MigrationRange,
}

#[derive(Debug, PartialEq, Serialize)]
struct MigrationRange {
    first: &'static str,
    current: &'static str,
}

#[derive(Debug, PartialEq, Serialize)]
struct Protocols {
    mcp_stdio: &'static str,
    mcp_http: &'static str,
}

#[derive(Debug, PartialEq, Serialize)]
struct McpTools {
    query: [&'static str; 5],
    mutation: [&'static str; 3],
}

#[derive(Debug, PartialEq, Serialize)]
struct DeprecatedSurface {
    surface: &'static str,
    replacement: &'static str,
    removal_before: Option<&'static str>,
}

pub fn manifest() -> ContractManifest {
    ContractManifest {
        schema_version: "1",
        release: env!("CARGO_PKG_VERSION"),
        contract_status: "stable",
        contracts: Contracts {
            lsq: [LSQ_SCHEMA_VERSION],
            lsm: [LSM_SCHEMA_VERSION],
            semantic_snapshot_load: SUPPORTED_SNAPSHOT_SCHEMA_VERSIONS,
            semantic_snapshot_author: "2",
            compiler_semantics: COMPILER_SEMANTIC_VERSION,
            mutation_compiler_semantics: MUTATION_COMPILER_SEMANTIC_VERSION,
            catalog_snapshot: CATALOG_SNAPSHOT_SCHEMA_VERSION,
            catalog_diff: CATALOG_DIFF_SCHEMA_VERSION,
            authoring_scaffold: AUTHORING_SCHEMA_VERSION,
            osi_import: IMPORT_SCHEMA_VERSION,
            benchmark: BENCHMARK_SCHEMA_VERSION,
            mcp_tool_schema: TOOL_SCHEMA_VERSION,
            error_taxonomy: ERROR_TAXONOMY_VERSION,
            database_migrations: MigrationRange {
                first: FIRST_MIGRATION,
                current: CURRENT_MIGRATION,
            },
        },
        protocols: Protocols {
            mcp_stdio: MCP_PROTOCOL_VERSION,
            mcp_http: MODERN_PROTOCOL_VERSION,
        },
        cli_commands: [
            "benchmark",
            "catalog",
            "contract",
            "doctor",
            "mcp",
            "model",
            "mutation",
            "query",
            "report",
            "snapshot",
        ],
        mcp_tools: McpTools {
            query: [
                "describe_semantic_model",
                "explain_semantic_query",
                "list_semantic_models",
                "query_semantic_model",
                "validate_semantic_query",
            ],
            mutation: [
                "mutate_semantic_model",
                "reconcile_semantic_mutation",
                "validate_semantic_mutation",
            ],
        },
        deprecated: [DeprecatedSurface {
            surface: "report beta",
            replacement: "report operations",
            removal_before: Some("2.0.0"),
        }],
        deferred: [
            "automatic_materialized_view_routing",
            "connection_pooling",
            "distributed_rate_limits",
            "down_migrations",
            "dynamic_oidc_or_jwks_discovery",
            "general_update_delete",
            "pre_aggregation",
        ],
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::manifest;

    #[test]
    fn manifest_matches_frozen_stable_contract() {
        let expected: Value =
            serde_json::from_str(include_str!("../../../contracts/stable-v1.json"))
                .expect("manifest");
        assert_eq!(
            serde_json::to_value(manifest()).expect("serialize manifest"),
            expected["manifest"]
        );
    }
}
