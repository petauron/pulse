//! Versioned wire types shared by Pulse Service and Pulse Agent.

use serde::{Deserialize, Serialize};

/// Current pre-release protocol version.
pub const PROTOCOL_VERSION: u16 = 2;

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
    /// Not collected by the low-memory Agent unless a future protocol version
    /// adds an explicitly bounded process collector.
    pub process_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub accepted: bool,
    pub server_time_unix_ms: u64,
    /// Server receive time minus Agent collection time.
    pub clock_skew_ms: i64,
}
