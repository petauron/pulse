//! Bounded outbound monitoring: 16 schedules, four active probes, one batch.
//! Configuration and result transport use the Service client; targets use a
//! separate credential-free client with redirects, retries and proxies disabled.

use std::{
    collections::HashSet,
    net::IpAddr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pulse_protocol::{
    AGENT_CONFIG_SCHEMA_VERSION, AgentRuntimeConfig, MAX_PROBE_CONCURRENCY, MAX_PROBE_TASKS,
    PROBE_SCHEMA_VERSION, ProbeBatch, ProbeKind, ProbeResult, ProbeTask,
};
use reqwest::{Client, StatusCode, Url};
use tokio::{
    net::{TcpStream, lookup_host},
    sync::watch,
    task::{JoinHandle, JoinSet},
};
use uuid::Uuid;

use crate::system_command::{self, SystemCommand};

const CONFIG_REFRESH: Duration = Duration::from_secs(30);
const MAX_HTTP_RESPONSE_BYTES: usize = 64 * 1024;

pub struct ProbeRunner {
    pub interval: watch::Receiver<Duration>,
    worker: JoinHandle<()>,
}

impl ProbeRunner {
    pub fn start(
        service_client: Client,
        service_url: Url,
        token: String,
        initial_interval: Duration,
    ) -> Result<Self, reqwest::Error> {
        let target_client = target_client()?;
        let (sender, interval) = watch::channel(initial_interval);
        let worker = tokio::spawn(run_worker(
            service_client,
            target_client,
            service_url,
            token,
            sender,
        ));
        Ok(Self { interval, worker })
    }
}

impl Drop for ProbeRunner {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

struct ScheduledProbe {
    task: ProbeTask,
    next_run: Instant,
}

async fn run_worker(
    service_client: Client,
    target_client: Client,
    service_url: Url,
    token: String,
    interval: watch::Sender<Duration>,
) {
    let mut schedules: Vec<ScheduledProbe> = Vec::new();
    let mut next_config = Instant::now();
    let mut active = JoinSet::new();
    let mut in_flight = HashSet::new();
    loop {
        let now = Instant::now();
        if now >= next_config {
            next_config = now + CONFIG_REFRESH;
            match fetch_config(&service_client, &service_url, &token).await {
                Ok(config) => {
                    interval.send_replace(Duration::from_secs(config.interval_seconds));
                    let enabled_tasks: Vec<_> = config
                        .probes
                        .into_iter()
                        .filter(|task| task.enabled)
                        .collect();
                    if schedules.len() != enabled_tasks.len()
                        || schedules
                            .iter()
                            .any(|scheduled| !enabled_tasks.contains(&scheduled.task))
                    {
                        active.abort_all();
                        while active.join_next().await.is_some() {}
                        in_flight.clear();
                    }
                    let old_schedules = std::mem::take(&mut schedules);
                    for task in enabled_tasks {
                        let next_run = old_schedules
                            .iter()
                            .find(|old| old.task == task)
                            .map_or(now, |old| old.next_run);
                        schedules.push(ScheduledProbe { task, next_run });
                    }
                }
                Err("unauthorized") => {
                    tracing::warn!("Agent configuration credential rejected; probe worker stopped");
                    return;
                }
                Err(reason) => tracing::warn!(reason, "Agent configuration refresh failed"),
            }
        }

        // Each completed result exists in exactly one JoinSet entry or this
        // bounded batch. Delivery failure drops it; no historical retry queue.
        let mut results = Vec::with_capacity(MAX_PROBE_CONCURRENCY);
        while let Some(completed) = active.try_join_next() {
            if let Ok(result) = completed {
                let result: ProbeResult = result;
                in_flight.remove(&result.task_id);
                if schedules
                    .iter()
                    .any(|scheduled| scheduled.task.id == result.task_id)
                {
                    results.push(result);
                }
            } else {
                // No probe should panic. Recover the bounded worker state
                // without leaving a permanently reserved schedule slot.
                active.abort_all();
                while active.join_next().await.is_some() {}
                in_flight.clear();
                tracing::warn!("probe execution task failed");
            }
        }
        if !results.is_empty() {
            let batch = ProbeBatch {
                schema_version: PROBE_SCHEMA_VERSION,
                results,
            };
            if let Err(reason) = send_results(&service_client, &service_url, &token, &batch).await {
                if reason == "unauthorized" {
                    tracing::warn!("Agent result credential rejected; probe worker stopped");
                    return;
                }
                tracing::warn!(reason, "probe result delivery failed; batch discarded");
            }
        }
        // Oldest due task gets the next slot, so slow targets cannot starve
        // later schedules when all 16 tasks compete for four slots.
        schedules.sort_by_key(|scheduled| scheduled.next_run);
        for scheduled in &mut schedules {
            if active.len() >= MAX_PROBE_CONCURRENCY {
                break;
            }
            if scheduled.next_run <= Instant::now() && !in_flight.contains(&scheduled.task.id) {
                scheduled.next_run =
                    Instant::now() + Duration::from_secs(scheduled.task.interval_seconds);
                in_flight.insert(scheduled.task.id.clone());
                active.spawn(run_probe(target_client.clone(), scheduled.task.clone()));
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn fetch_config(
    client: &Client,
    service_url: &Url,
    token: &str,
) -> Result<AgentRuntimeConfig, &'static str> {
    let endpoint = service_url
        .join("api/v1/agents/config")
        .map_err(|_| "invalid_service_url")?;
    let response = client
        .get(endpoint)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "transport")?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err("unauthorized");
    }
    if !response.status().is_success() {
        return Err("http_status");
    }
    let config: AgentRuntimeConfig = crate::read_json_with_limit(response, 64 * 1024)
        .await
        .map_err(|_| "invalid_config")?;
    validate_config(&config)?;
    Ok(config)
}

async fn send_results(
    client: &Client,
    service_url: &Url,
    token: &str,
    batch: &ProbeBatch,
) -> Result<(), &'static str> {
    let endpoint = service_url
        .join("api/v1/agents/probes")
        .map_err(|_| "invalid_service_url")?;
    let response = client
        .post(endpoint)
        .bearer_auth(token)
        .json(batch)
        .send()
        .await
        .map_err(|_| "transport")?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err("unauthorized");
    }
    if !response.status().is_success() {
        return Err("http_status");
    }
    // The acknowledgment has no fields needed by the Agent. Dropping it avoids
    // ever accumulating or reflecting Service response text.
    Ok(())
}

fn validate_config(config: &AgentRuntimeConfig) -> Result<(), &'static str> {
    if config.schema_version != AGENT_CONFIG_SCHEMA_VERSION
        || !(1..=300).contains(&config.interval_seconds)
        || config.probes.len() > MAX_PROBE_TASKS
    {
        return Err("invalid_config");
    }
    let mut ids = HashSet::new();
    for task in &config.probes {
        if Uuid::parse_str(&task.id).is_err()
            || !ids.insert(&task.id)
            || task.name.is_empty()
            || task.name.len() > 128
            || task.name.chars().any(char::is_control)
            || task.target.len() > 2048
            || !(5..=3600).contains(&task.interval_seconds)
            || !(1..=30).contains(&task.timeout_seconds)
            || task.timeout_seconds > task.interval_seconds
        {
            return Err("invalid_config");
        }
        match task.kind {
            ProbeKind::Icmp => {
                validate_host(&task.target)?;
            }
            ProbeKind::Tcp => {
                parse_tcp_target(&task.target)?;
            }
            ProbeKind::Http => {
                parse_http_target(&task.target)?;
            }
        }
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), &'static str> {
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    if host.is_empty() || host.len() > 253 {
        return Err("invalid_target");
    }
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("invalid_target");
        }
    }
    Ok(())
}

fn parse_tcp_target(target: &str) -> Result<(String, u16), &'static str> {
    let url = Url::parse(&format!("tcp://{target}")).map_err(|_| "invalid_target")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || !url.path().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_target");
    }
    let host = url
        .host_str()
        .ok_or("invalid_target")?
        .trim_matches(['[', ']']);
    validate_host(host)?;
    let port = url
        .port()
        .filter(|port| *port != 0)
        .ok_or("invalid_target")?;
    Ok((host.to_owned(), port))
}

fn parse_http_target(target: &str) -> Result<Url, &'static str> {
    let url = Url::parse(target).map_err(|_| "invalid_target")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || target.chars().any(char::is_control)
    {
        return Err("invalid_target");
    }
    Ok(url)
}

fn target_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .pool_max_idle_per_host(0)
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("pulse-probe/", env!("CARGO_PKG_VERSION")))
        .build()
}

async fn run_probe(client: Client, task: ProbeTask) -> ProbeResult {
    let started = Instant::now();
    let deadline = Duration::from_secs(task.timeout_seconds);
    let outcome = tokio::time::timeout(deadline, async {
        match task.kind {
            ProbeKind::Icmp => {
                let address = lookup_host((task.target.as_str(), 0))
                    .await
                    .map_err(|_| "dns")?
                    .next()
                    .ok_or("dns")?
                    .ip();
                let output = system_command::run(SystemCommand::Ping(address), deadline).await?;
                parse_ping_latency(&output)
                    .map(Some)
                    .ok_or("response_parse")
            }
            ProbeKind::Tcp => {
                let (host, port) = parse_tcp_target(&task.target)?;
                let addresses = lookup_host((host.as_str(), port))
                    .await
                    .map_err(|_| "dns")?;
                for address in addresses.take(8) {
                    if TcpStream::connect(address).await.is_ok() {
                        return Ok(Some(started.elapsed().as_secs_f64() * 1000.0));
                    }
                }
                Err("connect")
            }
            ProbeKind::Http => {
                let mut response = client
                    .get(parse_http_target(&task.target)?)
                    .timeout(deadline)
                    .send()
                    .await
                    .map_err(|error| {
                        if error.is_timeout() {
                            "timeout"
                        } else {
                            "connect"
                        }
                    })?;
                if !response.status().is_success() {
                    return Err("http_status");
                }
                if response
                    .content_length()
                    .is_some_and(|length| length > MAX_HTTP_RESPONSE_BYTES as u64)
                {
                    return Err("response_too_large");
                }
                let mut received = 0_usize;
                while let Some(chunk) = response.chunk().await.map_err(|_| "response_read")? {
                    received = received.saturating_add(chunk.len());
                    if received > MAX_HTTP_RESPONSE_BYTES {
                        return Err("response_too_large");
                    }
                }
                Ok(Some(started.elapsed().as_secs_f64() * 1000.0))
            }
        }
    })
    .await
    .unwrap_or(Err("timeout"));
    let (success, latency_ms, error) = match outcome {
        Ok(latency) => (true, latency, None),
        Err(reason) => (false, None, Some(reason.to_owned())),
    };
    ProbeResult {
        task_id: task.id,
        sample_id: Uuid::new_v4().to_string(),
        collected_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                duration.as_millis().try_into().unwrap_or(u64::MAX)
            }),
        latency_ms,
        success,
        error,
    }
}

fn parse_ping_latency(output: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(output).ok()?;
    for line in text.lines() {
        if let Some((_, suffix)) = line
            .split_once("time=")
            .or_else(|| line.split_once("time<"))
        {
            let value = suffix.split_whitespace().next()?.parse::<f64>().ok()?;
            if value.is_finite() && value >= 0.0 {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(kind: ProbeKind, target: &str) -> ProbeTask {
        ProbeTask {
            id: Uuid::new_v4().to_string(),
            name: "Example".to_owned(),
            kind,
            target: target.to_owned(),
            interval_seconds: 60,
            timeout_seconds: 5,
            enabled: true,
        }
    }

    #[test]
    fn rejects_command_like_targets_and_credentials() {
        for host in ["-c", "localhost;id", "$(id)", "a b", "a/b", "", "a..b"] {
            assert!(validate_host(host).is_err(), "{host}");
        }
        for target in [
            "localhost",
            "localhost:0",
            "user@localhost:80",
            "localhost:80/path",
            "localhost:80?x",
        ] {
            assert!(parse_tcp_target(target).is_err(), "{target}");
        }
        for target in [
            "file:///etc/passwd",
            "http://user:secret@localhost",
            "http://localhost/#token",
        ] {
            assert!(parse_http_target(target).is_err(), "{target}");
        }
        assert_eq!(
            parse_tcp_target("[::1]:443").unwrap(),
            ("::1".to_owned(), 443)
        );
        assert!(validate_host("example.com").is_ok());
    }

    #[test]
    fn enforces_schema_task_count_intervals_and_unique_ids() {
        let mut config = AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 1,
            probes: vec![task(ProbeKind::Tcp, "localhost:443")],
        };
        assert!(validate_config(&config).is_ok());
        config.interval_seconds = 0;
        assert!(validate_config(&config).is_err());
        config.interval_seconds = 300;
        config.probes.push(config.probes[0].clone());
        assert!(validate_config(&config).is_err());
        config.probes = (0..17)
            .map(|_| task(ProbeKind::Icmp, "127.0.0.1"))
            .collect();
        assert!(validate_config(&config).is_err());
        config.probes.truncate(1);
        config.probes[0].timeout_seconds = 31;
        assert!(validate_config(&config).is_err());
        config.schema_version = 99;
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn parses_only_finite_ping_times() {
        assert_eq!(
            parse_ping_latency(b"64 bytes from 127.0.0.1: icmp_seq=1 ttl=64 time=0.123 ms\n"),
            Some(0.123)
        );
        assert_eq!(parse_ping_latency(b"time<1 ms"), Some(1.0));
        assert_eq!(parse_ping_latency(b"time=NaN ms"), None);
        assert_eq!(parse_ping_latency(b"unrecognized output"), None);
    }

    #[tokio::test]
    async fn tcp_connect_failure_is_a_fixed_error_category() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let result = run_probe(
            target_client().unwrap(),
            task(ProbeKind::Tcp, &address.to_string()),
        )
        .await;
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("connect"));
        assert!(result.latency_ms.is_none());
    }

    async fn http_fixture(response: String) -> (String, JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 4096);
                stream.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
            }
            let _ = stream.write_all(response.as_bytes()).await;
            String::from_utf8(request).unwrap().to_ascii_lowercase()
        });
        (url, worker)
    }

    #[tokio::test]
    async fn http_requests_are_credential_free_and_redirects_fail() {
        let (url, worker) = http_fixture(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_owned(),
        )
        .await;
        let result = run_probe(target_client().unwrap(), task(ProbeKind::Http, &url)).await;
        assert!(result.success);
        assert!(result.latency_ms.is_some());
        let request = worker.await.unwrap();
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("cookie:"));

        let redirect = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (url, worker) = http_fixture(format!("HTTP/1.1 302 Found\r\nLocation: http://{}/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", redirect.local_addr().unwrap())).await;
        let result = run_probe(target_client().unwrap(), task(ProbeKind::Http, &url)).await;
        assert_eq!(result.error.as_deref(), Some("http_status"));
        worker.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), redirect.accept())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn http_caps_declared_and_streamed_response_sizes() {
        let body = "x".repeat(MAX_HTTP_RESPONSE_BYTES + 1);
        for response in [
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                body.len()
            ),
        ] {
            let (url, worker) = http_fixture(response).await;
            let result = run_probe(target_client().unwrap(), task(ProbeKind::Http, &url)).await;
            assert_eq!(result.error.as_deref(), Some("response_too_large"));
            assert!(result.latency_ms.is_none());
            worker.await.unwrap();
        }
    }

    #[tokio::test]
    async fn configuration_uses_only_the_service_bearer_credential() {
        let config = AgentRuntimeConfig {
            schema_version: AGENT_CONFIG_SCHEMA_VERSION,
            interval_seconds: 3,
            probes: vec![task(ProbeKind::Icmp, "127.0.0.1")],
        };
        let body = serde_json::to_string(&config).unwrap();
        let (url, worker) = http_fixture(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ))
        .await;
        let received = fetch_config(
            &target_client().unwrap(),
            &Url::parse(&url).unwrap(),
            "test-agent-token",
        )
        .await
        .unwrap();
        assert_eq!(received, config);
        let request = worker.await.unwrap();
        assert!(request.starts_with("get /api/v1/agents/config "));
        assert!(request.contains("authorization: bearer test-agent-token\r\n"));
    }
}
