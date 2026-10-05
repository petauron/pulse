use crate::{
    control::{NodeOptions, ProbeDefinition},
    storage::{Storage, StorageError, unix_time_ms},
};

fn sample(now: u64, up: u64, down: u64) -> pulse_protocol::SystemSnapshot {
    pulse_protocol::SystemSnapshot {
        protocol_version: PROTOCOL_VERSION,
        sample_id: Uuid::new_v4().to_string(),
        collected_at_unix_ms: now,
        host_name: Some("agent-name".into()),
        agent_version: "test".into(),
        operating_system: "Linux".into(),
        kernel_version: "6".into(),
        architecture: "x86_64".into(),
        cpu_name: "test".into(),
        cpu_cores: 1,
        virtualization: String::new(),
        region: "SG".into(),
        group: "agent-group".into(),
        uptime_seconds: 10,
        cpu_usage_percent: 10.0,
        load_one: 0.1,
        load_five: 0.1,
        load_fifteen: 0.1,
        memory_total_bytes: 1024,
        memory_used_bytes: 512,
        swap_total_bytes: 0,
        swap_used_bytes: 0,
        disk_total_bytes: 1024,
        disk_used_bytes: 512,
        network_receive_bytes_per_second: 1,
        network_transmit_bytes_per_second: 1,
        network_total_received_bytes: down,
        network_total_transmitted_bytes: up,
        process_count: Some(10),
        tcp_connection_count: Some(5),
        udp_connection_count: Some(3),
        gpus: Some(vec![pulse_protocol::GpuSnapshot {
            name: "test GPU".into(),
            usage_percent: Some(20.0),
            memory_total_bytes: Some(1024),
            memory_used_bytes: Some(128),
            temperature_celsius: Some(40.0),
        }]),
    }
}

#[test]
fn billing_deltas_reset_and_extended_history_survive_new_snapshots() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let token_hash = crate::storage::hash_token(&node.agent_token);
    storage
        .ingest(&token_hash, &sample(now, 1000, 2000), now, 7)
        .unwrap();
    storage
        .ingest(&token_hash, &sample(now + 2000, 1100, 2200), now + 2000, 7)
        .unwrap();
    let current = storage.admin_state().unwrap();
    assert_eq!(current["nodes"][0]["traffic_used_up"], 100);
    assert_eq!(current["nodes"][0]["traffic_used_down"], 200);
    storage.reset_traffic(&node.node_id).unwrap();
    storage
        .ingest(&token_hash, &sample(now + 4000, 1150, 2300), now + 4000, 7)
        .unwrap();
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        50
    );
    storage
        .ingest(&token_hash, &sample(now + 6000, 20, 30), now + 6000, 7)
        .unwrap();
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        50
    );
    let history = storage.history(&node.node_id, 1, 100, now + 7000).unwrap();
    assert_eq!(history.records[0]["connections"], 5);
    assert_eq!(history.records[0]["gpus"][0]["name"], "test GPU");
    assert_eq!(history.records[0]["gpu"], 20.0);
    let aggregate = storage.history(&node.node_id, 1, 1, now + 7000).unwrap();
    assert_eq!(aggregate.records.len(), 1);
    assert_eq!(aggregate.records[0]["gpu"], 20.0);
    assert_eq!(aggregate.records[0]["temp"], 40.0);
}

#[test]
fn traffic_correction_preserves_deltas_and_rejects_stale_baselines() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let hash = crate::storage::hash_token(&node.agent_token);
    storage
        .ingest(&hash, &sample(now, 1000, 2000), now, 7)
        .unwrap();
    let mut correction = crate::control::TrafficCorrection {
        cycle_start_ms: crate::control::period_start(now, 1),
        sampled_at_ms: now,
        raw_up: 1000,
        raw_down: 2000,
        used_up: 500,
        used_down: 800,
    };
    storage
        .ingest(&hash, &sample(now + 1000, 1100, 2200), now + 1000, 7)
        .unwrap();
    storage
        .correct_traffic(&node.node_id, &correction, now + 1000)
        .unwrap();
    storage
        .correct_traffic(&node.node_id, &correction, now + 1000)
        .unwrap();
    let state = storage.admin_state().unwrap();
    assert_eq!(state["nodes"][0]["traffic_used_up"], 600);
    assert_eq!(state["nodes"][0]["traffic_used_down"], 1000);
    storage
        .ingest(&hash, &sample(now + 2000, 1150, 2300), now + 2000, 7)
        .unwrap();
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        650
    );
    assert!(
        storage
            .correct_traffic(&node.node_id, &correction, now + 120_001)
            .is_err()
    );
    correction.cycle_start_ms = 0;
    assert!(
        storage
            .correct_traffic(&node.node_id, &correction, now + 2000)
            .is_err()
    );
    correction.cycle_start_ms = crate::control::period_start(now, 1);
    correction.raw_up = 2000;
    assert!(
        storage
            .correct_traffic(&node.node_id, &correction, now + 2000)
            .is_err()
    );
    assert_eq!(
        storage.admin_state().unwrap()["nodes"][0]["traffic_used_up"],
        650
    );
    assert!(
        storage
            .audit_events(10)
            .unwrap()
            .iter()
            .any(|event| event.action == "node.traffic.correct")
    );
}

#[test]
fn traffic_counter_decreases_rebaseline_each_direction_without_recounting() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let token_hash = crate::storage::hash_token(&node.agent_token);
    for (offset, up, down) in [
        (0, 1000, 2000),
        (1000, 1100, 2200),
        (2000, 1090, 2250),
        (3000, 1110, 2240),
        (4000, 1130, 2270),
        (5000, 5, 10),
        (6000, 15, 30),
    ] {
        storage
            .ingest(
                &token_hash,
                &sample(now + offset, up, down),
                now + offset,
                7,
            )
            .unwrap();
    }
    let current = storage.admin_state().unwrap();
    assert_eq!(current["nodes"][0]["traffic_used_up"], 150);
    assert_eq!(current["nodes"][0]["traffic_used_down"], 300);
}
use pulse_protocol::{EnrollmentRequest, PROTOCOL_VERSION, ProbeBatch, ProbeKind, ProbeResult};
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

fn fixture() -> (TempDir, Storage, pulse_protocol::EnrollmentResponse) {
    let directory = tempfile::tempdir().unwrap();
    let storage = Storage::open(&directory.path().join("pulse.db"), 64 * 1024 * 1024).unwrap();
    let now = unix_time_ms().unwrap();
    let token = storage.create_enrollment(600, now).unwrap();
    let credentials = storage
        .enroll(
            &EnrollmentRequest {
                protocol_version: PROTOCOL_VERSION,
                node_name: "example".into(),
                agent_version: "test".into(),
                region: "SG".into(),
                group: "test".into(),
            },
            &crate::storage::hash_token(&token.token),
            100,
            now,
        )
        .unwrap();
    (directory, storage, credentials)
}

#[test]
fn hidden_metadata_applies_to_all_public_read_models() {
    let (_dir, storage, node) = fixture();
    let mut options = NodeOptions {
        id: node.node_id.clone(),
        name: "managed-name".into(),
        currency: "USD".into(),
        traffic_limit_type: "sum".into(),
        traffic_reset_day: 1,
        price: 5.0,
        ..NodeOptions::default()
    };
    storage
        .save_node_options(&node.node_id, &mut options)
        .unwrap();
    let nodes = storage.native_nodes(0, 100, 90).unwrap();
    assert_eq!(nodes.1[0]["client"]["name"], "managed-name");
    assert_eq!(nodes.1[0]["client"]["price"], 5.0);
    options.hidden = true;
    storage
        .save_node_options(&node.node_id, &mut options)
        .unwrap();
    assert_eq!(storage.native_nodes(0, 100, 90).unwrap().0, 0);
    assert!(
        storage.emerald_dashboard(90, 100).unwrap()["clients"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        storage.history(&node.node_id, 1, 10, unix_time_ms().unwrap()),
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        storage.probe_history(&node.node_id, 1, unix_time_ms().unwrap()),
        Err(StorageError::NotFound)
    ));
    assert_eq!(
        storage.admin_state().unwrap()["nodes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn probes_are_assigned_idempotent_bounded_and_revocable() {
    let (_dir, storage, node) = fixture();
    let mut task = ProbeDefinition {
        id: String::new(),
        name: "local test".into(),
        kind: ProbeKind::Tcp,
        target: "127.0.0.1:8080".into(),
        interval_seconds: 5,
        timeout_seconds: 1,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut task).unwrap();
    let config = storage.agent_runtime_config(&node.agent_token).unwrap();
    assert_eq!(config.probes.len(), 1);
    assert_eq!(config.interval_seconds, 3);
    let now = unix_time_ms().unwrap();
    let mut batch = ProbeBatch {
        schema_version: 1,
        results: vec![ProbeResult {
            task_id: task.id.clone(),
            sample_id: Uuid::new_v4().to_string(),
            collected_at_unix_ms: now,
            latency_ms: Some(1.0),
            success: true,
            error: None,
        }],
    };
    storage
        .ingest_probes(&node.agent_token, &batch, now, 7)
        .unwrap();
    storage
        .ingest_probes(&node.agent_token, &batch, now, 7)
        .unwrap();
    batch.results[0].sample_id = Uuid::new_v4().to_string();
    storage
        .ingest_probes(&node.agent_token, &batch, now + 100, 7)
        .unwrap();
    batch.results[0].sample_id = Uuid::new_v4().to_string();
    assert!(matches!(
        storage.ingest_probes(&node.agent_token, &batch, now + 200, 7),
        Err(StorageError::RateLimited)
    ));
    let history = storage.probe_history(&node.node_id, 1, now + 200).unwrap();
    assert_eq!(history["summary"][0]["samples"], 2);
    assert_eq!(history["summary"][0]["loss_percent"], 0.0);
    assert!(history["tasks"][0].get("target").is_none());
    storage.revoke_node(&node.node_id, now + 1000).unwrap();
    assert!(matches!(
        storage.agent_runtime_config(&node.agent_token),
        Err(StorageError::Unauthorized)
    ));
    assert!(matches!(
        storage.ingest_probes(&node.agent_token, &batch, now + 10000, 7),
        Err(StorageError::Unauthorized)
    ));
}

#[test]
fn dashboard_probe_preview_is_bounded_assigned_and_private() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let mut task = ProbeDefinition {
        id: String::new(),
        name: "carrier".into(),
        kind: ProbeKind::Tcp,
        target: "private-probe.example:80".into(),
        interval_seconds: 30,
        timeout_seconds: 5,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut task).unwrap();
    let db = storage.connection().unwrap();
    for i in 0..25_i64 {
        db.execute(
            "INSERT INTO probe_results(node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success)
             VALUES(?1,?2,?3,?4,?4,?5,?6)",
            rusqlite::params![node.node_id, task.id, format!("point-{i}"), i64::try_from(now).unwrap() - 25000 + i * 1000, if i == 24 { None } else { Some(20.0) }, i != 24],
        ).unwrap();
    }
    // Exclude samples at the left boundary and in the future from the hour.
    for (id, time) in [("outside-hour", now - 3_600_000), ("future", now + 1)] {
        db.execute(
            "INSERT INTO probe_results(node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success) VALUES(?1,?2,?3,?4,?4,NULL,0)",
            rusqlite::params![node.node_id, task.id, id, i64::try_from(time).unwrap()],
        ).unwrap();
    }
    let tasks = vec![task.clone(); 5];
    let preview = crate::control::dashboard_probes(&db, &node.node_id, &tasks, now).unwrap();
    assert_eq!(preview.len(), 3);
    assert_eq!(preview[0]["points"].as_array().unwrap().len(), 1);
    assert_eq!(preview[0]["kind"], "tcp");
    assert_eq!(preview[0]["points"][0]["samples"], 25);
    assert_eq!(preview[0]["points"][0]["successful_samples"], 24);
    assert_eq!(preview[0]["points"][0]["loss"], 4.0);
    assert_eq!(preview[0]["points"][0]["latency"], 20.0);
    assert_eq!(preview[0]["points"][0]["bucket"], 9);
    assert!(preview[0].get("target").is_none());
    assert!(preview[0].get("node_ids").is_none());
    task.enabled = false;
    assert!(
        crate::control::dashboard_probes(&db, &node.node_id, &[task.clone()], now)
            .unwrap()
            .is_empty()
    );
    task.enabled = true;
    task.node_ids = vec!["another-node".into()];
    assert!(
        crate::control::dashboard_probes(&db, &node.node_id, &[task], now)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn migration_from_released_schema_creates_backup_and_privacy_default() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pulse.db");
    // Build the complete released v2 baseline with the same historical migration.
    let mut db = rusqlite::Connection::open(&path).unwrap();
    crate::storage::create_schema_v2(&mut db).unwrap();
    drop(db);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
    assert!(storage.settings().unwrap().private_site);
    let db = storage.connection().unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        crate::storage::SCHEMA_VERSION
    );
    assert!(
        directory
            .path()
            .read_dir()
            .unwrap()
            .filter_map(Result::ok)
            .any(|f| f.file_name().to_string_lossy().contains("backup"))
    );
}

#[test]
fn configuration_errors_cannot_create_arbitrary_probe_commands() {
    let (_dir, storage, node) = fixture();
    assert!(serde_json::from_value::<ProbeDefinition>(json!({"name":"shell","kind":"exec","target":"sh","interval_seconds":5,"timeout_seconds":1,"enabled":true,"node_ids":[]})).is_err());
    let mut task = ProbeDefinition {
        id: String::new(),
        name: "invalid".into(),
        kind: ProbeKind::Icmp,
        target: "-c 1 example.test".into(),
        interval_seconds: 5,
        timeout_seconds: 1,
        enabled: true,
        node_ids: vec![node.node_id],
    };
    assert!(storage.save_probe(&mut task).is_err());
    task.kind = ProbeKind::Tcp;
    task.target = "localhost:0".into();
    assert!(storage.save_probe(&mut task).is_err());
    task.kind = ProbeKind::Http;
    task.target = "https://secret:password@example.test/".into();
    assert!(storage.save_probe(&mut task).is_err());
}

#[test]
fn slow_reporting_extends_the_offline_window_consistently() {
    let (_dir, storage, _node) = fixture();
    storage
        .save_settings(&crate::control::Settings {
            site_name: "Pulse".into(),
            private_site: true,
            agent_interval_seconds: 300,
            ip_info_enabled: false,
            daily_exchange_rates: false,
        })
        .unwrap();
    let db = storage.connection().unwrap();
    assert_eq!(
        crate::control::effective_offline_ms(&db, 90_000).unwrap(),
        900_000
    );
    assert_eq!(
        crate::control::effective_offline_ms(&db, 1_000_000).unwrap(),
        1_000_000
    );
}

#[test]
fn deleting_the_last_assigned_node_never_turns_a_task_into_global_monitoring() {
    let (_dir, storage, node) = fixture();
    let mut probe = ProbeDefinition {
        id: String::new(),
        name: "scoped".into(),
        kind: ProbeKind::Tcp,
        target: "127.0.0.1:8080".into(),
        interval_seconds: 5,
        timeout_seconds: 1,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut probe).unwrap();
    let mut channel = crate::control::Channel {
        id: String::new(),
        name: "disabled".into(),
        kind: "webhook".into(),
        url: "https://example.invalid/".into(),
        enabled: false,
    };
    storage.save_channel(&mut channel).unwrap();
    let mut rule = crate::control::AlertRule {
        id: String::new(),
        name: "scoped rule".into(),
        node_ids: vec![node.node_id.clone()],
        metric: "offline".into(),
        threshold: 0.0,
        duration_seconds: 0,
        cooldown_seconds: 60,
        channel_ids: vec![channel.id.clone()],
        enabled: true,
    };
    storage.save_alert_rule(&mut rule).unwrap();
    storage
        .delete_node(&node.node_id, unix_time_ms().unwrap())
        .unwrap();
    let state = storage.admin_state().unwrap();
    assert_eq!(state["probes"][0]["enabled"], false);
    assert_eq!(state["alert_rules"][0]["enabled"], false);
    storage
        .delete_monitoring_config("channels", &channel.id)
        .unwrap();
    assert!(
        storage.admin_state().unwrap()["alert_rules"][0]["channel_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
