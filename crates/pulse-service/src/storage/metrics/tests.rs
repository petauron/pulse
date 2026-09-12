//! Isolated migration fixtures; never open a production database.

use super::*;
use rusqlite::params;
use tempfile::{TempDir, tempdir};

fn legacy_store() -> (TempDir, PathBuf, Connection) {
    let directory = tempdir().unwrap();
    let control = directory.path().join("pulse.db");
    prepare_database_path(&control).unwrap();
    let mut db = Connection::open(&control).unwrap();
    super::super::create_schema_v2(&mut db).unwrap();
    {
        let tx = db.transaction().unwrap();
        crate::control::migrate(&tx).unwrap();
        crate::auth::migrate(&tx).unwrap();
        tx.pragma_update(None, "user_version", 3).unwrap();
        tx.commit().unwrap();
    }
    db.execute_batch(
        "PRAGMA foreign_keys=ON;
         INSERT INTO nodes(id,token_hash,name,agent_version,created_at_ms,updated_at_ms,last_seen_at_ms)
         VALUES('node-a','credential-hash','example','test',10,20,1000);
         INSERT INTO probe_tasks VALUES('task-a','{}');
         INSERT INTO auth_admin(id,username,password_hash,auth_version,totp_secret)
         VALUES(1,'admin','test-password-hash',2,X'0102');
         INSERT INTO auth_sessions VALUES('test-session-hash',1,2,10,999999,1);
         INSERT INTO snapshots VALUES(
            7,'node-a','sample-a',999,1000,123,12.5,1,2,3,
            100,50,0,0,1000,500,5,6,700,800,NULL,'{\"gpus\":[]}');
         INSERT INTO probe_results VALUES(4,'node-a','task-a','probe-a',990,1000,5.5,1,NULL);
         INSERT INTO traffic_periods VALUES('node-a',0,800,700,80,70);",
    ).unwrap();
    online_backup(&db, &directory.path().join("pre-split.db")).unwrap();
    let metrics = path(&control).unwrap();
    prepare_database_path(&metrics).unwrap();
    db.execute("ATTACH DATABASE ?1 AS metrics", [metrics.to_str().unwrap()])
        .unwrap();
    (directory, control, db)
}

#[test]
fn split_preserves_all_metric_fields_and_credentials() {
    let (_directory, _path, mut db) = legacy_store();
    split(&mut db).unwrap();
    validate_pair(&db).unwrap();
    assert_eq!(
        db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.query_row("PRAGMA metrics.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let old_tables: i64 = db.query_row(
        "SELECT count(*) FROM main.sqlite_schema WHERE name IN ('snapshots','probe_results','traffic_periods')",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(old_tables, 0);
    let token: String = db
        .query_row("SELECT token_hash FROM nodes WHERE id='node-a'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(token, "credential-hash");
    let account: (String, Vec<u8>) = db
        .query_row(
            "SELECT password_hash,totp_secret FROM auth_admin WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(account, ("test-password-hash".into(), vec![1, 2]));
    assert_eq!(
        db.query_row("SELECT token_hash FROM auth_sessions", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "test-session-hash"
    );
    let row: (i64, String, i64, Option<i64>, String) = db.query_row(
        "SELECT id,sample_id,network_total_received_bytes,process_count,extensions FROM snapshots",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).unwrap();
    assert_eq!(
        row,
        (7, "sample-a".into(), 700, None, "{\"gpus\":[]}".into())
    );
    let state: i64 = db
        .query_row(
            "SELECT last_seen_at_ms FROM node_state WHERE node_id='node-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, 1000);
    let traffic: (i64, i64) = db
        .query_row("SELECT used_up,used_down FROM traffic_periods", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(traffic, (80, 70));
    assert!(
        db.execute("INSERT INTO probe_results SELECT * FROM probe_results", [])
            .is_err()
    );
    assert!(
        db.execute("INSERT INTO snapshots SELECT * FROM snapshots", [])
            .is_err()
    );
    let sequences: i64 = db
        .query_row(
            "SELECT count(*) FROM metrics.sqlite_schema WHERE name='sqlite_sequence'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        sequences, 0,
        "metrics must not update an AUTOINCREMENT sequence page"
    );
}

#[test]
fn occupied_metrics_path_is_never_overwritten() {
    let (_directory, _path, mut db) = legacy_store();
    db.execute("CREATE TABLE metrics.unrelated(value TEXT)", [])
        .unwrap();
    db.execute("INSERT INTO metrics.unrelated VALUES('preserve')", [])
        .unwrap();
    assert!(split(&mut db).is_err());
    assert_eq!(
        db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT value FROM metrics.unrelated", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM main.snapshots", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn full_destination_rolls_back_without_dropping_source() {
    let (_directory, _path, mut db) = legacy_store();
    db.execute_batch("PRAGMA metrics.max_page_count=2").unwrap();
    assert!(split(&mut db).is_err());
    assert_eq!(
        db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM main.snapshots", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("PRAGMA main.integrity_check(1)", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn late_migration_failure_rolls_back_the_copied_tables() {
    let (_directory, _path, mut db) = legacy_store();
    // An operator-added view blocks the final DROP COLUMN, after data copying.
    db.execute(
        "CREATE VIEW main.legacy_reader AS SELECT last_seen_at_ms FROM nodes",
        [],
    )
    .unwrap();
    assert!(split(&mut db).is_err());
    assert_eq!(
        db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM main.snapshots", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM metrics.sqlite_schema", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("PRAGMA main.integrity_check(1)", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn mismatched_pair_is_rejected() {
    let (_directory, _path, mut db) = legacy_store();
    split(&mut db).unwrap();
    db.execute(
        "UPDATE metrics.storage_identity SET store_id=?1",
        [Uuid::new_v4().to_string()],
    )
    .unwrap();
    assert!(validate_pair(&db).is_err());
}

#[test]
fn newer_version_of_either_file_fails_closed() {
    for (schema, version) in [("main", 5), ("metrics", 2)] {
        let (_directory, control, mut db) = legacy_store();
        split(&mut db).unwrap();
        db.pragma_update(Some(schema), "user_version", version)
            .unwrap();
        drop(db);
        assert!(super::super::Storage::open(&control, 64 * 1024 * 1024).is_err());
        let main = Connection::open(&control).unwrap();
        assert_eq!(
            main.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            if schema == "main" { 5 } else { 4 }
        );
        let metrics = Connection::open(path(&control).unwrap()).unwrap();
        assert_eq!(
            metrics
                .query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            metrics
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            if schema == "metrics" { 2 } else { 1 }
        );
    }
}

#[test]
fn interrupted_delete_cleanup_removes_all_node_metrics() {
    let (_directory, _path, mut db) = legacy_store();
    split(&mut db).unwrap();
    db.execute("DELETE FROM nodes WHERE id='node-a'", [])
        .unwrap();
    assert_eq!(prune_orphans(&mut db).unwrap(), 4);
    assert_eq!(prune_orphans(&mut db).unwrap(), 0);
}

#[test]
fn interrupted_probe_delete_cleanup_preserves_other_metrics() {
    let (_directory, _path, mut db) = legacy_store();
    split(&mut db).unwrap();
    db.execute("DELETE FROM probe_tasks WHERE id='task-a'", [])
        .unwrap();
    assert_eq!(prune_orphans(&mut db).unwrap(), 1);
    assert_eq!(
        db.query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn latest_and_probe_history_seek_without_sorting() {
    let (_directory, _path, mut db) = legacy_store();
    split(&mut db).unwrap();
    for (table, index) in [
        ("snapshots", "snapshots_node_received"),
        ("probe_results", "probe_node_time"),
    ] {
        let sql = format!(
            "EXPLAIN QUERY PLAN SELECT id FROM {table} WHERE node_id=?1 AND received_at_ms>=?2 ORDER BY received_at_ms DESC,id DESC LIMIT 1000"
        );
        let mut statement = db.prepare(&sql).unwrap();
        let plan = statement
            .query_map(params!["node-a", 0], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains(index), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
}

#[test]
fn backup_contains_a_restorable_pair_and_manifest() {
    let (_directory, control, mut db) = legacy_store();
    split(&mut db).unwrap();
    db.execute_batch("PRAGMA main.journal_mode=WAL; PRAGMA metrics.journal_mode=WAL;")
        .unwrap();
    let target = backup(&mut db, &control).unwrap();
    assert!(target.join("manifest.json").is_file());
    let restored = Connection::open(target.join("pulse.db")).unwrap();
    restored
        .execute(
            "ATTACH DATABASE ?1 AS metrics",
            [target.join("pulse.metrics.db").to_str().unwrap()],
        )
        .unwrap();
    validate_pair(&restored).unwrap();
    assert_eq!(
        restored
            .query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        restored
            .query_row("PRAGMA metrics.integrity_check(1)", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn real_open_migrates_once_and_keeps_a_pre_upgrade_backup() {
    let (directory, control, db) = legacy_store();
    drop(db);
    let storage = super::super::Storage::open(&control, 64 * 1024 * 1024).unwrap();
    assert_eq!(
        storage
            .connection()
            .unwrap()
            .query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(storage);
    let backups = || {
        directory
            .path()
            .read_dir()
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".backup-"))
            .collect::<Vec<_>>()
    };
    let initial = backups();
    assert_eq!(initial.len(), 1);
    let backup = Connection::open(initial[0].path()).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        backup
            .query_row("SELECT count(*) FROM snapshots", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let reopened = super::super::Storage::open(&control, 64 * 1024 * 1024).unwrap();
    assert_eq!(
        backups().len(),
        1,
        "normal reopens must not repeat migration"
    );
    assert_eq!(
        reopened
            .connection()
            .unwrap()
            .query_row("SELECT used_up FROM traffic_periods", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        80
    );
}

#[test]
fn missing_metrics_file_is_not_silently_recreated() {
    let (_directory, control, mut db) = legacy_store();
    split(&mut db).unwrap();
    drop(db);
    let metrics = path(&control).unwrap();
    let retained = metrics.with_extension("retained");
    fs::rename(&metrics, &retained).unwrap();
    assert!(super::super::Storage::open(&control, 64 * 1024 * 1024).is_err());
    assert!(!metrics.exists());
    fs::rename(&retained, &metrics).unwrap();
    assert!(super::super::Storage::open(&control, 64 * 1024 * 1024).is_ok());
}

#[test]
fn capacity_is_enforced_on_both_files() {
    let (_directory, control, mut db) = legacy_store();
    split(&mut db).unwrap();
    for schema in ["main", "metrics"] {
        configure_limit(&db, schema, 64 * 1024 * 1024 + 1).unwrap();
        let pages: i64 = db
            .query_row(&format!("PRAGMA {schema}.max_page_count"), [], |r| r.get(0))
            .unwrap();
        let size: i64 = db
            .query_row(&format!("PRAGMA {schema}.page_size"), [], |r| r.get(0))
            .unwrap();
        assert!(pages * size <= 64 * 1024 * 1024 + 1);
    }
    db.execute("CREATE TABLE metrics.padding(data BLOB)", [])
        .unwrap();
    db.execute("INSERT INTO metrics.padding VALUES(zeroblob(1048576))", [])
        .unwrap();
    drop(db);
    assert!(super::super::Storage::open(&control, 512 * 1024).is_err());
}

#[test]
fn paired_backup_reserves_external_writers_consistently() {
    let (_directory, control, mut db) = legacy_store();
    split(&mut db).unwrap();
    db.execute_batch(
        "PRAGMA main.journal_mode=WAL; PRAGMA metrics.journal_mode=WAL;
         CREATE TABLE main.backup_invariant(value INTEGER); INSERT INTO main.backup_invariant VALUES(0);
         CREATE TABLE metrics.backup_invariant(value INTEGER); INSERT INTO metrics.backup_invariant VALUES(0);"
    ).unwrap();
    let writer_path = control.clone();
    let (sender, ready) = std::sync::mpsc::sync_channel(1);
    let writer = std::thread::spawn(move || {
        let mut writer = Connection::open(&writer_path).unwrap();
        writer.busy_timeout(Duration::from_secs(5)).unwrap();
        writer
            .execute(
                "ATTACH DATABASE ?1 AS metrics",
                [path(&writer_path).unwrap().to_str().unwrap()],
            )
            .unwrap();
        for value in 1..=30 {
            let tx = writer
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute("UPDATE main.backup_invariant SET value=?1", [value])
                .unwrap();
            std::thread::sleep(Duration::from_millis(1));
            tx.execute("UPDATE metrics.backup_invariant SET value=?1", [value])
                .unwrap();
            tx.commit().unwrap();
            if value == 1 {
                sender.send(()).unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let target = backup(&mut db, &control).unwrap();
    writer.join().unwrap();
    let restored = Connection::open(target.join("pulse.db")).unwrap();
    restored
        .execute(
            "ATTACH DATABASE ?1 AS metrics",
            [target.join("pulse.metrics.db").to_str().unwrap()],
        )
        .unwrap();
    let values: (i64,i64) = restored.query_row(
        "SELECT (SELECT value FROM main.backup_invariant),(SELECT value FROM metrics.backup_invariant)",
        [], |r| Ok((r.get(0)?,r.get(1)?))
    ).unwrap();
    assert_eq!(values.0, values.1);
    assert!(values.0 > 0);
}
