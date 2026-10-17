//! Explicit offline, forward-only storage rename. Not a runtime path fallback.

use crate::Administration;
use rusqlite::{Connection, OpenFlags};
use std::{error::Error, fs, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Prepare the canonical managed store without treating an incomplete migrated
/// store as a fresh installation. A receipt alone is never proof of data health.
///
/// # Errors
/// Rejects missing database halves, invalid receipts and mismatched identities.
pub fn prepare_managed_storage(database: &Path, max_bytes: u64) -> Result<()> {
    let destination = database.parent().ok_or("database path has no directory")?;
    if !database.is_absolute()
        || database.file_name().and_then(|name| name.to_str()) != Some("senulun.db")
        || destination.file_name().and_then(|name| name.to_str()) != Some("store")
    {
        return Err("managed migration requires an absolute store/senulun.db path".into());
    }
    let root = destination.parent().ok_or("storage has no parent")?;
    for path in database.ancestors() {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && metadata.file_type().is_symlink()
        {
            return Err("managed storage refuses symlinks".into());
        }
    }
    let source = root.join("pulse.db");
    let receipt_path = destination.join("brand-migration.json");
    if receipt_path.try_exists()? {
        let metadata = fs::symlink_metadata(&receipt_path)?;
        if !metadata.is_file() || metadata.len() > 16 * 1024 {
            return Err("invalid storage migration receipt".into());
        }
        let receipt: serde_json::Value = serde_json::from_slice(&fs::read(receipt_path)?)?;
        // Only read the identity row on ordinary startup, not a full history
        // integrity scan. The migration already verified the backup completely.
        if receipt["version"] != 1
            || receipt["source"] != source.to_string_lossy().as_ref()
            || receipt["store_id"] != database_identity(database, false)?
            || receipt["store_id"]
                != database_identity(&destination.join("senulun.metrics.db"), false)?
        {
            return Err(
                "migrated store differs from its receipt; restore the complete pair".into(),
            );
        }
        return Ok(());
    }
    if source.try_exists()? {
        return migrate_storage(&source, destination, max_bytes);
    }
    if !destination.try_exists()? {
        fs::create_dir(destination)?;
        return Ok(());
    }
    if fs::read_dir(destination)?.next().is_none() {
        return Ok(());
    }
    // A previously initialized canonical store also needs both original halves.
    if database_identity(database, false)?
        != database_identity(&destination.join("senulun.metrics.db"), false)?
    {
        return Err("canonical database pair has different identities".into());
    }
    Ok(())
}

/// Copy a stopped legacy store into a new directory, publishing the pair with
/// one directory rename. The original and the matched backup are retained.
///
/// # Errors
/// Refuses unsafe paths, a missing/mismatched pair, occupied destinations or
/// failed backup/verification. The caller must stop the old writer first.
pub fn migrate_storage(source: &Path, destination: &Path, max_bytes: u64) -> Result<()> {
    if !source.is_absolute() || !destination.is_absolute() {
        return Err("storage migration requires absolute paths".into());
    }
    let parent = destination.parent().ok_or("destination has no parent")?;
    let source_parent = source.parent().ok_or("source has no parent")?;
    if source_parent != parent || destination.file_name().is_none() {
        return Err("storage migration must remain on the same owned filesystem".into());
    }
    for path in source.ancestors().chain(destination.ancestors()) {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && metadata.file_type().is_symlink()
        {
            return Err("storage migration refuses symlinks".into());
        }
    }
    if destination.try_exists()? {
        // A completed copy is idempotent, but never adopt another database or
        // silently initialize an incomplete destination.
        if !destination.join("brand-migration.json").is_file() {
            return Err("storage migration destination is already occupied".into());
        }
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(destination.join("brand-migration.json"))?)?;
        if receipt["source"] != source.to_string_lossy().as_ref()
            || receipt["store_id"] != store_id(&destination.join("senulun.db"))?
            || receipt["store_id"] != store_id(&destination.join("senulun.metrics.db"))?
        {
            return Err("storage migration identity differs from its receipt".into());
        }
        if source.try_exists()? && receipt["store_id"] != store_id(source)? {
            return Err("legacy source changed after storage migration".into());
        }
        return Ok(());
    }
    if !source.is_file() {
        return Err("legacy database is missing; migration cannot initialize a replacement".into());
    }
    // Administration uses the existing bounded SQLite paired-backup mechanism,
    // including schema backups and storage identity validation.
    let administration = Administration::open(source, max_bytes)?;
    let backup = administration.backup()?;
    drop(administration);
    let id = store_id(&backup.join("senulun.db"))?;
    if store_id(&backup.join("senulun.metrics.db"))? != id {
        return Err("legacy backup database pair does not match".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".senulun-brand-")
        .tempdir_in(parent)?;
    for name in ["senulun.db", "senulun.metrics.db"] {
        fs::copy(backup.join(name), staging.path().join(name))?;
        fs::File::open(staging.path().join(name))?.sync_all()?;
    }
    let receipt = serde_json::json!({
        "version": 1, "source": source, "backup": backup, "store_id": id
    });
    let receipt_path = staging.path().join("brand-migration.json");
    fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["senulun.db", "senulun.metrics.db", "brand-migration.json"] {
            fs::set_permissions(staging.path().join(name), fs::Permissions::from_mode(0o600))?;
        }
    }
    fs::File::open(&receipt_path)?.sync_all()?;
    fs::File::open(staging.path())?.sync_all()?;
    if destination.try_exists()? {
        return Err("storage migration destination appeared during preparation".into());
    }
    fs::rename(staging.path(), destination)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn store_id(path: &Path) -> Result<String> {
    database_identity(path, true)
}

fn database_identity(path: &Path, integrity: bool) -> Result<String> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err("storage migration database is not a regular file".into());
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    if integrity {
        let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err("storage migration database failed integrity verification".into());
        }
    }
    Ok(db.query_row(
        "SELECT store_id FROM storage_identity WHERE id=1",
        [],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_restart_never_reinitializes_a_missing_migrated_database() {
        for missing in ["senulun.db", "senulun.metrics.db"] {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("pulse.db");
            drop(Administration::open(&source, 64 << 20).unwrap());
            let destination = directory.path().join("store");
            let database = destination.join("senulun.db");
            prepare_managed_storage(&database, 64 << 20).unwrap();
            prepare_managed_storage(&database, 64 << 20).unwrap();
            fs::rename(
                destination.join(missing),
                directory.path().join("retained.db"),
            )
            .unwrap();
            assert!(prepare_managed_storage(&database, 64 << 20).is_err());
            assert!(!destination.join(missing).exists());
            assert!(source.exists());
        }
    }

    #[test]
    fn managed_start_distinguishes_fresh_storage_from_partial_canonical_storage() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("store/senulun.db");
        prepare_managed_storage(&database, 64 << 20).unwrap();
        drop(Administration::open(&database, 64 << 20).unwrap());
        prepare_managed_storage(&database, 64 << 20).unwrap();
        fs::rename(
            database.with_file_name("senulun.metrics.db"),
            directory.path().join("retained.db"),
        )
        .unwrap();
        assert!(prepare_managed_storage(&database, 64 << 20).is_err());
        assert!(!database.with_file_name("senulun.metrics.db").exists());
    }

    #[test]
    fn managed_restart_rejects_a_receipt_with_the_wrong_identity() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("pulse.db");
        drop(Administration::open(&source, 64 << 20).unwrap());
        let database = directory.path().join("store/senulun.db");
        prepare_managed_storage(&database, 64 << 20).unwrap();
        fs::write(
            database.with_file_name("brand-migration.json"),
            b"{\"version\":1,\"store_id\":\"another-store\"}",
        )
        .unwrap();
        assert!(prepare_managed_storage(&database, 64 << 20).is_err());
        assert!(source.exists());
        assert!(database.exists());
    }

    #[test]
    fn rename_preserves_pair_and_identity_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("pulse.db");
        let admin = Administration::open(&source, 64 << 20).unwrap();
        let enrollment = admin.create_enrollment(600).unwrap();
        drop(admin);
        let original = Connection::open(&source).unwrap();
        original
            .execute(
                "ATTACH DATABASE ?1 AS metrics",
                [directory.path().join("pulse.metrics.db").to_str().unwrap()],
            )
            .unwrap();
        original
            .execute_batch(
                "INSERT INTO nodes(id,token_hash,name,agent_version,created_at_ms,updated_at_ms)
             VALUES('retained-node','unchanged-hash','custom name','old-version',10,20);
             INSERT INTO metrics.traffic_periods VALUES('retained-node',0,800,700,80,70);
             INSERT INTO metrics.node_state VALUES('retained-node',12345);",
            )
            .unwrap();
        drop(original);
        let destination = directory.path().join("store");
        migrate_storage(&source, &destination, 64 << 20).unwrap();
        assert_eq!(
            store_id(&source).unwrap(),
            store_id(&destination.join("senulun.db")).unwrap()
        );
        let db = Connection::open(destination.join("senulun.db")).unwrap();
        let original = Connection::open(&source).unwrap();
        let before: String = original
            .query_row("SELECT token_hash FROM enrollment_tokens", [], |r| r.get(0))
            .unwrap();
        let after: String = db
            .query_row("SELECT token_hash FROM enrollment_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            db.query_row(
                "SELECT token_hash FROM nodes WHERE id='retained-node'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "unchanged-hash"
        );
        let metrics = Connection::open(destination.join("senulun.metrics.db")).unwrap();
        assert_eq!(
            metrics
                .query_row(
                    "SELECT used_up+used_down FROM traffic_periods WHERE node_id='retained-node'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            150
        );
        assert_eq!(
            metrics
                .query_row(
                    "SELECT last_seen_at_ms FROM node_state WHERE node_id='retained-node'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            12345
        );
        drop(metrics);
        assert!(!enrollment.token.is_empty());
        drop(db);
        drop(original);
        migrate_storage(&source, &destination, 64 << 20).unwrap();
    }

    #[test]
    fn rename_refuses_missing_source_or_unowned_destination() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("pulse.db");
        let destination = directory.path().join("store");
        assert!(migrate_storage(&source, &destination, 64 << 20).is_err());
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("sentinel"), b"keep").unwrap();
        assert!(migrate_storage(&source, &destination, 64 << 20).is_err());
        assert_eq!(fs::read(destination.join("sentinel")).unwrap(), b"keep");
    }

    #[test]
    fn missing_metrics_never_creates_an_empty_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("pulse.db");
        drop(Administration::open(&source, 64 << 20).unwrap());
        fs::rename(
            directory.path().join("pulse.metrics.db"),
            directory.path().join("retained.metrics.db"),
        )
        .unwrap();
        let destination = directory.path().join("store");
        assert!(migrate_storage(&source, &destination, 64 << 20).is_err());
        assert!(!destination.exists());
        assert!(source.exists());
    }
}
