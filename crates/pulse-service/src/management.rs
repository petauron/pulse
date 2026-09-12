//! Administrator routes are protected centrally before any body is handled.
use crate::{
    ApiError, AppState, bearer_token,
    control::{AlertRule, Channel, NodeOptions, ProbeDefinition, Settings},
    current_time, history_hours,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use pulse_protocol::{AgentRuntimeConfig, ProbeBatch};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/state", get(admin_state))
        .route("/api/admin/settings", post(settings))
        .route("/api/admin/nodes/{id}", post(node))
        .route("/api/admin/nodes/{id}/{action}", post(node_action))
        .route("/api/admin/enrollment", post(enrollment))
        .route("/api/admin/probes", post(probe))
        .route("/api/admin/channels", post(channel))
        .route("/api/admin/alert-rules", post(rule))
        .route("/api/admin/{kind}/{id}/delete", post(delete_config))
}

async fn admin_state(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state.database(crate::storage::Storage::admin_state).await?,
    ))
}
async fn settings(
    State(state): State<AppState>,
    Json(value): Json<Settings>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.save_settings(&value))
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn node(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut value): Json<NodeOptions>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.save_node_options(&id, &mut value))
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn node_action(
    State(state): State<AppState>,
    Path((id, action)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let now = current_time()?;
    Ok(Json(
        state
            .database(move |storage| {
                match action.as_str() {
                    "rotate" => {
                        return Ok(json!({"agent_token":storage.rotate_node_token(&id,now)?}));
                    }
                    "revoke" => storage.revoke_node(&id, now)?,
                    "delete" => storage.delete_node(&id, now)?,
                    "reset-traffic" => storage.reset_traffic(&id)?,
                    _ => return Err(crate::storage::StorageError::NotFound),
                }
                Ok(json!({"ok":true}))
            })
            .await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentInput {
    ttl_seconds: u64,
}
async fn enrollment(
    State(state): State<AppState>,
    Json(value): Json<EnrollmentInput>,
) -> Result<Json<Value>, ApiError> {
    if !(60..=86400).contains(&value.ttl_seconds) {
        return Err(ApiError::bad_request("TTL must be 60..86400 seconds"));
    }
    let now = current_time()?;
    let secret = state
        .database(move |storage| storage.create_enrollment(value.ttl_seconds, now))
        .await?;
    Ok(Json(json!(secret)))
}
async fn probe(
    State(state): State<AppState>,
    Json(mut value): Json<ProbeDefinition>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.save_probe(&mut value))
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn channel(
    State(state): State<AppState>,
    Json(mut value): Json<Channel>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.save_channel(&mut value))
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn rule(
    State(state): State<AppState>,
    Json(mut value): Json<AlertRule>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.save_alert_rule(&mut value))
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn delete_config(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.delete_monitoring_config(&kind, &id))
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub(crate) async fn agent_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AgentRuntimeConfig>, ApiError> {
    let token = bearer_token(&headers)?.to_owned();
    Ok(Json(
        state
            .database(move |storage| storage.agent_runtime_config(&token))
            .await?,
    ))
}
pub(crate) async fn agent_probes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<ProbeBatch>,
) -> Result<Json<Value>, ApiError> {
    let token = bearer_token(&headers)?.to_owned();
    let now = current_time()?;
    let retention = state.retention_days;
    state
        .database(move |storage| storage.ingest_probes(&token, &batch, now, retention))
        .await?;
    Ok(Json(json!({"accepted":true})))
}
#[derive(Deserialize)]
pub(crate) struct ProbeQuery {
    hours: Option<u32>,
}
pub(crate) async fn probe_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProbeQuery>,
) -> Result<Json<Value>, ApiError> {
    let now = current_time()?;
    let hours = history_hours(query.hours, state.retention_days);
    Ok(Json(
        state
            .database(move |storage| storage.probe_history(&id, hours, now))
            .await?,
    ))
}
