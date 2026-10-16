use crate::{
    control::{NodeOptions, ProbeDefinition, probe_percentile},
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
        storage
            .emerald_dashboard(90, 100, &crate::control::DashboardProbeConfig::default())
            .unwrap()["clients"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        storage.history(&node.node_id, 1, 10, unix_time_ms().unwrap()),
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        storage.probe_history(&node.node_id, None, 1, unix_time_ms().unwrap()),
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
        weight: 0,
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
    let history = storage
        .probe_history(&node.node_id, None, 1, now + 200)
        .unwrap();
    assert_eq!(history["summary"][0]["samples"], 2);
    assert_eq!(history["summary"][0]["loss_percent"], 0.0);
    assert!(history["tasks"][0].get("target").is_none());
    let selected = storage
        .probe_history(&node.node_id, Some(&task.id), 1, now + 200)
        .unwrap();
    assert_eq!(selected["records"].as_array().unwrap().len(), 2);
    assert_eq!(selected["summary"][0]["samples"], 2);
    assert!(matches!(
        storage.probe_history(&node.node_id, Some("unknown"), 1, now + 200),
        Err(StorageError::NotFound)
    ));
    let comparison = storage.probe_comparison(&task.id, 1, now + 200).unwrap();
    assert_eq!(comparison.len(), 1);
    assert_eq!(comparison[0]["node_id"], node.node_id);
    assert_eq!(comparison[0]["samples"], 2);
    assert_eq!(comparison[0]["avg_latency_ms"], 1.0);
    assert!(comparison[0].get("target").is_none());
    assert!(
        storage
            .probe_comparison(&task.id, 1, now + 2 * 3_600_000)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        storage
            .probe_comparison(&task.id, 3, now + 2 * 3_600_000)
            .unwrap()[0]["samples"],
        2
    );
    assert!(matches!(
        storage.probe_comparison("unknown", 1, now + 200),
        Err(StorageError::NotFound)
    ));
    storage.revoke_node(&node.node_id, now + 1000).unwrap();
    assert!(
        storage
            .probe_comparison(&task.id, 1, now + 1000)
            .unwrap()
            .is_empty()
    );
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
fn probe_history_statistics_cover_samples_outside_chart_limit() {
    let (_dir, storage, node) = fixture();
    let mut task = ProbeDefinition {
        id: String::new(),
        name: "percentiles".into(),
        weight: 0,
        kind: ProbeKind::Tcp,
        target: "127.0.0.1:8080".into(),
        interval_seconds: 5,
        timeout_seconds: 1,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut task).unwrap();
    let now = unix_time_ms().unwrap();
    {
        let mut db = storage.connection().unwrap();
        let tx = db.transaction().unwrap();
        for index in 0..1_002 {
            let latency = if index == 0 {
                Some(1_000.0)
            } else if index == 1 {
                None
            } else {
                Some(10.0)
            };
            tx.execute(
                "INSERT INTO metrics.probe_results (node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success,error) VALUES (?1,?2,?3,?4,?4,?5,?6,NULL)",
                rusqlite::params![node.node_id, task.id, index.to_string(), i64::try_from(now).unwrap() - 3_003 + index * 3, latency, latency.is_some()],
            ).unwrap();
        }
        tx.commit().unwrap();
    }
    let history = storage.probe_history(&node.node_id, None, 1, now).unwrap();
    let summary = &history["summary"][0];
    let recent = history["records"].as_array().unwrap();
    let chart = history["chart"].as_array().unwrap();
    assert_eq!(recent.len(), 100);
    assert!(chart.len() <= 1_000);
    assert_eq!(history["chart_source_points"], 1_002);
    assert!(history["chart_bucket_ms"].as_u64().unwrap() > 1);
    assert_eq!(history["range_end_unix_ms"], now);
    assert!(
        chart[0]["received_at_unix_ms"].as_i64().unwrap()
            < recent[99]["received_at_unix_ms"].as_i64().unwrap()
    );
    assert_eq!(
        chart
            .iter()
            .map(|point| point["samples"].as_u64().unwrap())
            .sum::<u64>(),
        1_002
    );
    assert_eq!(
        chart
            .iter()
            .map(|point| point["failed_samples"].as_u64().unwrap())
            .sum::<u64>(),
        1
    );
    assert_eq!(summary["samples"], 1_002);
    assert_eq!(summary["min_latency_ms"], 10.0);
    assert_eq!(summary["max_latency_ms"], 1_000.0);
    assert_eq!(summary["latest_latency_ms"], 10.0);
    assert_eq!(summary["p50_latency_ms"], 10.0);
    assert_eq!(summary["p95_latency_ms"], 10.0);
    assert_eq!(summary["p99_latency_ms"], 10.0);
    assert_eq!(summary["p99_p50_ratio"], 0.0);
    assert!((summary["loss_percent"].as_f64().unwrap() - 100.0 / 1_002.0).abs() < 0.001);
}

#[test]
fn probe_percentile_interpolates_and_preserves_empty_sample_state() {
    assert_eq!(probe_percentile(&[], 50), None);
    assert_eq!(probe_percentile(&[10.0], 99), Some(10.0));
    assert_eq!(probe_percentile(&[10.0, 20.0, 30.0, 40.0], 50), Some(25.0));
    assert_eq!(probe_percentile(&[10.0, 20.0, 30.0, 40.0], 95), Some(38.5));
    assert!((probe_percentile(&[10.0, 20.0, 30.0, 40.0], 99).unwrap() - 39.7).abs() < 1e-9);
}

#[test]
fn dashboard_probe_preview_is_bounded_assigned_and_private() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let mut task = ProbeDefinition {
        id: String::new(),
        name: "carrier".into(),
        weight: 0,
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
    let config = crate::control::DashboardProbeConfig {
        row_count: 3,
        ..Default::default()
    };
    let preview =
        crate::control::dashboard_probes(&db, &node.node_id, &tasks, now, &config).unwrap();
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
        crate::control::dashboard_probes(&db, &node.node_id, &[task.clone()], now, &config)
            .unwrap()
            .is_empty()
    );
    task.enabled = true;
    task.node_ids = vec!["another-node".into()];
    assert!(
        crate::control::dashboard_probes(&db, &node.node_id, &[task], now, &config)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn probe_tasks_keep_creation_order_across_admin_public_and_dashboard_views() {
    let (_dir, storage, node) = fixture();
    let mut first = ProbeDefinition {
        id: String::new(),
        name: "z-first".into(),
        weight: 0,
        kind: ProbeKind::Tcp,
        target: "127.0.0.1:8080".into(),
        interval_seconds: 60,
        timeout_seconds: 5,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut first).unwrap();
    let mut legacy = serde_json::to_value(&first).unwrap();
    legacy.as_object_mut().unwrap().remove("weight");
    let restored: ProbeDefinition = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.weight, 0);
    let mut second = ProbeDefinition {
        id: String::new(),
        name: "a-second".into(),
        ..first.clone()
    };
    storage.save_probe(&mut second).unwrap();

    let admin = storage.admin_state().unwrap();
    assert_eq!(admin["probes"][0]["id"], first.id);
    assert_eq!(admin["probes"][1]["id"], second.id);
    let public = storage.public_ping_tasks().unwrap();
    assert_eq!(public[0]["id"], first.id);
    assert_eq!(public[1]["id"], second.id);

    let db = storage.connection().unwrap();
    let tasks = crate::control::dashboard_probe_tasks(&db).unwrap();
    let preview = crate::control::dashboard_probes(
        &db,
        &node.node_id,
        &tasks,
        unix_time_ms().unwrap(),
        &crate::control::DashboardProbeConfig::default(),
    )
    .unwrap();
    assert_eq!(preview[0]["id"], first.id);
    assert_eq!(preview[1]["id"], second.id);
    drop(db);

    second.weight = 100_001;
    assert!(storage.save_probe(&mut second).is_err());
    second.weight = -1;
    storage.save_probe(&mut second).unwrap();
    let admin = storage.admin_state().unwrap();
    assert_eq!(admin["probes"][0]["id"], second.id);
    let public = storage.public_ping_tasks().unwrap();
    assert_eq!(public[0]["id"], second.id);
    assert_eq!(public[0]["weight"], -1);
    let db = storage.connection().unwrap();
    let tasks = crate::control::dashboard_probe_tasks(&db).unwrap();
    let preview = crate::control::dashboard_probes(
        &db,
        &node.node_id,
        &tasks,
        unix_time_ms().unwrap(),
        &crate::control::DashboardProbeConfig::default(),
    )
    .unwrap();
    assert_eq!(preview[0]["id"], second.id);
}

#[test]
fn dashboard_probe_preview_honors_history_task_selection_and_row_limit() {
    let (_dir, storage, node) = fixture();
    let now = unix_time_ms().unwrap();
    let mut task = ProbeDefinition {
        id: String::new(),
        name: "carrier".into(),
        weight: 0,
        kind: ProbeKind::Tcp,
        target: "private-probe.example:80".into(),
        interval_seconds: 30,
        timeout_seconds: 5,
        enabled: true,
        node_ids: vec![node.node_id.clone()],
    };
    storage.save_probe(&mut task).unwrap();
    let db = storage.connection().unwrap();
    db.execute(
        "INSERT INTO probe_results(node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success) VALUES(?1,?2,'older',?3,?3,30,1)",
        rusqlite::params![node.node_id, task.id, i64::try_from(now - 5_400_000).unwrap()],
    ).unwrap();
    let other = ProbeDefinition {
        id: uuid::Uuid::new_v4().to_string(),
        name: "other".into(),
        ..task.clone()
    };
    let configured = crate::control::DashboardProbeConfig {
        hours: 2,
        row_count: 2,
        tasks_by_node: std::collections::HashMap::from([(
            node.node_id.clone(),
            vec![other.id.clone(), task.id.clone()],
        )]),
    };
    let custom = crate::control::dashboard_probes(
        &db,
        &node.node_id,
        &[task.clone(), other.clone()],
        now,
        &configured,
    )
    .unwrap();
    assert_eq!(custom.len(), 2);
    assert_eq!(custom[0]["id"], other.id);
    assert_eq!(custom[1]["id"], task.id);
    assert_eq!(custom[1]["points"].as_array().unwrap().len(), 1);
    assert_eq!(custom[1]["points"][0]["bucket"], 2);
    assert_eq!(custom[1]["points"][0]["latency"], 30.0);
    let eight_tasks = (0..8)
        .map(|index| ProbeDefinition {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("task-{index}"),
            ..task.clone()
        })
        .collect::<Vec<_>>();
    let eight_rows = crate::control::DashboardProbeConfig {
        row_count: 8,
        ..Default::default()
    };
    assert_eq!(
        crate::control::dashboard_probes(&db, &node.node_id, &eight_tasks, now, &eight_rows)
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn dashboard_probe_theme_limits_match_retention_and_eight_rows() {
    let node = uuid::Uuid::new_v4().to_string();
    let settings = serde_json::json!({
        "homePingHistoryHours": 168,
        "homePingRowCount": 99,
        "homePingTasksByNode": { (node.clone()): ["task-8", "task-8", "task-1"] }
    });
    let config = crate::control::DashboardProbeConfig::from_theme(&settings, 1);
    assert_eq!(config.hours, 24);
    assert_eq!(config.row_count, 8);
    assert_eq!(
        config.tasks_by_node[&node],
        vec!["task-8".to_owned(), "task-1".to_owned()]
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
        weight: 0,
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
fn emerald_header_mode_defaults_for_existing_settings_and_rejects_unknown_modes() {
    let old_settings = serde_json::json!({
        "site_name": "Senulun",
        "private_site": true,
        "agent_interval_seconds": 3
    });
    let settings: crate::control::Settings = serde_json::from_value(old_settings).unwrap();
    assert_eq!(
        serde_json::to_value(settings.earth_view_mode).unwrap(),
        serde_json::json!("earth")
    );
    for mode in ["earth", "earth-stop", "maps", "cards", "hide"] {
        let settings: crate::control::Settings = serde_json::from_value(serde_json::json!({
            "earth_view_mode": mode
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(settings.earth_view_mode).unwrap(),
            serde_json::json!(mode)
        );
    }
    assert!(
        serde_json::from_value::<crate::control::Settings>(serde_json::json!({
            "earth_view_mode": "unsupported"
        }))
        .is_err()
    );
}

#[test]
fn slow_reporting_extends_the_offline_window_consistently() {
    let (_dir, storage, _node) = fixture();
    storage
        .save_settings(&crate::control::Settings {
            site_name: "Senulun".into(),
            private_site: true,
            agent_interval_seconds: 300,
            ip_info_enabled: false,
            daily_exchange_rates: false,
            earth_view_mode: crate::control::EarthViewMode::Earth,
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
        weight: 0,
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
