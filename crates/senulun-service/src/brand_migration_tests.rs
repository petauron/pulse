//! Synthetic fixtures only; never open deployed storage or credentials.

use crate::storage::{SCHEMA_VERSION, Storage};
use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn schema_four_fixture(directory: &Path, title: &str) -> PathBuf {
    let path = directory.join("senulun.db");
    let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
    let db = storage.connection().unwrap();
    // Schema 5 only updates one setting. The schema-4 shape is identical.
    db.execute(
        "UPDATE monitoring_settings SET data=json_set(data,'$.site_name',?1,'$.private_site',json('false'))",
        [title],
    )
    .unwrap();
    db.execute_batch(
        "INSERT INTO nodes(id,token_hash,name,agent_version,created_at_ms,updated_at_ms)
         VALUES('node-a','credential-hash','example','test',10,20);
         INSERT INTO metrics.traffic_periods VALUES('node-a',0,800,700,80,70);
         INSERT INTO auth_admin(id,username,password_hash,auth_version,totp_secret)
         VALUES(1,'admin','test-password-hash',2,X'0102');
         INSERT INTO auth_sessions VALUES('test-session-hash',1,2,10,999999,1);
         DROP TABLE external_traffic_sources;
         PRAGMA main.user_version=4;",
    )
    .unwrap();
    drop(db);
    drop(storage);
    path
}

fn migration_backups(directory: &Path) -> Vec<PathBuf> {
    directory
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir() && path.join("manifest.json").is_file())
        .collect()
}

#[test]
fn brand_migration_backs_up_pair_and_preserves_custom_titles_and_identity() {
    for (before, expected) in [
        ("Pulse", "Senulun"),
        ("My Pulse", "My Pulse"),
        ("Custom monitor", "Custom monitor"),
        ("Senulun", "Senulun"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = schema_four_fixture(directory.path(), before);
        let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
        let settings = storage.settings().unwrap();
        assert_eq!(settings.site_name, expected);
        assert!(!settings.private_site);
        let db = storage.connection().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );
        assert!(db
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='external_traffic_sources'",
                [],
                |r| r.get::<_, String>(0)
            )
            .is_ok());
        assert_eq!(
            db.query_row("SELECT token_hash FROM nodes WHERE id='node-a'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "credential-hash"
        );
        assert_eq!(
            db.query_row(
                "SELECT used_up+used_down FROM metrics.traffic_periods WHERE node_id='node-a'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            150
        );
        assert_eq!(
            db.query_row("SELECT token_hash FROM auth_sessions", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "test-session-hash"
        );
        drop(db);
        drop(storage);
        let backups = migration_backups(directory.path());
        assert_eq!(backups.len(), 1);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(backups[0].join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["control_schema"], 4);
        assert!(backups[0].join("senulun.metrics.db").is_file());
        let original = Connection::open(backups[0].join("senulun.db")).unwrap();
        assert_eq!(
            original
                .query_row(
                    "SELECT json_extract(data,'$.site_name') FROM monitoring_settings",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            before
        );
        let reopened = Storage::open(&path, 64 * 1024 * 1024).unwrap();
        assert_eq!(reopened.settings().unwrap().site_name, expected);
        assert_eq!(migration_backups(directory.path()).len(), 1);
    }
}

#[test]
fn external_traffic_migration_from_five_preserves_existing_state_and_backs_up_pair() {
    let directory = tempfile::tempdir().unwrap();
    let path = schema_four_fixture(directory.path(), "Senulun");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA user_version=5;").unwrap();
    drop(db);

    let storage = Storage::open(&path, 64 * 1024 * 1024).unwrap();
    let db = storage.connection().unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    assert_eq!(
        db.query_row("SELECT token_hash FROM nodes WHERE id='node-a'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "credential-hash"
    );
    assert_eq!(
        db.query_row(
            "SELECT used_up+used_down FROM metrics.traffic_periods WHERE node_id='node-a'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        150
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM external_traffic_sources", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    drop(db);
    drop(storage);

    let backups = migration_backups(directory.path());
    assert_eq!(backups.len(), 1);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(backups[0].join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["control_schema"], 5);
    assert!(backups[0].join("senulun.metrics.db").is_file());
}

#[test]
fn external_traffic_migration_failure_keeps_schema_five() {
    let directory = tempfile::tempdir().unwrap();
    let path = schema_four_fixture(directory.path(), "Senulun");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE external_traffic_sources (collision INTEGER); PRAGMA user_version=5;",
    )
    .unwrap();
    drop(db);

    assert!(Storage::open(&path, 64 * 1024 * 1024).is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        db.query_row("SELECT token_hash FROM nodes WHERE id='node-a'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "credential-hash"
    );
    assert_eq!(migration_backups(directory.path()).len(), 1);
}

#[test]
fn brand_migration_failure_leaves_old_schema_and_title() {
    let directory = tempfile::tempdir().unwrap();
    let path = schema_four_fixture(directory.path(), "Pulse");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_brand BEFORE UPDATE ON monitoring_settings
        BEGIN SELECT RAISE(ABORT,'fixture migration failure'); END;",
    )
    .unwrap();
    drop(db);
    assert!(Storage::open(&path, 64 * 1024 * 1024).is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.query_row(
            "SELECT json_extract(data,'$.site_name') FROM monitoring_settings",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Pulse"
    );
    assert_eq!(migration_backups(directory.path()).len(), 1);
}

#[test]
fn brand_migration_rejects_missing_metrics_without_replacing_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = schema_four_fixture(directory.path(), "Pulse");
    let metrics = directory.path().join("senulun.metrics.db");
    std::fs::rename(&metrics, directory.path().join("saved.metrics.db")).unwrap();
    assert!(Storage::open(&path, 64 * 1024 * 1024).is_err());
    assert!(!metrics.exists());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
}
