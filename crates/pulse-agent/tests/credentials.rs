use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

fn command(path: &std::path::Path, arguments: &[&str], token: &str) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_pulse-agent"))
        .args(arguments)
        .env("PULSE_CREDENTIALS_PATH", path)
        .env("PULSE_SERVICE_URL", "http://[::1]:8080")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(token.as_bytes())
        .unwrap();
    process.wait_with_output().unwrap()
}

#[test]
fn lost_credentials_can_be_imported_without_overwriting_existing_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state/credentials.json");
    let node_id = uuid::Uuid::new_v4().to_string();
    let first = "test-initial-credential-00000000000000";
    let second = "test-rotated-credential-00000000000000";

    let imported = command(&path, &["credentials", "import", &node_id], first);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["node_id"], node_id);
    assert_eq!(stored["agent_token"], first);
    assert_eq!(stored["service_url"], "http://[::1]:8080/");

    let duplicate = command(&path, &["credentials", "import", &node_id], second);
    assert!(!duplicate.status.success());
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["agent_token"], first);

    let replaced = command(&path, &["credentials", "replace"], second);
    assert!(replaced.status.success());
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["node_id"], node_id);
    assert_eq!(stored["agent_token"], second);
    assert!(!String::from_utf8_lossy(&imported.stdout).contains(first));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}
