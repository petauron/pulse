use std::{
    collections::VecDeque,
    error::Error,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequest, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use pulse_protocol::{
    EnrollmentRequest, EnrollmentResponse, HealthResponse, PROTOCOL_VERSION, SnapshotResponse,
    SystemSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio::time::timeout;
use uuid::Uuid;

mod assets;
mod auth;
mod control;
#[cfg(test)]
mod control_tests;
mod management;
mod notifications;
mod session_gate;
mod storage;

pub use auth::{AuthConfig, GithubOAuthConfig};
pub use storage::{AuditEvent, EnrollmentSecret};
use storage::{
    HistorySeries, RETENTION_PRUNE_BATCH_SIZE, Storage, StorageError, hash_token,
    signed_difference, unix_time_ms,
};

const DEFAULT_RETENTION_DAYS: u32 = 7;
const DEFAULT_OFFLINE_AFTER_SECONDS: u64 = 90;
const DEFAULT_MAX_NODES: u32 = 100;
const DEFAULT_MAX_DATABASE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_HISTORY_POINTS: u32 = 1_000;
const MAX_CONCURRENT_REQUESTS: usize = 128;
const REQUEST_QUEUE_TIMEOUT: Duration = Duration::from_secs(2);
const HANDLER_TIMEOUT: Duration = Duration::from_secs(15);
const ENROLLMENT_ATTEMPTS_PER_MINUTE: usize = 10;

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub database_path: PathBuf,
    pub retention_days: u32,
    pub offline_after_seconds: u64,
    pub max_nodes: u32,
    pub max_database_bytes: u64,
    pub auth: AuthConfig,
}

impl ServiceConfig {
    #[must_use]
    pub fn with_database(database_path: PathBuf) -> Self {
        Self {
            database_path,
            retention_days: DEFAULT_RETENTION_DAYS,
            offline_after_seconds: DEFAULT_OFFLINE_AFTER_SECONDS,
            max_nodes: DEFAULT_MAX_NODES,
            max_database_bytes: DEFAULT_MAX_DATABASE_BYTES,
            auth: AuthConfig::default(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    storage: Arc<Storage>,
    database_permit: Arc<Semaphore>,
    request_permits: Arc<Semaphore>,
    enrollment_attempts: Arc<Mutex<VecDeque<Instant>>>,
    retention_days: u32,
    offline_after_seconds: u64,
    max_nodes: u32,
    auth: auth::AuthState,
    session_gate: session_gate::SessionGate,
}

impl AppState {
    /// Opens and validates the configured database.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration is invalid or the database cannot be opened,
    /// secured, backed up, or migrated.
    pub fn open(config: &ServiceConfig) -> Result<Self, Box<dyn Error + Send + Sync>> {
        validate_config(config)?;
        let storage = Storage::open(&config.database_path, config.max_database_bytes)?;
        storage.prune_expired(unix_time_ms()?, config.retention_days)?;
        Ok(Self {
            storage: Arc::new(storage),
            database_permit: Arc::new(Semaphore::new(1)),
            request_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
            enrollment_attempts: Arc::new(Mutex::new(VecDeque::new())),
            retention_days: config.retention_days,
            offline_after_seconds: config.offline_after_seconds,
            max_nodes: config.max_nodes,
            auth: auth::AuthState::new(
                &config.database_path,
                config.auth.clone(),
                config.max_database_bytes,
            )?,
            session_gate: session_gate::SessionGate::default(),
        })
    }

    /// Removes expired history in bounded transactions until the current retention window is clean.
    ///
    /// # Errors
    ///
    /// Returns an error when the clock or a bounded database maintenance operation fails.
    pub async fn maintain_retention(&self) -> Result<usize, String> {
        let mut total_removed = 0_usize;
        loop {
            let now = unix_time_ms().map_err(|error| error.to_string())?;
            let retention_days = self.retention_days;
            let removed = self
                .database(move |storage| storage.prune_expired(now, retention_days))
                .await
                .map_err(|error| error.message)?;
            total_removed = total_removed.saturating_add(removed);
            if removed < RETENTION_PRUNE_BATCH_SIZE {
                return Ok(total_removed);
            }
            tokio::task::yield_now().await;
        }
    }

    async fn database<T, F>(&self, operation: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&Storage) -> Result<T, StorageError> + Send + 'static,
    {
        let permit = timeout(
            HANDLER_TIMEOUT,
            self.database_permit.clone().acquire_owned(),
        )
        .await
        .map_err(|_| ApiError::unavailable("database queue timed out"))?
        .map_err(|_| ApiError::unavailable("database worker is unavailable"))?;
        let storage = Arc::clone(&self.storage);
        let session_lease = session_gate::current();
        let result = timeout(
            HANDLER_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let _session_lease = session_lease;
                let _permit = permit;
                operation(&storage)
            }),
        )
        .await
        .map_err(|_| ApiError::unavailable("database operation timed out"))?
        .map_err(|error| {
            tracing::error!(%error, "database worker failed");
            ApiError::unavailable("database worker failed")
        })?;
        result.map_err(map_storage_error)
    }

    /// Evaluates monitoring rules and delivers at most four notifications to
    /// HTTPS channels explicitly configured and enabled by the administrator.
    ///
    /// # Errors
    /// Returns an error when durable alert state cannot be read or updated.
    pub async fn maintain_alerts(&self) -> Result<(), String> {
        let now = current_time().map_err(|e| e.message)?;
        let offline = self.offline_after_seconds.saturating_mul(1000);
        let deliveries = self
            .database(move |storage| {
                storage.evaluate_alerts(now, offline)?;
                storage.pending_deliveries(now)
            })
            .await
            .map_err(|e| e.message)?;
        let mut tasks = tokio::task::JoinSet::new();
        for delivery in deliveries {
            tasks.spawn(async move {
                let success = notifications::send_delivery(&delivery).await;
                (delivery.id, success)
            });
        }
        while let Some(result) = tasks.join_next().await {
            let (id, success) = result.map_err(|_| "notification worker failed".to_owned())?;
            let now = current_time().map_err(|e| e.message)?;
            self.database(move |storage| storage.delivery_result(&id, success, now))
                .await
                .map_err(|e| e.message)?;
        }
        Ok(())
    }

    async fn allow_enrollment_attempt(&self) -> Result<(), ApiError> {
        let mut attempts = self.enrollment_attempts.lock().await;
        let now = Instant::now();
        let cutoff = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        while attempts.front().is_some_and(|attempt| *attempt < cutoff) {
            attempts.pop_front();
        }
        if attempts.len() >= ENROLLMENT_ATTEMPTS_PER_MINUTE {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "too many enrollment attempts; retry later".to_owned(),
            });
        }
        attempts.push_back(now);
        Ok(())
    }
}

pub struct Administration {
    storage: Storage,
}

impl Administration {
    /// Opens the Service database for a bounded administrative operation.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened, secured, backed up, or migrated.
    pub fn open(
        database_path: &FsPath,
        max_database_bytes: u64,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self {
            storage: Storage::open(database_path, max_database_bytes)?,
        })
    }

    /// Creates a single-use enrollment token with an explicit lifetime.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe lifetime or a database failure.
    pub fn create_enrollment(
        &self,
        ttl_seconds: u64,
    ) -> Result<EnrollmentSecret, Box<dyn Error + Send + Sync>> {
        if !(60..=86_400).contains(&ttl_seconds) {
            return Err("enrollment TTL must be between 60 and 86400 seconds".into());
        }
        Ok(self
            .storage
            .create_enrollment(ttl_seconds, unix_time_ms()?)?)
    }

    /// Revokes an unused enrollment token.
    ///
    /// # Errors
    ///
    /// Returns an error when the token does not exist or the database operation fails.
    pub fn revoke_enrollment(&self, id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(self.storage.revoke_enrollment(id, unix_time_ms()?)?)
    }

    /// Replaces an active node's credential and returns the new secret once.
    ///
    /// # Errors
    ///
    /// Returns an error when the node does not exist or the database operation fails.
    pub fn rotate_node_token(&self, node_id: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        Ok(self.storage.rotate_node_token(node_id, unix_time_ms()?)?)
    }

    /// Revokes an active node without deleting its stored history.
    ///
    /// # Errors
    ///
    /// Returns an error when the node does not exist or the database operation fails.
    pub fn revoke_node(&self, node_id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(self.storage.revoke_node(node_id, unix_time_ms()?)?)
    }

    /// Permanently deletes a node and its stored snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error when the node does not exist or the database operation fails.
    pub fn delete_node(&self, node_id: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        Ok(self.storage.delete_node(node_id, unix_time_ms()?)?)
    }

    /// Returns the newest bounded set of credential lifecycle audit events.
    ///
    /// # Errors
    ///
    /// Returns an error when the database query fails.
    pub fn audit_events(
        &self,
        limit: u32,
    ) -> Result<Vec<AuditEvent>, Box<dyn Error + Send + Sync>> {
        Ok(self.storage.audit_events(limit.clamp(1, 1_000))?)
    }

    /// Creates a consistent private online backup beside the configured database.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot create or validate the online backup.
    pub fn backup(&self) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
        self.storage.backup()
    }
}

pub fn router(state: AppState) -> Router {
    let authentication = state.auth.router();
    Router::new()
        .route("/healthz", get(health_response))
        .route("/api/v1/agents/enroll", post(enroll_agent))
        .route("/api/v1/agents/snapshots", post(ingest_snapshot))
        .route("/api/v1/agents/config", get(management::agent_config))
        .route("/api/v1/agents/probes", post(management::agent_probes))
        .route("/api/v1/nodes", get(native_nodes))
        .route("/api/v1/nodes/{node_id}/history", get(native_history))
        .route(
            "/api/v1/nodes/{node_id}/probes",
            get(management::probe_history),
        )
        .merge(management::routes())
        .route("/api/rpc2", post(rpc_handler))
        .route("/api/public", get(public_settings))
        .route("/api/me", get(me))
        .route("/api/version", get(version))
        .route("/api/records/load", get(load_records))
        .fallback(assets::static_asset)
        .with_state(state.clone())
        .merge(authentication)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .layer(middleware::from_fn_with_state(state, request_guard))
}

async fn request_guard(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let Ok(Ok(permit)) = timeout(
        REQUEST_QUEUE_TIMEOUT,
        state.request_permits.clone().acquire_owned(),
    )
    .await
    else {
        return ApiError::unavailable("server is busy; retry later").into_response();
    };
    let path = request.uri().path();
    let administrative = path.starts_with("/api/admin/");
    let protected = administrative
        || path == "/api/rpc2"
        || path == "/api/v1/nodes"
        || path.starts_with("/api/v1/nodes/")
        || path.starts_with("/api/records/");
    let auth_mutation = path.starts_with("/api/auth/")
        && !matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD
        );
    // OAuth callback is a GET but issues sessions and may evict an older one.
    let changes_sessions = auth_mutation || path == "/api/auth/oauth/callback";
    // Never let an anonymous slow upload hold the exclusive session lease.
    // Retain the original request parts so cookies, Origin, and CSRF are unchanged.
    let request = if auth_mutation {
        match buffer_authentication_body(request).await {
            Ok(request) => request,
            Err(response) => return *response,
        }
    } else {
        request
    };
    let lease = if administrative || changes_sessions {
        match state.session_gate.acquire(changes_sessions).await {
            Ok(lease) => Some(lease),
            Err(message) => return ApiError::unavailable(message).into_response(),
        }
    } else {
        None
    };
    let response = session_gate::scope(lease, async move {
        timeout(HANDLER_TIMEOUT, async move {
            if protected {
                let private_site = match state.database(Storage::settings).await {
                    Ok(settings) => settings.private_site,
                    Err(error) => return error.into_response(),
                };
                match state
                    .auth
                    .authorize(request.headers(), administrative, private_site)
                    .await
                {
                    Err(error) => return error.into_response(),
                    Ok(Some(session))
                        if administrative
                            && !matches!(
                                *request.method(),
                                axum::http::Method::GET | axum::http::Method::HEAD
                            ) =>
                    {
                        if let Err(error) = state.auth.verify_mutation(request.headers(), &session)
                        {
                            return error.into_response();
                        }
                    }
                    Ok(_) => {}
                }
            }
            next.run(request).await
        })
        .await
    })
    .await;
    drop(permit);
    let mut response =
        response.unwrap_or_else(|_| ApiError::unavailable("request timed out").into_response());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("same-origin"));
    response
}

async fn buffer_authentication_body(
    request: Request<Body>,
) -> Result<Request<Body>, Box<Response>> {
    let (parts, body) = request.into_parts();
    let mut body_request = Request::new(body);
    DefaultBodyLimit::max(auth::MAX_AUTH_REQUEST_BYTES).apply(&mut body_request);
    let bytes = timeout(
        REQUEST_QUEUE_TIMEOUT,
        Bytes::from_request(body_request, &()),
    )
    .await
    .map_err(|_| {
        Box::new(
            ApiError {
                status: StatusCode::REQUEST_TIMEOUT,
                message: "authentication request body timed out".to_owned(),
            }
            .into_response(),
        )
    })?
    .map_err(|rejection| Box::new(rejection.into_response()))?;
    Ok(Request::from_parts(parts, Body::from(bytes)))
}

async fn health_response() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

async fn enroll_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EnrollmentRequest>,
) -> Result<Response, ApiError> {
    state.allow_enrollment_attempt().await?;
    ensure_protocol(request.protocol_version)?;
    validate_enrollment(&request)?;
    let enrollment_token_hash = hash_token(bearer_token(&headers)?);
    let max_nodes = state.max_nodes;
    let now = current_time()?;
    let enrolled: EnrollmentResponse = state
        .database(move |storage| storage.enroll(&request, &enrollment_token_hash, max_nodes, now))
        .await?;
    let mut response = Json(enrolled).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn ingest_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(snapshot): Json<SystemSnapshot>,
) -> Result<Json<SnapshotResponse>, ApiError> {
    ensure_protocol(snapshot.protocol_version)?;
    validate_snapshot(&snapshot)?;
    let token_hash = hash_token(bearer_token(&headers)?);
    let received_at = current_time()?;
    let clock_skew_ms = signed_difference(received_at, snapshot.collected_at_unix_ms);
    let retention_days = state.retention_days;
    state
        .database(move |storage| {
            storage.ingest(&token_hash, &snapshot, received_at, retention_days)
        })
        .await?;
    Ok(Json(SnapshotResponse {
        accepted: true,
        server_time_unix_ms: received_at,
        clock_skew_ms,
    }))
}

#[derive(Debug, Deserialize)]
struct NativeNodesQuery {
    offset: Option<u32>,
    limit: Option<u32>,
}

async fn native_nodes(
    State(state): State<AppState>,
    Query(query): Query<NativeNodesQuery>,
) -> Result<Json<Value>, ApiError> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offline_after = state.offline_after_seconds;
    let (total, nodes) = state
        .database(move |storage| storage.native_nodes(offset, limit, offline_after))
        .await?;
    Ok(Json(
        json!({ "total": total, "offset": offset, "limit": limit, "nodes": nodes }),
    ))
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    hours: Option<u32>,
    limit: Option<u32>,
}

async fn native_history(
    State(state): State<AppState>,
    Path(node_id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Value>, ApiError> {
    let hours = history_hours(query.hours, state.retention_days);
    let limit = query.limit.unwrap_or(300).clamp(1, MAX_HISTORY_POINTS);
    let now = current_time()?;
    let id = node_id.clone();
    let series = state
        .database(move |storage| storage.history(&id, hours, limit, now))
        .await?;
    Ok(Json(json!({
        "node_id": node_id,
        "records": series.records,
        "coverage": series.coverage
    })))
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Value,
}

async fn rpc_handler(
    State(state): State<AppState>,
    Json(request): Json<RpcRequest>,
) -> Json<Value> {
    if request.jsonrpc != "2.0" {
        return rpc_error(&request.id, -32600, "Invalid Request");
    }
    let result = rpc_result(&state, &request).await;
    match result {
        Ok(result) => Json(json!({ "jsonrpc": "2.0", "result": result, "id": request.id })),
        Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            ..
        }) => rpc_error(&request.id, -32602, "Invalid params"),
        Err(ApiError {
            status: StatusCode::NOT_FOUND,
            ..
        }) => rpc_error(&request.id, -32601, "Method not found"),
        Err(error) => {
            tracing::warn!(message = %error.message, "RPC request failed");
            rpc_error(&request.id, -32603, "Internal error")
        }
    }
}

async fn rpc_result(state: &AppState, request: &RpcRequest) -> Result<Value, ApiError> {
    match request.method.as_str() {
        "rpc.ping" => Ok(json!("pong")),
        "rpc.getVersion" | "common:getBackendVersion" => Ok(version_data()),
        "common:getPublicInfo" => public_settings_data(state).await,
        "common:getDashboard" => {
            let offline_after = state.offline_after_seconds;
            let max_nodes = state.max_nodes;
            state
                .database(move |storage| storage.emerald_dashboard(offline_after, max_nodes))
                .await
        }
        "common:getNodeRecentStatus" | "common:getRecords" => {
            let node_id = rpc_node_id(&request.params)?.to_owned();
            let hours = history_hours(
                request
                    .params
                    .get("hours")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
                state.retention_days,
            );
            let limit = request
                .params
                .get("max_count")
                .or_else(|| request.params.get("limit"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(150)
                .clamp(1, MAX_HISTORY_POINTS);
            let now = current_time()?;
            if request.params.get("type").and_then(Value::as_str) == Some("ping") {
                let mut result = state
                    .database(move |storage| storage.probe_history(&node_id, hours, now))
                    .await?;
                let count = result["records"].as_array().map_or(0, Vec::len);
                result["count"] = json!(count);
                return Ok(result);
            }
            let series = state
                .database(move |storage| storage.history(&node_id, hours, limit, now))
                .await?;
            Ok(history_result(&series))
        }
        _ => Err(ApiError {
            status: StatusCode::NOT_FOUND,
            message: "method not found".to_owned(),
        }),
    }
}

fn history_result(series: &HistorySeries) -> Value {
    json!({
        "count": series.records.len(),
        "records": &series.records,
        "coverage": &series.coverage
    })
}

fn rpc_node_id(params: &Value) -> Result<&str, ApiError> {
    params
        .get("uuid")
        .and_then(Value::as_str)
        .filter(|value| Uuid::parse_str(value).is_ok())
        .ok_or_else(|| ApiError::bad_request("uuid is required"))
}

fn rpc_error(id: &Value, code: i32, message: &str) -> Json<Value> {
    Json(json!({
        "jsonrpc": "2.0",
        "error": { "code": code, "message": message },
        "id": id
    }))
}

async fn public_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "status": "success",
        "message": "",
        "data": public_settings_data(&state).await?
    })))
}

async fn public_settings_data(state: &AppState) -> Result<Value, ApiError> {
    let settings = state.database(Storage::settings).await?;
    let oauth_enabled = state.auth.oauth_enabled();
    Ok(json!({
        "allow_cors": false,
        "custom_body": "",
        "custom_head": "",
        "description": "Pulse 节点运行状态",
        "disable_password_login": false,
        "oauth_enable": oauth_enabled,
        "oauth_provider": oauth_enabled.then_some("github"),
        "ping_record_preserve_time": state.retention_days * 24,
        "private_site": settings.private_site,
        "record_enabled": true,
        "record_preserve_time": state.retention_days * 24,
        "sitename": settings.site_name,
        "theme": "emerald",
        "theme_settings": {
            "dataUpdateInterval": settings.agent_interval_seconds,
            "rpcTransportMode": "http",
            "defaultViewMode": "card",
            "earthViewMode": "earth",
            "visitorInfoCardEnabled": false,
            "hideAdminEntryWhenLoggedOut": false,
            "offlineNodesLast": true,
            "backgroundEnabled": false,
            "alertEnabled": false
        }
    }))
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match state.auth.session(&headers).await {
        Ok(session)=>Json(json!({ "logged_in":session.is_some(), "username":session.map(|s|s.username).unwrap_or_default() })).into_response(),
        Err(error)=>error.into_response(),
    }
}

async fn version() -> Json<Value> {
    Json(json!({ "status": "success", "message": "", "data": version_data() }))
}

fn version_data() -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "hash": option_env!("PULSE_BUILD_GIT_HASH").unwrap_or("dev")
    })
}

#[derive(Debug, Deserialize)]
struct LoadQuery {
    uuid: String,
    hours: Option<u32>,
}

async fn load_records(
    State(state): State<AppState>,
    Query(query): Query<LoadQuery>,
) -> Result<Json<Value>, ApiError> {
    if Uuid::parse_str(&query.uuid).is_err() {
        return Err(ApiError::bad_request("uuid is invalid"));
    }
    let hours = history_hours(query.hours, state.retention_days);
    let node_id = query.uuid;
    let now = current_time()?;
    let series = state
        .database(move |storage| storage.history(&node_id, hours, MAX_HISTORY_POINTS, now))
        .await?;
    Ok(Json(json!({
        "status": "success",
        "message": "",
        "data": history_result(&series)
    })))
}

#[derive(Debug, Serialize)]
struct ApiError {
    #[serde(skip)]
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": { "message": self.message } })),
        )
            .into_response()
    }
}

fn map_storage_error(error: StorageError) -> ApiError {
    match error {
        StorageError::Unauthorized | StorageError::InvalidEnrollment => {
            ApiError::unauthorized("invalid or expired credential")
        }
        StorageError::NodeLimit => ApiError {
            status: StatusCode::CONFLICT,
            message: "node limit reached".to_owned(),
        },
        StorageError::RateLimited => ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "snapshots are being submitted too frequently".to_owned(),
        },
        StorageError::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            message: "record not found".to_owned(),
        },
        StorageError::InvalidInput(message) => ApiError::bad_request(message),
        StorageError::Database(error) => {
            tracing::error!(%error, "database operation failed");
            ApiError::unavailable("database operation failed")
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("bearer token required"))?;
    value
        .strip_prefix("Bearer ")
        .filter(|token| {
            (16..=512).contains(&token.len()) && !token.chars().any(char::is_whitespace)
        })
        .ok_or_else(|| ApiError::unauthorized("invalid bearer token"))
}

fn ensure_protocol(version: u16) -> Result<(), ApiError> {
    if version != PROTOCOL_VERSION {
        return Err(ApiError::bad_request(format!(
            "unsupported protocol version {version}; expected {PROTOCOL_VERSION}"
        )));
    }
    Ok(())
}

fn validate_enrollment(request: &EnrollmentRequest) -> Result<(), ApiError> {
    validate_text("node_name", &request.node_name, 1, 128)?;
    validate_text("agent_version", &request.agent_version, 1, 64)?;
    validate_text("region", &request.region, 0, 16)?;
    validate_text("group", &request.group, 0, 128)
}

fn validate_snapshot(snapshot: &SystemSnapshot) -> Result<(), ApiError> {
    if Uuid::parse_str(&snapshot.sample_id).is_err() {
        return Err(ApiError::bad_request("sample_id must be a UUID"));
    }
    validate_text("agent_version", &snapshot.agent_version, 1, 64)?;
    validate_text("operating_system", &snapshot.operating_system, 1, 128)?;
    validate_text("kernel_version", &snapshot.kernel_version, 0, 128)?;
    validate_text("architecture", &snapshot.architecture, 1, 64)?;
    validate_text("cpu_name", &snapshot.cpu_name, 0, 256)?;
    validate_text("virtualization", &snapshot.virtualization, 0, 64)?;
    validate_text("region", &snapshot.region, 0, 16)?;
    validate_text("group", &snapshot.group, 0, 128)?;
    if let Some(host_name) = &snapshot.host_name {
        validate_text("host_name", host_name, 1, 128)?;
    }
    if snapshot.cpu_cores == 0 || snapshot.cpu_cores > 65_536 {
        return Err(ApiError::bad_request("cpu_cores is out of range"));
    }
    if snapshot
        .process_count
        .is_some_and(|count| count > 10_000_000)
    {
        return Err(ApiError::bad_request("process_count is out of range"));
    }
    if [snapshot.tcp_connection_count, snapshot.udp_connection_count]
        .into_iter()
        .flatten()
        .any(|count| count > 10_000_000)
    {
        return Err(ApiError::bad_request("connection count is out of range"));
    }
    if let Some(gpus) = &snapshot.gpus {
        if gpus.len() > 16 {
            return Err(ApiError::bad_request("too many GPUs"));
        }
        for gpu in gpus {
            validate_text("GPU name", &gpu.name, 1, 256)?;
            if gpu
                .usage_percent
                .is_some_and(|v| !v.is_finite() || !(0.0..=100.0).contains(&v))
                || gpu
                    .temperature_celsius
                    .is_some_and(|v| !v.is_finite() || !(-100.0..=250.0).contains(&v))
                || gpu
                    .memory_used_bytes
                    .zip(gpu.memory_total_bytes)
                    .is_some_and(|(used, total)| used > total)
            {
                return Err(ApiError::bad_request("invalid GPU metrics"));
            }
        }
    }
    if !snapshot.cpu_usage_percent.is_finite()
        || !(0.0..=100.0).contains(&snapshot.cpu_usage_percent)
        || !snapshot.load_one.is_finite()
        || !snapshot.load_five.is_finite()
        || !snapshot.load_fifteen.is_finite()
        || snapshot.load_one < 0.0
        || snapshot.load_five < 0.0
        || snapshot.load_fifteen < 0.0
    {
        return Err(ApiError::bad_request(
            "snapshot contains invalid load values",
        ));
    }
    if snapshot.memory_used_bytes > snapshot.memory_total_bytes
        || snapshot.swap_used_bytes > snapshot.swap_total_bytes
        || snapshot.disk_used_bytes > snapshot.disk_total_bytes
    {
        return Err(ApiError::bad_request(
            "used capacity exceeds total capacity",
        ));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, min: usize, max: usize) -> Result<(), ApiError> {
    let length = value.chars().count();
    if length < min || length > max || value.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!("{name} is invalid")));
    }
    Ok(())
}

fn history_hours(requested: Option<u32>, retention_days: u32) -> u32 {
    requested
        .unwrap_or(4)
        .clamp(1, retention_days.saturating_mul(24).max(1))
}

fn current_time() -> Result<u64, ApiError> {
    unix_time_ms().map_err(map_storage_error)
}

fn validate_config(config: &ServiceConfig) -> Result<(), Box<dyn Error + Send + Sync>> {
    if !(1..=3_650).contains(&config.retention_days) {
        return Err("PULSE_RETENTION_DAYS must be between 1 and 3650".into());
    }
    if !(10..=86_400).contains(&config.offline_after_seconds) {
        return Err("PULSE_OFFLINE_AFTER_SECONDS must be between 10 and 86400".into());
    }
    if !(1..=1_000).contains(&config.max_nodes) {
        return Err("PULSE_MAX_NODES must be between 1 and 1000".into());
    }
    if !(64 * 1024 * 1024..=1024_u64.pow(4)).contains(&config.max_database_bytes) {
        return Err("PULSE_MAX_DATABASE_BYTES must be between 64 MiB and 1 TiB".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use pulse_protocol::{EnrollmentResponse, SystemSnapshot};
    use serde_json::{Value, json};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::*;

    fn test_state() -> (TempDir, AppState) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut config = ServiceConfig::with_database(directory.path().join("pulse.db"));
        config.max_database_bytes = 64 * 1024 * 1024;
        let setup_path = directory.path().join("setup-token");
        std::fs::write(&setup_path, "test-bootstrap-token-with-32-characters").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&setup_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        config.auth.setup_token_file = Some(setup_path);
        let state = AppState::open(&config).expect("test state");
        (directory, state)
    }

    async fn initialize_admin(state: &AppState) -> (String, String) {
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/auth/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let status = response_json(response).await;
        let response=router(state.clone()).oneshot(Request::builder().method("POST").uri("/api/auth/setup")
            .header(header::CONTENT_TYPE,"application/json").header(header::ORIGIN,"http://127.0.0.1:8080")
            .header(header::COOKIE,cookie).header("X-CSRF-Token",status["csrf_token"].as_str().unwrap())
            .body(Body::from(json!({"token":"test-bootstrap-token-with-32-characters","username":"admin","password":"example-test-password-please-change"}).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body = response_json(response).await;
        (cookie, body["csrf_token"].as_str().unwrap().to_owned())
    }

    fn json_request(path: &str, body: &Value, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).expect("request")
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("JSON response")
    }

    fn snapshot() -> SystemSnapshot {
        SystemSnapshot {
            protocol_version: PROTOCOL_VERSION,
            sample_id: Uuid::new_v4().to_string(),
            collected_at_unix_ms: unix_time_ms().expect("time"),
            host_name: Some("route-test-node".to_owned()),
            agent_version: "0.1.0-test".to_owned(),
            operating_system: "Test Linux".to_owned(),
            kernel_version: "test".to_owned(),
            architecture: "x86_64".to_owned(),
            cpu_name: "Test CPU".to_owned(),
            cpu_cores: 2,
            virtualization: "kvm".to_owned(),
            region: "SG".to_owned(),
            group: "tests".to_owned(),
            uptime_seconds: 100,
            cpu_usage_percent: 12.5,
            load_one: 0.1,
            load_five: 0.2,
            load_fifteen: 0.3,
            memory_total_bytes: 1_024,
            memory_used_bytes: 512,
            swap_total_bytes: 1_024,
            swap_used_bytes: 128,
            disk_total_bytes: 4_096,
            disk_used_bytes: 1_024,
            network_receive_bytes_per_second: 20,
            network_transmit_bytes_per_second: 10,
            network_total_received_bytes: 200,
            network_total_transmitted_bytes: 100,
            process_count: None,
            tcp_connection_count: None,
            udp_connection_count: None,
            gpus: None,
        }
    }

    #[tokio::test]
    async fn enrollment_ingestion_and_read_routes_work_together() {
        let (_directory, state) = test_state();
        let (cookie, _) = initialize_admin(&state).await;
        let secret = state
            .database(|storage| storage.create_enrollment(600, unix_time_ms()?))
            .await
            .expect("enrollment secret");
        let enrollment_request = json_request(
            "/api/v1/agents/enroll",
            &json!({
                "protocol_version": PROTOCOL_VERSION,
                "node_name": "route-test-node",
                "agent_version": "0.1.0-test",
                "region": "SG",
                "group": "tests"
            }),
            Some(&secret.token),
        );
        let enrollment_response = router(state.clone())
            .oneshot(enrollment_request)
            .await
            .expect("enrollment response");
        assert_eq!(enrollment_response.status(), StatusCode::OK);
        assert_eq!(
            enrollment_response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("no-store"))
        );
        let enrolled: EnrollmentResponse =
            serde_json::from_value(response_json(enrollment_response).await)
                .expect("enrollment payload");

        let snapshot_request = json_request(
            "/api/v1/agents/snapshots",
            &serde_json::to_value(snapshot()).expect("snapshot JSON"),
            Some(&enrolled.agent_token),
        );
        let snapshot_response = router(state.clone())
            .oneshot(snapshot_request)
            .await
            .expect("snapshot response");
        assert_eq!(snapshot_response.status(), StatusCode::OK);

        let nodes_response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/nodes?limit=1")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("nodes request"),
            )
            .await
            .expect("nodes response");
        assert_eq!(nodes_response.status(), StatusCode::OK);
        let nodes = response_json(nodes_response).await;
        assert_eq!(nodes["total"], 1);
        assert_eq!(nodes["nodes"][0]["client"]["id"], enrolled.node_id);

        let history_response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/nodes/{}/history?hours=1&limit=10",
                        enrolled.node_id
                    ))
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .expect("history request"),
            )
            .await
            .expect("history response");
        assert_eq!(history_response.status(), StatusCode::OK);
        let history = response_json(history_response).await;
        assert_eq!(history["records"].as_array().map(Vec::len), Some(1));
        assert_eq!(history["coverage"]["source_points"], 1);
        for method in ["common:getRecords", "common:getNodeRecentStatus"] {
            let response = router(state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/rpc2")
                        .header(header::COOKIE, &cookie)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            json!({"jsonrpc":"2.0","id":1,"method":method,
                                "params":{"uuid":enrolled.node_id,"type":"ping","hours":1}})
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let result = response_json(response).await;
            assert!(result.get("error").is_none());
            assert_eq!(result["result"]["count"], 0);
            assert_eq!(result["result"]["records"], json!([]));
            assert!(result["result"]["summary"].is_array());
        }
    }

    #[tokio::test]
    async fn public_settings_report_explicit_oauth_configuration() {
        let (directory, mut state) = test_state();
        let disabled = public_settings_data(&state).await.unwrap();
        assert_eq!(disabled["oauth_enable"], false);
        assert!(disabled["oauth_provider"].is_null());
        state.auth = auth::AuthState::new(
            &directory.path().join("pulse.db"),
            AuthConfig {
                github: Some(GithubOAuthConfig {
                    client_id: "test-client-id".to_owned(),
                    client_secret: "test-client-secret".to_owned(),
                    allowed_user_id: 123,
                }),
                ..AuthConfig::default()
            },
            64 * 1024 * 1024,
        )
        .unwrap();
        let enabled = public_settings_data(&state).await.unwrap();
        assert_eq!(enabled["oauth_enable"], true);
        assert_eq!(enabled["oauth_provider"], "github");
    }

    #[tokio::test]
    async fn all_monitoring_reads_fail_closed_and_admin_requires_csrf() {
        let (_directory, state) = test_state();
        for path in [
            "/api/v1/nodes",
            "/api/records/load?uuid=00000000-0000-0000-0000-000000000000",
            "/api/v1/nodes/00000000-0000-0000-0000-000000000000/probes",
            "/api/admin/state",
        ] {
            let response = router(state.clone())
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
        let (cookie, csrf) = initialize_admin(&state).await;
        let body =
            json!({"site_name":"Pulse Test","private_site":false,"agent_interval_seconds":1})
                .to_string();
        let missing = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/settings")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::FORBIDDEN);
        let valid = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/settings")
                    .header(header::COOKIE, &cookie)
                    .header(header::ORIGIN, "http://127.0.0.1:8080")
                    .header("X-CSRF-Token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid.status(), StatusCode::OK);
        let public = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/nodes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(public.status(), StatusCode::OK);
        let admin = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/admin/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(admin.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn oversized_authentication_body_is_rejected_before_acquiring_session_gate() {
        let (_directory, state) = test_state();
        let administration = state.session_gate.acquire(false).await.unwrap();
        let response = timeout(
            Duration::from_secs(1),
            router(state.clone()).oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("x".repeat(auth::MAX_AUTH_REQUEST_BYTES + 1)))
                    .unwrap(),
            ),
        )
        .await
        .expect("body rejection must not wait for an exclusive lease")
        .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        drop(administration);
        assert!(state.session_gate.write_available());
    }

    #[tokio::test]
    async fn cancelled_admin_http_worker_keeps_lease_until_completion_and_logout_revokes_access() {
        let (_directory, state) = test_state();
        let (cookie, csrf) = initialize_admin(&state).await;
        let gate = state.session_gate.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::sync_channel(1);
        let signals = Arc::new(std::sync::Mutex::new(Some((started_tx, finish_rx))));
        let handler_state = state.clone();
        let app = Router::new()
            .route(
                "/api/admin/test-blocking-worker",
                get(move || {
                    let handler_state = handler_state.clone();
                    let signals = Arc::clone(&signals);
                    async move {
                        let (started, finish) = signals.lock().unwrap().take().unwrap();
                        match handler_state
                            .database(move |_| {
                                let _ = started.send(());
                                finish.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                                    StorageError::InvalidInput("test worker was not released")
                                })?;
                                Ok(())
                            })
                            .await
                        {
                            Ok(()) => StatusCode::NO_CONTENT.into_response(),
                            Err(error) => error.into_response(),
                        }
                    }
                }),
            )
            .layer(middleware::from_fn_with_state(state.clone(), request_guard));
        let request = tokio::spawn(
            app.oneshot(
                Request::builder()
                    .uri("/api/admin/test-blocking-worker")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            ),
        );
        started_rx.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert!(
            !gate.write_available(),
            "HTTP cancellation must not release a running database worker's lease"
        );
        finish_tx.send(()).unwrap();
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/logout")
                    .header(header::COOKIE, &cookie)
                    .header(header::ORIGIN, "http://127.0.0.1:8080")
                    .header("X-CSRF-Token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
        for (method, path, body) in [
            ("GET", "/api/admin/state", ""),
            ("POST", "/api/admin/settings", "{}"),
        ] {
            let response = router(state.clone())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::COOKIE, &cookie)
                        .header(header::ORIGIN, "http://127.0.0.1:8080")
                        .header("X-CSRF-Token", &csrf)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn enrollment_authentication_rate_and_body_limits_fail_closed() {
        let (_directory, state) = test_state();
        let enrollment_body = json!({
            "protocol_version": PROTOCOL_VERSION,
            "node_name": "route-test-node",
            "agent_version": "0.1.0-test",
            "region": "SG",
            "group": "tests"
        });

        for _ in 0..ENROLLMENT_ATTEMPTS_PER_MINUTE {
            let response = router(state.clone())
                .oneshot(json_request(
                    "/api/v1/agents/enroll",
                    &enrollment_body,
                    Some("invalid-enrollment-token"),
                ))
                .await
                .expect("invalid enrollment response");
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let limited = router(state.clone())
            .oneshot(json_request(
                "/api/v1/agents/enroll",
                &enrollment_body,
                Some("invalid-enrollment-token"),
            ))
            .await
            .expect("rate-limit response");
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);

        let oversized = Request::builder()
            .method("POST")
            .uri("/api/v1/agents/snapshots")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer invalid-agent-token")
            .body(Body::from("x".repeat(MAX_REQUEST_BYTES + 1)))
            .expect("oversized request");
        let response = router(state)
            .oneshot(oversized)
            .await
            .expect("body-limit response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
