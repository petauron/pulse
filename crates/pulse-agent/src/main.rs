mod extended_metrics;
mod probes;
mod region;
mod system_command;

use std::{
    collections::HashSet,
    env,
    error::Error,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pulse_protocol::{
    EnrollmentRequest, EnrollmentResponse, PROTOCOL_VERSION, SnapshotResponse, SystemSnapshot,
};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sysinfo::{Disks, Networks, System};
use tempfile::NamedTempFile;
use tokio::time::{MissedTickBehavior, interval};
use uuid::Uuid;

const DEFAULT_SERVICE_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_INTERVAL_SECONDS: u64 = 3;
const MIN_INTERVAL_SECONDS: u64 = 1;
const MAX_INTERVAL_SECONDS: u64 = 300;
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Debug)]
struct AgentConfig {
    service_url: Url,
    credentials_path: PathBuf,
    enrollment_token: Option<String>,
    enrollment_token_file: Option<PathBuf>,
    node_name: String,
    region: String,
    geoip_provider: region::Provider,
    group: String,
    virtualization: String,
    interval: Duration,
    disk_mount_points: Option<HashSet<String>>,
    network_interfaces: Option<HashSet<String>>,
    extended_metrics: bool,
    gpu_metrics: bool,
}

impl AgentConfig {
    fn from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let service_url = parse_service_url(
            &env::var("PULSE_SERVICE_URL").unwrap_or_else(|_| DEFAULT_SERVICE_URL.to_owned()),
        )?;
        let interval_seconds = env::var("PULSE_INTERVAL_SECONDS")
            .map_or(Ok(DEFAULT_INTERVAL_SECONDS), |value| value.parse::<u64>())?;
        if !(MIN_INTERVAL_SECONDS..=MAX_INTERVAL_SECONDS).contains(&interval_seconds) {
            return Err(format!(
                "PULSE_INTERVAL_SECONDS must be between {MIN_INTERVAL_SECONDS} and {MAX_INTERVAL_SECONDS}"
            )
            .into());
        }
        let credentials_path = match env::var_os("PULSE_CREDENTIALS_PATH") {
            Some(path) => PathBuf::from(path),
            None => default_credentials_path()?,
        };

        let enrollment_token = env::var("PULSE_ENROLLMENT_TOKEN").ok();
        let enrollment_token_file = env::var_os("PULSE_ENROLLMENT_TOKEN_FILE").map(PathBuf::from);
        if enrollment_token.is_some() && enrollment_token_file.is_some() {
            return Err(
                "set only one of PULSE_ENROLLMENT_TOKEN and PULSE_ENROLLMENT_TOKEN_FILE".into(),
            );
        }

        let mut config = Self {
            service_url,
            credentials_path,
            enrollment_token,
            enrollment_token_file,
            node_name: env::var("PULSE_NODE_NAME")
                .ok()
                .filter(|name| !name.trim().is_empty())
                .or_else(System::host_name)
                .unwrap_or_else(|| "unnamed-node".to_owned()),
            region: env::var("PULSE_NODE_REGION").unwrap_or_default(),
            geoip_provider: region::Provider::parse(
                &env::var("PULSE_GEOIP_PROVIDER").unwrap_or_default(),
            )?,
            group: env::var("PULSE_NODE_GROUP").unwrap_or_default(),
            virtualization: env::var("PULSE_NODE_VIRTUALIZATION").unwrap_or_default(),
            interval: Duration::from_secs(interval_seconds),
            disk_mount_points: parse_allowlist("PULSE_DISK_MOUNT_POINTS")?,
            network_interfaces: parse_allowlist("PULSE_NETWORK_INTERFACES")?,
            extended_metrics: parse_enabled("PULSE_EXTENDED_METRICS")?,
            gpu_metrics: parse_enabled("PULSE_GPU_METRICS")?,
        };
        validate_local_text("PULSE_NODE_NAME", &config.node_name, 1, 128)?;
        validate_local_text("PULSE_NODE_REGION", &config.region, 0, 16)?;
        validate_local_text("PULSE_NODE_GROUP", &config.group, 0, 128)?;
        validate_local_text("PULSE_NODE_VIRTUALIZATION", &config.virtualization, 0, 64)?;
        if config.virtualization.trim().is_empty()
            && parse_enabled("PULSE_AUTODETECT_VIRTUALIZATION")?
        {
            config.virtualization = extended_metrics::detect_virtualization().unwrap_or_default();
        }
        Ok(config)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentCredentials {
    protocol_version: u16,
    service_url: String,
    node_id: String,
    agent_token: String,
}

struct Collector {
    system: System,
    disks: Disks,
    networks: Networks,
    last_network_refresh: Instant,
}

impl Collector {
    async fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu_all();
        tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
        system.refresh_cpu_all();

        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last_network_refresh: Instant::now(),
        }
    }

    fn collect(
        &mut self,
        config: &AgentConfig,
        extra: extended_metrics::ExtendedSnapshot,
    ) -> Result<SystemSnapshot, Box<dyn Error + Send + Sync>> {
        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.disks.refresh(true);

        let elapsed = self.last_network_refresh.elapsed();
        self.networks.refresh(true);
        self.last_network_refresh = Instant::now();

        let mut seen_disks = HashSet::new();
        let mut disk_total_bytes = 0_u64;
        let mut disk_available_bytes = 0_u64;
        for disk in self.disks.list() {
            let mount_point = disk.mount_point().to_string_lossy();
            if config
                .disk_mount_points
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(mount_point.as_ref()))
            {
                continue;
            }
            let identity = (
                disk.name().to_string_lossy().into_owned(),
                disk.total_space(),
            );
            if disk.total_space() == 0 || !seen_disks.insert(identity) {
                continue;
            }
            disk_total_bytes = disk_total_bytes.saturating_add(disk.total_space());
            disk_available_bytes = disk_available_bytes.saturating_add(disk.available_space());
        }
        let mut network_received = 0_u64;
        let mut network_transmitted = 0_u64;
        let mut network_total_received = 0_u64;
        let mut network_total_transmitted = 0_u64;
        for (name, network) in self.networks.list() {
            let selected = config.network_interfaces.as_ref().map_or_else(
                || !is_loopback_interface(name),
                |allowed| allowed.contains(name),
            );
            if !selected {
                continue;
            }
            network_received = network_received.saturating_add(network.received());
            network_transmitted = network_transmitted.saturating_add(network.transmitted());
            network_total_received =
                network_total_received.saturating_add(network.total_received());
            network_total_transmitted =
                network_total_transmitted.saturating_add(network.total_transmitted());
        }
        let load = System::load_average();
        let cpu_name = self
            .system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_owned())
            .unwrap_or_default();
        let collected_at = SystemTime::now().duration_since(UNIX_EPOCH)?;

        Ok(SystemSnapshot {
            protocol_version: PROTOCOL_VERSION,
            sample_id: Uuid::new_v4().to_string(),
            collected_at_unix_ms: collected_at.as_millis().try_into()?,
            host_name: Some(config.node_name.clone()),
            agent_version: env!("CARGO_PKG_VERSION").to_owned(),
            operating_system: System::long_os_version().unwrap_or_else(|| "Unknown".to_owned()),
            kernel_version: System::kernel_long_version(),
            architecture: System::cpu_arch(),
            cpu_name,
            cpu_cores: self.system.cpus().len().try_into()?,
            virtualization: config.virtualization.clone(),
            region: config.region.clone(),
            group: config.group.clone(),
            uptime_seconds: System::uptime(),
            cpu_usage_percent: self.system.global_cpu_usage(),
            load_one: load.one,
            load_five: load.five,
            load_fifteen: load.fifteen,
            memory_total_bytes: self.system.total_memory(),
            memory_used_bytes: self.system.used_memory(),
            swap_total_bytes: self.system.total_swap(),
            swap_used_bytes: self.system.used_swap(),
            disk_total_bytes,
            disk_used_bytes: disk_total_bytes.saturating_sub(disk_available_bytes),
            network_receive_bytes_per_second: rate_per_second(network_received, elapsed),
            network_transmit_bytes_per_second: rate_per_second(network_transmitted, elapsed),
            network_total_received_bytes: network_total_received,
            network_total_transmitted_bytes: network_total_transmitted,
            process_count: extra.process_count,
            tcp_connection_count: extra.tcp_connection_count,
            udp_connection_count: extra.udp_connection_count,
            gpus: extra.gpus,
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::fmt().compact().init();
    if run_credentials_command()? {
        return Ok(());
    }
    let config = AgentConfig::from_env()?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(1)
        .user_agent(concat!("pulse-agent/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let credentials = load_or_enroll(&client, &config).await?;
    let region = region::RegionResolver::start(&config.region, config.geoip_provider);
    let extended =
        extended_metrics::ExtendedCollector::start(config.extended_metrics, config.gpu_metrics);
    let mut probes = probes::ProbeRunner::start(
        client.clone(),
        config.service_url.clone(),
        credentials.agent_token.clone(),
        config.interval,
    )?;
    tracing::info!(node_id = %credentials.node_id, "Pulse Agent started");

    let mut collector = Collector::new().await;
    let mut ticker = interval(config.interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            changed = probes.interval.changed() => {
                changed.map_err(|_| "Agent configuration worker stopped")?;
                let configured_interval = *probes.interval.borrow_and_update();
                if ticker.period() != configured_interval {
                    ticker = interval(configured_interval);
                    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    tracing::info!(interval_seconds = configured_interval.as_secs(), "metrics interval updated");
                }
                continue;
            }
        }
        let mut snapshot = collector.collect(&config, extended.current())?;
        snapshot.region = region.current();
        match send_snapshot(
            &client,
            &config.service_url,
            &credentials.agent_token,
            &snapshot,
        )
        .await
        {
            Ok(()) => tracing::debug!("snapshot accepted"),
            Err(SnapshotSendError::Unauthorized) => {
                return Err(
                    "agent credential was rejected; follow the documented rotate or re-enroll workflow"
                        .into(),
                );
            }
            Err(SnapshotSendError::Transient(error)) => {
                tracing::warn!(%error, "snapshot delivery failed; the next interval will retry with fresh data");
            }
        }
    }
}

async fn load_or_enroll(
    client: &Client,
    config: &AgentConfig,
) -> Result<AgentCredentials, Box<dyn Error + Send + Sync>> {
    if config.credentials_path.exists() {
        ensure_credentials_are_private(&config.credentials_path)?;
        let bytes = fs::read(&config.credentials_path)?;
        let credentials: AgentCredentials = serde_json::from_slice(&bytes)?;
        if credentials.protocol_version != PROTOCOL_VERSION {
            return Err("stored credentials use an unsupported protocol version".into());
        }
        if credentials.service_url != config.service_url.as_str() {
            return Err("stored credentials belong to a different Pulse Service URL".into());
        }
        return Ok(credentials);
    }

    let enrollment_token = match (&config.enrollment_token, &config.enrollment_token_file) {
        (Some(token), None) => token.clone(),
        (None, Some(path)) => read_secret_file(path, "enrollment token")?,
        (None, None) => {
            return Err(
                "PULSE_ENROLLMENT_TOKEN or PULSE_ENROLLMENT_TOKEN_FILE is required for first enrollment"
                    .into(),
            );
        }
        (Some(_), Some(_)) => unreachable!("validated while loading Agent configuration"),
    };
    if !(16..=512).contains(&enrollment_token.len())
        || enrollment_token.chars().any(char::is_whitespace)
    {
        return Err("enrollment token is invalid".into());
    }
    // Verify private, writable storage before consuming a single-use token.
    let credential_file = prepare_credentials_file(&config.credentials_path)?;
    let endpoint = config.service_url.join("api/v1/agents/enroll")?;
    let response = client
        .post(endpoint)
        .bearer_auth(&enrollment_token)
        .json(&EnrollmentRequest {
            protocol_version: PROTOCOL_VERSION,
            node_name: config.node_name.clone(),
            agent_version: env!("CARGO_PKG_VERSION").to_owned(),
            region: config.region.clone(),
            group: config.group.clone(),
        })
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("agent enrollment failed with HTTP {}", response.status()).into());
    }
    let enrolled: EnrollmentResponse = read_json_limited(response).await?;
    let credentials = AgentCredentials {
        protocol_version: PROTOCOL_VERSION,
        service_url: config.service_url.as_str().to_owned(),
        node_id: enrolled.node_id,
        agent_token: enrolled.agent_token,
    };
    persist_credentials(credential_file, &config.credentials_path, &credentials, false)
        .map_err(|error| {
            format!(
                "enrollment succeeded for node {}; credential storage failed: {error}. Stop the Agent, rotate this node's token on the Service, then restore it with pulse-agent credentials import NODE_ID (token on stdin)",
                credentials.node_id
            )
        })?;
    tracing::info!(path = %config.credentials_path.display(), "agent credentials saved");
    Ok(credentials)
}

#[cfg(unix)]
fn ensure_credentials_are_private(path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::os::unix::fs::PermissionsExt;

    if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
        return Err("agent credentials must not be readable or writable by group or others".into());
    }
    Ok(())
}

fn read_secret_file(
    path: &Path,
    description: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    ensure_credentials_are_private(path)?;
    let mut value = String::new();
    fs::File::open(path)?.take(513).read_to_string(&mut value)?;
    let value = value.trim().to_owned();
    if value.len() > 512 {
        return Err(format!("{description} exceeds 512 bytes").into());
    }
    Ok(value)
}

#[cfg(not(unix))]
fn ensure_credentials_are_private(_path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("Pulse Agent credential storage is supported only on Unix platforms".into())
}

enum SnapshotSendError {
    Unauthorized,
    Transient(Box<dyn Error + Send + Sync>),
}

async fn send_snapshot(
    client: &Client,
    service_url: &Url,
    agent_token: &str,
    snapshot: &SystemSnapshot,
) -> Result<(), SnapshotSendError> {
    let endpoint = service_url
        .join("api/v1/agents/snapshots")
        .map_err(|error| SnapshotSendError::Transient(Box::new(error)))?;
    let response = client
        .post(endpoint)
        .bearer_auth(agent_token)
        .json(snapshot)
        .send()
        .await
        .map_err(|error| SnapshotSendError::Transient(Box::new(error)))?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(SnapshotSendError::Unauthorized);
    }
    let response = response
        .error_for_status()
        .map_err(|error| SnapshotSendError::Transient(Box::new(error)))?;
    read_json_limited::<SnapshotResponse>(response)
        .await
        .map_err(SnapshotSendError::Transient)?;
    Ok(())
}

async fn read_json_limited<T>(
    response: reqwest::Response,
) -> Result<T, Box<dyn Error + Send + Sync>>
where
    T: serde::de::DeserializeOwned,
{
    read_json_with_limit(response, MAX_RESPONSE_BYTES).await
}

async fn read_json_with_limit<T>(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<T, Box<dyn Error + Send + Sync>>
where
    T: serde::de::DeserializeOwned,
{
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("Pulse Service response exceeded its size limit".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err("Pulse Service response exceeded its size limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&body)?)
}

fn parse_service_url(value: &str) -> Result<Url, Box<dyn Error + Send + Sync>> {
    let mut url = Url::parse(value)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "PULSE_SERVICE_URL must not include credentials, a query, or a fragment".into(),
        );
    }
    if url.scheme() != "https" {
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
        if url.scheme() != "http" || !loopback {
            return Err("remote PULSE_SERVICE_URL values must use HTTPS".into());
        }
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn default_credentials_path() -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state_home).join("pulse/agent-credentials.json"));
    }
    let home = env::var_os("HOME").ok_or("HOME is not set; configure PULSE_CREDENTIALS_PATH")?;
    Ok(PathBuf::from(home).join(".local/state/pulse/agent-credentials.json"))
}

fn run_credentials_command() -> Result<bool, Box<dyn Error + Send + Sync>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.is_empty() {
        return Ok(false);
    }
    if arguments.as_slice() == ["--version"] {
        println!("pulse-agent {}", env!("CARGO_PKG_VERSION"));
        return Ok(true);
    }
    if arguments.as_slice() == ["--help"] || arguments.as_slice() == ["-h"] {
        println!("{}", agent_usage());
        return Ok(true);
    }
    let import_node = match arguments.as_slice() {
        [area, action] if area == "credentials" && action == "replace" => None,
        [area, action, node_id] if area == "credentials" && action == "import" => {
            Some(Uuid::parse_str(node_id)?.to_string())
        }
        _ => return Err(agent_usage().into()),
    };

    let credentials_path = env::var_os("PULSE_CREDENTIALS_PATH")
        .map(PathBuf::from)
        .map_or_else(default_credentials_path, Ok)?;
    let overwrite = import_node.is_none();
    let mut credentials = if let Some(node_id) = import_node {
        if credentials_path.try_exists()? {
            return Err("credentials already exist; use credentials replace for rotation".into());
        }
        AgentCredentials {
            protocol_version: PROTOCOL_VERSION,
            service_url: parse_service_url(
                &env::var("PULSE_SERVICE_URL").unwrap_or_else(|_| DEFAULT_SERVICE_URL.to_owned()),
            )?
            .to_string(),
            node_id,
            agent_token: String::new(),
        }
    } else {
        ensure_credentials_are_private(&credentials_path)?;
        serde_json::from_slice(&fs::read(&credentials_path)?)?
    };
    let credential_file = prepare_credentials_file(&credentials_path)?;
    let mut replacement = String::new();
    io::stdin().take(513).read_to_string(&mut replacement)?;
    let replacement = replacement.trim();
    if !(32..=512).contains(&replacement.len()) || replacement.chars().any(char::is_whitespace) {
        return Err("replacement agent token from stdin is invalid".into());
    }
    credentials.protocol_version = PROTOCOL_VERSION;
    replacement.clone_into(&mut credentials.agent_token);
    persist_credentials(credential_file, &credentials_path, &credentials, overwrite)?;
    println!("agent credential saved");
    Ok(true)
}

fn agent_usage() -> &'static str {
    "usage: pulse-agent [--version | credentials replace | credentials import NODE_ID]"
}

fn prepare_credentials_file(path: &Path) -> Result<NamedTempFile, Box<dyn Error + Send + Sync>> {
    let parent = path
        .parent()
        .ok_or("credentials path has no parent directory")?;
    ensure_private_directory(parent)?;
    let file = tempfile::Builder::new()
        .prefix(".agent-credentials-")
        .tempfile_in(parent)?;
    ensure_credentials_are_private(file.path())?;
    file.as_file().sync_all()?;
    sync_directory(parent)?;
    Ok(file)
}

fn persist_credentials(
    mut file: NamedTempFile,
    path: &Path,
    credentials: &AgentCredentials,
    overwrite: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    serde_json::to_writer(&mut file, credentials)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    if overwrite {
        file.persist(path)?;
    } else {
        file.persist_noclobber(path)?;
    }
    ensure_credentials_are_private(path)?;
    sync_directory(
        path.parent()
            .ok_or("credentials path has no parent directory")?,
    )?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("Pulse Agent credential storage is supported only on Unix platforms".into())
}

#[cfg(unix)]
fn ensure_private_directory(path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
        return Err("agent credential directory must have mode 0700 or stricter".into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_directory(_path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("Pulse Agent credential storage is supported only on Unix platforms".into())
}

fn parse_allowlist(name: &str) -> Result<Option<HashSet<String>>, Box<dyn Error + Send + Sync>> {
    let Ok(value) = env::var(name) else {
        return Ok(None);
    };
    let entries: HashSet<String> = value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if entries.is_empty()
        || entries
            .iter()
            .any(|entry| entry.chars().any(char::is_control))
    {
        return Err(format!("{name} must be a comma-separated list of exact names").into());
    }
    Ok(Some(entries))
}

fn parse_enabled(name: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
    match env::var(name).unwrap_or_default().trim() {
        "" | "enabled" => Ok(true),
        "disabled" => Ok(false),
        _ => Err(format!("{name} must be enabled or disabled").into()),
    }
}

fn validate_local_text(
    name: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let length = value.chars().count();
    if length < minimum || length > maximum || value.chars().any(char::is_control) {
        return Err(format!("{name} is invalid").into());
    }
    Ok(())
}

fn is_loopback_interface(name: &str) -> bool {
    name == "lo" || name == "lo0" || name.starts_with("lo:")
}

fn rate_per_second(bytes: u64, elapsed: Duration) -> u64 {
    let elapsed_millis = elapsed.as_millis().max(1);
    let bytes_per_second = u128::from(bytes).saturating_mul(1_000) / elapsed_millis;
    bytes_per_second.try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_plain_http_is_rejected() {
        assert!(parse_service_url("http://example.com:8080").is_err());
        assert!(parse_service_url("http://127.0.0.1:8080").is_ok());
        assert!(parse_service_url("http://[::1]:8080").is_ok());
        assert!(parse_service_url("http://[::2]:8080").is_err());
        assert!(parse_service_url("http://[2001:db8::1]:8080").is_err());
        assert!(parse_service_url("https://pulse.example.com").is_ok());
    }

    #[tokio::test]
    async fn invalid_credential_storage_is_rejected_before_enrollment() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("not-a-directory");
        fs::write(&parent, b"existing file").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let config = AgentConfig {
            service_url: parse_service_url(&format!("http://{}", listener.local_addr().unwrap()))
                .unwrap(),
            credentials_path: parent.join("credentials.json"),
            enrollment_token: Some("test-enrollment-token".to_owned()),
            enrollment_token_file: None,
            node_name: "test-node".to_owned(),
            region: String::new(),
            geoip_provider: region::Provider::Disabled,
            group: String::new(),
            virtualization: String::new(),
            interval: Duration::from_secs(5),
            disk_mount_points: None,
            network_interfaces: None,
            extended_metrics: false,
            gpu_metrics: false,
        };

        assert!(load_or_enroll(&Client::new(), &config).await.is_err());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(fs::read(parent).unwrap(), b"existing file");
    }
}
