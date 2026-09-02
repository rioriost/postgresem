use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    io::{self, BufRead, Write},
    time::Instant,
};

use postgresem_compiler::{
    CompileError, CompilerOptions, DataType, Lineage, LogicalSemanticMutation,
    LogicalSemanticQuery, LsmError, LsqError, Model, MutationCompileError, MutationCompilerOptions,
    NormalizedLsm, NormalizedLsq, OutputColumn, SemanticSnapshot, compile_lsm, compile_lsq,
    normalize_lsm, normalize_lsq,
};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    executor::{self, ExecutionContext, ExecutorConfig, QueryResult},
    mutation_executor::{self, MutationExecutorConfig, MutationResult},
    published_model,
};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const TOOL_SCHEMA_VERSION: &str = "1";
const MAX_REQUEST_LINE_BYTES: usize = 1_048_576;
const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 100;
const MAX_MODEL_CURSOR_LENGTH: usize = 128;
const LSQ_SCHEMA: &str = include_str!("../../../schemas/lsq/v1.schema.json");
const LSM_SCHEMA: &str = include_str!("../../../schemas/lsm/v1.schema.json");

#[derive(Debug, Error)]
pub enum McpServerError {
    #[error("required environment variable {0} is not set")]
    MissingEnvironment(String),
    #[error("required environment variable {0} is not valid Unicode")]
    InvalidEnvironment(String),
    #[error("MCP project must not be empty")]
    InvalidProject,
    #[error(transparent)]
    ExecutorConfiguration(#[from] executor::ExecuteError),
    #[error(transparent)]
    MutationExecutorConfiguration(#[from] mutation_executor::MutationExecuteError),
    #[error("MCP stdio I/O failed")]
    Io(#[from] io::Error),
    #[error("failed to serialize an MCP protocol message")]
    Serialization(#[from] serde_json::Error),
}

struct McpConfig {
    project: String,
    executor: ExecutorConfig,
    mutation_executor: Option<MutationExecutorConfig>,
    execution_context: ExecutionContext,
}

impl McpConfig {
    fn from_environment() -> Result<Self, McpServerError> {
        let project = required_environment("POSTGRESEM_MCP_PROJECT")?;
        if project.trim().is_empty() {
            return Err(McpServerError::InvalidProject);
        }
        let runtime_url_environment =
            environment_or("POSTGRESEM_MCP_RUNTIME_URL_ENV", "DATABASE_URL")?;
        let audit_url_environment = environment_or(
            "POSTGRESEM_MCP_AUDIT_URL_ENV",
            "POSTGRESEM_AUDIT_DATABASE_URL",
        )?;
        let database_role_environment =
            environment_or("POSTGRESEM_MCP_DB_ROLE_ENV", "POSTGRESEM_DB_ROLE")?;
        let runtime_password_environment = environment_or(
            "POSTGRESEM_MCP_RUNTIME_PASSWORD_ENV",
            "POSTGRESEM_RUNTIME_PASSWORD",
        )?;
        let audit_password_environment = environment_or(
            "POSTGRESEM_MCP_AUDIT_PASSWORD_ENV",
            "POSTGRESEM_AUDIT_WRITER_PASSWORD",
        )?;
        let executor = ExecutorConfig::from_environment_with_passwords(
            &runtime_url_environment,
            Some(&runtime_password_environment),
            &audit_url_environment,
            Some(&audit_password_environment),
            &database_role_environment,
        )?;
        let mutation_executor = match env::var("POSTGRESEM_MCP_MUTATION_URL_ENV") {
            Ok(mutation_url_environment) => {
                let mutation_password_environment = environment_or(
                    "POSTGRESEM_MCP_MUTATION_PASSWORD_ENV",
                    "POSTGRESEM_MUTATION_RUNTIME_PASSWORD",
                )?;
                let mutation_role_environment = environment_or(
                    "POSTGRESEM_MCP_MUTATION_DB_ROLE_ENV",
                    "POSTGRESEM_MUTATION_DB_ROLE",
                )?;
                Some(MutationExecutorConfig::from_environment_with_passwords(
                    &mutation_url_environment,
                    Some(&mutation_password_environment),
                    &audit_url_environment,
                    Some(&audit_password_environment),
                    &mutation_role_environment,
                )?)
            }
            Err(env::VarError::NotPresent) => None,
            Err(env::VarError::NotUnicode(_)) => {
                return Err(McpServerError::InvalidEnvironment(
                    "POSTGRESEM_MCP_MUTATION_URL_ENV".to_owned(),
                ));
            }
        };
        let execution_context = ExecutionContext::new("mcp:stdio", "mcp-stdio")?;
        Ok(Self {
            project,
            executor,
            mutation_executor,
            execution_context,
        })
    }
}

pub fn serve() -> Result<(), McpServerError> {
    let config = McpConfig::from_environment()?;
    let server = McpServer { config };
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_io(stdin.lock(), stdout.lock(), &server)
}

fn serve_io<R: BufRead, W: Write>(
    mut reader: R,
    mut writer: W,
    server: &McpServer,
) -> Result<(), McpServerError> {
    loop {
        match read_bounded_line(&mut reader, MAX_REQUEST_LINE_BYTES)? {
            LineRead::Eof => return Ok(()),
            LineRead::Oversized => {
                log_request(
                    "unknown",
                    None,
                    "error",
                    "MCP_REQUEST_TOO_LARGE",
                    Instant::now(),
                );
                write_message(
                    &mut writer,
                    &rpc_error(
                        Value::Null,
                        -32600,
                        "request line is too large",
                        "MCP_REQUEST_TOO_LARGE",
                    ),
                )?;
            }
            LineRead::Line(line) => {
                if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                    continue;
                }
                if let Some(response) = server.handle_line(&line) {
                    write_message(&mut writer, &response)?;
                }
            }
        }
    }
}

fn write_message(writer: &mut impl Write, message: &Value) -> Result<(), McpServerError> {
    serde_json::to_writer(&mut *writer, message)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

enum LineRead {
    Eof,
    Line(Vec<u8>),
    Oversized,
}

fn read_bounded_line(reader: &mut impl BufRead, maximum_bytes: usize) -> io::Result<LineRead> {
    let mut line = Vec::new();
    let mut oversized = false;
    let mut read_any = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return if !read_any {
                Ok(LineRead::Eof)
            } else if oversized {
                Ok(LineRead::Oversized)
            } else {
                Ok(LineRead::Line(line))
            };
        }
        read_any = true;
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(buffer.len(), |position| position + 1);
        let content_length = newline.unwrap_or(buffer.len());
        if !oversized {
            if line.len().saturating_add(content_length) > maximum_bytes {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(&buffer[..content_length]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return if oversized {
                Ok(LineRead::Oversized)
            } else {
                Ok(LineRead::Line(line))
            };
        }
    }
}

struct McpServer {
    config: McpConfig,
}

impl McpServer {
    fn handle_line(&self, line: &[u8]) -> Option<Value> {
        let started = Instant::now();
        let message: Value = match serde_json::from_slice(line) {
            Ok(message) => message,
            Err(_) => {
                log_request("unknown", None, "error", "MCP_PARSE_ERROR", started);
                return Some(rpc_error(
                    Value::Null,
                    -32700,
                    "parse error",
                    "MCP_PARSE_ERROR",
                ));
            }
        };
        let Some(object) = message.as_object() else {
            log_request("unknown", None, "error", "MCP_INVALID_REQUEST", started);
            return Some(rpc_error(
                Value::Null,
                -32600,
                "invalid request",
                "MCP_INVALID_REQUEST",
            ));
        };
        let id = match object.get("id") {
            None => None,
            Some(id) if valid_request_id(id) => Some(id.clone()),
            Some(_) => {
                log_request("unknown", None, "error", "MCP_INVALID_REQUEST", started);
                return Some(rpc_error(
                    Value::Null,
                    -32600,
                    "invalid request",
                    "MCP_INVALID_REQUEST",
                ));
            }
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || !matches!(object.get("method"), Some(Value::String(_)))
        {
            log_request("unknown", None, "error", "MCP_INVALID_REQUEST", started);
            return Some(rpc_error(
                id.unwrap_or(Value::Null),
                -32600,
                "invalid request",
                "MCP_INVALID_REQUEST",
            ));
        }
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        let public_method = public_method_name(method);
        let public_tool = public_tool_name(method, &params);
        let dispatched = self.dispatch(method, params);
        let Some(id) = id else {
            match dispatched {
                Ok(_) => log_request(public_method, public_tool, "success", "OK", started),
                Err(error) => log_request(
                    public_method,
                    public_tool,
                    "error",
                    error.public_code,
                    started,
                ),
            }
            return None;
        };
        Some(match dispatched {
            Ok(result) => {
                let (status, code) = tool_result_status(&result);
                log_request(public_method, public_tool, status, code, started);
                json!({"jsonrpc": "2.0", "id": id, "result": result})
            }
            Err(error) => {
                log_request(
                    public_method,
                    public_tool,
                    "error",
                    error.public_code,
                    started,
                );
                rpc_error(id, error.rpc_code, error.message, error.public_code)
            }
        })
    }

    fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcFailure> {
        match method {
            "initialize" => {
                parse_params::<InitializeParams>(params)?.validate()?;
                Ok(json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": {"listChanged": false},
                        "resources": {"subscribe": false, "listChanged": false}
                    },
                    "serverInfo": {
                        "name": "postgresem",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }))
            }
            "notifications/initialized" => {
                parse_params::<EmptyParams>(params)?;
                Ok(json!({}))
            }
            "ping" => {
                parse_params::<EmptyParams>(params)?;
                Ok(json!({}))
            }
            "tools/list" => {
                let params = parse_params::<PaginationParams>(params)?;
                reject_protocol_cursor(params.cursor.as_deref())?;
                Ok(json!({
                    "tools": tool_definitions(self.config.mutation_executor.is_some())
                }))
            }
            "tools/call" => self.call_tool(parse_params(params)?),
            "resources/list" => {
                let params = parse_params::<PaginationParams>(params)?;
                reject_protocol_cursor(params.cursor.as_deref())?;
                self.list_resources()
            }
            "resources/read" => self.read_resource(parse_params(params)?),
            _ => Err(RpcFailure::method_not_found()),
        }
    }

    fn call_tool(&self, request: ToolCallParams) -> Result<Value, RpcFailure> {
        let result = match request.name.as_str() {
            "list_semantic_models" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.list_semantic_models(params)
            }
            "describe_semantic_model" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.describe_semantic_model(params)
            }
            "validate_semantic_query" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.validate_semantic_query(params)
            }
            "query_semantic_model" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.query_semantic_model(params)
            }
            "explain_semantic_query" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.explain_semantic_query(params)
            }
            "validate_semantic_mutation" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.validate_semantic_mutation(params)
            }
            "mutate_semantic_model" => {
                let params = parse_tool_arguments(&request.arguments)
                    .map_err(|_| RpcFailure::invalid_tool_arguments())?;
                self.mutate_semantic_model(params)
            }
            _ => return Err(RpcFailure::tool_not_found()),
        };
        Ok(match result {
            Ok(data) => tool_success(data),
            Err(error) => tool_error(error),
        })
    }

    fn list_semantic_models(&self, params: ListModelsParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let limit = params.limit.unwrap_or(DEFAULT_PAGE_LIMIT);
        if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
            return Err(ToolFailure::new(
                "MCP_INVALID_PAGINATION",
                "limit must be between 1 and 100",
            ));
        }
        let published = self.load_published()?;
        let offset = parse_cursor(params.cursor.as_deref(), &published.snapshot.revision_hash)?;
        let models = queryable_models(&published.snapshot);
        if offset > models.len() {
            return Err(ToolFailure::new(
                "MCP_INVALID_CURSOR",
                "cursor is not valid",
            ));
        }
        let end = offset.saturating_add(limit).min(models.len());
        let page = models[offset..end]
            .iter()
            .map(|model| {
                json!({
                    "name": model.semantic_name,
                    "field_count": model.fields.iter().filter(|field| field.visible).count(),
                    "metric_count": model.metrics.iter().filter(|metric| metric.visible).count()
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "schema_version": TOOL_SCHEMA_VERSION,
            "semantic_revision": published.snapshot.revision_hash,
            "models": page,
            "next_cursor": (end < models.len()).then(|| {
                model_cursor(&published.snapshot.revision_hash, end)
            })
        }))
    }

    fn describe_semantic_model(&self, params: DescribeModelParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let published = self.load_published()?;
        let model = find_queryable_model(&published.snapshot, &params.model)?;
        Ok(describe_model(
            &published.snapshot.revision_hash,
            model,
            self.config.executor.max_result_bytes(),
        ))
    }

    fn validate_semantic_query(&self, params: QueryToolParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let published = self.load_published()?;
        let normalized = match normalize_value(&params.lsq) {
            Ok(normalized) => normalized,
            Err(error) => {
                return Ok(validation_failure(
                    error.code(),
                    public_lsq_message(&error),
                    None,
                    &published.snapshot.revision_hash,
                ));
            }
        };
        match compile_lsq(&normalized, &published.snapshot, CompilerOptions::default()) {
            Ok(compiled) => Ok(json!({
                "schema_version": TOOL_SCHEMA_VERSION,
                "valid": true,
                "normalized_lsq_hash": normalized.hash,
                "semantic_revision": published.snapshot.revision_hash,
                "output_schema": public_output_schema(&compiled.output_schema),
                "lineage": public_lineage(&normalized.query, &compiled.lineage),
                "warnings": []
            })),
            Err(error) => Ok(validation_failure(
                error.code(),
                public_compile_message(&error),
                Some(&normalized.hash),
                &published.snapshot.revision_hash,
            )),
        }
    }

    fn explain_semantic_query(&self, params: QueryToolParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let published = self.load_published()?;
        let normalized = normalize_value(&params.lsq).map_err(ToolFailure::from_lsq)?;
        let compiled = compile_lsq(&normalized, &published.snapshot, CompilerOptions::default())
            .map_err(ToolFailure::from_compile)?;
        let normalized_lsq: Value = serde_json::from_str(&normalized.canonical_json)
            .map_err(|_| ToolFailure::internal())?;
        let lineage = public_lineage(&normalized.query, &compiled.lineage);
        Ok(json!({
            "schema_version": TOOL_SCHEMA_VERSION,
            "normalized_lsq": normalized_lsq,
            "normalized_lsq_hash": normalized.hash,
            "semantic_revision": published.snapshot.revision_hash,
            "semantic_models": lineage.models,
            "semantic_relationships": lineage.relationships,
            "source_lineage": lineage,
            "output_schema": public_output_schema(&compiled.output_schema),
            "limits": {
                "requested": normalized.query.limit,
                "effective": normalized.query.limit.unwrap_or(CompilerOptions::default().default_limit),
                "maximum": CompilerOptions::default().hard_limit
            },
            "warnings": []
        }))
    }

    fn query_semantic_model(&self, params: QueryToolParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let normalized = normalize_value(&params.lsq).map_err(ToolFailure::from_lsq)?;
        let input = serde_json::to_vec(&params.lsq).map_err(|_| ToolFailure::internal())?;
        let result = executor::execute(
            &input,
            &self.config.project,
            &self.config.executor,
            &self.config.execution_context,
        )
        .map_err(ToolFailure::from_execute)?;
        Ok(public_query_result(result, &normalized.query))
    }

    fn validate_semantic_mutation(&self, params: MutationToolParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let published = self.load_published_mutation()?;
        let normalized = match normalize_mutation_value(&params.lsm) {
            Ok(normalized) => normalized,
            Err(error) => {
                return Ok(mutation_validation_failure(
                    error.code(),
                    public_lsm_message(&error),
                    None,
                    &published.published.snapshot.revision_hash,
                ));
            }
        };
        match compile_lsm(
            &normalized,
            &published.published.snapshot,
            &published.capabilities,
            MutationCompilerOptions::default(),
        ) {
            Ok(compiled) => Ok(json!({
                "schema_version": TOOL_SCHEMA_VERSION,
                "valid": true,
                "normalized_lsm_hash": normalized.hash,
                "semantic_revision": published.published.snapshot.revision_hash,
                "operation": compiled.operation,
                "model": compiled.model,
                "expected_rows": compiled.expected_rows,
                "returning_schema": public_output_schema(&compiled.returning_schema),
                "lineage": public_mutation_lineage(&normalized.mutation),
                "warnings": []
            })),
            Err(error) => Ok(mutation_validation_failure(
                error.code(),
                public_mutation_compile_message(&error),
                Some(&normalized.hash),
                &published.published.snapshot.revision_hash,
            )),
        }
    }

    fn mutate_semantic_model(&self, params: MutationToolParams) -> Result<Value, ToolFailure> {
        validate_tool_version(&params.schema_version)?;
        let input = serde_json::to_vec(&params.lsm).map_err(|_| ToolFailure::internal())?;
        let config = self.config.mutation_executor.as_ref().ok_or_else(|| {
            ToolFailure::new(
                "MUTATION_CAPABILITY_DISABLED",
                "semantic mutation capability is not enabled",
            )
        })?;
        let result = mutation_executor::execute(
            &input,
            &self.config.project,
            config,
            &self.config.execution_context,
        )
        .map_err(ToolFailure::from_mutation_execute)?;
        Ok(public_mutation_result(result))
    }

    fn list_resources(&self) -> Result<Value, RpcFailure> {
        let published = self
            .load_published()
            .map_err(|error| RpcFailure::resource(error.code, error.message))?;
        let mut resources = vec![
            json!({
                "uri": current_revision_uri(&self.config.project),
                "name": "Current semantic revision",
                "mimeType": "application/json"
            }),
            json!({
                "uri": "semantic://schemas/lsq/v1",
                "name": "Logical Semantic Query v1 schema",
                "mimeType": "application/schema+json"
            }),
        ];
        if self.config.mutation_executor.is_some() {
            resources.push(json!({
                "uri": "semantic://schemas/lsm/v1",
                "name": "Logical Semantic Mutation v1 schema",
                "mimeType": "application/schema+json"
            }));
        }
        resources.extend(
            queryable_models(&published.snapshot)
                .into_iter()
                .map(|model| {
                    json!({
                        "uri": model_uri(&self.config.project, &model.semantic_name),
                        "name": format!("Semantic model {}", model.semantic_name),
                        "mimeType": "application/json"
                    })
                }),
        );
        Ok(json!({"resources": resources}))
    }

    fn read_resource(&self, params: ResourceReadParams) -> Result<Value, RpcFailure> {
        if params.uri == "semantic://schemas/lsq/v1" {
            let schema: Value = serde_json::from_str(LSQ_SCHEMA).map_err(|_| {
                RpcFailure::resource("MCP_INTERNAL_ERROR", "resource is unavailable")
            })?;
            return resource_contents(&params.uri, "application/schema+json", schema);
        }
        if params.uri == "semantic://schemas/lsm/v1" && self.config.mutation_executor.is_some() {
            let schema: Value = serde_json::from_str(LSM_SCHEMA).map_err(|_| {
                RpcFailure::resource("MCP_INTERNAL_ERROR", "resource is unavailable")
            })?;
            return resource_contents(&params.uri, "application/schema+json", schema);
        }
        let published = self
            .load_published()
            .map_err(|error| RpcFailure::resource(error.code, error.message))?;
        if params.uri == current_revision_uri(&self.config.project) {
            let models = queryable_models(&published.snapshot)
                .into_iter()
                .map(|model| model.semantic_name.clone())
                .collect::<Vec<_>>();
            return resource_contents(
                &params.uri,
                "application/json",
                json!({
                    "schema_version": TOOL_SCHEMA_VERSION,
                    "project": self.config.project,
                    "semantic_revision": published.snapshot.revision_hash,
                    "models": models
                }),
            );
        }
        for model in queryable_models(&published.snapshot) {
            if params.uri == model_uri(&self.config.project, &model.semantic_name) {
                return resource_contents(
                    &params.uri,
                    "application/json",
                    describe_model(
                        &published.snapshot.revision_hash,
                        model,
                        self.config.executor.max_result_bytes(),
                    ),
                );
            }
        }
        Err(RpcFailure::resource(
            "MCP_RESOURCE_NOT_FOUND",
            "resource is not available",
        ))
    }

    fn load_published(&self) -> Result<published_model::PublishedModel, ToolFailure> {
        let mut runtime = self.config.executor.connect_runtime().map_err(|_| {
            ToolFailure::new(
                "SEMANTIC_SNAPSHOT_UNAVAILABLE",
                "current semantic snapshot is unavailable",
            )
        })?;
        published_model::load_published(&mut runtime, &self.config.project).map_err(|_| {
            ToolFailure::new(
                "SEMANTIC_SNAPSHOT_UNAVAILABLE",
                "current semantic snapshot is unavailable",
            )
        })
    }

    fn load_published_mutation(
        &self,
    ) -> Result<published_model::PublishedMutationModel, ToolFailure> {
        let config = self.config.mutation_executor.as_ref().ok_or_else(|| {
            ToolFailure::new(
                "MUTATION_CAPABILITY_DISABLED",
                "semantic mutation capability is not enabled",
            )
        })?;
        let mut mutation = config.connect_mutation().map_err(|_| {
            ToolFailure::new(
                "SEMANTIC_SNAPSHOT_UNAVAILABLE",
                "current semantic snapshot is unavailable",
            )
        })?;
        published_model::load_published_for_mutation(
            &mut mutation,
            &self.config.project,
            config.database_role(),
        )
        .map_err(|_| {
            ToolFailure::new(
                "SEMANTIC_SNAPSHOT_UNAVAILABLE",
                "current semantic snapshot is unavailable",
            )
        })
    }
}

#[derive(Debug)]
struct RpcFailure {
    rpc_code: i64,
    public_code: &'static str,
    message: &'static str,
}

impl RpcFailure {
    const fn method_not_found() -> Self {
        Self {
            rpc_code: -32601,
            public_code: "MCP_METHOD_NOT_FOUND",
            message: "method not found",
        }
    }

    const fn invalid_params() -> Self {
        Self {
            rpc_code: -32602,
            public_code: "MCP_INVALID_PARAMS",
            message: "invalid method parameters",
        }
    }

    const fn invalid_tool_arguments() -> Self {
        Self {
            rpc_code: -32602,
            public_code: "MCP_INVALID_TOOL_ARGUMENTS",
            message: "tool arguments do not match the declared schema",
        }
    }

    const fn tool_not_found() -> Self {
        Self {
            rpc_code: -32602,
            public_code: "MCP_TOOL_NOT_FOUND",
            message: "requested tool is not available",
        }
    }

    const fn resource(public_code: &'static str, message: &'static str) -> Self {
        Self {
            rpc_code: -32002,
            public_code,
            message,
        }
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, RpcFailure> {
    serde_json::from_value(params).map_err(|_| RpcFailure::invalid_params())
}

fn reject_protocol_cursor(cursor: Option<&str>) -> Result<(), RpcFailure> {
    if cursor.is_some() {
        Err(RpcFailure::invalid_params())
    } else {
        Ok(())
    }
}

#[derive(Debug)]
struct ToolFailure {
    code: &'static str,
    message: &'static str,
}

impl ToolFailure {
    const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    const fn internal() -> Self {
        Self::new("MCP_INTERNAL_ERROR", "tool operation failed")
    }

    fn from_lsq(error: LsqError) -> Self {
        Self::new(error.code(), public_lsq_message(&error))
    }

    fn from_compile(error: CompileError) -> Self {
        Self::new(error.code(), public_compile_message(&error))
    }

    fn from_execute(error: executor::ExecuteError) -> Self {
        match error {
            executor::ExecuteError::Lsq(error) => Self::from_lsq(error),
            executor::ExecuteError::Compile(error) => Self::from_compile(error),
            executor::ExecuteError::SourceCancelled(_) => {
                Self::new("EXECUTOR_QUERY_CANCELLED", "semantic query was cancelled")
            }
            executor::ExecuteError::RowSerialization(_)
            | executor::ExecuteError::InvalidRowShape => Self::new(
                "EXECUTOR_RESULT_SERIALIZATION_FAILED",
                "semantic query result could not be serialized",
            ),
            _ => Self::new("EXECUTOR_QUERY_FAILED", "semantic query execution failed"),
        }
    }

    fn from_mutation_execute(error: mutation_executor::MutationExecuteError) -> Self {
        match error {
            mutation_executor::MutationExecuteError::Lsm(error) => {
                Self::new(error.code(), public_lsm_message(&error))
            }
            mutation_executor::MutationExecuteError::Compile(error) => {
                Self::new(error.code(), public_mutation_compile_message(&error))
            }
            mutation_executor::MutationExecuteError::IdempotencyConflict => Self::new(
                "MUTATION_IDEMPOTENCY_CONFLICT",
                "idempotency key was already used for a different mutation",
            ),
            mutation_executor::MutationExecuteError::CommitIndeterminate(_) => Self::new(
                "MUTATION_COMMIT_INDETERMINATE",
                "mutation outcome is indeterminate; retry with the same idempotency key",
            ),
            mutation_executor::MutationExecuteError::Cancelled(_) => {
                Self::new("MUTATION_CANCELLED", "semantic mutation was cancelled")
            }
            error => Self::new(
                mutation_executor::mutation_error_code(&error),
                "semantic mutation failed",
            ),
        }
    }
}

fn tool_success(data: Value) -> Value {
    let text = serde_json::to_string(&data).unwrap_or_else(|_| {
        r#"{"error":{"code":"MCP_INTERNAL_ERROR","message":"tool operation failed"}}"#.to_owned()
    });
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": data,
        "isError": false
    })
}

fn tool_error(error: ToolFailure) -> Value {
    let data = json!({"error": {"code": error.code, "message": error.message}});
    let text = serde_json::to_string(&data).unwrap_or_else(|_| {
        r#"{"error":{"code":"MCP_INTERNAL_ERROR","message":"tool operation failed"}}"#.to_owned()
    });
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": data,
        "isError": true
    })
}

fn rpc_error(id: Value, rpc_code: i64, message: &'static str, public_code: &'static str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": rpc_code,
            "message": message,
            "data": {"code": public_code}
        }
    })
}

fn valid_request_id(id: &Value) -> bool {
    match id {
        Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: String,
    capabilities: serde_json::Map<String, Value>,
    client_info: ClientInfo,
}

impl InitializeParams {
    fn validate(self) -> Result<(), RpcFailure> {
        let _ = (self.protocol_version, self.capabilities);
        if self.client_info.name.trim().is_empty() || self.client_info.version.trim().is_empty() {
            return Err(RpcFailure::invalid_params());
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct ClientInfo {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct EmptyParams {}

#[derive(Debug, Deserialize)]
struct PaginationParams {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default = "empty_object")]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct ResourceReadParams {
    uri: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListModelsParams {
    schema_version: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DescribeModelParams {
    schema_version: String,
    model: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryToolParams {
    schema_version: String,
    lsq: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationToolParams {
    schema_version: String,
    lsm: Value,
}

fn empty_object() -> Value {
    json!({})
}

fn parse_tool_arguments<T: for<'de> Deserialize<'de>>(arguments: &Value) -> Result<T, ToolFailure> {
    serde_json::from_value(arguments.clone()).map_err(|_| {
        ToolFailure::new(
            "MCP_INVALID_TOOL_ARGUMENTS",
            "tool arguments do not match the declared schema",
        )
    })
}

fn validate_tool_version(version: &str) -> Result<(), ToolFailure> {
    if version == TOOL_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ToolFailure::new(
            "MCP_TOOL_SCHEMA_VERSION_UNSUPPORTED",
            "tool schema version is not supported",
        ))
    }
}

fn model_cursor(revision: &str, offset: usize) -> String {
    format!("v1:{revision}:{offset}")
}

fn parse_cursor(cursor: Option<&str>, revision: &str) -> Result<usize, ToolFailure> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let (_cursor_revision, offset) = cursor
        .strip_prefix("v1:")
        .and_then(|body| body.rsplit_once(':'))
        .filter(|(cursor_revision, _)| *cursor_revision == revision)
        .ok_or_else(invalid_cursor)?;
    offset.parse().map_err(|_| invalid_cursor())
}

const fn invalid_cursor() -> ToolFailure {
    ToolFailure::new("MCP_INVALID_CURSOR", "cursor is not valid")
}

fn normalize_value(value: &Value) -> Result<NormalizedLsq, LsqError> {
    let bytes = serde_json::to_vec(value).map_err(LsqError::InvalidJson)?;
    normalize_lsq(&bytes)
}

fn normalize_mutation_value(value: &Value) -> Result<NormalizedLsm, LsmError> {
    let bytes = serde_json::to_vec(value).map_err(LsmError::InvalidJson)?;
    normalize_lsm(&bytes)
}

fn validation_failure(
    code: &'static str,
    message: &'static str,
    normalized_lsq_hash: Option<&str>,
    semantic_revision: &str,
) -> Value {
    json!({
        "schema_version": TOOL_SCHEMA_VERSION,
        "valid": false,
        "normalized_lsq_hash": normalized_lsq_hash,
        "semantic_revision": semantic_revision,
        "output_schema": [],
        "lineage": null,
        "error": {"code": code, "message": message},
        "warnings": []
    })
}

fn mutation_validation_failure(
    code: &'static str,
    message: &'static str,
    normalized_lsm_hash: Option<&str>,
    semantic_revision: &str,
) -> Value {
    json!({
        "schema_version": TOOL_SCHEMA_VERSION,
        "valid": false,
        "normalized_lsm_hash": normalized_lsm_hash,
        "semantic_revision": semantic_revision,
        "operation": null,
        "model": null,
        "expected_rows": 0,
        "returning_schema": [],
        "lineage": null,
        "error": {"code": code, "message": message},
        "warnings": []
    })
}

fn public_lsq_message(error: &LsqError) -> &'static str {
    match error {
        LsqError::InvalidJson(_) => "LSQ document is invalid",
        LsqError::UnsupportedSchemaVersion(_) => "LSQ schema version is not supported",
        LsqError::EmptyModel => "semantic model is required",
        LsqError::EmptyProjection => "at least one semantic output is required",
        LsqError::EmptyReference => "semantic references must not be empty",
        LsqError::DuplicateReference(_) => "semantic references must be unique",
        LsqError::DuplicateOrderReference(_) => "order references must be unique",
        LsqError::InvalidLiteralValue(_) => "literal value is invalid",
        LsqError::InvalidLimit => "query limit is invalid",
        LsqError::FilterTooDeep => "filter is too deeply nested",
        LsqError::FilterTooLarge => "filter is too large",
        LsqError::EmptyLogicalFilter => "logical filter requires arguments",
        LsqError::InvalidInFilterSize => "IN filter size is invalid",
    }
}

fn public_compile_message(error: &CompileError) -> &'static str {
    match error {
        CompileError::UnknownModel => "semantic model is not available",
        CompileError::UnknownField(_) | CompileError::HiddenField(_) => {
            "semantic field is not available"
        }
        CompileError::UnknownMetric(_) | CompileError::HiddenMetric(_) => {
            "semantic metric is not available"
        }
        CompileError::UnknownRelationship(_) => "semantic relationship is not available",
        CompileError::InvalidTimeGrain(_) => "time grain is not valid for the semantic field",
        CompileError::LiteralTypeMismatch(_) => {
            "literal type is not compatible with the semantic field"
        }
        CompileError::UnknownOrderReference(_) => {
            "order reference is not a projected semantic output"
        }
        CompileError::LimitExceeded => "query limit exceeds the configured maximum",
        _ => "semantic query is not valid for the current revision",
    }
}

fn public_lsm_message(error: &LsmError) -> &'static str {
    match error {
        LsmError::InputTooLarge => "semantic mutation document is too large",
        LsmError::InvalidJson(_) => "LSM document is invalid",
        LsmError::UnsupportedSchemaVersion(_) => "LSM schema version is not supported",
        LsmError::InvalidModel => "semantic mutation model is invalid",
        LsmError::InvalidIdempotencyKey => "idempotency key is invalid",
        LsmError::InvalidRowCount => "mutation row count is invalid",
        LsmError::InvalidFieldCount => "mutation field count is invalid",
        LsmError::InvalidFieldName => "semantic mutation field name is invalid",
        LsmError::InconsistentRowFields => "mutation rows must contain the same fields",
        LsmError::InvalidValue => "mutation value is invalid",
    }
}

fn public_mutation_compile_message(error: &MutationCompileError) -> &'static str {
    match error {
        MutationCompileError::ModelNotWritable => "semantic model is not writable",
        MutationCompileError::OperationNotEnabled => "mutation operation is not enabled",
        MutationCompileError::RowLimitExceeded => "mutation row limit was exceeded",
        MutationCompileError::RequestByteLimitExceeded => "mutation byte limit was exceeded",
        MutationCompileError::FieldNotWritable(_) => "semantic mutation field is not writable",
        MutationCompileError::RequiredFieldMissing(_) => {
            "required semantic mutation field is missing"
        }
        MutationCompileError::FieldTypeMismatch(_) => {
            "mutation value is not compatible with the semantic field"
        }
        MutationCompileError::NullNotAllowed(_) => "semantic mutation field does not accept null",
        MutationCompileError::ConflictFieldMissing(_) => {
            "approved upsert conflict field is missing"
        }
        MutationCompileError::NoUpdatableField => "upsert requires an approved mutable field",
        _ => "semantic mutation is not valid for the current revision",
    }
}

#[derive(serde::Serialize)]
struct PublicLineage {
    models: Vec<String>,
    metrics: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    aggregation_anchors: BTreeMap<String, String>,
    relationships: Vec<String>,
    fields: Vec<String>,
}

fn public_lineage(query: &LogicalSemanticQuery, compiled: &Lineage) -> PublicLineage {
    let mut fields = BTreeSet::new();
    for dimension in &query.dimensions {
        fields.insert(dimension.field.clone());
    }
    if let Some(filter) = &query.filters {
        collect_filter_fields(filter, &mut fields);
    }
    PublicLineage {
        models: vec![query.model.clone()],
        metrics: compiled.metrics.clone(),
        aggregation_anchors: compiled
            .aggregation_anchors
            .iter()
            .map(|anchor| (anchor.metric.clone(), anchor.field.clone()))
            .collect(),
        relationships: compiled.relationships.clone(),
        fields: fields.into_iter().collect(),
    }
}

fn collect_filter_fields(filter: &postgresem_compiler::Filter, fields: &mut BTreeSet<String>) {
    use postgresem_compiler::Filter;
    match filter {
        Filter::And { args } | Filter::Or { args } => {
            for argument in args {
                collect_filter_fields(argument, fields);
            }
        }
        Filter::Not { arg } => collect_filter_fields(arg, fields),
        Filter::Eq { field, .. }
        | Filter::NotEq { field, .. }
        | Filter::Gt { field, .. }
        | Filter::Gte { field, .. }
        | Filter::Lt { field, .. }
        | Filter::Lte { field, .. }
        | Filter::In { field, .. }
        | Filter::IsNull { field }
        | Filter::IsNotNull { field } => {
            fields.insert(field.clone());
        }
    }
}

fn public_query_result(result: QueryResult, query: &LogicalSemanticQuery) -> Value {
    json!({
        "schema_version": result.schema_version,
        "query_id": result.query_id,
        "semantic_revision": result.semantic_revision,
        "columns": public_output_schema(&result.columns),
        "rows": result.rows,
        "truncated": result.truncated,
        "lineage": public_lineage(query, &result.lineage),
        "warnings": result.warnings
    })
}

fn public_mutation_lineage(mutation: &LogicalSemanticMutation) -> Value {
    json!({
        "model": mutation.model,
        "fields": mutation.rows[0].keys().collect::<Vec<_>>()
    })
}

fn public_mutation_result(result: MutationResult) -> Value {
    json!({
        "schema_version": result.schema_version,
        "mutation_id": result.mutation_id,
        "semantic_revision": result.semantic_revision,
        "operation": result.operation,
        "model": result.model,
        "columns": public_output_schema(&result.columns),
        "rows": result.rows,
        "affected_rows": result.affected_rows,
        "replayed": result.replayed,
        "lineage": {
            "model": result.lineage.model,
            "fields": result.lineage.fields,
            "returning_fields": result.lineage.returning_fields
        },
        "warnings": result.warnings
    })
}

fn public_output_schema(columns: &[OutputColumn]) -> Vec<Value> {
    columns
        .iter()
        .map(|column| {
            json!({
                "name": column.name,
                "type": column.data_type
            })
        })
        .collect()
}

fn queryable_models(snapshot: &SemanticSnapshot) -> Vec<&Model> {
    snapshot
        .models
        .iter()
        .filter(|model| model.queryable)
        .collect()
}

fn find_queryable_model<'a>(
    snapshot: &'a SemanticSnapshot,
    name: &str,
) -> Result<&'a Model, ToolFailure> {
    snapshot
        .models
        .iter()
        .find(|model| model.queryable && model.semantic_name == name)
        .ok_or_else(|| {
            ToolFailure::new(
                "SEMANTIC_MODEL_NOT_AVAILABLE",
                "semantic model is not available",
            )
        })
}

fn describe_model(revision: &str, model: &Model, max_result_bytes: usize) -> Value {
    let fields = model
        .fields
        .iter()
        .filter(|field| field.visible)
        .map(|field| {
            json!({
                "name": field.semantic_name,
                "type": field.data_type,
                "time_dimension": field.time_dimension,
                "entity_key": field.entity_key,
                "relationship": field.relationship,
                "supported_time_grains": supported_time_grains(field.data_type, field.time_dimension)
            })
        })
        .collect::<Vec<_>>();
    let metrics = model
        .metrics
        .iter()
        .filter(|metric| metric.visible)
        .map(|metric| {
            json!({
                "name": metric.semantic_name,
                "type": metric.data_type,
                "aggregation": metric.aggregation,
                "additivity": metric.additivity,
                "aggregation_anchor": metric.aggregation_anchor
            })
        })
        .collect::<Vec<_>>();
    let usable_relationships = model
        .fields
        .iter()
        .filter(|field| field.visible)
        .filter_map(|field| field.relationship.as_deref())
        .chain(
            model
                .metrics
                .iter()
                .filter(|metric| metric.visible)
                .filter_map(|metric| {
                    model
                        .fields
                        .iter()
                        .find(|field| field.semantic_name == metric.field)
                        .and_then(|field| field.relationship.as_deref())
                }),
        )
        .collect::<BTreeSet<_>>();
    let relationships = model
        .relationships
        .iter()
        .filter(|relationship| usable_relationships.contains(relationship.semantic_name.as_str()))
        .map(|relationship| {
            json!({
                "name": relationship.semantic_name,
                "cardinality": relationship.cardinality,
                "join_type": relationship.join_type
            })
        })
        .collect::<Vec<_>>();
    let compiler_options = CompilerOptions::default();
    json!({
        "schema_version": TOOL_SCHEMA_VERSION,
        "semantic_revision": revision,
        "model": {
            "name": model.semantic_name,
            "timezone": model.timezone,
            "fields": fields,
            "metrics": metrics,
            "relationships": relationships
        },
        "query_limits": {
            "default": compiler_options.default_limit,
            "hard": compiler_options.hard_limit,
            "max_result_bytes": max_result_bytes
        }
    })
}

fn supported_time_grains(data_type: DataType, time_dimension: bool) -> &'static [&'static str] {
    if time_dimension
        && matches!(
            data_type,
            DataType::Date | DataType::Timestamp | DataType::TimestampTz
        )
    {
        &["day", "week", "month", "quarter", "year"]
    } else {
        &[]
    }
}

fn resource_contents(uri: &str, mime_type: &str, value: Value) -> Result<Value, RpcFailure> {
    let text = serde_json::to_string(&value)
        .map_err(|_| RpcFailure::resource("MCP_INTERNAL_ERROR", "resource is unavailable"))?;
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": mime_type,
            "text": text
        }]
    }))
}

fn current_revision_uri(project: &str) -> String {
    format!(
        "semantic://projects/{}/revisions/current",
        percent_encode(project)
    )
}

fn model_uri(project: &str, model: &str) -> String {
    format!(
        "semantic://projects/{}/models/{}",
        percent_encode(project),
        percent_encode(model)
    )
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn required_environment(variable: &str) -> Result<String, McpServerError> {
    env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => McpServerError::MissingEnvironment(variable.to_owned()),
        env::VarError::NotUnicode(_) => McpServerError::InvalidEnvironment(variable.to_owned()),
    })
}

fn environment_or(variable: &str, default: &str) -> Result<String, McpServerError> {
    match env::var(variable) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(env::VarError::NotUnicode(_)) => {
            Err(McpServerError::InvalidEnvironment(variable.to_owned()))
        }
    }
}

fn tool_definitions(mutation_enabled: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "list_semantic_models",
            "description": "List queryable semantic models in the current published revision.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version"],
                "properties": {
                    "schema_version": {"const": TOOL_SCHEMA_VERSION},
                    "cursor": {"type": "string", "maxLength": MAX_MODEL_CURSOR_LENGTH},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_PAGE_LIMIT}
                }
            }
        }),
        json!({
            "name": "describe_semantic_model",
            "description": "Describe visible fields and metrics for a queryable semantic model.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["schema_version", "model"],
                "properties": {
                    "schema_version": {"const": TOOL_SCHEMA_VERSION},
                    "model": {"type": "string", "minLength": 1, "maxLength": 255}
                }
            }
        }),
        query_tool_definition(
            "validate_semantic_query",
            "Validate and type-check an LSQ against the current published revision.",
        ),
        query_tool_definition(
            "query_semantic_model",
            "Execute an LSQ through the guarded semantic query executor.",
        ),
        query_tool_definition(
            "explain_semantic_query",
            "Explain normalized semantic references, output shape, and limits for an LSQ.",
        ),
    ];
    if mutation_enabled {
        tools.push(mutation_tool_definition(
            "validate_semantic_mutation",
            "Validate and type-check an LSM against the current writable revision.",
        ));
        tools.push(mutation_tool_definition(
            "mutate_semantic_model",
            "Execute an LSM through the guarded semantic mutation executor.",
        ));
    }
    tools
}

fn query_tool_definition(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": query_tool_schema()
    })
}

fn query_tool_schema() -> Value {
    embedded_document_tool_schema(LSQ_SCHEMA, "lsq")
}

fn mutation_tool_definition(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": mutation_tool_schema()
    })
}

fn mutation_tool_schema() -> Value {
    embedded_document_tool_schema(LSM_SCHEMA, "lsm")
}

fn embedded_document_tool_schema(schema: &str, property: &str) -> Value {
    let mut document_schema: Value =
        serde_json::from_str(schema).unwrap_or_else(|_| json!({"type": "object"}));
    if let Some(object) = document_schema.as_object_mut() {
        object.remove("$schema");
        object.remove("$id");
    }
    rewrite_schema_references(&mut document_schema, property);
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["schema_version", property],
        "properties": {
            "schema_version": {"const": TOOL_SCHEMA_VERSION},
            (property): document_schema
        }
    })
}

fn rewrite_schema_references(value: &mut Value, property: &str) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref") {
                if let Some(suffix) = reference.strip_prefix("#/$defs/") {
                    *reference = format!("#/properties/{property}/$defs/{suffix}");
                }
            }
            for child in object.values_mut() {
                rewrite_schema_references(child, property);
            }
        }
        Value::Array(values) => {
            for child in values {
                rewrite_schema_references(child, property);
            }
        }
        _ => {}
    }
}

fn public_method_name(method: &str) -> &'static str {
    match method {
        "initialize" => "initialize",
        "notifications/initialized" => "notifications/initialized",
        "ping" => "ping",
        "tools/list" => "tools/list",
        "tools/call" => "tools/call",
        "resources/list" => "resources/list",
        "resources/read" => "resources/read",
        _ => "unknown",
    }
}

fn public_tool_name(method: &str, params: &Value) -> Option<&'static str> {
    if method != "tools/call" {
        return None;
    }
    match params.get("name").and_then(Value::as_str) {
        Some("list_semantic_models") => Some("list_semantic_models"),
        Some("describe_semantic_model") => Some("describe_semantic_model"),
        Some("validate_semantic_query") => Some("validate_semantic_query"),
        Some("query_semantic_model") => Some("query_semantic_model"),
        Some("explain_semantic_query") => Some("explain_semantic_query"),
        Some("validate_semantic_mutation") => Some("validate_semantic_mutation"),
        Some("mutate_semantic_model") => Some("mutate_semantic_model"),
        Some(_) => Some("unknown"),
        None => None,
    }
}

fn tool_result_status(result: &Value) -> (&'static str, &str) {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let code = result
            .get("structuredContent")
            .and_then(|value| value.get("error"))
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("MCP_TOOL_ERROR");
        ("error", code)
    } else {
        ("success", "OK")
    }
}

fn log_request(method: &str, tool: Option<&str>, status: &str, code: &str, started: Instant) {
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let log = json!({
        "event": "mcp_request",
        "method": method,
        "tool": tool,
        "status": status,
        "code": code,
        "elapsed_ms": elapsed_ms
    });
    eprintln!("{log}");
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use serde_json::{Value, json};

    use super::{
        EmptyParams, InitializeParams, LineRead, MAX_REQUEST_LINE_BYTES, PaginationParams,
        ResourceReadParams, TOOL_SCHEMA_VERSION, ToolCallParams, model_cursor,
        mutation_tool_schema, parse_cursor, parse_params, parse_tool_arguments,
        public_output_schema, query_tool_schema, read_bounded_line, supported_time_grains,
        tool_definitions, valid_request_id,
    };
    use postgresem_compiler::{DataType, OutputColumn};

    #[test]
    fn tool_contract_gates_mutation_without_exposing_sql() {
        assert_eq!(tool_definitions(false).len(), 5);
        let tools = tool_definitions(true);
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "list_semantic_models",
                "describe_semantic_model",
                "validate_semantic_query",
                "query_semantic_model",
                "explain_semantic_query",
                "validate_semantic_mutation",
                "mutate_semantic_model"
            ]
        );
        let serialized = serde_json::to_string(&tools).expect("tools serialize");
        assert!(!serialized.contains("\"sql\""));
        assert!(!serialized.contains("compile"));
        for tool in tools {
            let schema = &tool["inputSchema"];
            assert_eq!(schema["additionalProperties"], false);
            assert_eq!(
                schema["properties"]["schema_version"]["const"],
                TOOL_SCHEMA_VERSION
            );
        }
    }

    #[test]
    fn nested_lsq_schema_keeps_strict_properties_and_resolves_local_definitions() {
        let schema = query_tool_schema();
        assert_eq!(schema["properties"]["lsq"]["additionalProperties"], false);
        let serialized = serde_json::to_string(&schema).expect("schema serializes");
        assert!(serialized.contains("#/properties/lsq/$defs/semanticName"));
        assert!(!serialized.contains("\"sql\""));
    }

    #[test]
    fn nested_lsm_schema_keeps_strict_properties_and_resolves_local_definitions() {
        let schema = mutation_tool_schema();
        assert_eq!(schema["properties"]["lsm"]["additionalProperties"], false);
        let serialized = serde_json::to_string(&schema).expect("schema serializes");
        assert!(serialized.contains("#/properties/lsm/$defs/semanticName"));
        assert!(!serialized.contains("\"sql\""));
    }

    #[test]
    fn tool_arguments_reject_request_supplied_security_context() {
        for forbidden in [
            "principal",
            "role",
            "project",
            "connection_url",
            "password",
            "password_env",
            "sql",
            "function",
        ] {
            let mut arguments = json!({
                "schema_version": TOOL_SCHEMA_VERSION,
                "lsq": {
                    "schema_version": "1",
                    "model": "orders",
                    "metrics": [{"metric": "revenue"}]
                }
            });
            arguments
                .as_object_mut()
                .expect("test arguments are an object")
                .insert(forbidden.to_owned(), json!("not-allowed"));
            assert!(parse_tool_arguments::<super::QueryToolParams>(&arguments).is_err());
        }
        for forbidden in [
            "principal",
            "role",
            "project",
            "connection_url",
            "password",
            "sql",
            "conflict_target",
            "returning",
        ] {
            let mut arguments = json!({
                "schema_version": TOOL_SCHEMA_VERSION,
                "lsm": {
                    "schema_version": "1",
                    "operation": "insert",
                    "model": "orders",
                    "idempotency_key": "request-1",
                    "rows": [{
                        "amount": {"type": "numeric", "value": "1.00"}
                    }]
                }
            });
            arguments
                .as_object_mut()
                .expect("test arguments are an object")
                .insert(forbidden.to_owned(), json!("not-allowed"));
            assert!(parse_tool_arguments::<super::MutationToolParams>(&arguments).is_err());
        }
    }

    #[test]
    fn protocol_envelopes_accept_meta_extensions() {
        let initialize = json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1"},
            "_meta": {"progressToken": "progress-1"},
            "experimentalExtension": {"enabled": true}
        });
        assert!(
            parse_params::<InitializeParams>(initialize)
                .and_then(InitializeParams::validate)
                .is_ok()
        );
        let meta = json!({"_meta": {"progressToken": "progress-1"}});
        assert!(parse_params::<EmptyParams>(meta.clone()).is_ok());
        assert!(
            parse_params::<PaginationParams>(
                json!({"cursor": null, "_meta": {"progressToken": 2}})
            )
            .is_ok()
        );
        assert!(
            parse_params::<ToolCallParams>(json!({
                "name": "list_semantic_models",
                "arguments": {"schema_version": "1"},
                "_meta": {"progressToken": "progress-3"}
            }))
            .is_ok()
        );
        assert!(
            parse_params::<ResourceReadParams>(json!({
                "uri": "semantic://schemas/lsq/v1",
                "_meta": {"progressToken": "progress-4"}
            }))
            .is_ok()
        );
    }

    #[test]
    fn initialize_requires_the_mcp_client_contract() {
        let valid = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "client", "version": "1"}
        });
        assert!(
            parse_params::<InitializeParams>(valid)
                .and_then(InitializeParams::validate)
                .is_ok()
        );
        for invalid in [
            json!({"capabilities": {}, "clientInfo": {"name": "client", "version": "1"}}),
            json!({"protocolVersion": 1, "capabilities": {}, "clientInfo": {"name": "client", "version": "1"}}),
            json!({"protocolVersion": "2024-11-05", "capabilities": [], "clientInfo": {"name": "client", "version": "1"}}),
            json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": null}),
            json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "", "version": "1"}}),
            json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "client", "version": " "}}),
        ] {
            assert!(
                parse_params::<InitializeParams>(invalid)
                    .and_then(InitializeParams::validate)
                    .is_err()
            );
        }
    }

    #[test]
    fn request_ids_are_non_null_strings_or_integer_numbers() {
        assert!(valid_request_id(&json!("request-1")));
        assert!(valid_request_id(&json!(0)));
        assert!(valid_request_id(&json!(-1)));
        for invalid in [json!(null), json!(true), json!(1.5), json!([]), json!({})] {
            assert!(!valid_request_id(&invalid));
        }
    }

    #[test]
    fn model_cursor_is_bound_to_revision_and_parsed_from_the_right() {
        let revision = "sha256:0123456789abcdef";
        let cursor = model_cursor(revision, 42);
        assert_eq!(
            parse_cursor(Some(&cursor), revision).expect("valid cursor"),
            42
        );
        let error = parse_cursor(Some(&cursor), "sha256:different").expect_err("revision mismatch");
        assert_eq!(error.code, "MCP_INVALID_CURSOR");
        assert_eq!(
            parse_cursor(Some("v1:sha256:0123456789abcdef:not-a-number"), revision)
                .expect_err("invalid offset")
                .code,
            "MCP_INVALID_CURSOR"
        );
    }

    #[test]
    fn public_output_schema_uses_name_and_type() {
        let schema = public_output_schema(&[OutputColumn {
            name: "revenue".to_owned(),
            data_type: DataType::Numeric,
        }]);
        assert_eq!(schema, [json!({"name": "revenue", "type": "numeric"})]);
        assert_eq!(
            supported_time_grains(DataType::TimestampTz, true),
            ["day", "week", "month", "quarter", "year"]
        );
    }

    #[test]
    fn bounded_reader_consumes_oversized_lines_and_recovers() {
        let oversized = "x".repeat(MAX_REQUEST_LINE_BYTES + 1);
        let input = format!("{oversized}\n{{}}\n");
        let mut reader = BufReader::new(Cursor::new(input.into_bytes()));
        assert!(matches!(
            read_bounded_line(&mut reader, MAX_REQUEST_LINE_BYTES),
            Ok(LineRead::Oversized)
        ));
        let second = read_bounded_line(&mut reader, MAX_REQUEST_LINE_BYTES);
        assert!(matches!(second, Ok(LineRead::Line(line)) if line == b"{}"));
    }
}
