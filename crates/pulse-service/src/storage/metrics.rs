//! Metrics storage, paired backups and the forward-only split migration.
//!
//! Runtime WAL commits are atomic per file, not across files. Migration uses
//! SQLite's rollback-journal super-journal instead; never perform the split in WAL.

use super::{
    StorageError, backup_path, containing_directory, online_backup, prepare_database_path,
    set_private_directory_permissions,
};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const VERSION: i64 = 1;

#[cfg(test)]
mod tests;

const SCHEMA: &str = "
CREATE TABLE metrics.storage_identity (
    id INTEGER PRIMARY KEY CHECK(id=1), store_id TEXT NOT NULL
);
CREATE TABLE metrics.snapshots (
    id INTEGER PRIMARY KEY,
    node_id TEXT NOT NULL,
    sample_id TEXT NOT NULL,
    collected_at_ms INTEGER NOT NULL,
    received_at_ms INTEGER NOT NULL,
    uptime_seconds INTEGER NOT NULL,
    cpu_usage_percent REAL NOT NULL,
    load_one REAL NOT NULL,
    load_five REAL NOT NULL,
    load_fifteen REAL NOT NULL,
    memory_total_bytes INTEGER NOT NULL,
    memory_used_bytes INTEGER NOT NULL,
    swap_total_bytes INTEGER NOT NULL,
    swap_used_bytes INTEGER NOT NULL,
    disk_total_bytes INTEGER NOT NULL,
    disk_used_bytes INTEGER NOT NULL,
    network_receive_bytes_per_second INTEGER NOT NULL,
    network_transmit_bytes_per_second INTEGER NOT NULL,
    network_total_received_bytes INTEGER NOT NULL,
    network_total_transmitted_bytes INTEGER NOT NULL,
    process_count INTEGER,
    extensions TEXT NOT NULL DEFAULT '{}',
    UNIQUE(node_id, sample_id)
);
CREATE INDEX metrics.snapshots_node_received ON snapshots(node_id, received_at_ms DESC, id DESC);
CREATE INDEX metrics.snapshots_received ON snapshots(received_at_ms);
CREATE TABLE metrics.probe_results (
    id INTEGER PRIMARY KEY, node_id TEXT NOT NULL, task_id TEXT NOT NULL,
    sample_id TEXT NOT NULL, collected_at_ms INTEGER NOT NULL,
    received_at_ms INTEGER NOT NULL, latency_ms REAL, success INTEGER NOT NULL,
    error TEXT, UNIQUE(node_id, sample_id)
);
CREATE INDEX metrics.probe_node_task_time ON probe_results(node_id, task_id, received_at_ms DESC);
CREATE INDEX metrics.probe_node_time ON probe_results(node_id, received_at_ms DESC, id DESC);
CREATE INDEX metrics.probe_received ON probe_results(received_at_ms);
CREATE TABLE metrics.traffic_periods (
    node_id TEXT PRIMARY KEY, cycle_start_ms INTEGER NOT NULL,
    raw_up INTEGER NOT NULL, raw_down INTEGER NOT NULL,
    used_up INTEGER NOT NULL, used_down INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE metrics.node_state (
    node_id TEXT PRIMARY KEY, last_seen_at_ms INTEGER NOT NULL
) WITHOUT ROWID;
";

pub(super) fn path(control: &Path) -> Result<PathBuf> {
    let name = control
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("database path must end in a valid UTF-8 file name")?;
    let stem = name.strip_suffix(".db").unwrap_or(name);
    Ok(control.with_file_name(format!("{stem}.metrics.db")))
}

pub(super) fn configure_limit(db: &Connection, schema: &str, max_bytes: u64) -> Result<()> {
    // Only static schema names from this module / Storage::open reach this function.
    let page_size = u64::try_from(db.query_row(
        &format!("PRAGMA {schema}.page_size"),
        [],
        |r| r.get::<_, i64>(0),
    )?)?;
    let pages = u64::try_from(
        db.query_row(&format!("PRAGMA {schema}.page_count"), [], |r| {
            r.get::<_, i64>(0)
        })?,
    )?;
    if page_size == 0 || max_bytes < page_size {
        return Err("PULSE_MAX_DATABASE_BYTES must contain at least one SQLite page".into());
    }
    if pages.saturating_mul(page_size) > max_bytes {
        return Err(format!(
            "{schema} database already exceeds PULSE_MAX_DATABASE_BYTES ({max_bytes} bytes)"
        )
        .into());
    }
    let max_pages = i64::try_from(max_bytes / page_size)?;
    db.pragma_update(Some(schema), "max_page_count", max_pages)?;
    Ok(())
}

pub(super) fn attach(db: &mut Connection, control: &Path, max_bytes: u64) -> Result<()> {
    let metrics = path(control)?;
    let version: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    if version == super::SCHEMA_VERSION && !metrics.is_file() {
        return Err(
            "metrics database is missing; restore the matching database pair, not an empty file"
                .into(),
        );
    }
    prepare_database_path(&metrics)?;
    db.execute(
        "ATTACH DATABASE ?1 AS metrics",
        [metrics.to_str().ok_or("invalid metrics path")?],
    )?;
    configure_limit(db, "metrics", max_bytes)?;
    if version == 3 {
        split(db)?;
    }
    validate_pair(db)?;
    db.execute_batch(
        "PRAGMA main.journal_mode=WAL;
         PRAGMA main.synchronous=FULL;
         PRAGMA main.cache_size=-1024;
         PRAGMA main.wal_autocheckpoint=1000;
         PRAGMA main.journal_size_limit=67108864;
         PRAGMA metrics.journal_mode=WAL;
         PRAGMA metrics.synchronous=FULL;
         PRAGMA metrics.cache_size=-3072;
         PRAGMA metrics.wal_autocheckpoint=1000;
         PRAGMA metrics.journal_size_limit=67108864;",
    )?;
    db.set_prepared_statement_cache_capacity(32);
    // Cross-file foreign keys are unsupported. A committed control-plane delete
    // is authoritative; recover interrupted cleanup before exposing read APIs.
    while prune_orphans(db)? != 0 {}
    Ok(())
}

fn split(db: &mut Connection) -> Result<()> {
    let metrics_version: i64 = db.query_row("PRAGMA metrics.user_version", [], |r| r.get(0))?;
    let objects: i64 = db.query_row("SELECT count(*) FROM metrics.sqlite_schema", [], |r| {
        r.get(0)
    })?;
    if metrics_version != 0 || objects != 0 {
        return Err(
            "refusing to migrate into an existing metrics database; restore a consistent backup"
                .into(),
        );
    }
    // Requires a stopped Service during upgrade. If another connection prevents
    // changing journal mode, fail before moving any data. FULL + DELETE enables
    // SQLite's crash-safe multi-file super-journal (main must be a disk file).
    for schema in ["main", "metrics"] {
        let mode: String =
            db.query_row(&format!("PRAGMA {schema}.journal_mode=DELETE"), [], |r| {
                r.get(0)
            })?;
        if mode != "delete" {
            return Err("split migration requires exclusive access and rollback journals".into());
        }
        db.pragma_update(Some(schema), "synchronous", "FULL")?;
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Recheck after taking the writer lock; a second startup must not repeat DDL.
    let version: i64 = tx.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    if version != 3 {
        return Err(
            "database changed during split migration; restart after other writers stop".into(),
        );
    }
    tx.execute_batch(SCHEMA)?;
    for table in ["snapshots", "probe_results", "traffic_periods"] {
        tx.execute(
            &format!("INSERT INTO metrics.{table} SELECT * FROM main.{table}"),
            [],
        )?;
        // Check all fields, not just row counts, before dropping the source.
        let mismatch: bool = tx.query_row(
            &format!(
                "SELECT EXISTS(SELECT * FROM main.{table} EXCEPT SELECT * FROM metrics.{table})
                OR EXISTS(SELECT * FROM metrics.{table} EXCEPT SELECT * FROM main.{table})"
            ),
            [],
            |r| r.get(0),
        )?;
        if mismatch {
            return Err("split migration data verification failed".into());
        }
    }
    tx.execute_batch(
        "INSERT INTO metrics.node_state SELECT id,last_seen_at_ms FROM main.nodes WHERE last_seen_at_ms IS NOT NULL;
         CREATE TABLE main.storage_identity (id INTEGER PRIMARY KEY CHECK(id=1), store_id TEXT NOT NULL);",
    )?;
    let identity = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO main.storage_identity VALUES(1,?1)",
        [&identity],
    )?;
    tx.execute(
        "INSERT INTO metrics.storage_identity VALUES(1,?1)",
        [&identity],
    )?;
    let integrity: String = tx.query_row("PRAGMA metrics.integrity_check(1)", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Err("split metrics integrity check failed".into());
    }
    tx.execute_batch(
        "DROP TABLE main.snapshots;
         DROP TABLE main.probe_results;
         DROP TABLE main.traffic_periods;
         ALTER TABLE main.nodes DROP COLUMN last_seen_at_ms;
         PRAGMA main.user_version=4;
         PRAGMA metrics.user_version=1;",
    )?;
    tx.commit()?;
    Ok(())
}

fn validate_pair(db: &Connection) -> Result<()> {
    let control_version: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    if control_version != super::SCHEMA_VERSION {
        return Err("unsupported control schema for paired storage".into());
    }
    let version: i64 = db.query_row("PRAGMA metrics.user_version", [], |r| r.get(0))?;
    if version != VERSION {
        return Err(
            format!("unsupported metrics schema version {version}; expected {VERSION}").into(),
        );
    }
    let matches: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.storage_identity c JOIN metrics.storage_identity m
         ON c.store_id=m.store_id WHERE c.id=1 AND m.id=1)",
        [],
        |r| r.get(0),
    )?;
    if !matches {
        return Err("control and metrics databases do not belong to the same store".into());
    }
    Ok(())
}

pub(super) fn prune_orphans(db: &mut Connection) -> std::result::Result<usize, StorageError> {
    // Reserve both writers before reading either snapshot. A concurrent
    // enrollment followed by ingestion must not be mistaken for an orphan.
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut removed = 0;
    for table in ["snapshots", "probe_results"] {
        let missing_task = if table == "probe_results" {
            " OR NOT EXISTS(SELECT 1 FROM main.probe_tasks WHERE id=m.task_id)"
        } else {
            ""
        };
        removed += tx.execute(&format!(
            "DELETE FROM metrics.{table} WHERE id IN (SELECT m.id FROM metrics.{table} m
             WHERE NOT EXISTS(SELECT 1 FROM main.nodes WHERE id=m.node_id){missing_task} LIMIT 10000)"
        ), [])?;
    }
    for table in ["traffic_periods", "node_state"] {
        removed += tx.execute(
            &format!(
            "DELETE FROM metrics.{table} WHERE node_id IN (SELECT m.node_id FROM metrics.{table} m
             WHERE NOT EXISTS(SELECT 1 FROM main.nodes WHERE id=m.node_id) LIMIT 10000)"
        ),
            [],
        )?;
    }
    tx.commit()?;
    Ok(removed)
}

pub(super) fn delete_node(
    db: &mut Connection,
    node: &str,
) -> std::result::Result<(), StorageError> {
    let tx = db.transaction()?;
    for table in [
        "snapshots",
        "probe_results",
        "traffic_periods",
        "node_state",
    ] {
        tx.execute(
            &format!("DELETE FROM metrics.{table} WHERE node_id=?1"),
            [node],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub(super) fn backup(db: &mut Connection, control: &Path) -> Result<PathBuf> {
    let target = backup_path(control)?;
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid backup name")?;
    let temporary = target.with_file_name(format!(".{name}.partial"));
    fs::create_dir(&temporary)?;
    set_private_directory_permissions(&temporary)?;
    // Hold writer reservations on BOTH databases. Separate read connections let
    // the Backup API progress without SQLITE_LOCKED on the reserving connection.
    // External authentication writes wait briefly; live reads remain available.
    let reservation = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    validate_pair(&reservation)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let main_source = Connection::open_with_flags(control, flags)?;
    let metrics_source = Connection::open_with_flags(path(control)?, flags)?;
    main_source.busy_timeout(Duration::from_secs(5))?;
    metrics_source.busy_timeout(Duration::from_secs(5))?;
    online_backup(&main_source, &temporary.join("pulse.db"))?;
    online_backup(&metrics_source, &temporary.join("pulse.metrics.db"))?;
    let identity: String = reservation.query_row(
        "SELECT store_id FROM main.storage_identity WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    reservation.rollback()?;
    let manifest = serde_json::json!({
        "format_version": 1, "store_id": identity,
        "control": "pulse.db", "metrics": "pulse.metrics.db",
        "control_schema": super::SCHEMA_VERSION, "metrics_schema": VERSION,
        "created_at_unix_ms": super::unix_time_ms()?
    });
    let manifest_path = temporary.join("manifest.json");
    prepare_database_path(&manifest_path)?;
    let mut file = fs::OpenOptions::new().write(true).open(&manifest_path)?;
    file.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    file.sync_all()?;
    fs::File::open(&temporary)?.sync_all()?;
    if target.exists() {
        return Err("backup target already exists".into());
    }
    fs::rename(&temporary, &target)?;
    fs::File::open(containing_directory(&target))?.sync_all()?;
    Ok(target)
}
