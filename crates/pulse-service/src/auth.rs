//! Browser administrator authentication. Agent bearer credentials remain separate.

use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{sync::Semaphore, time::timeout};
use totp_rs::{Builder, Secret, Totp};
use uuid::Uuid;

const SESSION_TTL: i64 = 12 * 60 * 60;
const FLOW_TTL: i64 = 10 * 60;
const MAX_SESSIONS: i64 = 8;
const MAX_OAUTH_FLOWS: i64 = 32;
const MAX_ATTEMPTS: usize = 20;
const MAX_OAUTH_RESPONSE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_AUTH_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Clone)]
pub struct GithubOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    /// Immutable numeric GitHub account ID, never the mutable login name.
    pub allowed_user_id: u64,
}

impl fmt::Debug for GithubOAuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubOAuthConfig")
            .field("allowed_user_id", &self.allowed_user_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub public_url: String,
    pub setup_token_file: Option<PathBuf>,
    pub github: Option<GithubOAuthConfig>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            public_url: "http://127.0.0.1:8080".to_owned(),
            setup_token_file: None,
            github: None,
        }
    }
}

impl AuthConfig {
    /// Loads explicit browser-auth configuration without logging secret values.
    ///
    /// # Errors
    /// Returns an error for missing OAuth settings, invalid environment text, or an unreadable secret file.
    pub fn from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let public_url = match std::env::var("PULSE_PUBLIC_URL") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => Self::default().public_url,
            Err(_) => return Err("PULSE_PUBLIC_URL must contain valid Unicode".into()),
        };
        let setup_token_file = std::env::var_os("PULSE_SETUP_TOKEN_FILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let client_id =
            std::env::var_os("PULSE_GITHUB_CLIENT_ID").filter(|value| !value.is_empty());
        let secret_file =
            std::env::var_os("PULSE_GITHUB_CLIENT_SECRET_FILE").filter(|value| !value.is_empty());
        let allowed_user_id =
            std::env::var_os("PULSE_GITHUB_ALLOWED_USER_ID").filter(|value| !value.is_empty());
        let github = match (client_id, secret_file, allowed_user_id) {
            (None, None, None) => None,
            (Some(client_id), Some(secret_file), Some(allowed_user_id)) => {
                let client_id = client_id.into_string().map_err(|_| "GitHub client ID must contain valid Unicode")?;
                let allowed_user_id: u64 = allowed_user_id.into_string()
                    .map_err(|_| "GitHub allowed user ID must be a positive integer")?
                    .parse().map_err(|_| "GitHub allowed user ID must be a positive integer")?;
                let client_secret = read_secret_file(&PathBuf::from(secret_file), 1)?;
                Some(GithubOAuthConfig { client_id, client_secret, allowed_user_id })
            }
            _ => return Err("GitHub OAuth requires PULSE_GITHUB_CLIENT_ID, PULSE_GITHUB_CLIENT_SECRET_FILE, and PULSE_GITHUB_ALLOWED_USER_ID together".into()),
        };
        Ok(Self {
            public_url,
            setup_token_file,
            github,
        })
    }
}

#[derive(Clone)]
pub struct AuthState {
    inner: Arc<AuthInner>,
}

struct AuthInner {
    connection: Mutex<Connection>,
    database_permit: Arc<Semaphore>,
    attempts: Mutex<VecDeque<Instant>>,
    origin: String,
    secure: bool,
    setup_token_hash: Option<String>,
    github: Option<GithubOAuthConfig>,
    http: reqwest::Client,
    dummy_password_hash: String,
}

#[derive(Clone)]
pub struct AuthSession {
    pub username: String,
    pub csrf_token: String,
    pub totp_enabled: bool,
    token_hash: String,
    mfa_complete: bool,
}

#[derive(Serialize)]
// Public API capability flags are independent; changing them to enums would alter the wire format.
#[allow(clippy::struct_excessive_bools)]
struct AuthStatus {
    initialized: bool,
    logged_in: bool,
    username: Option<String>,
    csrf_token: String,
    totp_enabled: bool,
    oauth_enabled: bool,
    oauth_totp_required: bool,
}

#[derive(Debug)]
pub struct AuthError {
    status: StatusCode,
    message: &'static str,
}

impl AuthError {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "authentication required or credentials invalid",
        }
    }

    fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn internal() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "authentication service unavailable",
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for AuthError {}

impl From<rusqlite::Error> for AuthError {
    fn from(_: rusqlite::Error) -> Self {
        Self::internal()
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(json!({ "error": self.message }))).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

/// Called only inside Storage's backed-up, forward-only schema migration.
pub(crate) fn migrate(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE auth_admin (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            username TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            auth_version INTEGER NOT NULL DEFAULT 1,
            totp_secret BLOB,
            totp_last_step INTEGER NOT NULL DEFAULT -1,
            pending_totp_secret BLOB,
            pending_totp_expires_at INTEGER,
            pending_totp_session_hash TEXT
         );
         CREATE TABLE auth_sessions (
            token_hash TEXT PRIMARY KEY,
            admin_id INTEGER NOT NULL REFERENCES auth_admin(id) ON DELETE CASCADE,
            auth_version INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            mfa_complete INTEGER NOT NULL CHECK (mfa_complete IN (0, 1))
         );
         CREATE INDEX auth_sessions_expiry ON auth_sessions(expires_at);
         CREATE TABLE auth_oauth_flows (
            state_hash TEXT PRIMARY KEY,
            binding_hash TEXT NOT NULL,
            verifier TEXT NOT NULL,
            auth_version INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
         );
         CREATE INDEX auth_oauth_flows_expiry ON auth_oauth_flows(expires_at);",
    )
}

impl AuthState {
    /// Opens only an existing, already migrated database. Never creates tables or migrates.
    ///
    /// # Errors
    /// Returns an error for invalid configuration or unavailable authentication storage.
    pub fn new(
        database_path: &Path,
        config: AuthConfig,
        max_database_bytes: u64,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let url = reqwest::Url::parse(&config.public_url)?;
        let loopback = matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        );
        if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("PULSE_PUBLIC_URL must be an HTTPS origin, or HTTP on localhost for local development".into());
        }
        if let Some(github) = &config.github
            && (github.client_id.trim().is_empty()
                || github.client_secret.trim().is_empty()
                || github.allowed_user_id == 0)
        {
            return Err(
                "GitHub OAuth requires a client ID, client secret, and allowed numeric user ID"
                    .into(),
            );
        }
        let setup_token_hash = config
            .setup_token_file
            .map(|path| read_secret_file(&path, 32).map(|token| hash(&token)))
            .transpose()?;
        let connection =
            Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL; PRAGMA cache_size = -1024;",
        )?;
        limit_database_pages(&connection, max_database_bytes)?;
        // Query all tables up front so startup fails closed if Storage did not migrate.
        connection.query_row("SELECT COUNT(*) FROM auth_admin", [], |row| {
            row.get::<_, i64>(0)
        })?;
        connection.query_row("SELECT COUNT(*) FROM auth_sessions", [], |row| {
            row.get::<_, i64>(0)
        })?;
        connection.query_row("SELECT COUNT(*) FROM auth_oauth_flows", [], |row| {
            row.get::<_, i64>(0)
        })?;
        let dummy_password_hash = hash_password(&random_token())?;
        Ok(Self {
            inner: Arc::new(AuthInner {
                connection: Mutex::new(connection),
                database_permit: Arc::new(Semaphore::new(1)),
                attempts: Mutex::new(VecDeque::with_capacity(MAX_ATTEMPTS)),
                origin: url.origin().ascii_serialization(),
                secure: url.scheme() == "https",
                setup_token_hash,
                github: config.github,
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .connect_timeout(Duration::from_secs(5))
                    .redirect(reqwest::redirect::Policy::none())
                    .pool_max_idle_per_host(1)
                    .user_agent("Pulse-Service")
                    .build()?,
                dummy_password_hash,
            }),
        })
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/api/auth/status", get(status))
            .route("/api/auth/setup", post(setup))
            .route("/api/auth/login", post(login))
            .route("/api/auth/logout", post(logout))
            .route("/api/auth/password", post(change_password))
            .route("/api/auth/totp/setup", post(totp_setup))
            .route("/api/auth/totp/enable", post(totp_enable))
            .route("/api/auth/totp/disable", post(totp_disable))
            .route("/api/auth/oauth/start", post(oauth_start))
            .route("/api/auth/oauth/callback", get(oauth_callback))
            .route("/api/auth/oauth/complete", post(oauth_complete))
            .layer(DefaultBodyLimit::max(MAX_AUTH_REQUEST_BYTES))
            .layer(middleware::from_fn(auth_response_headers))
            .with_state(self.clone())
    }

    #[must_use]
    pub fn oauth_enabled(&self) -> bool {
        self.inner.github.is_some()
    }

    async fn database<T, F>(&self, operation: F) -> Result<T, AuthError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, AuthError> + Send + 'static,
    {
        let permit = timeout(
            Duration::from_secs(2),
            self.inner.database_permit.clone().acquire_owned(),
        )
        .await
        .map_err(|_| AuthError::internal())?
        .map_err(|_| AuthError::internal())?;
        let inner = Arc::clone(&self.inner);
        let session_lease = crate::session_gate::current();
        timeout(
            Duration::from_secs(15),
            tokio::task::spawn_blocking(move || {
                let _session_lease = session_lease;
                let _permit = permit;
                let mut connection = inner.connection.lock().map_err(|_| AuthError::internal())?;
                operation(&mut connection)
            }),
        )
        .await
        .map_err(|_| AuthError::internal())?
        .map_err(|_| AuthError::internal())?
    }

    fn allow_attempt(&self) -> Result<(), AuthError> {
        let mut attempts = self
            .inner
            .attempts
            .lock()
            .map_err(|_| AuthError::internal())?;
        let now = Instant::now();
        while attempts
            .front()
            .is_some_and(|attempt| now.duration_since(*attempt) >= Duration::from_secs(60))
        {
            attempts.pop_front();
        }
        if attempts.len() >= MAX_ATTEMPTS {
            return Err(AuthError {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "too many authentication attempts; retry in one minute",
            });
        }
        attempts.push_back(now);
        Ok(())
    }

    /// Reports whether the single administrator has completed setup.
    ///
    /// # Errors
    /// Returns an error if authentication storage cannot be queried.
    pub async fn initialized(&self) -> Result<bool, AuthError> {
        self.database(|connection| {
            Ok(
                connection.query_row("SELECT EXISTS(SELECT 1 FROM auth_admin)", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .await
    }

    async fn candidate_session(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<AuthSession>, AuthError> {
        let Some(token) = cookie(headers, self.session_cookie_name()) else {
            return Ok(None);
        };
        let token_hash = hash(&token);
        let csrf_token = csrf_for_session(&token);
        self.database(move |connection| {
            Ok(connection
                .query_row(
                    "SELECT a.username, a.totp_secret IS NOT NULL, s.mfa_complete
                 FROM auth_sessions s JOIN auth_admin a ON a.id = s.admin_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND s.auth_version = a.auth_version",
                    params![token_hash, now_seconds()?],
                    |row| {
                        Ok(AuthSession {
                            username: row.get(0)?,
                            csrf_token,
                            totp_enabled: row.get(1)?,
                            token_hash: token_hash.clone(),
                            mfa_complete: row.get(2)?,
                        })
                    },
                )
                .optional()?)
        })
        .await
    }

    /// Only returns fully authenticated administrator sessions.
    ///
    /// # Errors
    /// Returns an error if authentication storage cannot be queried.
    pub async fn session(&self, headers: &HeaderMap) -> Result<Option<AuthSession>, AuthError> {
        Ok(self
            .candidate_session(headers)
            .await?
            .filter(|session| session.mfa_complete))
    }

    /// Read APIs must call this even for public sites: initialization always precedes exposure.
    ///
    /// # Errors
    /// Returns an error for missing required authentication or unavailable storage.
    pub async fn authorize(
        &self,
        headers: &HeaderMap,
        requires_admin: bool,
        private_site: bool,
    ) -> Result<Option<AuthSession>, AuthError> {
        let session = self.session(headers).await?;
        if !self.initialized().await? || ((requires_admin || private_site) && session.is_none()) {
            return Err(AuthError::unauthorized());
        }
        Ok(session)
    }

    /// Call for every browser administration mutation, including public-site settings.
    ///
    /// # Errors
    /// Returns an error if the Origin or CSRF token does not match.
    pub fn verify_mutation(
        &self,
        headers: &HeaderMap,
        session: &AuthSession,
    ) -> Result<(), AuthError> {
        self.verify_origin(headers)?;
        verify_csrf(headers, &session.csrf_token)
    }

    fn verify_origin(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        if headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            != Some(self.inner.origin.as_str())
        {
            return Err(AuthError {
                status: StatusCode::FORBIDDEN,
                message: "request Origin does not match PULSE_PUBLIC_URL",
            });
        }
        Ok(())
    }

    async fn verify_browser(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        self.verify_origin(headers)?;
        if let Some(session) = self.candidate_session(headers).await? {
            return verify_csrf(headers, &session.csrf_token);
        }
        let csrf = cookie(headers, self.csrf_cookie_name()).ok_or(AuthError::unauthorized())?;
        verify_csrf(headers, &csrf)
    }

    async fn require_mutation(&self, headers: &HeaderMap) -> Result<AuthSession, AuthError> {
        let session = self
            .session(headers)
            .await?
            .ok_or(AuthError::unauthorized())?;
        self.verify_mutation(headers, &session)?;
        Ok(session)
    }

    fn session_cookie_name(&self) -> &'static str {
        if self.inner.secure {
            "__Host-pulse-session"
        } else {
            "pulse_session"
        }
    }

    fn csrf_cookie_name(&self) -> &'static str {
        if self.inner.secure {
            "__Host-pulse-csrf"
        } else {
            "pulse_csrf"
        }
    }

    fn oauth_cookie_name(&self) -> &'static str {
        if self.inner.secure {
            "__Host-pulse-oauth"
        } else {
            "pulse_oauth"
        }
    }

    fn set_cookie(
        &self,
        response: &mut Response,
        name: &str,
        token: &str,
        max_age: i64,
    ) -> Result<(), AuthError> {
        let secure = if self.inner.secure { "; Secure" } else { "" };
        let value =
            format!("{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}");
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).map_err(|_| AuthError::internal())?,
        );
        Ok(())
    }

    fn logout_response(&self) -> Result<Response, AuthError> {
        let mut response = Json(json!({ "ok": true })).into_response();
        self.set_cookie(&mut response, self.session_cookie_name(), "", 0)?;
        Ok(response)
    }

    async fn session_response(&self, token: String) -> Result<Response, AuthError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{}={token}", self.session_cookie_name()))
                .map_err(|_| AuthError::internal())?,
        );
        let session = self
            .candidate_session(&headers)
            .await?
            .ok_or(AuthError::unauthorized())?;
        let mut response =
            Json(self.status_data(Some(session), String::new(), true)).into_response();
        self.set_cookie(
            &mut response,
            self.session_cookie_name(),
            &token,
            SESSION_TTL,
        )?;
        Ok(response)
    }

    fn status_data(
        &self,
        session: Option<AuthSession>,
        anonymous_csrf: String,
        initialized: bool,
    ) -> AuthStatus {
        let logged_in = session.as_ref().is_some_and(|session| session.mfa_complete);
        AuthStatus {
            initialized,
            logged_in,
            username: session
                .as_ref()
                .filter(|session| session.mfa_complete)
                .map(|session| session.username.clone()),
            csrf_token: session
                .as_ref()
                .map_or(anonymous_csrf, |session| session.csrf_token.clone()),
            totp_enabled: session.as_ref().is_some_and(|session| session.totp_enabled),
            oauth_enabled: initialized && self.inner.github.is_some(),
            oauth_totp_required: session.is_some_and(|session| !session.mfa_complete),
        }
    }
}

async fn auth_response_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn status(State(state): State<AuthState>, headers: HeaderMap) -> Result<Response, AuthError> {
    let initialized = state.initialized().await?;
    let session = state.candidate_session(&headers).await?;
    let anonymous = session.is_none();
    let csrf = cookie(&headers, state.csrf_cookie_name()).unwrap_or_else(random_token);
    let mut response = Json(state.status_data(session, csrf.clone(), initialized)).into_response();
    if anonymous {
        state.set_cookie(&mut response, state.csrf_cookie_name(), &csrf, SESSION_TTL)?;
    }
    Ok(response)
}

#[derive(Deserialize)]
struct SetupRequest {
    token: String,
    username: String,
    password: String,
}

async fn setup(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<SetupRequest>,
) -> Result<Response, AuthError> {
    state.verify_browser(&headers).await?;
    state.allow_attempt()?;
    let configured = state.inner.setup_token_hash.as_ref().ok_or(AuthError {
        status: StatusCode::FORBIDDEN,
        message: "administrator must configure PULSE_SETUP_TOKEN_FILE before setup",
    })?;
    if !secure_equal(configured, &hash(&request.token)) {
        return Err(AuthError::unauthorized());
    }
    if request.username.is_empty()
        || request.username.len() > 64
        || !request
            .username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return Err(AuthError::bad_request(
            "username must contain 1 to 64 ASCII letters, digits, underscores, hyphens, or periods",
        ));
    }
    validate_password(&request.password)?;
    let token = state
        .database(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let exists: bool =
                transaction.query_row("SELECT EXISTS(SELECT 1 FROM auth_admin)", [], |row| {
                    row.get(0)
                })?;
            if exists {
                return Err(AuthError {
                    status: StatusCode::CONFLICT,
                    message: "administrator already initialized",
                });
            }
            let password_hash = hash_password(&request.password)?;
            transaction.execute(
                "INSERT INTO auth_admin (id, username, password_hash) VALUES (1, ?1, ?2)",
                params![request.username, password_hash],
            )?;
            let token = insert_session(&transaction, true)?;
            audit(&transaction, "auth.setup")?;
            transaction.commit()?;
            Ok(token)
        })
        .await?;
    state.session_response(token).await
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
    code: Option<String>,
}

async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response, AuthError> {
    state.verify_browser(&headers).await?;
    state.allow_attempt()?;
    let dummy = state.inner.dummy_password_hash.clone();
    let token = state
        .database(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let admin = admin(&transaction)?;
            let valid_password = verify_password(
                &request.password,
                admin
                    .as_ref()
                    .map_or(dummy.as_str(), |admin| admin.password_hash.as_str()),
            );
            let admin = admin.ok_or(AuthError::unauthorized())?;
            if !valid_password || request.username != admin.username {
                return Err(AuthError::unauthorized());
            }
            verify_totp(&transaction, &admin, request.code.as_deref())?;
            let token = insert_session(&transaction, true)?;
            audit(&transaction, "auth.login")?;
            transaction.commit()?;
            Ok(token)
        })
        .await?;
    state.session_response(token).await
}

async fn logout(State(state): State<AuthState>, headers: HeaderMap) -> Result<Response, AuthError> {
    state.verify_browser(&headers).await?;
    if let Some(session) = state.candidate_session(&headers).await? {
        state
            .database(move |connection| {
                connection.execute(
                    "DELETE FROM auth_sessions WHERE token_hash = ?1",
                    [session.token_hash],
                )?;
                Ok(())
            })
            .await?;
    }
    state.logout_response()
}

#[derive(Deserialize)]
struct PasswordRequest {
    current_password: String,
    new_password: String,
    code: Option<String>,
}

async fn change_password(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<PasswordRequest>,
) -> Result<Response, AuthError> {
    let session = state.require_mutation(&headers).await?;
    state.allow_attempt()?;
    validate_password(&request.new_password)?;
    state
        .database(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            require_current_session(&transaction, &session, true)?;
            let admin = admin(&transaction)?.ok_or(AuthError::unauthorized())?;
            if !verify_password(&request.current_password, &admin.password_hash) {
                return Err(AuthError::unauthorized());
            }
            verify_totp(&transaction, &admin, request.code.as_deref())?;
            transaction.execute(
                "UPDATE auth_admin SET password_hash = ?1 WHERE id = 1",
                [hash_password(&request.new_password)?],
            )?;
            revoke_sessions(&transaction)?;
            audit(&transaction, "auth.password.change")?;
            transaction.commit()?;
            Ok(())
        })
        .await?;
    state.logout_response()
}

#[derive(Deserialize)]
struct TotpSetupRequest {
    password: String,
}

async fn totp_setup(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<TotpSetupRequest>,
) -> Result<Json<Value>, AuthError> {
    let session = state.require_mutation(&headers).await?;
    state.allow_attempt()?;
    state.database(move |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_current_session(&transaction, &session, true)?;
        let admin = admin(&transaction)?.ok_or(AuthError::unauthorized())?;
        if !verify_password(&request.password, &admin.password_hash) { return Err(AuthError::unauthorized()); }
        if admin.totp_secret.is_some() { return Err(AuthError::bad_request("TOTP is already enabled")); }
        let secret = random_secret();
        let totp = totp(&secret, &admin.username)?;
        let otpauth_url = totp.to_url().map_err(|_| AuthError::internal())?;
        let encoded = Secret::from(secret.clone()).to_base32();
        transaction.execute(
            "UPDATE auth_admin SET pending_totp_secret = ?1, pending_totp_expires_at = ?2, pending_totp_session_hash = ?3 WHERE id = 1",
            params![secret, now_seconds()? + FLOW_TTL, session.token_hash],
        )?;
        transaction.commit()?;
        Ok(Json(json!({ "secret": encoded, "otpauth_url": otpauth_url, "expires_in": FLOW_TTL })))
    }).await
}

#[derive(Deserialize)]
struct TotpCodeRequest {
    code: String,
}

async fn totp_enable(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<TotpCodeRequest>,
) -> Result<Response, AuthError> {
    let session = state.require_mutation(&headers).await?;
    state.allow_attempt()?;
    state.database(move |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_current_session(&transaction, &session, true)?;
        let pending: Option<(String, Vec<u8>)> = transaction.query_row(
            "SELECT username, pending_totp_secret FROM auth_admin WHERE id = 1 AND totp_secret IS NULL
             AND pending_totp_secret IS NOT NULL AND pending_totp_expires_at > ?1 AND pending_totp_session_hash = ?2",
            params![now_seconds()?, session.token_hash], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let (username, secret) = pending.ok_or(AuthError::bad_request("TOTP setup is missing or expired"))?;
        let step = checked_totp_step(&secret, &username, &request.code)?;
        transaction.execute("UPDATE auth_admin SET totp_secret = ?1, totp_last_step = ?2 WHERE id = 1", params![secret, step])?;
        revoke_sessions(&transaction)?;
        audit(&transaction, "auth.totp.enable")?;
        transaction.commit()?;
        Ok(())
    }).await?;
    state.logout_response()
}

#[derive(Deserialize)]
struct TotpDisableRequest {
    password: String,
    code: String,
}

async fn totp_disable(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<TotpDisableRequest>,
) -> Result<Response, AuthError> {
    let session = state.require_mutation(&headers).await?;
    state.allow_attempt()?;
    state
        .database(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            require_current_session(&transaction, &session, true)?;
            let admin = admin(&transaction)?.ok_or(AuthError::unauthorized())?;
            if !verify_password(&request.password, &admin.password_hash)
                || admin.totp_secret.is_none()
            {
                return Err(AuthError::unauthorized());
            }
            verify_totp(&transaction, &admin, Some(&request.code))?;
            transaction.execute(
                "UPDATE auth_admin SET totp_secret = NULL, totp_last_step = -1 WHERE id = 1",
                [],
            )?;
            revoke_sessions(&transaction)?;
            audit(&transaction, "auth.totp.disable")?;
            transaction.commit()?;
            Ok(())
        })
        .await?;
    state.logout_response()
}

async fn oauth_start(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    state.verify_browser(&headers).await?;
    state.allow_attempt()?;
    let github = state
        .inner
        .github
        .as_ref()
        .ok_or(AuthError::bad_request("GitHub OAuth is not configured"))?;
    let flow_state = random_token();
    let binding = random_token();
    let verifier = random_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state_hash = hash(&flow_state);
    let binding_hash = hash(&binding);
    state.database(move |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let admin = admin(&transaction)?.ok_or(AuthError::unauthorized())?;
        transaction.execute("DELETE FROM auth_oauth_flows WHERE expires_at <= ?1", [now_seconds()?])?;
        let count: i64 = transaction.query_row("SELECT COUNT(*) FROM auth_oauth_flows", [], |row| row.get(0))?;
        if count >= MAX_OAUTH_FLOWS {
            return Err(AuthError { status: StatusCode::TOO_MANY_REQUESTS, message: "too many OAuth flows; retry later" });
        }
        transaction.execute(
            "INSERT INTO auth_oauth_flows (state_hash, binding_hash, verifier, auth_version, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![state_hash, binding_hash, verifier, admin.auth_version, now_seconds()? + FLOW_TTL],
        )?;
        transaction.commit()?;
        Ok(())
    }).await?;
    let mut authorization_url = reqwest::Url::parse("https://github.com/login/oauth/authorize")
        .map_err(|_| AuthError::internal())?;
    authorization_url
        .query_pairs_mut()
        .append_pair("client_id", &github.client_id)
        .append_pair(
            "redirect_uri",
            &format!("{}/api/auth/oauth/callback", state.inner.origin),
        )
        .append_pair("scope", "")
        .append_pair("state", &flow_state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("allow_signup", "false");
    let mut response =
        Json(json!({ "authorization_url": authorization_url.as_str() })).into_response();
    state.set_cookie(&mut response, state.oauth_cookie_name(), &binding, FLOW_TTL)?;
    Ok(response)
}

#[derive(Deserialize)]
struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
}

#[derive(Deserialize)]
struct GithubToken {
    access_token: String,
    token_type: String,
}

#[derive(Deserialize)]
struct GithubIdentity {
    id: u64,
}

async fn oauth_callback(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallback>,
) -> Result<Response, AuthError> {
    state.allow_attempt()?;
    let github = state
        .inner
        .github
        .as_ref()
        .ok_or(AuthError::unauthorized())?;
    let code = query
        .code
        .filter(|code| !code.is_empty() && code.len() <= 512)
        .ok_or(AuthError::unauthorized())?;
    let flow_state = query
        .state
        .filter(|state| valid_token(state))
        .ok_or(AuthError::unauthorized())?;
    let binding = cookie(&headers, state.oauth_cookie_name()).ok_or(AuthError::unauthorized())?;
    let state_hash = hash(&flow_state);
    let binding_hash = hash(&binding);
    let (verifier, auth_version) = state.database(move |connection| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let flow: Option<(String, i64)> = transaction.query_row(
            "SELECT f.verifier, f.auth_version FROM auth_oauth_flows f JOIN auth_admin a ON a.auth_version = f.auth_version
             WHERE f.state_hash = ?1 AND f.binding_hash = ?2 AND f.expires_at > ?3 AND a.id = 1",
            params![state_hash, binding_hash, now_seconds()?], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let flow = flow.ok_or(AuthError::unauthorized())?;
        transaction.execute("DELETE FROM auth_oauth_flows WHERE state_hash = ?1", [state_hash])?;
        transaction.commit()?;
        Ok(flow)
    }).await?;
    let identity = github_identity(&state, github, &code, &verifier).await?;
    if identity.id != github.allowed_user_id {
        return Err(AuthError::unauthorized());
    }
    let (token, complete) = state
        .database(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let admin = admin(&transaction)?.ok_or(AuthError::unauthorized())?;
            if admin.auth_version != auth_version {
                return Err(AuthError::unauthorized());
            }
            let complete = admin.totp_secret.is_none();
            let token = insert_session(&transaction, complete)?;
            audit(
                &transaction,
                if complete {
                    "auth.oauth.login"
                } else {
                    "auth.oauth.totp.required"
                },
            )?;
            transaction.commit()?;
            Ok((token, complete))
        })
        .await?;
    let mut response = Redirect::to(if complete {
        "/"
    } else {
        "/?oauth_totp=required"
    })
    .into_response();
    state.set_cookie(
        &mut response,
        state.session_cookie_name(),
        &token,
        if complete { SESSION_TTL } else { FLOW_TTL },
    )?;
    state.set_cookie(&mut response, state.oauth_cookie_name(), "", 0)?;
    Ok(response)
}

async fn oauth_complete(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(request): Json<TotpCodeRequest>,
) -> Result<Response, AuthError> {
    state.verify_browser(&headers).await?;
    state.allow_attempt()?;
    let session = state
        .candidate_session(&headers)
        .await?
        .ok_or(AuthError::unauthorized())?;
    if session.mfa_complete || !session.totp_enabled {
        return Err(AuthError::unauthorized());
    }
    let token = state
        .database(move |connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            require_current_session(&transaction, &session, false)?;
            let admin = admin(&transaction)?.ok_or(AuthError::unauthorized())?;
            verify_totp(&transaction, &admin, Some(&request.code))?;
            transaction.execute(
                "DELETE FROM auth_sessions WHERE token_hash = ?1",
                [session.token_hash],
            )?;
            let token = insert_session(&transaction, true)?;
            audit(&transaction, "auth.oauth.login")?;
            transaction.commit()?;
            Ok(token)
        })
        .await?;
    state.session_response(token).await
}

async fn bounded_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, AuthError> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > MAX_OAUTH_RESPONSE_BYTES as u64)
    {
        return Err(AuthError::unauthorized());
    }
    let mut bytes = Vec::with_capacity(2_048);
    while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::internal())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_OAUTH_RESPONSE_BYTES {
            return Err(AuthError::internal());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| AuthError::unauthorized())
}

async fn github_identity(
    state: &AuthState,
    github: &GithubOAuthConfig,
    code: &str,
    verifier: &str,
) -> Result<GithubIdentity, AuthError> {
    let token_response = state
        .inner
        .http
        .post("https://github.com/login/oauth/access_token")
        .header(header::ACCEPT, "application/json")
        .json(&json!({
            "client_id": github.client_id,
            "client_secret": github.client_secret,
            "code": code,
            "redirect_uri": format!("{}/api/auth/oauth/callback", state.inner.origin),
            "code_verifier": verifier,
        }))
        .send()
        .await
        .map_err(|_| AuthError::internal())?;
    let access: GithubToken = bounded_json(token_response).await?;
    if !access.token_type.eq_ignore_ascii_case("bearer")
        || access.access_token.is_empty()
        || access.access_token.len() > 2_048
    {
        return Err(AuthError::unauthorized());
    }
    let identity_response = state
        .inner
        .http
        .get("https://api.github.com/user")
        .header(header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .bearer_auth(&access.access_token)
        .send()
        .await
        .map_err(|_| AuthError::internal())?;
    bounded_json(identity_response).await
}

fn limit_database_pages(
    connection: &Connection,
    max_database_bytes: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let page_size = u64::try_from(page_size).map_err(|_| "invalid SQLite page size")?;
    if page_size == 0 || max_database_bytes == 0 {
        return Err("invalid authentication database size limit".into());
    }
    let pages: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let pages = u64::try_from(pages).map_err(|_| "invalid SQLite page count")?;
    if pages.saturating_mul(page_size) > max_database_bytes {
        return Err("database already exceeds PULSE_MAX_DATABASE_BYTES".into());
    }
    let max_pages = i64::try_from(max_database_bytes.div_ceil(page_size).max(1))
        .map_err(|_| "configured database size exceeds SQLite limits")?;
    connection.pragma_update(None, "max_page_count", max_pages)?;
    Ok(())
}

struct Admin {
    username: String,
    password_hash: String,
    auth_version: i64,
    totp_secret: Option<Vec<u8>>,
    totp_last_step: i64,
}

fn admin(connection: &Connection) -> Result<Option<Admin>, AuthError> {
    Ok(connection.query_row(
        "SELECT username, password_hash, auth_version, totp_secret, totp_last_step FROM auth_admin WHERE id = 1",
        [], |row| Ok(Admin { username: row.get(0)?, password_hash: row.get(1)?, auth_version: row.get(2)?,
            totp_secret: row.get(3)?, totp_last_step: row.get(4)? }),
    ).optional()?)
}

fn require_current_session(
    transaction: &Transaction<'_>,
    session: &AuthSession,
    complete: bool,
) -> Result<(), AuthError> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM auth_sessions s JOIN auth_admin a ON a.id = s.admin_id
         WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND s.auth_version = a.auth_version AND s.mfa_complete = ?3)",
        params![session.token_hash, now_seconds()?, complete], |row| row.get(0),
    )?;
    if !exists {
        return Err(AuthError::unauthorized());
    }
    Ok(())
}

fn insert_session(transaction: &Transaction<'_>, mfa_complete: bool) -> Result<String, AuthError> {
    let now = now_seconds()?;
    transaction.execute("DELETE FROM auth_sessions WHERE expires_at <= ?1", [now])?;
    // Keep at most eight credentials, including unfinished OAuth second factors.
    transaction.execute(
        "DELETE FROM auth_sessions WHERE token_hash IN (
           SELECT token_hash FROM auth_sessions ORDER BY created_at DESC, rowid DESC LIMIT -1 OFFSET ?1
         )", [MAX_SESSIONS - 1],
    )?;
    let token = random_token();
    transaction.execute(
        "INSERT INTO auth_sessions (token_hash, admin_id, auth_version, created_at, expires_at, mfa_complete)
         SELECT ?1, id, auth_version, ?2, ?3, ?4 FROM auth_admin WHERE id = 1",
        params![hash(&token), now, now + if mfa_complete { SESSION_TTL } else { FLOW_TTL }, mfa_complete],
    )?;
    Ok(token)
}

fn revoke_sessions(transaction: &Transaction<'_>) -> Result<(), AuthError> {
    transaction.execute("DELETE FROM auth_sessions", [])?;
    transaction.execute("DELETE FROM auth_oauth_flows", [])?;
    transaction.execute(
        "UPDATE auth_admin SET auth_version = auth_version + 1, pending_totp_secret = NULL,
         pending_totp_expires_at = NULL, pending_totp_session_hash = NULL WHERE id = 1",
        [],
    )?;
    Ok(())
}

fn audit(transaction: &Transaction<'_>, action: &str) -> Result<(), AuthError> {
    transaction.execute(
        "INSERT INTO audit_events (happened_at_ms, action, subject) VALUES (?1, ?2, 'administrator')",
        params![now_seconds()?.saturating_mul(1_000), action],
    )?;
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if !(12..=1_024).contains(&password.len()) {
        return Err(AuthError::bad_request(
            "password must contain 12 to 1024 bytes",
        ));
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt =
        SaltString::encode_b64(Uuid::new_v4().as_bytes()).map_err(|_| AuthError::internal())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::internal())
}

fn read_secret_file(
    path: &Path,
    minimum_length: usize,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let mut contents = String::new();
    File::open(path)?
        .take(4_097)
        .read_to_string(&mut contents)?;
    let token = contents.trim();
    if contents.len() > 4_096 || token.len() < minimum_length {
        return Err("secret file is empty, too short, or exceeds 4096 bytes".into());
    }
    Ok(token.to_owned())
}

fn verify_password(password: &str, encoded: &str) -> bool {
    if password.len() > 1_024 {
        return false;
    }
    PasswordHash::new(encoded).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

fn totp(secret: &[u8], username: &str) -> Result<Totp, AuthError> {
    Builder::new()
        .with_secret(secret.to_vec())
        .with_account_name(username)
        .with_issuer(Some("Pulse"))
        .build()
        .map_err(|_| AuthError::internal())
}

fn checked_totp_step(secret: &[u8], username: &str, code: &str) -> Result<i64, AuthError> {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AuthError::unauthorized());
    }
    let seconds = u64::try_from(now_seconds()?).map_err(|_| AuthError::internal())?;
    let step = totp(secret, username)?
        .check(code, seconds)
        .ok_or(AuthError::unauthorized())?;
    i64::try_from(step).map_err(|_| AuthError::internal())
}

fn verify_totp(
    transaction: &Transaction<'_>,
    admin: &Admin,
    code: Option<&str>,
) -> Result<(), AuthError> {
    if let Some(secret) = &admin.totp_secret {
        let step = checked_totp_step(
            secret,
            &admin.username,
            code.ok_or(AuthError::unauthorized())?,
        )?;
        if step <= admin.totp_last_step {
            return Err(AuthError::unauthorized());
        }
        transaction.execute(
            "UPDATE auth_admin SET totp_last_step = ?1 WHERE id = 1",
            [step],
        )?;
    }
    Ok(())
}

fn now_seconds() -> Result<i64, AuthError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AuthError::internal())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| AuthError::internal())
}

fn random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn random_secret() -> Vec<u8> {
    [
        Uuid::new_v4().as_bytes().as_slice(),
        Uuid::new_v4().as_bytes().as_slice(),
    ]
    .concat()
}

fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn csrf_for_session(token: &str) -> String {
    hash(&format!("pulse-session-csrf:{token}"))
}

fn secure_equal(left: &str, right: &str) -> bool {
    bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}

fn valid_token(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut result = None;
    for header in headers.get_all(header::COOKIE) {
        for pair in header.to_str().ok()?.split(';') {
            let Some((key, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if result.is_some() || !valid_token(value) {
                    return None;
                }
                result = Some(value.to_owned());
            }
        }
    }
    result
}

fn verify_csrf(headers: &HeaderMap, expected: &str) -> Result<(), AuthError> {
    let supplied = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !valid_token(supplied) || !secure_equal(supplied, expected) {
        return Err(AuthError {
            status: StatusCode::FORBIDDEN,
            message: "CSRF token is missing or invalid; refresh authentication status",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;

    const TEST_ORIGIN: &str = "http://127.0.0.1:8080";
    const BOOTSTRAP: &str = "pulse-test-bootstrap-token-of-at-least-32-characters";
    const PASSWORD: &str = "a sufficiently long test password";

    fn state() -> (tempfile::TempDir, AuthState) {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("pulse.db");
        let token_file = directory.path().join("setup-token");
        std::fs::write(&token_file, BOOTSTRAP).unwrap();
        let _storage = crate::storage::Storage::open(&database, 32 * 1024 * 1024).unwrap();
        let state = AuthState::new(
            &database,
            AuthConfig {
                public_url: TEST_ORIGIN.to_owned(),
                setup_token_file: Some(token_file),
                github: None,
            },
            32 * 1024 * 1024,
        )
        .unwrap();
        (directory, state)
    }

    async fn request(
        state: &AuthState,
        method: &str,
        path: &str,
        cookie: &str,
        csrf: Option<&str>,
        origin: Option<&str>,
        body: Value,
    ) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if !cookie.is_empty() {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = csrf {
            builder = builder.header("x-csrf-token", csrf);
        }
        if let Some(origin) = origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        state
            .router()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    fn response_cookie(response: &Response, name: &str) -> String {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with(&format!("{name}=")))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }

    async fn payload(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 16 * 1024).await.unwrap()).unwrap()
    }

    async fn anonymous(state: &AuthState) -> (String, String) {
        let response = request(
            state,
            "GET",
            "/api/auth/status",
            "",
            None,
            None,
            json!(null),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let cookie = response_cookie(&response, state.csrf_cookie_name());
        let body = payload(response).await;
        (cookie, body["csrf_token"].as_str().unwrap().to_owned())
    }

    async fn initialize(state: &AuthState) -> (String, String) {
        let (cookie, csrf) = anonymous(state).await;
        let response = request(
            state,
            "POST",
            "/api/auth/setup",
            &cookie,
            Some(&csrf),
            Some(TEST_ORIGIN),
            json!({ "token": BOOTSTRAP, "username": "admin", "password": PASSWORD }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response_cookie(&response, state.session_cookie_name());
        let body = payload(response).await;
        assert_eq!(body["logged_in"], true);
        (cookie, body["csrf_token"].as_str().unwrap().to_owned())
    }

    fn headers(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        headers
    }

    #[tokio::test]
    async fn initialization_requires_bootstrap_origin_and_csrf_and_is_single_use() {
        let (_directory, state) = state();
        assert!(
            state
                .authorize(&HeaderMap::new(), false, false)
                .await
                .is_err()
        );
        let (cookie, csrf) = anonymous(&state).await;
        let body = json!({ "token": BOOTSTRAP, "username": "admin", "password": PASSWORD });
        let missing_origin = request(
            &state,
            "POST",
            "/api/auth/setup",
            &cookie,
            Some(&csrf),
            None,
            body.clone(),
        )
        .await;
        assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN);
        let missing_csrf = request(
            &state,
            "POST",
            "/api/auth/setup",
            &cookie,
            None,
            Some(TEST_ORIGIN),
            body.clone(),
        )
        .await;
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
        let wrong_token = request(
            &state,
            "POST",
            "/api/auth/setup",
            &cookie,
            Some(&csrf),
            Some(TEST_ORIGIN),
            json!({ "token": "wrong", "username": "admin", "password": PASSWORD }),
        )
        .await;
        assert_eq!(wrong_token.status(), StatusCode::UNAUTHORIZED);
        let (session_cookie, _) = initialize(&state).await;
        let repeated = request(
            &state,
            "POST",
            "/api/auth/setup",
            &cookie,
            Some(&csrf),
            Some(TEST_ORIGIN),
            body,
        )
        .await;
        assert_eq!(repeated.status(), StatusCode::CONFLICT);
        assert!(
            state
                .authorize(&HeaderMap::new(), false, true)
                .await
                .is_err()
        );
        assert!(
            state
                .authorize(&HeaderMap::new(), true, false)
                .await
                .is_err()
        );
        assert!(
            state
                .authorize(&HeaderMap::new(), false, false)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .authorize(&headers(&session_cookie), true, true)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn password_change_revokes_all_sessions_and_old_password() {
        let (_directory, state) = state();
        let (first_cookie, first_csrf) = initialize(&state).await;
        let (anonymous_cookie, csrf) = anonymous(&state).await;
        let login = request(
            &state,
            "POST",
            "/api/auth/login",
            &anonymous_cookie,
            Some(&csrf),
            Some(TEST_ORIGIN),
            json!({ "username": "admin", "password": PASSWORD }),
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let second_cookie = response_cookie(&login, state.session_cookie_name());
        let change = request(&state, "POST", "/api/auth/password", &first_cookie, Some(&first_csrf), Some(TEST_ORIGIN),
            json!({ "current_password": PASSWORD, "new_password": "new sufficiently long password" })).await;
        assert_eq!(change.status(), StatusCode::OK);
        assert!(
            state
                .session(&headers(&first_cookie))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .session(&headers(&second_cookie))
                .await
                .unwrap()
                .is_none()
        );
        let old_login = request(
            &state,
            "POST",
            "/api/auth/login",
            &anonymous_cookie,
            Some(&csrf),
            Some(TEST_ORIGIN),
            json!({ "username": "admin", "password": PASSWORD }),
        )
        .await;
        assert_eq!(old_login.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sessions_expire_are_bounded_and_partial_oauth_cannot_authorize() {
        let (_directory, state) = state();
        initialize(&state).await;
        let partial = state
            .database(|connection| {
                let transaction = connection.transaction()?;
                for _ in 0..12 {
                    insert_session(&transaction, true)?;
                }
                let partial = insert_session(&transaction, false)?;
                let count: i64 =
                    transaction
                        .query_row("SELECT COUNT(*) FROM auth_sessions", [], |row| row.get(0))?;
                assert_eq!(count, MAX_SESSIONS);
                transaction.commit()?;
                Ok(partial)
            })
            .await
            .unwrap();
        let cookie = format!("{}={partial}", state.session_cookie_name());
        assert!(
            state
                .candidate_session(&headers(&cookie))
                .await
                .unwrap()
                .is_some()
        );
        assert!(state.session(&headers(&cookie)).await.unwrap().is_none());
        assert!(
            state
                .authorize(&headers(&cookie), true, false)
                .await
                .is_err()
        );
        assert!(
            state
                .authorize(&headers(&cookie), false, true)
                .await
                .is_err()
        );
        state
            .database(|connection| {
                connection.execute("UPDATE auth_sessions SET expires_at = 0", [])?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            state
                .candidate_session(&headers(&cookie))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn totp_accepts_a_step_once_and_stale_sessions_cannot_modify_account() {
        let (_directory, state) = state();
        let (cookie, _) = initialize(&state).await;
        let session = state.session(&headers(&cookie)).await.unwrap().unwrap();
        state
            .database(move |connection| {
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let secret = random_secret();
                transaction.execute(
                    "UPDATE auth_admin SET totp_secret = ?1 WHERE id = 1",
                    [&secret],
                )?;
                let administrator = admin(&transaction)?.unwrap();
                let seconds = u64::try_from(now_seconds()?).unwrap();
                let code = totp(&secret, &administrator.username)?
                    .generate(seconds)
                    .to_string();
                verify_totp(&transaction, &administrator, Some(&code))?;
                let administrator = admin(&transaction)?.unwrap();
                assert!(verify_totp(&transaction, &administrator, Some(&code)).is_err());
                revoke_sessions(&transaction)?;
                assert!(require_current_session(&transaction, &session, true).is_err());
                transaction.commit()?;
                Ok(())
            })
            .await
            .unwrap();
    }

    #[test]
    fn authentication_does_not_create_or_migrate_a_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.db");
        assert!(AuthState::new(&path, AuthConfig::default(), 32 * 1024 * 1024).is_err());
        assert!(!path.exists());
        let connection = Connection::open(&path).unwrap();
        assert!(AuthState::new(&path, AuthConfig::default(), 32 * 1024 * 1024).is_err());
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn cancelled_authentication_worker_keeps_its_exclusive_session_lease() {
        let (_directory, state) = state();
        let gate = crate::session_gate::SessionGate::default();
        let lease = gate.acquire(true).await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::sync_channel(1);
        let request = tokio::spawn(crate::session_gate::scope(Some(lease), async move {
            state
                .database(move |_| {
                    let _ = started_tx.send(());
                    finish_rx
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| AuthError::internal())?;
                    Ok(())
                })
                .await
        }));
        started_rx.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        assert!(
            !gate.write_available(),
            "authentication cancellation must retain the running worker's exclusive lease"
        );
        finish_tx.send(()).unwrap();
        let next_admin = gate.acquire(false).await.unwrap();
        drop(next_admin);
        assert!(gate.write_available());
    }

    #[test]
    fn migration_rolls_back_and_sensitive_cookie_options_are_fixed() {
        let mut connection = Connection::open_in_memory().unwrap();
        {
            let transaction = connection.transaction().unwrap();
            migrate(&transaction).unwrap();
        }
        let tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name LIKE 'auth_%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
        let (_directory, state) = state();
        let mut response = Json(json!({})).into_response();
        state
            .set_cookie(
                &mut response,
                state.session_cookie_name(),
                &random_token(),
                SESSION_TTL,
            )
            .unwrap();
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("HttpOnly; SameSite=Lax; Max-Age=43200"));
        assert!(!cookie.contains("Domain="));
    }

    #[test]
    fn origin_validation_rejects_insecure_remote_urls_and_forwarded_headers() {
        let (_directory, state) = state();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("trusted.example"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://trusted.example"),
        );
        assert!(state.verify_origin(&headers).is_err());
        let config = AuthConfig {
            public_url: "http://pulse.example.com".to_owned(),
            ..AuthConfig::default()
        };
        assert!(AuthState::new(Path::new("not-opened.db"), config, 32 * 1024 * 1024).is_err());
        let mut duplicate = HeaderMap::new();
        let token = random_token();
        duplicate.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("pulse_session={token}; pulse_session={token}"))
                .unwrap(),
        );
        assert!(cookie(&duplicate, "pulse_session").is_none());
    }
}
