//! Opt-in, admin-only IP enrichment. No Agent command channel or plugin runtime.
use crate::{
    ApiError, AppState,
    storage::{Storage, format_timestamp},
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_CACHE: usize = 256;
const MAX_BODY: usize = 256 * 1024;
const DAY: u64 = 86_400;
const PROVIDER: &str = "https://ip.net.coffee";

pub(crate) struct IpInfo {
    client: reqwest::Client,
    inner: Mutex<Cache>,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    next_request: Option<Instant>,
    day: u64,
    calls: u32,
    minute: u64,
    minute_calls: u32,
}
#[derive(Clone)]
struct Entry {
    data: Value,
    updated: u64,
    ttl: u64,
}

impl IpInfo {
    pub(crate) fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(3))
                .no_proxy()
                .build()?,
            inner: Mutex::new(Cache::default()),
        })
    }

    async fn fetch(
        &self,
        cache: &mut Cache,
        ip: IpAddr,
        latency: bool,
        force: bool,
    ) -> Result<Value, ApiError> {
        let now = crate::current_time()? / 1000;
        let key = format!("{ip}:{latency}");
        let old = cache.entries.get(&key).cloned();
        if let Some(entry) = &old
            && !force
            && now < entry.updated + entry.ttl
        {
            return Ok(envelope(entry, now, "hit", None));
        }
        if cache.day != now / DAY {
            cache.day = now / DAY;
            cache.calls = 0;
        }
        if cache.minute != now / 60 {
            cache.minute = now / 60;
            cache.minute_calls = 0;
        }
        if cache.calls >= 200
            || cache.minute_calls >= 6
            || cache.next_request.is_some_and(|next| Instant::now() < next)
        {
            return stale_or_error(old.as_ref(), now, "IP information rate limit; retry later");
        }
        cache.calls += 1;
        cache.minute_calls += 1;
        // Set before I/O so cancellation cannot bypass the cooldown.
        cache.next_request = Some(Instant::now() + Duration::from_secs(10));
        let url = if latency {
            format!(
                "{PROVIDER}/api/ping/global?host={ip}&node=n02&node=n03&node=n04&node=n09&node=n11&node=n13"
            )
        } else {
            format!("{PROVIDER}/api/ip/lookup/{ip}")
        };
        let result = self.request(&url).await.and_then(|raw| {
            if latency {
                normalize_latency(&raw, ip)
            } else {
                normalize_lookup(&raw, ip)
            }
        });
        if let Ok(data) = result {
            let entry = Entry {
                data,
                updated: now,
                ttl: if latency { 3600 } else { DAY },
            };
            if cache.entries.len() >= MAX_CACHE
                && !cache.entries.contains_key(&key)
                && let Some(key) = cache
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.updated)
                    .map(|(key, _)| key.clone())
            {
                cache.entries.remove(&key);
            }
            let result = envelope(&entry, now, "miss", None);
            cache.entries.insert(key, entry);
            // A lookup and its following latency request are one logical operation.
            cache.next_request = None;
            Ok(result)
        } else {
            cache.next_request = Some(Instant::now() + Duration::from_secs(300));
            stale_or_error(old.as_ref(), now, "IP provider unavailable")
        }
    }

    async fn request(&self, url: &str) -> Result<Value, ApiError> {
        let mut response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(if url.contains("/ping/") {
                20
            } else {
                6
            }))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| ApiError::unavailable("IP provider unavailable"))?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BODY as u64)
        {
            return Err(ApiError::unavailable("IP response too large"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ApiError::unavailable("IP response failed"))?
        {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(ApiError::unavailable("IP response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ApiError::unavailable("invalid IP response"))
    }
}

fn envelope(entry: &Entry, now: u64, cache: &str, warning: Option<&str>) -> Value {
    json!({"ok":true,"data":entry.data,"meta":{
        "cache":cache,"stale":now >= entry.updated + entry.ttl,"warning":warning,
        "updated_at":format_timestamp(entry.updated * 1000),
        "expires_at":format_timestamp((entry.updated + entry.ttl) * 1000),
        "stale_until":format_timestamp((entry.updated + entry.ttl + DAY) * 1000)}})
}
fn stale_or_error(old: Option<&Entry>, now: u64, warning: &str) -> Result<Value, ApiError> {
    if let Some(entry) = old.filter(|entry| now < entry.updated + entry.ttl + DAY) {
        return Ok(envelope(entry, now, "stale", Some(warning)));
    }
    Err(ApiError::unavailable(warning))
}

pub(crate) fn valid_address(raw: &str, ipv6: bool) -> bool {
    raw.is_empty()
        || raw
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_ipv6() == ipv6 && public_ip(ip))
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(matches!(a, 0 | 10 | 127 | 224..=255)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && matches!(c, 0 | 2)) || (b == 88 && c == 99)))
                || (a == 198 && (matches!(b, 18 | 19) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            (0x2000..0x4000).contains(&s[0])
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] <= 0xfff)
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct Input {
    uuid: String,
    ip: String,
    #[serde(default)]
    include_latency: bool,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/public/ip-info/v1/status", get(status))
        .route("/api/admin/ip-info/v1/status", get(status))
        .route("/api/public/ip-info/v1/lookup", get(lookup))
        .route("/api/public/ip-info/v1/latency", get(latency))
        .route("/api/admin/ip-info/v1/refresh", post(refresh))
}
async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let enabled = state.database(Storage::settings).await?.ip_info_enabled;
    Ok(Json(
        json!({"ok":true,"data":{"available":enabled,"version":"pulse-1","schema_version":6,
        "mainland_china_excluded":true,"capabilities":{"geo":enabled,"network":enabled,
        "native_classification":enabled,"global_latency":enabled,"reputation":false,"media_unlock":false,"ai_unlock":false}}}),
    ))
}
async fn validate(state: &AppState, input: &Input) -> Result<IpAddr, ApiError> {
    let ip = input
        .ip
        .parse::<IpAddr>()
        .map_err(|_| ApiError::bad_request("invalid IP"))?;
    if !public_ip(ip) || uuid::Uuid::parse_str(&input.uuid).is_err() {
        return Err(ApiError::bad_request("invalid public IP or node"));
    }
    let admin = state.database(Storage::admin_state).await?;
    if admin["settings"]["ip_info_enabled"] != true {
        return Err(ApiError::bad_request("IP information is disabled"));
    }
    let node = admin["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node["id"] == input.uuid && node["disabled"] == false)
        })
        .ok_or_else(|| ApiError::bad_request("node unavailable"))?;
    if !["ipv4", "ipv6"].iter().any(|key| {
        node[key]
            .as_str()
            .and_then(|value| value.parse::<IpAddr>().ok())
            == Some(ip)
    }) {
        return Err(ApiError::bad_request("IP is not registered to this node"));
    }
    if matches!(
        node["region"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_uppercase()
            .as_str(),
        "CN" | "CHINA" | "中国" | "🇨🇳"
    ) {
        return Err(ApiError::bad_request("mainland China nodes are excluded"));
    }
    Ok(ip)
}
async fn run(
    state: &AppState,
    input: &Input,
    latency: bool,
    force: bool,
) -> Result<Value, ApiError> {
    // One bounded serialized provider lane coalesces normal duplicate requests via cache.
    let mut cache = state.ip_info.inner.lock().await;
    let ip = validate(state, input).await?;
    let mut base = state.ip_info.fetch(&mut cache, ip, false, force).await?;
    base["data"]["uuid"] = json!(input.uuid);
    if latency && base["data"]["excluded"] != true {
        let result = state.ip_info.fetch(&mut cache, ip, true, force).await;
        match result {
            Ok(mut profile) => {
                profile["data"]["uuid"] = json!(input.uuid);
                profile["data"]["classification"] = base["data"]["classification"].clone();
                profile["data"]["provider"]["classification_available"] =
                    json!(base["data"]["classification"]["type"] != "unknown");
                if !force {
                    return Ok(profile);
                }
                base["related"] = json!({"latency":profile});
            }
            Err(error) if force => {
                base["meta"]["latency_warning"] = json!(error.message);
            }
            Err(error) => return Err(error),
        }
    } else if latency && !force {
        return Err(ApiError::bad_request("mainland China IPs are excluded"));
    }
    Ok(base)
}
async fn lookup(
    State(state): State<AppState>,
    Query(input): Query<Input>,
) -> Result<Json<Value>, ApiError> {
    run(&state, &input, false, false).await.map(Json)
}
async fn latency(
    State(state): State<AppState>,
    Query(input): Query<Input>,
) -> Result<Json<Value>, ApiError> {
    run(&state, &input, true, false).await.map(Json)
}
async fn refresh(
    State(state): State<AppState>,
    Json(input): Json<Input>,
) -> Result<Json<Value>, ApiError> {
    run(&state, &input, input.include_latency, true)
        .await
        .map(Json)
}

pub(crate) async fn enrich_nodes(
    state: &AppState,
    headers: &HeaderMap,
    value: &mut Value,
) -> Result<(), ApiError> {
    if state.auth.authorize(headers, true, true).await.is_err() {
        return Ok(());
    }
    let admin = state.database(Storage::admin_state).await?;
    if admin["settings"]["ip_info_enabled"] != true {
        return Ok(());
    }
    let Some(nodes) = admin["nodes"].as_array() else {
        return Ok(());
    };
    enrich(value, nodes);
    Ok(())
}

fn enrich(value: &mut Value, nodes: &[Value]) {
    if let Some(id) = value["uuid"].as_str() {
        if let Some(node) = nodes
            .iter()
            .find(|node| node["id"] == id && node["disabled"] == false)
        {
            value["ipv4"] = node["ipv4"].clone();
            value["ipv6"] = node["ipv6"].clone();
        }
    } else if let Some(items) = value.as_object_mut() {
        for item in items.values_mut() {
            enrich(item, nodes);
        }
    } else if let Some(items) = value.as_array_mut() {
        for item in items {
            enrich(item, nodes);
        }
    }
}
fn string(raw: &Value, key: &str) -> Value {
    raw[key]
        .as_str()
        .filter(|s| s.len() <= 512)
        .map_or(Value::Null, |s| json!(s))
}
fn country(raw: &Value, key: &str) -> Value {
    raw[key]
        .as_str()
        .filter(|s| s.len() == 2 && s.bytes().all(|c| c.is_ascii_alphabetic()))
        .map_or(Value::Null, |s| json!(s.to_uppercase()))
}
fn normalize_lookup(raw: &Value, ip: IpAddr) -> Result<Value, ApiError> {
    let code = country(raw, "countryCode");
    if raw["ip"].as_str().and_then(|s| s.parse::<IpAddr>().ok()) != Some(ip)
        || raw["is_bogon"] == true
        || code.is_null()
    {
        return Err(ApiError::unavailable("invalid IP provider identity"));
    }
    let registered = country(raw, "registered_country_code");
    let verdict = raw["ai_verdict"]["label"].as_str().unwrap_or("");
    let (kind, label, source) = if verdict.contains("任播")
        || raw["public_service"]["service_type"]
            .as_str()
            .is_some_and(|v| v.eq_ignore_ascii_case("anycast"))
    {
        ("anycast", "任播 IP", "provider_verdict")
    } else if verdict.contains("广播") {
        ("broadcast", "广播 IP", "provider_verdict")
    } else if verdict.contains("原生") {
        ("native", "原生 IP", "provider_verdict")
    } else if !registered.is_null() {
        if code == registered {
            ("native", "原生 IP", "country_comparison")
        } else {
            ("broadcast", "广播 IP", "country_comparison")
        }
    } else {
        ("unknown", "未知", "unavailable")
    };
    let asn = raw["asn"].as_u64();
    let geo = raw["geo_sources"].as_array().and_then(|sources| {
        sources
            .iter()
            .find(|source| {
                source["src"] == raw["src"]
                    && source["lat"].is_number()
                    && source["lon"].is_number()
            })
            .or_else(|| {
                sources
                    .iter()
                    .find(|source| source["lat"].is_number() && source["lon"].is_number())
            })
    });
    let latitude = geo
        .and_then(|geo| geo["lat"].as_f64())
        .filter(|n| (-90.0..=90.0).contains(n));
    let longitude = geo
        .and_then(|geo| geo["lon"].as_f64())
        .filter(|n| (-180.0..=180.0).contains(n));
    Ok(
        json!({"schema_version":6,"excluded":code=="CN","excluded_reason":if code=="CN" {json!("mainland_china")} else {Value::Null},
        "address":{"value":ip.to_string(),"family":if ip.is_ipv4(){4}else{6}},
        "location":{"continent":null,"country":string(raw,"country"),"country_code":code,
            "registered_country":string(raw,"registered_country"),"registered_country_code":registered,
            "region":string(raw,"region"),"city":string(raw,"city"),"timezone":null,"latitude":latitude,"longitude":longitude},
        "network":{"asn":asn.map(|n| format!("AS{n}")),"asn_number":asn,"organization":string(raw,"asOrganization"),
            "operator":string(raw,"isp"),"network_type":string(raw,"asn_kind"),"route":string(raw,"cidr"),"rir":null,"domain":string(raw,"rdns"),"datacenter":null},
        "classification":{"type":kind,"label":label,"geolocated_country_code":code,"registered_country_code":registered,
            "confidence":raw["ai_verdict"]["confidence"].as_f64().filter(|n| (0.0..=100.0).contains(n)),"source":source},
        "reputation":{"available":false,"purity_score":null,"risk_score":null,"pollution_score":null,"risk_level":null,"pollution_level":null,
            "signals":{"proxy":null,"tor":null,"vpn":null,"datacenter":null,"abuser":null,"crawler":null},"method":{"id":"none","status":"unavailable"}},
        "capabilities":{"media_unlock":false,"ai_unlock":false},
        "provider":{"id":"net-coffee","name":"Net.Coffee","homepage":PROVIDER,"base_source":"net-coffee","security_data_available":false}}),
    )
}
fn normalize_latency(raw: &Value, ip: IpAddr) -> Result<Value, ApiError> {
    if !raw["results"].is_object() {
        return Err(ApiError::unavailable("invalid latency response"));
    }
    let mut available = 0;
    let mut timeouts = 0;
    let nodes: Vec<Value> = [
        ("n02", "HK", "香港"),
        ("n03", "JP", "东京"),
        ("n04", "SG", "新加坡"),
        ("n09", "US", "洛杉矶"),
        ("n11", "CA", "温哥华"),
        ("n13", "DE", "法兰克福"),
    ]
    .into_iter()
    .map(|(id, code, city)| {
        let ms = raw["results"][id]
            .as_f64()
            .filter(|n| (0.0..=60000.0).contains(n));
        let status = if ms.is_some() {
            available += 1;
            "ok"
        } else if raw["timeouts"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(id)))
        {
            timeouts += 1;
            "timeout"
        } else {
            "unavailable"
        };
        json!({"id":id,"name":city,"city":city,"country_code":code,"latency_ms":ms,"status":status})
    })
    .collect();
    Ok(
        json!({"schema_version":6,"address":{"value":ip.to_string(),"family":if ip.is_ipv4(){4}else{6}},
        "latency":{"nodes":nodes,"available_count":available,"timeout_count":timeouts,"provider_cached":raw["cached"]==true},
        "provider":{"id":"net-coffee","name":"Net.Coffee","homepage":PROVIDER,"latency_available":available>0,"classification_available":false}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cache_hits_do_not_consume_quota_and_force_cannot_bypass_limit() {
        let service = IpInfo::new().unwrap();
        let now = crate::current_time().unwrap() / 1000;
        let mut cache = Cache {
            day: now / DAY,
            calls: 200,
            ..Cache::default()
        };
        let ip = "1.1.1.1".parse().unwrap();
        cache.entries.insert(
            format!("{ip}:false"),
            Entry {
                data: json!({"test":true}),
                updated: now,
                ttl: 60,
            },
        );
        assert_eq!(
            service.fetch(&mut cache, ip, false, false).await.unwrap()["meta"]["cache"],
            "hit"
        );
        assert_eq!(
            service.fetch(&mut cache, ip, false, true).await.unwrap()["meta"]["cache"],
            "stale"
        );
        assert_eq!(cache.calls, 200);
        assert!(service.fetch(&mut cache, ip, true, true).await.is_err());
    }
    #[test]
    fn blocks_non_public_and_mapped_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "203.0.113.1",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2001:db8::1",
            "2002::1",
            "3fff::1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_ip(ip.parse().unwrap()));
        }
        assert!(!valid_address("8.8.8.8", true));
    }
    #[test]
    fn validates_provider_identity_and_classification() {
        let ip = "8.8.8.8".parse().unwrap();
        assert!(normalize_lookup(&json!({"ip":"1.1.1.1","countryCode":"US"}), ip).is_err());
        assert!(normalize_lookup(&json!({"ip":"8.8.8.8"}), ip).is_err());
        let result = normalize_lookup(
            &json!({"ip":"8.8.8.8","countryCode":"US","registered_country_code":"US"}),
            ip,
        )
        .unwrap();
        assert_eq!(result["classification"]["type"], "native");
        assert_eq!(result["reputation"]["available"], false);
        assert_eq!(
            normalize_lookup(&json!({"ip":"8.8.8.8","countryCode":"CN"}), ip).unwrap()["excluded"],
            true
        );
    }
    #[test]
    fn latency_does_not_invent_measurements() {
        let result = normalize_latency(
            &json!({"results":{"n02":25,"n03":-1},"timeouts":["n04"]}),
            "8.8.8.8".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(result["latency"]["available_count"], 1);
        assert_eq!(result["latency"]["timeout_count"], 1);
        assert!(result["latency"]["nodes"][1]["latency_ms"].is_null());
    }
    #[test]
    fn stale_cache_is_bounded() {
        let entry = Entry {
            data: json!({}),
            updated: 100,
            ttl: 60,
        };
        assert_eq!(
            stale_or_error(Some(&entry), 161, "offline").unwrap()["meta"]["stale"],
            true
        );
        assert!(stale_or_error(Some(&entry), 160 + DAY, "offline").is_err());
    }
}
