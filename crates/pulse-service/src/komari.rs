//! Bounded read-only compatibility for Komari monitoring themes.
use axum::{Json, extract::State};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{ApiError, AppState, MAX_HISTORY_POINTS, current_time};

pub(crate) async fn nodes(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let offline_after = state.offline_after_seconds;
    let max_nodes = state.max_nodes;
    let dashboard = state
        .database(move |storage| storage.emerald_dashboard(offline_after, max_nodes))
        .await?;
    let clients = dashboard["clients"]
        .as_object()
        .ok_or_else(|| ApiError::unavailable("node data unavailable"))?;
    let mut values = json!(clients.values().cloned().collect::<Vec<Value>>());
    crate::ip_info::enrich_nodes(&state, &headers, &mut values).await?;
    Ok(Json(json!({"status":"success","message":"","data":values})))
}

pub(crate) async fn recent(
    State(state): State<AppState>,
    axum::extract::Path(uuid): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    if Uuid::parse_str(&uuid).is_err() {
        return Err(ApiError::bad_request("uuid is invalid"));
    }
    let now = current_time()?;
    let series = state
        .database(move |storage| storage.history(&uuid, 1, MAX_HISTORY_POINTS, now))
        .await?;
    // Komari's recent endpoint uses nested live snapshots. Pulse's flat metric
    // records are returned here only as a documented best-effort fallback.
    Ok(Json(
        json!({"status":"success","message":"","data":series.records}),
    ))
}
