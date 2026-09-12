//! Versioned wire types shared by Pulse Service and Pulse Agent.

use serde::{Deserialize, Serialize};

/// Released enrollment/credentials/snapshot protocol. Optional snapshot metrics
/// below are an additive v2 extension; existing v2 credentials remain valid.
pub const PROTOCOL_VERSION: u16 = 2;

pub const AGENT_CONFIG_SCHEMA_VERSION: u16 = 1;
pub const PROBE_SCHEMA_VERSION: u16 = 1;
pub const MAX_PROBE_TASKS: usize = 16;
pub const MAX_PROBE_CONCURRENCY: usize = 4;
pub const MAX_GPU_DEVICES: usize = 16;

/// Administrator-selected monitoring configuration, fetched by the Agent.
/// This is deliberately a closed set of probes, never a command channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRuntimeConfig {
    pub schema_version: u16,
    pub interval_seconds: u64,
    pub probes: Vec<ProbeTask>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    Icmp,
    Tcp,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTask {
    pub id: String,
    pub name: String,
    pub kind: ProbeKind,
    pub target: String,
    pub interval_seconds: u64,
    pub timeout_seconds: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeBatch {
    pub schema_version: u16,
    pub results: Vec<ProbeResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeResult {
    pub task_id: String,
    pub sample_id: String,
    pub collected_at_unix_ms: u64,
    pub latency_ms: Option<f64>,
    pub success: bool,
    /// Fixed error category only: never target response bodies or credentials.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuSnapshot {
    pub name: String,
    pub usage_percent: Option<f32>,
    pub memory_total_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub temperature_celsius: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentRequest {
    pub protocol_version: u16,
    pub node_name: String,
    pub agent_version: String,
    pub region: String,
    pub group: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    pub node_id: String,
    pub agent_token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemSnapshot {
    pub protocol_version: u16,
    /// Agent-generated idempotency key. Timestamps are diagnostic data and are
    /// intentionally not used as database ordering or deduplication keys.
    pub sample_id: String,
    pub collected_at_unix_ms: u64,
    pub host_name: Option<String>,
    pub agent_version: String,
    pub operating_system: String,
    pub kernel_version: String,
    pub architecture: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub virtualization: String,
    pub region: String,
    pub group: String,
    pub uptime_seconds: u64,
    pub cpu_usage_percent: f32,
    pub load_one: f64,
    pub load_five: f64,
    pub load_fifteen: f64,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub network_receive_bytes_per_second: u64,
    pub network_transmit_bytes_per_second: u64,
    pub network_total_received_bytes: u64,
    pub network_total_transmitted_bytes: u64,
    /// Optional additive v2 metrics. Missing capabilities are null, not zero.
    #[serde(default)]
    pub process_count: Option<u64>,
    /// All TCP sockets in the Agent's network namespace, including listeners.
    #[serde(default)]
    pub tcp_connection_count: Option<u64>,
    /// All UDP sockets in the Agent's network namespace, including unconnected.
    #[serde(default)]
    pub udp_connection_count: Option<u64>,
    #[serde(default)]
    pub gpus: Option<Vec<GpuSnapshot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub accepted: bool,
    pub server_time_unix_ms: u64,
    /// Server receive time minus Agent collection time.
    pub clock_skew_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_v2_snapshots_decode_without_additive_metrics() {
        let snapshot: SystemSnapshot = serde_json::from_value(serde_json::json!({
            "protocol_version": 2,
            "sample_id": "existing-v2-sample",
            "collected_at_unix_ms": 1,
            "host_name": "existing-v2-node",
            "agent_version": "0.1.0-alpha.2",
            "operating_system": "Linux",
            "kernel_version": "6.8",
            "architecture": "x86_64",
            "cpu_name": "Example CPU",
            "cpu_cores": 1,
            "virtualization": "",
            "region": "",
            "group": "",
            "uptime_seconds": 1,
            "cpu_usage_percent": 0,
            "load_one": 0,
            "load_five": 0,
            "load_fifteen": 0,
            "memory_total_bytes": 1,
            "memory_used_bytes": 0,
            "swap_total_bytes": 0,
            "swap_used_bytes": 0,
            "disk_total_bytes": 1,
            "disk_used_bytes": 0,
            "network_receive_bytes_per_second": 0,
            "network_transmit_bytes_per_second": 0,
            "network_total_received_bytes": 0,
            "network_total_transmitted_bytes": 0,
            "process_count": null
        }))
        .unwrap();
        assert_eq!(snapshot.protocol_version, PROTOCOL_VERSION);
        assert!(snapshot.tcp_connection_count.is_none());
        assert!(snapshot.udp_connection_count.is_none());
        assert!(snapshot.gpus.is_none());
    }

    #[test]
    fn probe_kinds_form_a_closed_set() {
        assert_eq!(
            serde_json::from_str::<ProbeKind>("\"icmp\"").unwrap(),
            ProbeKind::Icmp
        );
        assert!(serde_json::from_str::<ProbeKind>("\"exec\"").is_err());
        assert!(serde_json::from_str::<ProbeKind>("\"shell\"").is_err());
    }
}
