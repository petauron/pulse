use std::{
    error::Error,
    time::{SystemTime, UNIX_EPOCH},
};

use pulse_protocol::{PROTOCOL_VERSION, SystemSnapshot};
use sysinfo::System;

fn main() -> Result<(), Box<dyn Error>> {
    let mut system = System::new_all();
    system.refresh_all();

    let collected_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    let snapshot = SystemSnapshot {
        protocol_version: PROTOCOL_VERSION,
        collected_at_unix_ms,
        host_name: System::host_name(),
        uptime_seconds: System::uptime(),
        cpu_usage_percent: system.global_cpu_usage(),
        memory_total_bytes: system.total_memory(),
        memory_used_bytes: system.used_memory(),
    };

    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}
