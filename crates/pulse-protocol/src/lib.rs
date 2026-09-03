//! Versioned wire types shared by Pulse Service and Pulse Agent.

use serde::{Deserialize, Serialize};

/// Current pre-release protocol version.
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub protocol_version: u16,
    pub collected_at_unix_ms: u64,
    pub host_name: Option<String>,
    pub uptime_seconds: u64,
    pub cpu_usage_percent: f32,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
}
