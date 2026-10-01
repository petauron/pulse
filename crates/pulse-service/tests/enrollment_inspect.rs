use std::process::Command;

use pulse_service::Administration;
use serde_json::Value;

#[test]
fn admin_cli_inspects_exact_enrollment_without_returning_a_secret() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pulse.db");
    let administration = Administration::open(&path, 64 * 1024 * 1024).unwrap();
    let enrollment = administration.create_enrollment(600).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_pulse-service"))
        .env("PULSE_DATABASE_PATH", &path)
        .args(["enrollment", "inspect", &enrollment.id])
        .output()
        .unwrap();
    assert!(output.status.success());
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record["id"], enrollment.id);
    assert_eq!(record["expires_at_unix_ms"], enrollment.expires_at_unix_ms);
    assert!(record["node_id"].is_null());
    assert!(record["consumed_at_unix_ms"].is_null());
    assert_eq!(record["node_active"], false);
    assert!(record.get("token").is_none());
    assert!(record.get("token_hash").is_none());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&enrollment.token));
    let missing = Command::new(env!("CARGO_BIN_EXE_pulse-service"))
        .env("PULSE_DATABASE_PATH", &path)
        .args(["enrollment", "inspect", "missing"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
}
