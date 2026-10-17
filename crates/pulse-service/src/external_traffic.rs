//! Administrator-configured, read-only HTTP traffic sources.
//! Provider credentials and complete provider responses never leave this module.

use std::{
    error::Error,
    fs,
    net::{IpAddr, SocketAddr},
    os::unix::fs::PermissionsExt,
    path::Path,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path as RoutePath, State},
    routing::{get, post},
};
use reqwest::{Client, Url, redirect::Policy};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::lookup_host;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    control::configuration_audit,
    ip_info::public_ip,
    storage::{Storage, StorageError},
};

const MAX_SOURCES: i64 = 32;
const POLL_INTERVAL_MS: i64 = 30 * 60 * 1_000;
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
const MAX_SECRET_BYTES: usize = 4_096;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthMode {
    None,
    Bearer,
    LoginRefresh,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceConfig {
    pub name: String,
    pub enabled: bool,
    pub auth_mode: AuthMode,
    pub traffic_url: String,
    pub login_url: String,
    pub refresh_url: String,
    pub username_field: String,
    pub password_field: String,
    pub refresh_token_field: String,
    pub access_pointer: String,
    pub refresh_pointer: String,
    pub up_pointer: String,
    pub down_pointer: String,
    pub used_pointer: String,
    pub limit_pointer: String,
    pub limit_unit: String,
    pub reset_pointer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceInput {
    pub config: SourceConfig,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub bearer_token: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    username: String,
    password: String,
    bearer_token: String,
    access_token: String,
    refresh_token: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TrafficSnapshot {
    pub up_bytes: u64,
    pub down_bytes: u64,
    pub used_bytes: u64,
    pub limit_bytes: Option<u64>,
    pub reset_at: Option<String>,
    pub fetched_at_unix_ms: u64,
}

#[derive(Serialize)]
struct AdminSource {
    node_id: String,
    config: SourceConfig,
    has_credential: bool,
    snapshot: Option<TrafficSnapshot>,
    last_attempt_unix_ms: u64,
    last_success_unix_ms: u64,
    status: String,
}

struct DueSource {
    node_id: String,
    config: SourceConfig,
    encrypted_credential: Vec<u8>,
    generation: String,
}

pub(crate) struct CredentialKey(aead::LessSafeKey);

impl CredentialKey {
    pub(crate) fn from_file(path: &Path) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
            return Err("traffic credential key file must be a private regular file (0600)".into());
        }
        let key = fs::read(path)?;
        if key.len() != 32 {
            return Err("traffic credential key file must contain exactly 32 raw bytes".into());
        }
        let key = aead::UnboundKey::new(&aead::AES_256_GCM, &key)
            .map_err(|_| "invalid traffic credential key")?;
        Ok(Self(aead::LessSafeKey::new(key)))
    }

    fn encrypt(&self, credential: &Credential) -> Result<Vec<u8>, StorageError> {
        let mut value = serde_json::to_vec(credential)
            .map_err(|_| StorageError::InvalidInput("invalid traffic credential"))?;
        if value.len() > MAX_SECRET_BYTES {
            return Err(StorageError::InvalidInput("traffic credential too large"));
        }
        let mut nonce = [0_u8; aead::NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| StorageError::InvalidInput("credential encryption failed"))?;
        self.0
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut value,
            )
            .map_err(|_| StorageError::InvalidInput("credential encryption failed"))?;
        let mut sealed = nonce.to_vec();
        sealed.extend_from_slice(&value);
        Ok(sealed)
    }

    fn decrypt(&self, sealed: &[u8]) -> Result<Credential, StorageError> {
        if sealed.len() < aead::NONCE_LEN + aead::AES_256_GCM.tag_len()
            || sealed.len() > MAX_SECRET_BYTES + aead::NONCE_LEN + aead::AES_256_GCM.tag_len()
        {
            return Err(StorageError::InvalidInput(
                "stored traffic credential is invalid",
            ));
        }
        let mut nonce = [0_u8; aead::NONCE_LEN];
        nonce.copy_from_slice(&sealed[..aead::NONCE_LEN]);
        let mut value = sealed[aead::NONCE_LEN..].to_vec();
        let plain = self
            .0
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::empty(),
                &mut value,
            )
            .map_err(|_| {
                StorageError::InvalidInput("stored traffic credential cannot be decrypted")
            })?;
        serde_json::from_slice(plain)
            .map_err(|_| StorageError::InvalidInput("stored traffic credential is invalid"))
    }
}

fn validate_config(config: &SourceConfig) -> Result<(), StorageError> {
    if config.name.trim().is_empty()
        || config.name.len() > 128
        || config.name.chars().any(char::is_control)
        || !["bytes", "gb", "gib"].contains(&config.limit_unit.as_str())
        || config.login_url.len() > 2_048
        || config.refresh_url.len() > 2_048
        || [
            &config.username_field,
            &config.password_field,
            &config.refresh_token_field,
        ]
        .iter()
        .any(|value| value.len() > 64)
        || [
            &config.access_pointer,
            &config.refresh_pointer,
            &config.up_pointer,
            &config.down_pointer,
            &config.used_pointer,
            &config.limit_pointer,
            &config.reset_pointer,
        ]
        .iter()
        .any(|value| value.len() > 256)
    {
        return Err(StorageError::InvalidInput(
            "invalid traffic source configuration",
        ));
    }
    let traffic = validate_url(&config.traffic_url)?;
    if config.auth_mode == AuthMode::LoginRefresh {
        let login = validate_url(&config.login_url)?;
        let refresh = validate_url(&config.refresh_url)?;
        if login.origin() != traffic.origin()
            || refresh.origin() != traffic.origin()
            || !valid_field_name(&config.username_field)
            || !valid_field_name(&config.password_field)
            || !valid_field_name(&config.refresh_token_field)
            || config.username_field == config.password_field
            || !valid_pointer(&config.access_pointer, true)
            || !valid_pointer(&config.refresh_pointer, true)
        {
            return Err(StorageError::InvalidInput(
                "invalid traffic login configuration",
            ));
        }
    }
    if !valid_pointer(&config.up_pointer, true)
        || !valid_pointer(&config.down_pointer, true)
        || !valid_pointer(&config.used_pointer, false)
        || !valid_pointer(&config.limit_pointer, false)
        || !valid_pointer(&config.reset_pointer, false)
    {
        return Err(StorageError::InvalidInput("invalid traffic JSON pointer"));
    }
    Ok(())
}

fn valid_field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_pointer(value: &str, required: bool) -> bool {
    (!required && value.is_empty())
        || (value.starts_with('/') && value.len() <= 256 && !value.chars().any(char::is_control))
}

fn validate_url(raw: &str) -> Result<Url, StorageError> {
    if raw.len() > 2_048 {
        return Err(StorageError::InvalidInput("traffic URL too long"));
    }
    let url = Url::parse(raw).map_err(|_| StorageError::InvalidInput("invalid traffic URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(StorageError::InvalidInput(
            "traffic URL must use HTTPS without user information",
        ));
    }
    Ok(url)
}

impl Storage {
    pub(crate) fn has_traffic_sources(&self) -> Result<bool, StorageError> {
        let db = self.connection()?;
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM external_traffic_sources)",
            [],
            |row| row.get(0),
        )?)
    }

    fn list_traffic_sources(&self) -> Result<Vec<AdminSource>, StorageError> {
        let db = self.connection()?;
        let mut statement = db.prepare("SELECT node_id,config,credential,snapshot,last_attempt_ms,last_success_ms,status FROM external_traffic_sources ORDER BY node_id LIMIT ?1")?;
        let rows = statement.query_map([MAX_SOURCES], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (node_id, config, credential, snapshot, attempt, success, status) = row?;
            let config: SourceConfig = serde_json::from_str(&config).map_err(|_| {
                StorageError::InvalidInput("stored traffic configuration is invalid")
            })?;
            Ok(AdminSource {
                node_id,
                has_credential: config.auth_mode != AuthMode::None && !credential.is_empty(),
                config,
                snapshot: snapshot
                    .map(|value| {
                        serde_json::from_str(&value).map_err(|_| {
                            StorageError::InvalidInput("stored traffic snapshot is invalid")
                        })
                    })
                    .transpose()?,
                last_attempt_unix_ms: u64::try_from(attempt).unwrap_or(0),
                last_success_unix_ms: u64::try_from(success).unwrap_or(0),
                status,
            })
        })
        .collect()
    }

    fn save_traffic_source(
        &self,
        node_id: &str,
        input: SourceInput,
        key: &CredentialKey,
    ) -> Result<(), StorageError> {
        validate_config(&input.config)?;
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=?1 AND disabled_at_ms IS NULL)",
            [node_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::NotFound);
        }
        let old: Option<(String, Vec<u8>)> = tx
            .query_row(
                "SELECT config,credential FROM external_traffic_sources WHERE node_id=?1",
                [node_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if old.is_none() {
            let count: i64 =
                tx.query_row("SELECT COUNT(*) FROM external_traffic_sources", [], |row| {
                    row.get(0)
                })?;
            if count >= MAX_SOURCES {
                return Err(StorageError::InvalidInput("traffic source limit reached"));
            }
        }
        let old_config: Option<SourceConfig> = old
            .as_ref()
            .map(|(value, _)| {
                serde_json::from_str(value).map_err(|_| {
                    StorageError::InvalidInput("stored traffic configuration is invalid")
                })
            })
            .transpose()?;
        let changed_secret = !input.username.is_empty()
            || !input.password.is_empty()
            || !input.bearer_token.is_empty();
        let mut credential = old
            .as_ref()
            .map(|(_, value)| key.decrypt(value))
            .transpose()?
            .unwrap_or_default();
        if !input.username.is_empty() || !input.password.is_empty() {
            if input.username.is_empty() || input.password.is_empty() {
                return Err(StorageError::InvalidInput(
                    "username and password must be updated together",
                ));
            }
            credential.username = input.username;
            credential.password = input.password;
            credential.access_token.clear();
            credential.refresh_token.clear();
        }
        if !input.bearer_token.is_empty() {
            credential.bearer_token = input.bearer_token;
        }
        match input.config.auth_mode {
            AuthMode::None => credential = Credential::default(),
            AuthMode::Bearer => {
                if credential.bearer_token.is_empty() {
                    return Err(StorageError::InvalidInput("bearer token is required"));
                }
                credential.username.clear();
                credential.password.clear();
                credential.access_token.clear();
                credential.refresh_token.clear();
            }
            AuthMode::LoginRefresh => {
                if credential.username.is_empty() || credential.password.is_empty() {
                    return Err(StorageError::InvalidInput("login credentials are required"));
                }
                credential.bearer_token.clear();
            }
        }
        let changed = changed_secret || old_config.as_ref().is_none_or(|old| old != &input.config);
        if changed && input.config.auth_mode == AuthMode::LoginRefresh {
            credential.access_token.clear();
            credential.refresh_token.clear();
        }
        let config = serde_json::to_string(&input.config)
            .map_err(|_| StorageError::InvalidInput("invalid traffic configuration"))?;
        let sealed = key.encrypt(&credential)?;
        let generation = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO external_traffic_sources(node_id,config,credential,last_attempt_ms,status,generation)
             VALUES(?1,?2,?3,0,'pending',?5) ON CONFLICT(node_id) DO UPDATE SET
             config=excluded.config,credential=excluded.credential,last_attempt_ms=0,
             snapshot=CASE WHEN ?4 THEN NULL ELSE external_traffic_sources.snapshot END,
             last_success_ms=CASE WHEN ?4 THEN 0 ELSE external_traffic_sources.last_success_ms END,
             generation=excluded.generation,
             status='pending'",
            params![node_id, config, sealed, changed, generation],
        )?;
        configuration_audit(&tx, "traffic_source.save", node_id)?;
        tx.commit()?;
        Ok(())
    }

    fn delete_traffic_source(&self, node_id: &str) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        if tx.execute(
            "DELETE FROM external_traffic_sources WHERE node_id=?1",
            [node_id],
        )? == 0
        {
            return Err(StorageError::NotFound);
        }
        configuration_audit(&tx, "traffic_source.delete", node_id)?;
        tx.commit()?;
        Ok(())
    }

    fn claim_due_traffic_sources(&self, now: u64) -> Result<Vec<DueSource>, StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let due = i64::try_from(now)
            .unwrap_or(i64::MAX)
            .saturating_sub(POLL_INTERVAL_MS);
        let mut statement = tx.prepare("SELECT s.node_id,s.config,s.credential,s.generation FROM external_traffic_sources s JOIN nodes n ON n.id=s.node_id
            WHERE n.disabled_at_ms IS NULL AND json_extract(s.config,'$.enabled')=1 AND s.last_attempt_ms<=?1
            ORDER BY s.last_attempt_ms,s.node_id LIMIT 8")?;
        let rows = statement.query_map([due], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut sources = Vec::new();
        for row in rows {
            let (node_id, config, encrypted_credential, generation) = row?;
            sources.push(DueSource {
                node_id,
                config: serde_json::from_str(&config).map_err(|_| {
                    StorageError::InvalidInput("stored traffic configuration is invalid")
                })?,
                encrypted_credential,
                generation,
            });
        }
        drop(statement);
        for source in &sources {
            tx.execute(
                "UPDATE external_traffic_sources SET last_attempt_ms=?2 WHERE node_id=?1",
                params![source.node_id, i64::try_from(now).unwrap_or(i64::MAX)],
            )?;
        }
        tx.commit()?;
        Ok(sources)
    }

    fn complete_traffic_source(
        &self,
        node_id: &str,
        generation: &str,
        credential: Option<&[u8]>,
        snapshot: Option<&TrafficSnapshot>,
        status: &str,
        now: u64,
    ) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let encoded = snapshot
            .map(|value| {
                serde_json::to_string(value)
                    .map_err(|_| StorageError::InvalidInput("invalid traffic snapshot"))
            })
            .transpose()?;
        tx.execute("UPDATE external_traffic_sources SET credential=COALESCE(?2,credential),snapshot=COALESCE(?3,snapshot),
            last_success_ms=CASE WHEN ?3 IS NULL THEN last_success_ms ELSE ?5 END,status=?4 WHERE node_id=?1 AND generation=?6",
            params![node_id, credential, encoded, status, i64::try_from(now).unwrap_or(i64::MAX), generation])?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/traffic-sources", get(admin_sources))
        .route("/api/admin/traffic-sources/{node_id}", post(save_source))
        .route(
            "/api/admin/traffic-sources/{node_id}/delete",
            post(delete_source),
        )
}

async fn admin_sources(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sources = state.database(Storage::list_traffic_sources).await?;
    Ok(Json(
        json!({"sources":sources,"credential_key_ready":state.traffic_key.is_some()}),
    ))
}

async fn save_source(
    State(state): State<AppState>,
    RoutePath(node_id): RoutePath<String>,
    Json(input): Json<SourceInput>,
) -> Result<Json<Value>, ApiError> {
    let key = state
        .traffic_key
        .clone()
        .ok_or_else(|| ApiError::bad_request("traffic credential key is not configured"))?;
    state
        .database(move |storage| storage.save_traffic_source(&node_id, input, &key))
        .await?;
    Ok(Json(json!({"ok":true})))
}

async fn delete_source(
    State(state): State<AppState>,
    RoutePath(node_id): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .database(move |storage| storage.delete_traffic_source(&node_id))
        .await?;
    Ok(Json(json!({"ok":true})))
}

impl AppState {
    /// Poll at most eight due sources serially; the Service never sends commands to Agents.
    ///
    /// # Errors
    ///
    /// Returns an error if the clock, storage, or credential encryption fails.
    pub async fn maintain_external_traffic(&self) -> Result<(), String> {
        let now = crate::current_time().map_err(|error| error.message)?;
        let sources = self
            .database(move |storage| storage.claim_due_traffic_sources(now))
            .await
            .map_err(|error| error.message)?;
        for source in sources {
            let (snapshot, credential, status) = if let Some(key) = self.traffic_key.as_ref() {
                match key.decrypt(&source.encrypted_credential) {
                    Ok(mut credential) => {
                        let result = poll_source(&source.config, &mut credential, now).await;
                        let sealed = key
                            .encrypt(&credential)
                            .map_err(|error| error.to_string())?;
                        match result {
                            Ok(snapshot) => (Some(snapshot), Some(sealed), "ok"),
                            Err(status) => (None, Some(sealed), status),
                        }
                    }
                    Err(_) => (None, None, "credential_unavailable"),
                }
            } else {
                (None, None, "credential_unavailable")
            };
            let node_id = source.node_id;
            let generation = source.generation;
            self.database(move |storage| {
                storage.complete_traffic_source(
                    &node_id,
                    &generation,
                    credential.as_deref(),
                    snapshot.as_ref(),
                    status,
                    now,
                )
            })
            .await
            .map_err(|error| error.message)?;
        }
        Ok(())
    }
}

async fn poll_source(
    config: &SourceConfig,
    credential: &mut Credential,
    now: u64,
) -> Result<TrafficSnapshot, &'static str> {
    let traffic_url = validate_url(&config.traffic_url).map_err(|_| "invalid_configuration")?;
    let mut token = match config.auth_mode {
        AuthMode::None => None,
        AuthMode::Bearer => Some(credential.bearer_token.clone()),
        AuthMode::LoginRefresh => {
            if credential.access_token.is_empty() {
                authenticate(config, credential).await?;
            }
            Some(credential.access_token.clone())
        }
    };
    let mut response =
        request_json(&traffic_url, reqwest::Method::GET, None, token.as_deref()).await;
    if matches!(response, Err("authentication_required"))
        && config.auth_mode == AuthMode::LoginRefresh
    {
        if !credential.refresh_token.is_empty() && refresh(config, credential).await.is_ok() {
            token = Some(credential.access_token.clone());
        } else {
            authenticate(config, credential).await?;
            token = Some(credential.access_token.clone());
        }
        response = request_json(&traffic_url, reqwest::Method::GET, None, token.as_deref()).await;
    }
    let value = response?;
    parse_snapshot(config, &value, now)
}

async fn authenticate(
    config: &SourceConfig,
    credential: &mut Credential,
) -> Result<(), &'static str> {
    let url = validate_url(&config.login_url).map_err(|_| "invalid_configuration")?;
    let mut fields = serde_json::Map::new();
    fields.insert(
        config.username_field.clone(),
        Value::String(credential.username.clone()),
    );
    fields.insert(
        config.password_field.clone(),
        Value::String(credential.password.clone()),
    );
    let body = Value::Object(fields);
    let value = request_json(&url, reqwest::Method::POST, Some(&body), None).await?;
    let access = json_string(&value, &config.access_pointer).ok_or("authentication_required")?;
    access.clone_into(&mut credential.access_token);
    json_string(&value, &config.refresh_pointer)
        .unwrap_or_default()
        .clone_into(&mut credential.refresh_token);
    Ok(())
}

async fn refresh(config: &SourceConfig, credential: &mut Credential) -> Result<(), &'static str> {
    let url = validate_url(&config.refresh_url).map_err(|_| "invalid_configuration")?;
    let mut fields = serde_json::Map::new();
    fields.insert(
        config.refresh_token_field.clone(),
        Value::String(credential.refresh_token.clone()),
    );
    let body = Value::Object(fields);
    let value = request_json(&url, reqwest::Method::POST, Some(&body), None).await?;
    let access = json_string(&value, &config.access_pointer).ok_or("authentication_required")?;
    let next = json_string(&value, &config.refresh_pointer)
        .unwrap_or(&credential.refresh_token)
        .to_owned();
    access.clone_into(&mut credential.access_token);
    credential.refresh_token = next;
    Ok(())
}

fn json_string<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_SECRET_BYTES)
}

fn required_bytes(value: &Value, pointer: &str) -> Result<u64, &'static str> {
    value
        .pointer(pointer)
        .and_then(|field| {
            field.as_u64().or_else(|| {
                field
                    .as_str()
                    .filter(|text| {
                        !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .and_then(|text| text.parse().ok())
            })
        })
        .ok_or("invalid_data")
}

fn parse_snapshot(
    config: &SourceConfig,
    value: &Value,
    now: u64,
) -> Result<TrafficSnapshot, &'static str> {
    let up_bytes = required_bytes(value, &config.up_pointer)?;
    let down_bytes = required_bytes(value, &config.down_pointer)?;
    let used_bytes = if config.used_pointer.is_empty() {
        up_bytes.checked_add(down_bytes).ok_or("invalid_data")?
    } else {
        required_bytes(value, &config.used_pointer)?
    };
    let limit_bytes = if config.limit_pointer.is_empty() {
        None
    } else {
        let raw = required_bytes(value, &config.limit_pointer)?;
        let multiplier = match config.limit_unit.as_str() {
            "bytes" => 1_u64,
            "gb" => 1_000_000_000,
            "gib" => 1_073_741_824,
            _ => return Err("invalid_configuration"),
        };
        Some(raw.checked_mul(multiplier).ok_or("invalid_data")?)
    };
    let reset_at = if config.reset_pointer.is_empty() {
        None
    } else {
        Some(
            json_string(value, &config.reset_pointer)
                .ok_or("invalid_data")?
                .to_owned(),
        )
    };
    Ok(TrafficSnapshot {
        up_bytes,
        down_bytes,
        used_bytes,
        limit_bytes,
        reset_at,
        fetched_at_unix_ms: now,
    })
}

async fn request_json(
    url: &Url,
    method: reqwest::Method,
    body: Option<&Value>,
    token: Option<&str>,
) -> Result<Value, &'static str> {
    let host = url.host_str().ok_or("invalid_configuration")?;
    let port = url.port_or_known_default().ok_or("invalid_configuration")?;
    let mut addresses = lookup_host((host, port))
        .await
        .map_err(|_| "provider_unavailable")?;
    let first = addresses.next().ok_or("provider_unavailable")?;
    if !public_ip(first.ip()) || addresses.any(|address| !public_ip(address.ip())) {
        return Err("private_address_rejected");
    }
    let mut builder = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(10))
        .no_proxy();
    if host.parse::<IpAddr>().is_err() {
        builder = builder.resolve(host, SocketAddr::new(first.ip(), port));
    }
    let client = builder.build().map_err(|_| "provider_unavailable")?;
    let mut request = client.request(method, url.clone());
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let mut response = request.send().await.map_err(|_| "provider_unavailable")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("authentication_required");
    }
    if !response.status().is_success() {
        return Err("provider_http_error");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("response_too_large");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "provider_unavailable")? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("response_too_large");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid_data")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SourceConfig {
        SourceConfig {
            name: "example".to_owned(),
            enabled: true,
            auth_mode: AuthMode::None,
            traffic_url: "https://example.com/usage".to_owned(),
            login_url: String::new(),
            refresh_url: String::new(),
            username_field: String::new(),
            password_field: String::new(),
            refresh_token_field: String::new(),
            access_pointer: String::new(),
            refresh_pointer: String::new(),
            up_pointer: "/data/up".to_owned(),
            down_pointer: "/data/down".to_owned(),
            used_pointer: "/data/billed".to_owned(),
            limit_pointer: "/data/quota".to_owned(),
            limit_unit: "gib".to_owned(),
            reset_pointer: "/data/reset".to_owned(),
        }
    }

    #[test]
    fn parses_billable_usage_separately_from_directional_counters() {
        let source = config();
        let response =
            json!({"data":{"up":100,"down":200,"billed":250,"quota":2,"reset":"2026-11-01"}});
        let snapshot = parse_snapshot(&source, &response, 123).expect("valid snapshot");
        assert_eq!(snapshot.up_bytes, 100);
        assert_eq!(snapshot.down_bytes, 200);
        assert_eq!(snapshot.used_bytes, 250);
        assert_eq!(snapshot.limit_bytes, Some(2 * 1_073_741_824));
        assert_eq!(snapshot.reset_at.as_deref(), Some("2026-11-01"));
        assert_eq!(snapshot.fetched_at_unix_ms, 123);
    }

    #[test]
    fn rejects_non_https_and_bad_pointers() {
        let mut source = config();
        source.traffic_url = "http://example.com/usage".to_owned();
        assert!(validate_config(&source).is_err());
        source.traffic_url = "https://example.com/usage".to_owned();
        source.up_pointer = "data/up".to_owned();
        assert!(validate_config(&source).is_err());
    }

    #[test]
    fn rejects_overflow_and_missing_counter() {
        let mut source = config();
        source.used_pointer.clear();
        assert!(
            parse_snapshot(
                &source,
                &json!({"data":{"up":u64::MAX,"down":1,"quota":2,"reset":"next"}}),
                1
            )
            .is_err()
        );
        assert!(
            parse_snapshot(
                &source,
                &json!({"data":{"up":1,"quota":2,"reset":"next"}}),
                1
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_decimal_integer_strings_but_not_fractional_units() {
        let source = config();
        let response =
            json!({"data":{"up":"100","down":"200","billed":"250","quota":"2","reset":"next"}});
        assert_eq!(
            parse_snapshot(&source, &response, 1)
                .expect("decimal integer")
                .used_bytes,
            250
        );
        let response =
            json!({"data":{"up":"1.5","down":200,"billed":250,"quota":2,"reset":"next"}});
        assert!(parse_snapshot(&source, &response, 1).is_err());
    }
}
