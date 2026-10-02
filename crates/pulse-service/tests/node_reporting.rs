use std::{
    io::Write,
    process::{Command, Stdio},
};

use pulse_service::Administration;
use rusqlite::{Connection, params};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn reporting_cli_authenticates_stdin_without_returning_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pulse.db");
    let administration = Administration::open(&path, 64 * 1024 * 1024).unwrap();
    let node_id = "11111111-1111-4111-8111-111111111111";
    let old_token = "test-original-credential-never-publish";
    Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO nodes(id,token_hash,name,agent_version,created_at_ms,updated_at_ms) VALUES(?1,?2,'original','test',1,1)",
            params![node_id, hex::encode(Sha256::digest(old_token.as_bytes()))],
        )
        .unwrap();
    let token = administration.rotate_node_token(node_id).unwrap();
    for input in [
        format!("{token}\n"),
        old_token.to_owned(),
        "short".to_owned(),
        "x".repeat(1024),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pulse-service"))
            .env("PULSE_DATABASE_PATH", &path)
            .args(["node", "reporting", node_id])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&token));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(&token));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(old_token));
        if input == format!("{token}\n") {
            assert!(output.status.success());
            let record: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(record["node_id"], node_id);
            assert!(record["rotated_at_unix_ms"].as_u64().is_some());
            assert!(record["last_seen_at_unix_ms"].is_null());
            assert!(record["observed_at_unix_ms"].as_u64().is_some());
            assert!(record.get("token").is_none());
        } else {
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
        }
    }
}
