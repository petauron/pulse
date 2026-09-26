//! Read-only Komari data shapes used by `LuminaPlus`. No remote Agent control.
use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    ApiError, AppState, current_time, history_hours,
    storage::{Storage, format_timestamp},
};

/// A stable, JavaScript-safe numeric view of a Pulse probe UUID.
fn numeric_task_id(raw: &str) -> Option<u64> {
    let uuid = Uuid::parse_str(raw).ok()?;
    let bytes = uuid.as_bytes();
    let upper = u64::from_be_bytes(bytes[..8].try_into().ok()?);
    let lower = u64::from_be_bytes(bytes[8..].try_into().ok()?);
    Some(((upper ^ lower) & ((1_u64 << 53) - 1)).max(1))
}

fn compat_task(raw: &Value, admin: bool) -> Option<Value> {
    let id = numeric_task_id(raw.get("id")?.as_str()?)?;
    let mut value = json!({
        "id": id,
        "name": raw.get("name")?.as_str()?,
        "type": raw.get("type").or_else(|| raw.get("kind")).cloned().unwrap_or(json!("icmp")),
        "interval": raw.get("interval").or_else(|| raw.get("interval_seconds")).cloned().unwrap_or(json!(60)),
        "clients": raw.get("clients").or_else(|| raw.get("node_ids")).cloned().unwrap_or(json!([])),
        "default_on": raw.get("default_on").cloned().unwrap_or(json!(false)),
        "loss": 0,
        "weight": 0,
        "target": ""
    });
    if admin {
        value["target"] = raw.get("target").cloned().unwrap_or(json!(""));
    }
    Some(value)
}

fn compat_tasks(raw: &[Value], admin: bool) -> Result<Vec<Value>, ApiError> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for task in raw {
        let value =
            compat_task(task, admin).ok_or_else(|| ApiError::unavailable("invalid probe task"))?;
        let id = value["id"].as_u64().unwrap_or_default();
        if !seen.insert(id) {
            return Err(ApiError::unavailable("probe ID collision"));
        }
        output.push(value);
    }
    Ok(output)
}

pub(crate) async fn public_tasks_data(state: &AppState) -> Result<Vec<Value>, ApiError> {
    let tasks = state.database(Storage::public_ping_tasks).await?;
    compat_tasks(&tasks, false)
}

pub(crate) async fn ping_tasks(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let tasks = public_tasks_data(&state).await?;
    Ok(Json(json!({"status":"success","message":"","data":tasks})))
}

pub(crate) async fn admin_ping_tasks(
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let admin = state.database(Storage::admin_state).await?;
    let raw = admin["probes"]
        .as_array()
        .ok_or_else(|| ApiError::unavailable("probe data unavailable"))?;
    let tasks = compat_tasks(raw, true)?;
    Ok(Json(json!({"status":"success","message":"","data":tasks})))
}

pub(crate) async fn admin_clients(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let admin = state.database(Storage::admin_state).await?;
    let raw = admin["nodes"]
        .as_array()
        .ok_or_else(|| ApiError::unavailable("node data unavailable"))?;
    let nodes: Vec<Value> = raw
        .iter()
        .filter_map(|node| {
            let uuid = node["id"].as_str()?;
            Some(json!({
                "uuid": uuid,
                "name": node["name"],
                "group": node["group"],
                "region": node["region"],
                "weight": node["weight"]
            }))
        })
        .collect();
    Ok(Json(json!({"status":"success","message":"","data":nodes})))
}

pub(crate) async fn admin_plugins() -> Json<Value> {
    Json(json!({"status":"success","message":"","data":[]}))
}

#[derive(Deserialize)]
pub(crate) struct PingQuery {
    uuid: Option<String>,
    task_id: Option<u64>,
    hours: Option<u32>,
}

pub(crate) async fn ping_records(
    State(state): State<AppState>,
    Query(query): Query<PingQuery>,
) -> Result<Json<Value>, ApiError> {
    let result =
        ping_records_data(&state, query.uuid.as_deref(), query.hours, query.task_id).await?;
    Ok(Json(json!({"status":"success","message":"","data":result})))
}

pub(crate) async fn ping_records_data(
    state: &AppState,
    uuid: Option<&str>,
    hours: Option<u32>,
    task_id: Option<u64>,
) -> Result<Value, ApiError> {
    if let Some(uuid) = uuid
        && Uuid::parse_str(uuid).is_err()
    {
        return Err(ApiError::bad_request("uuid is invalid"));
    }
    if uuid.is_none() && task_id.is_none() {
        return Err(ApiError::bad_request("uuid or task_id is required"));
    }
    let tasks = public_tasks_data(state).await?;
    let requested_task =
        task_id.and_then(|id| tasks.iter().find(|task| task["id"].as_u64() == Some(id)));
    if task_id.is_some() && requested_task.is_none() {
        return Ok(json!({"count":0,"records":[],"tasks":tasks}));
    }
    let hours = history_hours(hours, state.retention_days);
    let now = current_time()?;
    let (raw, mut summary) = if let Some(uuid) = uuid {
        let uuid = uuid.to_owned();
        let result = state
            .database(move |storage| storage.probe_history(&uuid, hours, now))
            .await?;
        (
            result["records"].as_array().cloned().unwrap_or_default(),
            result["summary"].clone(),
        )
    } else {
        let raw_tasks = state.database(Storage::public_ping_tasks).await?;
        let task_uuid = task_id.and_then(|id| {
            raw_tasks.iter().find_map(|task| {
                let uuid = task["id"].as_str()?;
                (numeric_task_id(uuid) == Some(id)).then(|| uuid.to_owned())
            })
        });
        let records = state
            .database(move |storage| storage.probe_overview(hours, now, task_uuid.as_deref()))
            .await?;
        (records, json!([]))
    };
    if let Some(items) = summary.as_array_mut() {
        for item in items {
            if let Some(id) = item["task_id"].as_str().and_then(numeric_task_id) {
                item["task_id"] = json!(id);
            }
        }
    }
    let public_ids: HashSet<u64> = tasks
        .iter()
        .filter_map(|task| task["id"].as_u64())
        .collect();
    let mut records = Vec::new();
    for record in raw {
        let Some(raw_id) = record["task_id"].as_str() else {
            continue;
        };
        let Some(id) = numeric_task_id(raw_id) else {
            continue;
        };
        if !public_ids.contains(&id) {
            continue;
        }
        if task_id.is_some_and(|requested| requested != id) {
            continue;
        }
        let Some(time) = record["received_at_unix_ms"].as_u64() else {
            continue;
        };
        let client = uuid
            .or_else(|| record["node_id"].as_str())
            .unwrap_or_default();
        let success = record["success"].as_bool().unwrap_or(false);
        let value = if success {
            record["latency_ms"].as_f64().unwrap_or(-1.0)
        } else {
            -1.0
        };
        records.push(json!({
            "task_id": id,
            "client": client,
            "time": format_timestamp(time),
            "value": value,
            "loss": if success { 0 } else { 100 },
            "count": 1
        }));
    }
    Ok(json!({"count":records.len(),"records":records,"tasks":tasks,"summary":summary}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_ids_are_stable_and_javascript_safe() {
        let id = numeric_task_id("5df0b95a-1d1b-4911-9b9e-a1d274adab41").unwrap();
        assert!(id > 0 && id < (1_u64 << 53));
        assert_eq!(
            id,
            numeric_task_id("5df0b95a-1d1b-4911-9b9e-a1d274adab41").unwrap()
        );
        assert!(numeric_task_id("not-a-uuid").is_none());
    }

    #[test]
    fn task_mapping_matches_luminaplus_schema() {
        let source = json!({"id":"5df0b95a-1d1b-4911-9b9e-a1d274adab41","name":"Test","type":"icmp","interval":60});
        let mapped = compat_tasks(&[source], false).unwrap();
        assert!(mapped[0]["id"].is_number());
        assert_eq!(mapped[0]["name"], "Test");
        assert_eq!(mapped[0]["clients"], json!([]));
    }
}
