//! Default-on, opt-out country discovery. Never shares Pulse credentials or metrics.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tokio::{sync::watch, task::JoinHandle};

const REFRESH_INTERVAL: Duration = Duration::from_hours(24);
const RETRY_INTERVAL: Duration = Duration::from_mins(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Disabled,
    Ipinfo,
    Geojs,
}

impl Provider {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value.trim() {
            "disabled" => Ok(Self::Disabled),
            "ipinfo" => Ok(Self::Ipinfo),
            "" | "geojs" => Ok(Self::Geojs),
            _ => Err("PULSE_GEOIP_PROVIDER must be disabled, ipinfo, or geojs"),
        }
    }

    fn endpoint(self) -> Option<&'static str> {
        match self {
            Self::Disabled => None,
            Self::Ipinfo => Some("https://ipinfo.io/json"),
            Self::Geojs => Some("https://get.geojs.io/v1/ip/geo.json"),
        }
    }
}

/// One task, one in-flight request, and one cached country. No sample queue.
pub struct RegionResolver {
    current: watch::Receiver<String>,
    worker: Option<JoinHandle<()>>,
}

impl RegionResolver {
    pub fn start(manual_region: &str, provider: Provider) -> Self {
        let manual_region = manual_region.trim();
        let (sender, current) = watch::channel(manual_region.to_owned());
        let worker = provider
            .endpoint()
            .filter(|_| manual_region.is_empty())
            .map(|endpoint| {
                tracing::info!(
                    provider = ?provider,
                    "automatic country discovery enabled; provider sees public egress IP and Agent version, not Pulse credentials or metrics; set PULSE_GEOIP_PROVIDER=disabled to opt out"
                );
                tokio::spawn(async move {
                    loop {
                        let delay = update_region(&sender, lookup(endpoint).await);
                        tokio::time::sleep(delay).await;
                    }
                })
            });
        if manual_region.is_empty() && provider == Provider::Disabled {
            tracing::warn!(
                "automatic country discovery is disabled and node region is empty; set PULSE_NODE_REGION or enable PULSE_GEOIP_PROVIDER for globe placement"
            );
        }
        Self { current, worker }
    }

    pub fn current(&self) -> String {
        self.current.borrow().clone()
    }
}

fn update_region(sender: &watch::Sender<String>, result: Result<String, &'static str>) -> Duration {
    match result {
        Ok(country) => {
            if *sender.borrow() != country {
                tracing::info!(region = %country, "automatic node region updated");
            }
            sender.send_replace(country);
            REFRESH_INTERVAL
        }
        Err(reason) => {
            // Do not log provider bodies, URLs containing IPs, or transport errors.
            tracing::warn!(
                reason,
                "automatic region lookup failed; retaining the last region and retrying in 15 minutes"
            );
            RETRY_INTERVAL
        }
    }
}

impl Drop for RegionResolver {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

fn client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(REQUEST_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(0)
        .user_agent(concat!("pulse-agent/", env!("CARGO_PKG_VERSION")))
}

async fn lookup(endpoint: &str) -> Result<String, &'static str> {
    let client = client_builder()
        .build()
        .map_err(|_| "could not initialize GeoIP HTTPS client")?;
    lookup_with_client(&client, endpoint).await
}

async fn lookup_with_client(client: &Client, endpoint: &str) -> Result<String, &'static str> {
    let mut response = client
        .get(endpoint)
        .send()
        .await
        .map_err(|_| "GeoIP request failed")?;
    if !response.status().is_success() {
        return Err("GeoIP provider returned a non-success HTTP status");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("GeoIP response exceeds 8 KiB");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "GeoIP response read failed")?
    {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            return Err("GeoIP response exceeds 8 KiB");
        }
        body.extend_from_slice(&chunk);
    }
    parse_country(&body)
}

fn parse_country(body: &[u8]) -> Result<String, &'static str> {
    // GeoJS also has a human-readable `country` field, so decode its country_code
    // explicitly rather than treating the two JSON keys as aliases.
    #[derive(Deserialize)]
    struct GeoCountry {
        country_code: Option<String>,
        country: Option<String>,
    }
    let record: GeoCountry = serde_json::from_slice(body).map_err(|_| "invalid GeoIP JSON")?;
    let country = record
        .country_code
        .or(record.country)
        .ok_or("GeoIP country is missing")?;
    let country = country.trim().to_ascii_uppercase();
    if country.len() != 2
        || !country.bytes().all(|byte| byte.is_ascii_uppercase())
        || country == "XX"
        || country == "ZZ"
    {
        return Err("GeoIP provider returned an invalid country code");
    }
    Ok(country)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_default_to_geojs_and_allow_explicit_opt_out() {
        assert_eq!(Provider::parse("").unwrap(), Provider::Geojs);
        assert_eq!(Provider::parse("  ").unwrap(), Provider::Geojs);
        assert_eq!(Provider::parse("disabled").unwrap().endpoint(), None);
        assert_eq!(Provider::parse("ipinfo").unwrap(), Provider::Ipinfo);
        assert_eq!(Provider::parse("geojs").unwrap(), Provider::Geojs);
        assert_eq!(
            Provider::parse("").unwrap().endpoint(),
            Some("https://get.geojs.io/v1/ip/geo.json")
        );
        assert!(Provider::parse("https://arbitrary.example").is_err());
        assert!(Provider::parse("auto").is_err());
    }

    #[test]
    fn manual_region_and_disabled_mode_never_start_a_worker() {
        // This runs without a Tokio runtime: spawning any task would panic.
        let manual = RegionResolver::start(" US ", Provider::Ipinfo);
        assert_eq!(manual.current(), "US");
        assert!(manual.worker.is_none());
        let default_manual = RegionResolver::start("US", Provider::parse("").unwrap());
        assert_eq!(default_manual.current(), "US");
        assert!(default_manual.worker.is_none());
        let disabled = RegionResolver::start("", Provider::Disabled);
        assert_eq!(disabled.current(), "");
        assert!(disabled.worker.is_none());
    }

    #[test]
    fn decodes_both_providers_without_retaining_ip_or_city() {
        assert_eq!(
            parse_country(br#"{"country":"US","ip":"192.0.2.1","city":"Example"}"#).unwrap(),
            "US"
        );
        assert_eq!(
            parse_country(br#"{"country":"United States","country_code":"US"}"#).unwrap(),
            "US"
        );
        assert_eq!(parse_country(br#"{"country":" sg "}"#).unwrap(), "SG");
        assert_eq!(parse_country(br#"{"country_code":"HK"}"#).unwrap(), "HK");
    }

    #[test]
    fn rejects_missing_malformed_or_unknown_country() {
        for body in [
            "{}",
            "null",
            "not json",
            "<html>login</html>",
            r#"{"country":"United States"}"#,
            r#"{"country":""}"#,
            r#"{"country":"ZZ"}"#,
            r#"{"country":"XX"}"#,
            r#"{"country":"U1"}"#,
            r#"{"country":42}"#,
        ] {
            assert!(parse_country(body.as_bytes()).is_err(), "{body}");
        }
    }

    #[test]
    fn failures_preserve_last_success_and_use_bounded_retry() {
        let (sender, current) = watch::channel(String::new());
        assert_eq!(update_region(&sender, Err("offline")), RETRY_INTERVAL);
        assert_eq!(*current.borrow(), "");
        assert_eq!(
            update_region(&sender, Ok("US".to_owned())),
            REFRESH_INTERVAL
        );
        assert_eq!(*current.borrow(), "US");
        assert_eq!(update_region(&sender, Err("rate limited")), RETRY_INTERVAL);
        assert_eq!(*current.borrow(), "US");
        assert_eq!(
            update_region(&sender, Ok("JP".to_owned())),
            REFRESH_INTERVAL
        );
        assert_eq!(*current.borrow(), "JP");
    }

    #[tokio::test]
    async fn dropping_resolver_cancels_its_worker() {
        let (_sender, current) = watch::channel(String::new());
        let worker = tokio::spawn(std::future::pending::<()>());
        let handle = worker.abort_handle();
        drop(RegionResolver {
            current,
            worker: Some(worker),
        });
        tokio::task::yield_now().await;
        assert!(handle.is_finished());
    }

    // Local HTTP fixtures only: these tests never contact a GeoIP provider.
    async fn mock_lookup(response: String) -> Result<String, &'static str> {
        mock_lookup_with_delay(response, Duration::ZERO, Duration::from_secs(2)).await
    }

    async fn mock_lookup_with_delay(
        response: String,
        delay: Duration,
        timeout: Duration,
    ) -> Result<String, &'static str> {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::Instant,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            started.elapsed() < Duration::from_secs(5),
                            "mock was not contacted"
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("mock accept failed: {error}"),
                }
            };
            // macOS may inherit the listener's nonblocking flag on accepted sockets.
            // The fixture reads synchronously with an explicit bounded timeout.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0_u8];
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 4096);
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(!request.contains("authorization:"));
            assert!(!request.contains("cookie:"));
            std::thread::sleep(delay);
            // The client is allowed to reject headers before reading the body.
            let _ = stream.write_all(response.as_bytes());
        });
        let client = client_builder()
            .https_only(false)
            .timeout(timeout)
            .build()
            .unwrap();
        let result = lookup_with_client(&client, &endpoint).await;
        server.join().unwrap();
        result
    }

    #[tokio::test]
    async fn reads_country_from_bounded_http_response() {
        let result = mock_lookup(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"country\":\"US\"}".to_owned(),
        )
        .await;
        assert_eq!(result.unwrap(), "US");
    }

    #[tokio::test]
    async fn rejects_redirects_and_provider_errors() {
        for status in [
            "302 Found",
            "429 Too Many Requests",
            "503 Service Unavailable",
        ] {
            assert_eq!(mock_lookup(format!("HTTP/1.1 {status}\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")).await.unwrap_err(), "GeoIP provider returned a non-success HTTP status");
        }
    }

    #[tokio::test]
    async fn caps_declared_and_streamed_bodies() {
        assert_eq!(
            mock_lookup(format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_RESPONSE_BYTES + 1
            ))
            .await
            .unwrap_err(),
            "GeoIP response exceeds 8 KiB"
        );
        let body = "x".repeat(MAX_RESPONSE_BYTES + 1);
        assert_eq!(mock_lookup(format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len())).await.unwrap_err(), "GeoIP response exceeds 8 KiB");
    }

    #[tokio::test]
    async fn production_client_rejects_plain_http() {
        let client = client_builder().build().unwrap();
        assert!(
            lookup_with_client(&client, "http://127.0.0.1:1/")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn slow_provider_times_out_without_publishing_a_region() {
        let result = mock_lookup_with_delay(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"country\":\"US\"}".to_owned(),
            Duration::from_secs(2),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(result.unwrap_err(), "GeoIP request failed");
    }
}
