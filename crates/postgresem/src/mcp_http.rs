use std::{
    convert::Infallible,
    env,
    future::IntoFuture,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_stream::stream;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{DefaultBodyLimit, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, HOST, ORIGIN, VARY,
            WWW_AUTHENTICATE,
        },
    },
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{Semaphore, oneshot};
use url::Url;

use crate::{
    database::CancelHandle,
    executor::{ExecutionContext, ExecutorConfig},
    mcp::{McpServer, RpcFailure},
    mcp_http_auth::{AuthError, AuthenticatedPrincipal, Authority, ServerLimits},
    mcp_http_rate::{LimitError, PrincipalLimiter, PrincipalPermit},
    mutation_executor::MutationExecutorConfig,
};

pub(crate) const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
const AUTHORITY_FILE_ENV: &str = "POSTGRESEM_MCP_HTTP_AUTHORITY_FILE";
const BIND_ENV: &str = "POSTGRESEM_MCP_HTTP_BIND";
const RATE_LIMITED_RPC_CODE: i64 = -33_001;
const CONCURRENCY_LIMITED_RPC_CODE: i64 = -33_002;
const AUTHORITY_DENIED_RPC_CODE: i64 = -33_003;
const EXECUTION_TIMEOUT_RPC_CODE: i64 = -33_004;
const RESULT_TOO_LARGE_RPC_CODE: i64 = -33_005;
const REQUEST_BODY_TIMEOUT_RPC_CODE: i64 = -33_006;
const REQUEST_BODY_TIMEOUT: Duration = Duration::from_secs(5);
const GRACEFUL_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
const BLOCKING_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum HttpServerError {
    #[error("required environment variable {0} is not set")]
    MissingEnvironment(&'static str),
    #[error("required environment variable {0} is not valid Unicode")]
    InvalidEnvironment(&'static str),
    #[error("MCP project must not be empty")]
    InvalidProject,
    #[error("MCP HTTP bind address must be an explicit loopback socket")]
    InvalidBind,
    #[error("MCP HTTP authority configuration is invalid")]
    Authority(#[source] crate::mcp_http_auth::ConfigError),
    #[error(transparent)]
    Executor(#[from] crate::executor::ExecuteError),
    #[error(transparent)]
    MutationExecutor(#[from] crate::mutation_executor::MutationExecuteError),
    #[error("MCP HTTP listener failed")]
    Listener(#[source] std::io::Error),
    #[error("MCP HTTP runtime failed")]
    Runtime(#[source] std::io::Error),
}

#[derive(Clone)]
struct HttpState {
    authority: Arc<Authority>,
    project: Arc<str>,
    query_base: ExecutorConfig,
    mutation_base: Option<MutationExecutorConfig>,
    limiter: PrincipalLimiter,
    authenticated: Arc<Semaphore>,
    pre_auth: Arc<Semaphore>,
    database: Arc<Semaphore>,
    limits: ServerLimits,
    metadata_url: Arc<str>,
}

struct ExecutionPermits {
    _principal: PrincipalPermit,
    _authenticated: tokio::sync::OwnedSemaphorePermit,
    _database: Option<tokio::sync::OwnedSemaphorePermit>,
}

#[derive(Default)]
struct CancellationState {
    handle: Option<CancelHandle>,
    disconnected: bool,
}

struct CancellationControl {
    state: Mutex<CancellationState>,
    worker_finished: AtomicBool,
}

impl CancellationControl {
    fn new() -> Self {
        Self {
            state: Mutex::new(CancellationState::default()),
            worker_finished: AtomicBool::new(false),
        }
    }

    fn register(&self, handle: CancelHandle) -> bool {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if state.disconnected {
            false
        } else {
            state.handle = Some(handle);
            true
        }
    }

    fn disconnect(&self) -> Option<CancelHandle> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.disconnected = true;
        state.handle.clone()
    }

    fn finish(&self) {
        self.worker_finished.store(true, Ordering::Release);
    }

    fn worker_finished(&self) -> bool {
        self.worker_finished.load(Ordering::Acquire)
    }
}

struct CancelOnDrop {
    control: Arc<CancellationControl>,
    armed: bool,
    maximum_duration: Duration,
}

impl CancelOnDrop {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let handle = self.armed.then(|| self.control.disconnect()).flatten();
        if let Some(handle) = handle {
            let control = Arc::clone(&self.control);
            let maximum_duration = self.maximum_duration;
            let _ = std::thread::Builder::new()
                .name("postgresem-pg-cancel".to_owned())
                .spawn(move || {
                    let deadline = std::time::Instant::now() + maximum_duration;
                    let mut retry_delay = Duration::from_millis(10);
                    while !control.worker_finished() && std::time::Instant::now() < deadline {
                        let _ = handle.cancel();
                        std::thread::sleep(retry_delay);
                        retry_delay = retry_delay
                            .saturating_mul(2)
                            .min(Duration::from_millis(250));
                    }
                });
        }
    }
}

struct WorkerFinished(Arc<CancellationControl>);

impl Drop for WorkerFinished {
    fn drop(&mut self) {
        self.0.finish();
    }
}

#[derive(Debug)]
struct ModernRequest {
    id: Value,
    method: String,
    params: Value,
    name: Option<String>,
}

type HttpRejection = Box<Response>;

pub fn serve() -> Result<(), HttpServerError> {
    let (bind, app) = prepare_server()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(HttpServerError::Runtime)?;
    let result = runtime.block_on(serve_async(bind, app));
    runtime.shutdown_timeout(BLOCKING_SHUTDOWN_TIMEOUT);
    result
}

fn prepare_server() -> Result<(SocketAddr, Router), HttpServerError> {
    let authority_path = PathBuf::from(required_environment(AUTHORITY_FILE_ENV)?);
    let authority = Arc::new(Authority::load(&authority_path).map_err(HttpServerError::Authority)?);
    let limits = authority.server_limits();
    let bind = environment_or(BIND_ENV, "127.0.0.1:8080")?
        .parse::<SocketAddr>()
        .map_err(|_| HttpServerError::InvalidBind)?;
    if !bind.ip().is_loopback()
        || matches!(bind.ip(), IpAddr::V6(address) if address.is_unspecified())
    {
        return Err(HttpServerError::InvalidBind);
    }
    let project = required_environment("POSTGRESEM_MCP_PROJECT")?;
    if project.trim().is_empty() {
        return Err(HttpServerError::InvalidProject);
    }

    let runtime_url_environment = environment_or("POSTGRESEM_MCP_RUNTIME_URL_ENV", "DATABASE_URL")?;
    let audit_url_environment = environment_or(
        "POSTGRESEM_MCP_AUDIT_URL_ENV",
        "POSTGRESEM_AUDIT_DATABASE_URL",
    )?;
    let runtime_password_environment = environment_or(
        "POSTGRESEM_MCP_RUNTIME_PASSWORD_ENV",
        "POSTGRESEM_RUNTIME_PASSWORD",
    )?;
    let audit_password_environment = environment_or(
        "POSTGRESEM_MCP_AUDIT_PASSWORD_ENV",
        "POSTGRESEM_AUDIT_WRITER_PASSWORD",
    )?;
    let first_query_role = authority
        .query_roles()
        .into_iter()
        .next()
        .ok_or(HttpServerError::InvalidProject)?;
    let query_base = ExecutorConfig::from_environment_with_passwords_and_role(
        &runtime_url_environment,
        Some(&runtime_password_environment),
        &audit_url_environment,
        Some(&audit_password_environment),
        &first_query_role,
    )?;

    let mutation_base = if authority.remote_mutation_enabled() {
        let mutation_url_environment = required_environment("POSTGRESEM_MCP_MUTATION_URL_ENV")?;
        let mutation_password_environment = environment_or(
            "POSTGRESEM_MCP_MUTATION_PASSWORD_ENV",
            "POSTGRESEM_MUTATION_RUNTIME_PASSWORD",
        )?;
        let first_mutation_role = authority
            .mutation_roles()
            .into_iter()
            .next()
            .ok_or(HttpServerError::InvalidProject)?;
        Some(
            MutationExecutorConfig::from_environment_with_passwords_and_role(
                &mutation_url_environment,
                Some(&mutation_password_environment),
                &audit_url_environment,
                Some(&audit_password_environment),
                &first_mutation_role,
            )?,
        )
    } else {
        None
    };

    for role in authority.query_roles() {
        query_base.with_database_role(&role)?.preflight_role()?;
    }
    if let Some(base) = &mutation_base {
        for role in authority.mutation_roles() {
            base.with_database_role(&role)?.preflight_role()?;
        }
    }

    let resource = Url::parse(authority.resource()).map_err(|_| HttpServerError::InvalidProject)?;
    let endpoint_path = resource.path().to_owned();
    let metadata_path = protected_resource_metadata_path(resource.path());
    let metadata_url = protected_resource_metadata_url(&resource, &metadata_path)
        .ok_or(HttpServerError::InvalidProject)?;
    let state = HttpState {
        authority,
        project: Arc::from(project),
        query_base,
        mutation_base,
        limiter: PrincipalLimiter::default(),
        authenticated: Arc::new(Semaphore::new(
            usize::try_from(limits.max_concurrent_requests)
                .map_err(|_| HttpServerError::InvalidProject)?,
        )),
        pre_auth: Arc::new(Semaphore::new(
            usize::try_from(limits.max_pre_auth_concurrent_requests)
                .map_err(|_| HttpServerError::InvalidProject)?,
        )),
        database: Arc::new(Semaphore::new(
            usize::try_from(limits.max_database_connections)
                .map_err(|_| HttpServerError::InvalidProject)?,
        )),
        limits,
        metadata_url: Arc::from(metadata_url),
    };
    let maximum_body = usize::try_from(limits.max_request_body_bytes)
        .map_err(|_| HttpServerError::InvalidProject)?;
    let app = Router::new()
        .route(&endpoint_path, post(handle_mcp))
        .route(&metadata_path, get(handle_metadata))
        .layer(DefaultBodyLimit::max(maximum_body))
        .with_state(state);
    Ok((bind, app))
}

async fn serve_async(bind: SocketAddr, app: Router) -> Result<(), HttpServerError> {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(HttpServerError::Listener)?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result.map_err(HttpServerError::Listener),
        () = shutdown_signal() => {
            let _ = shutdown_tx.send(());
            match tokio::time::timeout(GRACEFUL_DRAIN_TIMEOUT, &mut server).await {
                Ok(result) => result.map_err(HttpServerError::Listener),
                Err(_) => Ok(()),
            }
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn handle_metadata(State(state): State<HttpState>, headers: HeaderMap) -> Response {
    let Ok(_pre_auth) = Arc::clone(&state.pre_auth).try_acquire_owned() else {
        return empty_response(StatusCode::TOO_MANY_REQUESTS);
    };
    if let Err(response) = validate_network_headers(&state, &headers) {
        return *response;
    }
    let response = json!({
        "resource": state.authority.resource(),
        "authorization_servers": state.authority.authorization_servers(),
        "scopes_supported": state.authority.scopes_supported(),
        "bearer_methods_supported": ["header"]
    });
    json_response(StatusCode::OK, response)
}

async fn handle_mcp(State(state): State<HttpState>, request: Request) -> Response {
    let Ok(pre_auth) = Arc::clone(&state.pre_auth).try_acquire_owned() else {
        return empty_response(StatusCode::TOO_MANY_REQUESTS);
    };
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    if let Err(response) = validate_network_headers(&state, &headers) {
        return *response;
    }
    if header_bytes(&headers) > state.limits.max_header_bytes {
        return json_error_response(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            Value::Null,
            -32600,
            "request headers are too large",
            "MCP_HEADERS_TOO_LARGE",
        );
    }
    if !accepts_modern_response(&headers) || !has_json_content_type(&headers) {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "invalid request",
            "MCP_INVALID_HTTP_HEADERS",
        );
    }
    let authorization = match single_header(&headers, AUTHORIZATION) {
        Ok(Some(value)) => value,
        Ok(None) => "",
        Err(()) => {
            return authentication_response(&state, AuthError::MalformedRequest);
        }
    };
    let principal = match state.authority.authenticate(authorization) {
        Ok(principal) => principal,
        Err(error) => return authentication_response(&state, error),
    };
    if !principal.has_scope(state.authority.query_scope()) {
        return insufficient_scope_response(&state, state.authority.query_scope());
    }
    let rate = principal.rate_limit();
    let principal_permit = match state.limiter.acquire(
        principal.authority_id(),
        rate.requests_per_minute,
        rate.burst,
        rate.max_concurrent,
    ) {
        Ok(permit) => permit,
        Err(error) => return limit_response(error),
    };
    let authenticated = match Arc::clone(&state.authenticated).try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return limit_response(LimitError::Concurrency),
    };
    let maximum_body = match usize::try_from(state.limits.max_request_body_bytes) {
        Ok(maximum_body) => maximum_body,
        Err(_) => {
            return json_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                Value::Null,
                -32603,
                "internal error",
                "MCP_INTERNAL_ERROR",
            );
        }
    };
    let body = match tokio::time::timeout(REQUEST_BODY_TIMEOUT, to_bytes(body, maximum_body)).await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => {
            return json_error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                Value::Null,
                -32600,
                "request is too large",
                "MCP_REQUEST_TOO_LARGE",
            );
        }
        Err(_) => {
            return json_error_response(
                StatusCode::REQUEST_TIMEOUT,
                Value::Null,
                REQUEST_BODY_TIMEOUT_RPC_CODE,
                "request body timed out",
                "MCP_REQUEST_BODY_TIMEOUT",
            );
        }
    };
    drop(pre_auth);
    let request = match parse_modern_request(&headers, &body) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    if !known_method(&request.method) {
        return json_error_response(
            StatusCode::NOT_FOUND,
            request.id,
            -32601,
            "method not found",
            "MCP_METHOD_NOT_FOUND",
        );
    }

    let database = if method_uses_database(&request.method) {
        match Arc::clone(&state.database).try_acquire_many_owned(2) {
            Ok(permit) => Some(permit),
            Err(_) => return limit_response(LimitError::Concurrency),
        }
    } else {
        None
    };
    let permits = ExecutionPermits {
        _principal: principal_permit,
        _authenticated: authenticated,
        _database: database,
    };
    let server = match server_for_principal(&state, &principal) {
        Ok(server) => server,
        Err(_) => {
            return json_error_response(
                StatusCode::FORBIDDEN,
                request.id,
                AUTHORITY_DENIED_RPC_CODE,
                "request is not authorized",
                "MCP_AUTHORITY_DENIED",
            );
        }
    };
    if is_streaming_execution(&request) {
        streaming_response(state, server, request, permits)
    } else {
        simple_response(state, server, request, permits).await
    }
}

fn server_for_principal(
    state: &HttpState,
    principal: &AuthenticatedPrincipal,
) -> Result<McpServer, ()> {
    let query = state
        .query_base
        .with_database_role(principal.query_role())
        .map_err(|_| ())?;
    let mutation = if state.authority.remote_mutation_enabled()
        && principal.has_scope(state.authority.mutation_scope())
    {
        match (&state.mutation_base, principal.mutation_role()) {
            (Some(base), Some(role)) => Some(base.with_database_role(role).map_err(|_| ())?),
            _ => None,
        }
    } else {
        None
    };
    let context = ExecutionContext::new_with_authority(
        principal.audit_pseudonym(),
        "mcp-http",
        principal.authority_id(),
    )
    .map_err(|_| ())?;
    Ok(McpServer::configured(
        state.project.to_string(),
        query,
        mutation,
        context,
    ))
}

async fn simple_response(
    state: HttpState,
    server: McpServer,
    request: ModernRequest,
    permits: ExecutionPermits,
) -> Response {
    let id = request.id;
    let method = request.method;
    let params = request.params;
    let timeout = Duration::from_secs(state.limits.max_execution_seconds);
    let task = tokio::task::spawn_blocking(move || {
        let _permits = permits;
        server.dispatch_modern(&method, params)
    });
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(Ok(result))) => complete_json_response(&state, id, result),
        Ok(Ok(Err(error))) => rpc_failure_response(id, error),
        Ok(Err(_)) => json_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            id,
            -32603,
            "internal error",
            "MCP_INTERNAL_ERROR",
        ),
        Err(_) => json_error_response(
            StatusCode::GATEWAY_TIMEOUT,
            id,
            EXECUTION_TIMEOUT_RPC_CODE,
            "request timed out",
            "MCP_EXECUTION_TIMEOUT",
        ),
    }
}

fn streaming_response(
    state: HttpState,
    server: McpServer,
    request: ModernRequest,
    permits: ExecutionPermits,
) -> Response {
    let id = request.id;
    let method = request.method;
    let params = request.params;
    let (result_tx, result_rx) = oneshot::channel();
    let cancellation = Arc::new(CancellationControl::new());
    let cancellation_for_task = Arc::clone(&cancellation);
    tokio::task::spawn_blocking(move || {
        let _worker_finished = WorkerFinished(Arc::clone(&cancellation_for_task));
        let _permits = permits;
        let result = server.dispatch_modern_with_cancel(&method, params, |handle| {
            cancellation_for_task.register(handle)
        });
        let _ = result_tx.send(result);
    });

    let maximum_result = state.limits.max_result_bytes;
    let maximum_duration = Duration::from_secs(
        state
            .limits
            .max_execution_seconds
            .min(state.limits.max_sse_seconds),
    );
    let deadline = tokio::time::Instant::now() + maximum_duration;
    let cancel_guard = CancelOnDrop {
        control: cancellation,
        armed: true,
        maximum_duration,
    };
    let response_stream = stream! {
        let mut cancel_guard = cancel_guard;
        let mut result_rx = result_rx;
        tokio::select! {
            result = &mut result_rx => {
                let value = streaming_result_value(&id, result, maximum_result);
                cancel_guard.disarm();
                yield Ok::<Event, Infallible>(Event::default().data(value.to_string()));
                return;
            }
            _ = tokio::time::sleep_until(deadline) => {
                let value = rpc_error(
                    id,
                    EXECUTION_TIMEOUT_RPC_CODE,
                    "request timed out",
                    "MCP_EXECUTION_TIMEOUT",
                );
                yield Ok::<Event, Infallible>(Event::default().data(value.to_string()));
                return;
            }
        }
    };
    let keep_alive = KeepAlive::new()
        .interval(Duration::from_secs(state.limits.sse_keepalive_seconds))
        .text("");
    let mut response = Sse::new(response_stream)
        .keep_alive(keep_alive)
        .into_response();
    apply_private_headers(response.headers_mut());
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

fn streaming_result_value(
    id: &Value,
    result: Result<Result<Value, RpcFailure>, oneshot::error::RecvError>,
    maximum_result: u64,
) -> Value {
    match result {
        Ok(Ok(value)) => {
            let response = json!({"jsonrpc": "2.0", "id": id, "result": value});
            if serialized_size(&response) <= maximum_result {
                response
            } else {
                rpc_error(
                    id.clone(),
                    RESULT_TOO_LARGE_RPC_CODE,
                    "result is too large",
                    "MCP_RESULT_TOO_LARGE",
                )
            }
        }
        Ok(Err(error)) => rpc_error(id.clone(), error.rpc_code, error.message, error.public_code),
        Err(_) => rpc_error(id.clone(), -32603, "internal error", "MCP_INTERNAL_ERROR"),
    }
}

fn complete_json_response(state: &HttpState, id: Value, result: Value) -> Response {
    let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
    if serialized_size(&response) > state.limits.max_result_bytes {
        return json_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            response["id"].clone(),
            RESULT_TOO_LARGE_RPC_CODE,
            "result is too large",
            "MCP_RESULT_TOO_LARGE",
        );
    }
    json_response(StatusCode::OK, response)
}

fn rpc_failure_response(id: Value, error: RpcFailure) -> Response {
    let status = if error.rpc_code == -32601 {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    json_error_response(status, id, error.rpc_code, error.message, error.public_code)
}

fn parse_modern_request(headers: &HeaderMap, body: &[u8]) -> Result<ModernRequest, HttpRejection> {
    let message: Value = serde_json::from_slice(body).map_err(|_| {
        Box::new(json_error_response(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32700,
            "parse error",
            "MCP_PARSE_ERROR",
        ))
    })?;
    let object = message.as_object().ok_or_else(|| {
        Box::new(json_error_response(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "invalid request",
            "MCP_INVALID_REQUEST",
        ))
    })?;
    let id = object.get("id").cloned().ok_or_else(|| {
        Box::new(json_error_response(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "notifications are not supported",
            "MCP_NOTIFICATION_UNSUPPORTED",
        ))
    })?;
    if !valid_request_id(&id) || object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(Box::new(json_error_response(
            StatusCode::BAD_REQUEST,
            Value::Null,
            -32600,
            "invalid request",
            "MCP_INVALID_REQUEST",
        )));
    }
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Box::new(json_error_response(
                StatusCode::BAD_REQUEST,
                id.clone(),
                -32600,
                "invalid request",
                "MCP_INVALID_REQUEST",
            ))
        })?
        .to_owned();
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    let parameter_object = params.as_object().ok_or_else(|| {
        Box::new(json_error_response(
            StatusCode::BAD_REQUEST,
            id.clone(),
            -32602,
            "invalid method parameters",
            "MCP_INVALID_PARAMS",
        ))
    })?;
    validate_request_meta(headers, parameter_object, &id)?;
    validate_method_headers(headers, &method, parameter_object, &id)?;
    let name = match method.as_str() {
        "tools/call" => parameter_object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        "resources/read" => parameter_object
            .get("uri")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    };
    Ok(ModernRequest {
        id,
        method,
        params,
        name,
    })
}

fn validate_request_meta(
    headers: &HeaderMap,
    params: &serde_json::Map<String, Value>,
    id: &Value,
) -> Result<(), HttpRejection> {
    let header_version = required_header(headers, "mcp-protocol-version", id)?;
    let metadata = params
        .get("_meta")
        .and_then(Value::as_object)
        .ok_or_else(|| header_mismatch(id.clone()))?;
    let body_version = metadata
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| header_mismatch(id.clone()))?;
    if header_version != body_version {
        return Err(header_mismatch(id.clone()));
    }
    if body_version != MODERN_PROTOCOL_VERSION {
        return Err(Box::new(unsupported_protocol_response(
            StatusCode::BAD_REQUEST,
            id.clone(),
            body_version,
        )));
    }
    let client = metadata
        .get("io.modelcontextprotocol/clientInfo")
        .and_then(Value::as_object)
        .ok_or_else(|| header_mismatch(id.clone()))?;
    if client
        .get("name")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        || client
            .get("version")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(header_mismatch(id.clone()));
    }
    if !metadata
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_some_and(Value::is_object)
    {
        return Err(Box::new(json_error_response(
            StatusCode::BAD_REQUEST,
            id.clone(),
            -32021,
            "required client capability is missing",
            "MCP_MISSING_REQUIRED_CLIENT_CAPABILITY",
        )));
    }
    Ok(())
}

fn validate_method_headers(
    headers: &HeaderMap,
    method: &str,
    params: &serde_json::Map<String, Value>,
    id: &Value,
) -> Result<(), HttpRejection> {
    if required_header(headers, "mcp-method", id)? != method {
        return Err(header_mismatch(id.clone()));
    }
    let expected_name = match method {
        "tools/call" => params.get("name").and_then(Value::as_str),
        "resources/read" => params.get("uri").and_then(Value::as_str),
        _ => None,
    };
    match expected_name {
        Some(expected) => {
            let encoded = required_header(headers, "mcp-name", id)?;
            let decoded =
                decode_header_value(encoded).ok_or_else(|| header_mismatch(id.clone()))?;
            if decoded != expected {
                return Err(header_mismatch(id.clone()));
            }
        }
        None if headers.contains_key("mcp-name") => return Err(header_mismatch(id.clone())),
        None => {}
    }
    Ok(())
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    id: &Value,
) -> Result<&'a str, HttpRejection> {
    single_header(headers, name)
        .map_err(|_| header_mismatch(id.clone()))?
        .ok_or_else(|| header_mismatch(id.clone()))
}

fn single_header(
    headers: &HeaderMap,
    name: impl axum::http::header::AsHeaderName,
) -> Result<Option<&str>, ()> {
    let mut values = headers.get_all(name).iter();
    let first = match values.next() {
        Some(value) => value.to_str().map_err(|_| ())?,
        None => return Ok(None),
    };
    if values.next().is_some() {
        return Err(());
    }
    Ok(Some(first))
}

fn decode_header_value(value: &str) -> Option<String> {
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|value| value.strip_suffix("?="))
    {
        let bytes = STANDARD.decode(encoded).ok()?;
        String::from_utf8(bytes).ok()
    } else if value.starts_with("=?base64?") || value.ends_with("?=") {
        None
    } else {
        Some(value.to_owned())
    }
}

fn validate_network_headers(state: &HttpState, headers: &HeaderMap) -> Result<(), HttpRejection> {
    let host = single_header(headers, HOST)
        .map_err(|_| Box::new(empty_response(StatusCode::BAD_REQUEST)))?
        .ok_or_else(|| Box::new(empty_response(StatusCode::BAD_REQUEST)))?;
    if !state.authority.is_allowed_host(host) {
        return Err(Box::new(empty_response(StatusCode::FORBIDDEN)));
    }
    if let Some(origin) = single_header(headers, ORIGIN)
        .map_err(|_| Box::new(empty_response(StatusCode::BAD_REQUEST)))?
    {
        if !state.authority.is_allowed_origin(origin) {
            return Err(Box::new(empty_response(StatusCode::FORBIDDEN)));
        }
    }
    Ok(())
}

fn accepts_modern_response(headers: &HeaderMap) -> bool {
    single_header(headers, ACCEPT)
        .ok()
        .flatten()
        .is_some_and(|value| {
            let values = value
                .split(',')
                .filter_map(|part| part.trim().split(';').next());
            let mut json = false;
            let mut sse = false;
            for value in values {
                json |= value.eq_ignore_ascii_case("application/json");
                sse |= value.eq_ignore_ascii_case("text/event-stream");
            }
            json && sse
        })
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    single_header(headers, CONTENT_TYPE)
        .ok()
        .flatten()
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

fn known_method(method: &str) -> bool {
    matches!(
        method,
        "server/discover"
            | "ping"
            | "tools/list"
            | "tools/call"
            | "resources/list"
            | "resources/read"
    )
}

fn method_uses_database(method: &str) -> bool {
    matches!(method, "tools/call" | "resources/list" | "resources/read")
}

fn is_streaming_execution(request: &ModernRequest) -> bool {
    request.method == "tools/call"
        && matches!(
            request.name.as_deref(),
            Some("query_semantic_model" | "mutate_semantic_model")
        )
}

fn valid_request_id(id: &Value) -> bool {
    match id {
        Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        _ => false,
    }
}

fn authentication_response(state: &HttpState, error: AuthError) -> Response {
    if error == AuthError::UnknownPrincipal {
        return empty_response(StatusCode::FORBIDDEN);
    }
    if error == AuthError::InsufficientScope {
        return insufficient_scope_response(state, state.authority.query_scope());
    }
    let mut response = empty_response(StatusCode::UNAUTHORIZED);
    let challenge = bearer_challenge(
        &state.metadata_url,
        state.authority.query_scope(),
        error.oauth_error(),
    );
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        response.headers_mut().insert(WWW_AUTHENTICATE, value);
    }
    response
}

fn insufficient_scope_response(state: &HttpState, scope: &str) -> Response {
    let mut response = empty_response(StatusCode::FORBIDDEN);
    let challenge = bearer_challenge(&state.metadata_url, scope, "insufficient_scope");
    if let Ok(value) = HeaderValue::from_str(&challenge) {
        response.headers_mut().insert(WWW_AUTHENTICATE, value);
    }
    response
}

fn bearer_challenge(metadata_url: &str, scope: &str, error: &str) -> String {
    let mut challenge = format!("Bearer resource_metadata=\"{metadata_url}\", scope=\"{scope}\"");
    if !error.is_empty() {
        challenge.push_str(&format!(", error=\"{error}\""));
    }
    challenge
}

fn limit_response(error: LimitError) -> Response {
    let (rpc_code, code) = match error {
        LimitError::Rate => (RATE_LIMITED_RPC_CODE, "MCP_RATE_LIMITED"),
        LimitError::Concurrency => (CONCURRENCY_LIMITED_RPC_CODE, "MCP_CONCURRENCY_LIMITED"),
        LimitError::Unavailable => (CONCURRENCY_LIMITED_RPC_CODE, "MCP_LIMITER_UNAVAILABLE"),
    };
    json_error_response(
        StatusCode::TOO_MANY_REQUESTS,
        Value::Null,
        rpc_code,
        "request limit exceeded",
        code,
    )
}

fn unsupported_protocol_response(status: StatusCode, id: Value, requested: &str) -> Response {
    json_response(
        status,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32022,
                "message": "unsupported protocol version",
                "data": {
                    "code": "MCP_UNSUPPORTED_PROTOCOL_VERSION",
                    "supported": [MODERN_PROTOCOL_VERSION],
                    "requested": requested
                }
            }
        }),
    )
}

fn header_mismatch(id: Value) -> HttpRejection {
    Box::new(json_error_response(
        StatusCode::BAD_REQUEST,
        id,
        -32020,
        "request headers do not match the request body",
        "MCP_HEADER_MISMATCH",
    ))
}

fn rpc_error(id: Value, code: i64, message: &'static str, public_code: &'static str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": {"code": public_code}
        }
    })
}

fn json_error_response(
    status: StatusCode,
    id: Value,
    code: i64,
    message: &'static str,
    public_code: &'static str,
) -> Response {
    json_response(status, rpc_error(id, code, message, public_code))
}

fn json_response(status: StatusCode, value: Value) -> Response {
    let mut response = (status, Json(value)).into_response();
    apply_private_headers(response.headers_mut());
    response
}

fn empty_response(status: StatusCode) -> Response {
    let mut response = status.into_response();
    apply_private_headers(response.headers_mut());
    response
}

fn apply_private_headers(headers: &mut HeaderMap) {
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(VARY, HeaderValue::from_static("Authorization"));
}

fn serialized_size(value: &Value) -> u64 {
    serde_json::to_vec(value)
        .ok()
        .and_then(|value| u64::try_from(value.len()).ok())
        .unwrap_or(u64::MAX)
}

fn header_bytes(headers: &HeaderMap) -> u64 {
    headers.iter().fold(0_u64, |total, (name, value)| {
        total
            .saturating_add(u64::try_from(name.as_str().len()).unwrap_or(u64::MAX))
            .saturating_add(u64::try_from(value.as_bytes().len()).unwrap_or(u64::MAX))
    })
}

fn protected_resource_metadata_path(resource_path: &str) -> String {
    if resource_path == "/" {
        "/.well-known/oauth-protected-resource".to_owned()
    } else {
        format!("/.well-known/oauth-protected-resource{resource_path}")
    }
}

fn protected_resource_metadata_url(resource: &Url, metadata_path: &str) -> Option<String> {
    let host = resource.host_str()?;
    let authority = match resource.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    Some(format!(
        "{}://{authority}{metadata_path}",
        resource.scheme()
    ))
}

fn required_environment(variable: &'static str) -> Result<String, HttpServerError> {
    env::var(variable).map_err(|error| match error {
        env::VarError::NotPresent => HttpServerError::MissingEnvironment(variable),
        env::VarError::NotUnicode(_) => HttpServerError::InvalidEnvironment(variable),
    })
}

fn environment_or(
    variable: &'static str,
    default: &'static str,
) -> Result<String, HttpServerError> {
    match env::var(variable) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(env::VarError::NotUnicode(_)) => Err(HttpServerError::InvalidEnvironment(variable)),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    use super::{
        MODERN_PROTOCOL_VERSION, decode_header_value, parse_modern_request,
        protected_resource_metadata_path, protected_resource_metadata_url,
    };

    fn headers(method: &'static str, name: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "mcp-protocol-version",
            HeaderValue::from_static(MODERN_PROTOCOL_VERSION),
        );
        headers.insert("mcp-method", HeaderValue::from_static(method));
        if let Some(name) = name {
            headers.insert(
                "mcp-name",
                HeaderValue::from_str(name).expect("valid test name"),
            );
        }
        headers
    }

    #[test]
    fn validates_modern_metadata_and_mirrored_headers() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "query_semantic_model",
                "arguments": {},
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
                    "io.modelcontextprotocol/clientInfo": {
                        "name": "test",
                        "version": "1"
                    },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        assert!(
            parse_modern_request(
                &headers("tools/call", Some("query_semantic_model")),
                &serde_json::to_vec(&body).expect("request serializes"),
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_header_body_mismatch_and_notifications() {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
                    "io.modelcontextprotocol/clientInfo": {
                        "name": "test",
                        "version": "1"
                    },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });
        assert!(
            parse_modern_request(
                &headers("tools/list", None),
                &serde_json::to_vec(&body).expect("request serializes"),
            )
            .is_err()
        );
        let mut notification = body;
        notification.as_object_mut().expect("object").remove("id");
        assert!(
            parse_modern_request(
                &headers("ping", None),
                &serde_json::to_vec(&notification).expect("request serializes"),
            )
            .is_err()
        );
    }

    #[test]
    fn decodes_protocol_base64_sentinel() {
        assert_eq!(
            decode_header_value("=?base64?SGVsbG8sIOS4lueVjA==?=").as_deref(),
            Some("Hello, 世界")
        );
        assert!(decode_header_value("=?base64?not-base64?=").is_none());
    }

    #[test]
    fn derives_rfc_9728_metadata_path() {
        assert_eq!(
            protected_resource_metadata_path("/mcp"),
            "/.well-known/oauth-protected-resource/mcp"
        );
        assert_eq!(
            protected_resource_metadata_path("/"),
            "/.well-known/oauth-protected-resource"
        );
        let resource =
            url::Url::parse("https://mcp.example.test:8443/mcp").expect("valid resource");
        assert_eq!(
            protected_resource_metadata_url(&resource, "/.well-known/oauth-protected-resource/mcp")
                .as_deref(),
            Some("https://mcp.example.test:8443/.well-known/oauth-protected-resource/mcp")
        );
    }
}
