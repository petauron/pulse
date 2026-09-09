use std::{
    collections::HashMap,
    error::Error,
    fmt::{self, Display, Formatter},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pulse_protocol::{EnrollmentRequest, EnrollmentResponse, SystemSnapshot};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, backup::Backup, params};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub(crate) const SCHEMA_VERSION: i64 = 2;
const MIN_SNAPSHOT_SPACING_MS: i64 = 4_750;
pub(crate) const RETENTION_PRUNE_BATCH_SIZE: usize = 10_000;

#[derive(Debug)]
pub(crate) enum StorageError {
    Unauthorized,
    InvalidEnrollment,
    NodeLimit,
    RateLimited,
    NotFound,
    Database(rusqlite::Error),
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => formatter.write_str("unauthorized"),
            Self::InvalidEnrollment => formatter.write_str("invalid enrollment token"),
            Self::NodeLimit => formatter.write_str("node limit reached"),
            Self::RateLimited => formatter.write_str("snapshot rate limited"),
            Self::NotFound => formatter.write_str("record not found"),
            Self::Database(error) => write!(formatter, "database error: {error}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

pub(crate) struct Storage {
    connection: Mutex<Connection>,
    path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct EnrollmentSecret {
    pub id: String,
    pub token: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct AuditEvent {
    pub happened_at_unix_ms: u64,
    pub action: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HistoryCoverage {
    pub requested_start_unix_ms: u64,
    pub requested_end_unix_ms: u64,
    pub actual_start_unix_ms: Option<u64>,
    pub actual_end_unix_ms: Option<u64>,
    pub source_points: u64,
    pub returned_points: usize,
    pub bucket_ms: u64,
    pub downsampled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HistorySeries {
    pub records: Vec<Value>,
    pub coverage: HistoryCoverage,
}

impl Storage {
    pub(crate) fn open(
        path: &Path,
        max_database_bytes: u64,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        prepare_database_path(path)?;
        let existed_with_data = path.metadata().is_ok_and(|metadata| metadata.len() > 0);
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let page_size = u64::try_from(page_size).map_err(|_| "invalid SQLite page size")?;
        let current_pages: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let current_pages =
            u64::try_from(current_pages).map_err(|_| "invalid SQLite page count")?;
        if current_pages.saturating_mul(page_size) > max_database_bytes {
            return Err(format!(
                "database already exceeds PULSE_MAX_DATABASE_BYTES ({max_database_bytes} bytes)"
            )
            .into());
        }
        let max_pages = i64::try_from(max_database_bytes.div_ceil(page_size).max(1))
            .map_err(|_| "configured database size exceeds SQLite limits")?;
        connection.pragma_update(None, "max_page_count", max_pages)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
            )
            .into());
        }

        if version < SCHEMA_VERSION {
            if existed_with_data {
                let backup_path = backup_path(path)?;
                online_backup(&connection, &backup_path)?;
                tracing::info!(path = %backup_path.display(), "database backup created before migration");
            }
            migrate(&mut connection, version)?;
        }

        // Credential rotation/revocation must survive power loss after success.
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA cache_size = -4096;
             PRAGMA wal_autocheckpoint = 1000;
             PRAGMA journal_size_limit = 67108864;",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: path.to_path_buf(),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::Database(rusqlite::Error::InvalidQuery))
    }

    pub(crate) fn create_enrollment(
        &self,
        ttl_seconds: u64,
        now_ms: u64,
    ) -> Result<EnrollmentSecret, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = Uuid::new_v4().to_string();
        let token = format!(
            "pulse_enroll_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let expires_at = now_ms.saturating_add(ttl_seconds.saturating_mul(1_000));
        transaction.execute(
            "INSERT INTO enrollment_tokens
                (id, token_hash, created_at_ms, expires_at_ms, consumed_at_ms, node_id)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL)",
            params![id, hash_token(&token), to_i64(now_ms)?, to_i64(expires_at)?],
        )?;
        insert_audit_transaction(&transaction, now_ms, "enrollment.create", &id)?;
        transaction.commit()?;
        Ok(EnrollmentSecret {
            id,
            token,
            expires_at_unix_ms: expires_at,
        })
    }

    pub(crate) fn revoke_enrollment(&self, id: &str, now_ms: u64) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE enrollment_tokens
             SET expires_at_ms = MIN(expires_at_ms, ?2)
             WHERE id = ?1 AND consumed_at_ms IS NULL",
            params![id, to_i64(now_ms)?],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        insert_audit_transaction(&transaction, now_ms, "enrollment.revoke", id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn enroll(
        &self,
        request: &EnrollmentRequest,
        enrollment_token_hash: &str,
        max_nodes: u32,
        now_ms: u64,
    ) -> Result<EnrollmentResponse, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let enrollment: Option<(String, i64, Option<i64>)> = transaction
            .query_row(
                "SELECT id, expires_at_ms, consumed_at_ms
                 FROM enrollment_tokens WHERE token_hash = ?1",
                params![enrollment_token_hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((enrollment_id, expires_at_ms, consumed_at_ms)) = enrollment else {
            return Err(StorageError::InvalidEnrollment);
        };
        let now = to_i64(now_ms)?;
        if consumed_at_ms.is_some() || expires_at_ms < now {
            return Err(StorageError::InvalidEnrollment);
        }
        let count: u32 = transaction.query_row(
            "SELECT COUNT(*) FROM nodes WHERE disabled_at_ms IS NULL",
            [],
            |row| row.get(0),
        )?;
        if count >= max_nodes {
            return Err(StorageError::NodeLimit);
        }

        let node_id = Uuid::new_v4().to_string();
        let agent_token = new_agent_token();
        transaction.execute(
            "INSERT INTO nodes (
                id, token_hash, name, agent_version, region, node_group,
                created_at_ms, updated_at_ms, disabled_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL)",
            params![
                node_id,
                hash_token(&agent_token),
                request.node_name,
                request.agent_version,
                request.region,
                request.group,
                now,
            ],
        )?;
        let consumed = transaction.execute(
            "UPDATE enrollment_tokens
             SET consumed_at_ms = ?2, node_id = ?3
             WHERE id = ?1 AND consumed_at_ms IS NULL AND expires_at_ms >= ?2",
            params![enrollment_id, now, node_id],
        )?;
        if consumed != 1 {
            return Err(StorageError::InvalidEnrollment);
        }
        insert_audit_transaction(&transaction, now_ms, "node.enroll", &node_id)?;
        transaction.commit()?;

        Ok(EnrollmentResponse {
            node_id,
            agent_token,
        })
    }

    pub(crate) fn rotate_node_token(
        &self,
        node_id: &str,
        now_ms: u64,
    ) -> Result<String, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let token = new_agent_token();
        let changed = transaction.execute(
            "UPDATE nodes SET token_hash = ?2, updated_at_ms = ?3
             WHERE id = ?1 AND disabled_at_ms IS NULL",
            params![node_id, hash_token(&token), to_i64(now_ms)?],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        insert_audit_transaction(&transaction, now_ms, "node.token.rotate", node_id)?;
        transaction.commit()?;
        Ok(token)
    }

    pub(crate) fn revoke_node(&self, node_id: &str, now_ms: u64) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE nodes
             SET disabled_at_ms = ?2, token_hash = ?3, updated_at_ms = ?2
             WHERE id = ?1 AND disabled_at_ms IS NULL",
            params![
                node_id,
                to_i64(now_ms)?,
                hash_token(&format!("revoked-{}", Uuid::new_v4()))
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        insert_audit_transaction(&transaction, now_ms, "node.revoke", node_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn delete_node(&self, node_id: &str, now_ms: u64) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute("DELETE FROM nodes WHERE id = ?1", params![node_id])?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        insert_audit_transaction(&transaction, now_ms, "node.delete", node_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn audit_events(&self, limit: u32) -> Result<Vec<AuditEvent>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT happened_at_ms, action, subject
             FROM audit_events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            Ok(AuditEvent {
                happened_at_unix_ms: from_i64(row.get(0)?),
                action: row.get(1)?,
                subject: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(crate) fn backup(&self) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
        let connection = self
            .connection()
            .map_err(|error| -> Box<dyn Error + Send + Sync> { Box::new(error) })?;
        let target = backup_path(&self.path)?;
        online_backup(&connection, &target)?;
        Ok(target)
    }

    pub(crate) fn prune_expired(
        &self,
        now_ms: u64,
        retention_days: u32,
    ) -> Result<usize, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let removed = prune_expired_snapshots(
            &transaction,
            to_i64(now_ms)?,
            retention_days,
            RETENTION_PRUNE_BATCH_SIZE,
        )?;
        transaction.commit()?;
        Ok(removed)
    }

    pub(crate) fn ingest(
        &self,
        token_hash: &str,
        snapshot: &SystemSnapshot,
        received_at_ms: u64,
        retention_days: u32,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let node_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM nodes
                 WHERE token_hash = ?1 AND disabled_at_ms IS NULL",
                params![token_hash],
                |row| row.get(0),
            )
            .optional()?;
        let Some(node_id) = node_id else {
            return Err(StorageError::Unauthorized);
        };

        let duplicate: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM snapshots WHERE node_id = ?1 AND sample_id = ?2
             )",
            params![node_id, snapshot.sample_id],
            |row| row.get(0),
        )?;
        if duplicate {
            return Ok(());
        }
        let received_at = to_i64(received_at_ms)?;
        let previous_received_at: Option<i64> = transaction
            .query_row(
                "SELECT received_at_ms FROM snapshots WHERE node_id = ?1
                 ORDER BY received_at_ms DESC LIMIT 1",
                params![node_id],
                |row| row.get(0),
            )
            .optional()?;
        if previous_received_at
            .is_some_and(|previous| received_at.saturating_sub(previous) < MIN_SNAPSHOT_SPACING_MS)
        {
            return Err(StorageError::RateLimited);
        }

        prune_expired_snapshots(
            &transaction,
            received_at,
            retention_days,
            RETENTION_PRUNE_BATCH_SIZE,
        )?;
        update_node_from_snapshot(&transaction, &node_id, snapshot, received_at)?;
        insert_snapshot(&transaction, &node_id, snapshot, received_at)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn native_nodes(
        &self,
        offset: u32,
        limit: u32,
        offline_after_seconds: u64,
    ) -> Result<(u32, Vec<DashboardNode>), StorageError> {
        let connection = self.connection()?;
        let total = active_node_count(&connection)?;
        let clients = query_clients(&connection, offset, limit)?;
        let statuses = query_latest_statuses(&connection, offline_after_seconds)?;
        let nodes = clients
            .into_iter()
            .map(|client| DashboardNode {
                status: statuses.get(&client.id).cloned(),
                client,
            })
            .collect();
        Ok((total, nodes))
    }

    pub(crate) fn emerald_dashboard(
        &self,
        offline_after_seconds: u64,
        max_nodes: u32,
    ) -> Result<Value, StorageError> {
        let connection = self.connection()?;
        let clients = query_clients(&connection, 0, max_nodes)?;
        let statuses = query_latest_statuses(&connection, offline_after_seconds)?;
        let mut client_values = serde_json::Map::new();
        let mut status_values = serde_json::Map::new();
        for client in clients {
            if let Some(status) = statuses.get(&client.id) {
                status_values.insert(client.id.clone(), status.emerald_value());
            }
            client_values.insert(client.id.clone(), client.emerald_value());
        }
        Ok(json!({ "clients": client_values, "statuses": status_values }))
    }

    pub(crate) fn history(
        &self,
        node_id: &str,
        hours: u32,
        limit: u32,
        now_ms: u64,
    ) -> Result<HistorySeries, StorageError> {
        let requested_start = now_ms.saturating_sub(u64::from(hours) * 60 * 60 * 1_000);
        let requested_end = now_ms;
        let connection = self.connection()?;
        let bounds: (i64, Option<i64>, Option<i64>) = connection.query_row(
            "SELECT COUNT(*), MIN(received_at_ms), MAX(received_at_ms)
             FROM snapshots
             WHERE node_id = ?1 AND received_at_ms BETWEEN ?2 AND ?3",
            params![node_id, to_i64(requested_start)?, to_i64(requested_end)?],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let source_points = from_i64(bounds.0);
        let downsampled = source_points > u64::from(limit);
        let span = requested_end
            .saturating_sub(requested_start)
            .saturating_add(1);
        let bucket_ms = if downsampled {
            span.div_ceil(u64::from(limit)).max(1)
        } else {
            0
        };
        let records = if downsampled {
            query_aggregate_history(
                &connection,
                node_id,
                requested_start,
                requested_end,
                bucket_ms,
            )?
        } else {
            query_raw_history(&connection, node_id, requested_start, requested_end)?
        };
        let coverage = HistoryCoverage {
            requested_start_unix_ms: requested_start,
            requested_end_unix_ms: requested_end,
            actual_start_unix_ms: bounds.1.map(from_i64),
            actual_end_unix_ms: bounds.2.map(from_i64),
            source_points,
            returned_points: records.len(),
            bucket_ms,
            downsampled,
        };
        Ok(HistorySeries { records, coverage })
    }
}

fn update_node_from_snapshot(
    transaction: &Transaction<'_>,
    node_id: &str,
    snapshot: &SystemSnapshot,
    received_at: i64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE nodes SET
            name = COALESCE(?2, name), agent_version = ?3, operating_system = ?4,
            kernel_version = ?5, architecture = ?6, cpu_name = ?7, cpu_cores = ?8,
            virtualization = ?9, region = ?10, node_group = ?11,
            memory_total_bytes = ?12, swap_total_bytes = ?13, disk_total_bytes = ?14,
            updated_at_ms = ?15, last_seen_at_ms = ?15
         WHERE id = ?1",
        params![
            node_id,
            snapshot.host_name.as_deref(),
            snapshot.agent_version,
            snapshot.operating_system,
            snapshot.kernel_version,
            snapshot.architecture,
            snapshot.cpu_name,
            snapshot.cpu_cores,
            snapshot.virtualization,
            snapshot.region,
            snapshot.group,
            to_i64(snapshot.memory_total_bytes)?,
            to_i64(snapshot.swap_total_bytes)?,
            to_i64(snapshot.disk_total_bytes)?,
            received_at,
        ],
    )?;
    Ok(())
}

fn insert_snapshot(
    transaction: &Transaction<'_>,
    node_id: &str,
    snapshot: &SystemSnapshot,
    received_at: i64,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO snapshots (
            node_id, sample_id, collected_at_ms, received_at_ms, uptime_seconds,
            cpu_usage_percent, load_one, load_five, load_fifteen,
            memory_total_bytes, memory_used_bytes, swap_total_bytes, swap_used_bytes,
            disk_total_bytes, disk_used_bytes, network_receive_bytes_per_second,
            network_transmit_bytes_per_second, network_total_received_bytes,
            network_total_transmitted_bytes, process_count
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
         )",
        params![
            node_id,
            snapshot.sample_id,
            to_i64(snapshot.collected_at_unix_ms)?,
            received_at,
            to_i64(snapshot.uptime_seconds)?,
            snapshot.cpu_usage_percent,
            snapshot.load_one,
            snapshot.load_five,
            snapshot.load_fifteen,
            to_i64(snapshot.memory_total_bytes)?,
            to_i64(snapshot.memory_used_bytes)?,
            to_i64(snapshot.swap_total_bytes)?,
            to_i64(snapshot.swap_used_bytes)?,
            to_i64(snapshot.disk_total_bytes)?,
            to_i64(snapshot.disk_used_bytes)?,
            to_i64(snapshot.network_receive_bytes_per_second)?,
            to_i64(snapshot.network_transmit_bytes_per_second)?,
            to_i64(snapshot.network_total_received_bytes)?,
            to_i64(snapshot.network_total_transmitted_bytes)?,
            snapshot.process_count.map(to_i64).transpose()?,
        ],
    )?;
    Ok(())
}

fn active_node_count(connection: &Connection) -> Result<u32, StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM nodes WHERE disabled_at_ms IS NULL",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn query_clients(
    connection: &Connection,
    offset: u32,
    limit: u32,
) -> Result<Vec<DashboardClient>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id, name, agent_version, operating_system, kernel_version, architecture,
                cpu_name, cpu_cores, virtualization, region, node_group, memory_total_bytes,
                swap_total_bytes, disk_total_bytes, created_at_ms, updated_at_ms,
                last_seen_at_ms
         FROM nodes WHERE disabled_at_ms IS NULL
         ORDER BY created_at_ms ASC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = statement.query_map(params![limit, offset], |row| {
        Ok(DashboardClient {
            id: row.get(0)?,
            name: row.get(1)?,
            agent_version: row.get(2)?,
            operating_system: row.get(3)?,
            kernel_version: row.get(4)?,
            architecture: row.get(5)?,
            cpu_name: row.get(6)?,
            cpu_cores: row.get(7)?,
            virtualization: row.get(8)?,
            region: row.get(9)?,
            group: row.get(10)?,
            memory_total_bytes: from_i64(row.get(11)?),
            swap_total_bytes: from_i64(row.get(12)?),
            disk_total_bytes: from_i64(row.get(13)?),
            created_at_ms: from_i64(row.get(14)?),
            updated_at_ms: from_i64(row.get(15)?),
            last_seen_at_ms: row.get::<_, Option<i64>>(16)?.map(from_i64),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

const LATEST_STATUSES_SQL: &str =
    "SELECT s.node_id, s.received_at_ms, s.collected_at_ms, s.uptime_seconds,
                s.cpu_usage_percent, s.load_one, s.load_five, s.load_fifteen,
                s.memory_total_bytes, s.memory_used_bytes, s.swap_total_bytes,
                s.swap_used_bytes, s.disk_total_bytes, s.disk_used_bytes,
                s.network_receive_bytes_per_second, s.network_transmit_bytes_per_second,
                s.network_total_received_bytes, s.network_total_transmitted_bytes,
                s.process_count, n.last_seen_at_ms
         FROM nodes n
         JOIN snapshots s ON s.id = (
            SELECT s2.id FROM snapshots s2 WHERE s2.node_id = n.id
            ORDER BY s2.received_at_ms DESC, s2.id DESC LIMIT 1
         )
         WHERE n.disabled_at_ms IS NULL";

fn query_latest_statuses(
    connection: &Connection,
    offline_after_seconds: u64,
) -> Result<HashMap<String, DashboardStatus>, StorageError> {
    let mut statement = connection.prepare(LATEST_STATUSES_SQL)?;
    let now = unix_time_ms()?;
    let offline_after_ms = offline_after_seconds.saturating_mul(1_000);
    let rows = statement.query_map([], |row| {
        let last_seen_at_ms = from_i64(row.get::<_, i64>(19)?);
        Ok(DashboardStatus {
            node_id: row.get(0)?,
            received_at_ms: from_i64(row.get(1)?),
            collected_at_ms: from_i64(row.get(2)?),
            uptime_seconds: from_i64(row.get(3)?),
            cpu_usage_percent: row.get(4)?,
            load_one: row.get(5)?,
            load_five: row.get(6)?,
            load_fifteen: row.get(7)?,
            memory_total_bytes: from_i64(row.get(8)?),
            memory_used_bytes: from_i64(row.get(9)?),
            swap_total_bytes: from_i64(row.get(10)?),
            swap_used_bytes: from_i64(row.get(11)?),
            disk_total_bytes: from_i64(row.get(12)?),
            disk_used_bytes: from_i64(row.get(13)?),
            network_receive_bytes_per_second: from_i64(row.get(14)?),
            network_transmit_bytes_per_second: from_i64(row.get(15)?),
            network_total_received_bytes: from_i64(row.get(16)?),
            network_total_transmitted_bytes: from_i64(row.get(17)?),
            process_count: row.get::<_, Option<i64>>(18)?.map(from_i64),
            online: now.saturating_sub(last_seen_at_ms) <= offline_after_ms,
        })
    })?;
    let statuses = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(statuses
        .into_iter()
        .map(|status| (status.node_id.clone(), status))
        .collect())
}

fn query_raw_history(
    connection: &Connection,
    node_id: &str,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<Value>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT node_id, received_at_ms, collected_at_ms, cpu_usage_percent,
                memory_used_bytes, memory_total_bytes, swap_used_bytes, swap_total_bytes,
                load_one, load_five, load_fifteen, disk_used_bytes, disk_total_bytes,
                network_receive_bytes_per_second, network_transmit_bytes_per_second,
                network_total_received_bytes, network_total_transmitted_bytes, process_count
         FROM snapshots
         WHERE node_id = ?1 AND received_at_ms BETWEEN ?2 AND ?3
         ORDER BY received_at_ms ASC, id ASC",
    )?;
    let rows = statement.query_map(
        params![node_id, to_i64(start_ms)?, to_i64(end_ms)?],
        history_row,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn query_aggregate_history(
    connection: &Connection,
    node_id: &str,
    start_ms: u64,
    end_ms: u64,
    bucket_ms: u64,
) -> Result<Vec<Value>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT MIN(node_id), MAX(received_at_ms), CAST(AVG(collected_at_ms) AS INTEGER),
                AVG(cpu_usage_percent), CAST(AVG(memory_used_bytes) AS INTEGER),
                MAX(memory_total_bytes), CAST(AVG(swap_used_bytes) AS INTEGER),
                MAX(swap_total_bytes), AVG(load_one), AVG(load_five), AVG(load_fifteen),
                CAST(AVG(disk_used_bytes) AS INTEGER), MAX(disk_total_bytes),
                CAST(AVG(network_receive_bytes_per_second) AS INTEGER),
                CAST(AVG(network_transmit_bytes_per_second) AS INTEGER),
                MAX(network_total_received_bytes), MAX(network_total_transmitted_bytes),
                CAST(AVG(process_count) AS INTEGER)
         FROM snapshots
         WHERE node_id = ?1 AND received_at_ms BETWEEN ?2 AND ?3
         GROUP BY ((received_at_ms - ?2) / ?4)
         ORDER BY MAX(received_at_ms) ASC",
    )?;
    let rows = statement.query_map(
        params![
            node_id,
            to_i64(start_ms)?,
            to_i64(end_ms)?,
            to_i64(bucket_ms)?
        ],
        history_row,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn history_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let received_at = from_i64(row.get(1)?);
    let collected_at = from_i64(row.get(2)?);
    Ok(json!({
        "client": row.get::<_, String>(0)?,
        "time": format_timestamp(received_at),
        "collected_time": format_timestamp(collected_at),
        "clock_skew_ms": signed_difference(received_at, collected_at),
        "cpu": row.get::<_, f64>(3)?,
        "gpu": Value::Null,
        "ram": from_i64(row.get(4)?),
        "ram_total": from_i64(row.get(5)?),
        "swap": from_i64(row.get(6)?),
        "swap_total": from_i64(row.get(7)?),
        "load": row.get::<_, f64>(8)?,
        "load5": row.get::<_, f64>(9)?,
        "load15": row.get::<_, f64>(10)?,
        "temp": Value::Null,
        "disk": from_i64(row.get(11)?),
        "disk_total": from_i64(row.get(12)?),
        "net_in": from_i64(row.get(13)?),
        "net_out": from_i64(row.get(14)?),
        "net_total_down": from_i64(row.get(15)?),
        "net_total_up": from_i64(row.get(16)?),
        "process": row.get::<_, Option<i64>>(17)?.map(from_i64),
        "connections": Value::Null,
        "connections_udp": Value::Null
    }))
}

fn prune_expired_snapshots(
    transaction: &Transaction<'_>,
    received_at_ms: i64,
    retention_days: u32,
    limit: usize,
) -> rusqlite::Result<usize> {
    let retention_ms = i64::from(retention_days) * 24 * 60 * 60 * 1_000;
    transaction.execute(
        "DELETE FROM snapshots
         WHERE id IN (
            SELECT id FROM snapshots
            WHERE received_at_ms < ?1
            ORDER BY received_at_ms ASC
            LIMIT ?2
         )",
        params![
            received_at_ms.saturating_sub(retention_ms),
            i64::try_from(limit).unwrap_or(i64::MAX)
        ],
    )
}

fn migrate(connection: &mut Connection, from_version: i64) -> rusqlite::Result<()> {
    match from_version {
        0 => create_schema_v2(connection),
        1 => migrate_v1_to_v2(connection),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn create_schema_v2(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE nodes (
            id TEXT PRIMARY KEY,
            token_hash TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            agent_version TEXT NOT NULL,
            operating_system TEXT NOT NULL DEFAULT '',
            kernel_version TEXT NOT NULL DEFAULT '',
            architecture TEXT NOT NULL DEFAULT '',
            cpu_name TEXT NOT NULL DEFAULT '',
            cpu_cores INTEGER NOT NULL DEFAULT 1,
            virtualization TEXT NOT NULL DEFAULT '',
            region TEXT NOT NULL DEFAULT '',
            node_group TEXT NOT NULL DEFAULT '',
            memory_total_bytes INTEGER NOT NULL DEFAULT 0,
            swap_total_bytes INTEGER NOT NULL DEFAULT 0,
            disk_total_bytes INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_seen_at_ms INTEGER,
            disabled_at_ms INTEGER
        );
        CREATE TABLE enrollment_tokens (
            id TEXT PRIMARY KEY,
            token_hash TEXT NOT NULL UNIQUE,
            created_at_ms INTEGER NOT NULL,
            expires_at_ms INTEGER NOT NULL,
            consumed_at_ms INTEGER,
            node_id TEXT UNIQUE REFERENCES nodes(id) ON DELETE SET NULL
        );
        CREATE TABLE snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
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
            UNIQUE(node_id, sample_id)
        );
        CREATE INDEX snapshots_node_received ON snapshots(node_id, received_at_ms DESC);
        CREATE INDEX snapshots_received ON snapshots(received_at_ms);
        CREATE TABLE audit_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            happened_at_ms INTEGER NOT NULL,
            action TEXT NOT NULL,
            subject TEXT NOT NULL
        );
        CREATE INDEX audit_events_time ON audit_events(happened_at_ms DESC);
        PRAGMA user_version = 2;",
    )?;
    transaction.commit()
}

fn migrate_v1_to_v2(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "ALTER TABLE nodes ADD COLUMN disabled_at_ms INTEGER;

         ALTER TABLE enrollment_tokens RENAME TO enrollment_tokens_v1;
         CREATE TABLE enrollment_tokens (
            id TEXT PRIMARY KEY,
            token_hash TEXT NOT NULL UNIQUE,
            created_at_ms INTEGER NOT NULL,
            expires_at_ms INTEGER NOT NULL,
            consumed_at_ms INTEGER,
            node_id TEXT UNIQUE REFERENCES nodes(id) ON DELETE SET NULL
         );
         INSERT INTO enrollment_tokens
            (id, token_hash, created_at_ms, expires_at_ms, consumed_at_ms, node_id)
         SELECT 'legacy-' || substr(token_hash, 1, 24), token_hash,
                consumed_at_ms, consumed_at_ms, consumed_at_ms, node_id
         FROM enrollment_tokens_v1;
         DROP TABLE enrollment_tokens_v1;

         ALTER TABLE snapshots RENAME TO snapshots_v1;
         CREATE TABLE snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
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
            UNIQUE(node_id, sample_id)
         );
         INSERT INTO snapshots (
            id, node_id, sample_id, collected_at_ms, received_at_ms, uptime_seconds,
            cpu_usage_percent, load_one, load_five, load_fifteen,
            memory_total_bytes, memory_used_bytes, swap_total_bytes, swap_used_bytes,
            disk_total_bytes, disk_used_bytes, network_receive_bytes_per_second,
            network_transmit_bytes_per_second, network_total_received_bytes,
            network_total_transmitted_bytes, process_count
         )
         SELECT id, node_id, 'legacy-' || id, collected_at_ms, received_at_ms,
                uptime_seconds, cpu_usage_percent, load_one, load_five, load_fifteen,
                memory_total_bytes, memory_used_bytes, swap_total_bytes, swap_used_bytes,
                disk_total_bytes, disk_used_bytes, network_receive_bytes_per_second,
                network_transmit_bytes_per_second, network_total_received_bytes,
                network_total_transmitted_bytes, NULLIF(process_count, 0)
         FROM snapshots_v1;
         DROP TABLE snapshots_v1;
         CREATE INDEX snapshots_node_received ON snapshots(node_id, received_at_ms DESC);
         CREATE INDEX snapshots_received ON snapshots(received_at_ms);

         CREATE TABLE audit_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            happened_at_ms INTEGER NOT NULL,
            action TEXT NOT NULL,
            subject TEXT NOT NULL
         );
         CREATE INDEX audit_events_time ON audit_events(happened_at_ms DESC);
         PRAGMA user_version = 2;",
    )?;
    transaction.commit()
}

fn insert_audit_transaction(
    transaction: &Transaction<'_>,
    now_ms: u64,
    action: &str,
    subject: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO audit_events (happened_at_ms, action, subject) VALUES (?1, ?2, ?3)",
        params![to_i64(now_ms)?, action, subject],
    )?;
    Ok(())
}

fn prepare_database_path(path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
        set_private_directory_permissions(parent)?;
    }
    if path.exists() {
        ensure_private_file(path, "database")?;
        return Ok(());
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    return Err("Pulse Service database storage is supported only on Unix platforms".into());
    #[cfg(unix)]
    {
        options.open(path)?;
        ensure_private_file(path, "database")?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("Pulse Service database storage is supported only on Unix platforms".into())
}

#[cfg(unix)]
fn ensure_private_file(path: &Path, description: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::os::unix::fs::PermissionsExt;
    if fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{description} {} must have mode 0600 or stricter; fix permissions before starting Pulse",
            path.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_file(
    _path: &Path,
    _description: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("Pulse Service database storage is supported only on Unix platforms".into())
}

#[cfg(unix)]
fn online_backup(source: &Connection, target: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::os::unix::fs::OpenOptionsExt;

    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("backup path must end in a valid UTF-8 file name")?;
    let temporary = target.with_file_name(format!(".{target_name}.partial"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let backup_result = (|| -> Result<(), Box<dyn Error + Send + Sync>> {
        drop(options.open(&temporary)?);
        let mut destination = Connection::open(&temporary)?;
        destination.busy_timeout(Duration::from_secs(5))?;
        {
            let backup = Backup::new(source, &mut destination)?;
            backup.run_to_completion(128, Duration::from_millis(10), None)?;
        }
        let integrity: String =
            destination.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(format!("SQLite backup integrity check failed: {integrity}").into());
        }
        drop(destination);
        ensure_private_file(&temporary, "database backup")?;
        fs::File::open(&temporary)?.sync_all()?;
        fs::hard_link(&temporary, target)?;
        fs::remove_file(&temporary)?;
        ensure_private_file(target, "database backup")?;
        fs::File::open(containing_directory(target))?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = backup_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
fn online_backup(_source: &Connection, _target: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("Pulse Service database backups are supported only on Unix platforms".into())
}

fn containing_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn backup_path(path: &Path) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("database path must end in a valid UTF-8 file name")?;
    Ok(path.with_file_name(format!(
        "{file_name}.backup-{timestamp}-{}",
        Uuid::new_v4().simple()
    )))
}

pub(crate) fn unix_time_ms() -> Result<u64, StorageError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Database(rusqlite::Error::InvalidQuery))?
        .as_millis()
        .try_into()
        .map_err(|_| StorageError::Database(rusqlite::Error::InvalidQuery))
}

fn to_i64(value: u64) -> Result<i64, StorageError> {
    value
        .try_into()
        .map_err(|_| StorageError::Database(rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX)))
}

fn from_i64(value: i64) -> u64 {
    value.try_into().unwrap_or_default()
}

pub(crate) fn format_timestamp(timestamp_ms: u64) -> String {
    let nanos = i128::from(timestamp_ms) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .ok()
        .and_then(|timestamp| timestamp.format(&Rfc3339).ok())
        .unwrap_or_default()
}

pub(crate) fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn signed_difference(left: u64, right: u64) -> i64 {
    let difference = i128::from(left) - i128::from(right);
    difference
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX))
        .try_into()
        .unwrap_or_else(|_| {
            if difference.is_negative() {
                i64::MIN
            } else {
                i64::MAX
            }
        })
}

fn new_agent_token() -> String {
    format!(
        "pulse_agent_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DashboardClient {
    id: String,
    name: String,
    agent_version: String,
    operating_system: String,
    kernel_version: String,
    architecture: String,
    cpu_name: String,
    cpu_cores: u32,
    virtualization: String,
    region: String,
    group: String,
    memory_total_bytes: u64,
    swap_total_bytes: u64,
    disk_total_bytes: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
    last_seen_at_ms: Option<u64>,
}

impl DashboardClient {
    fn emerald_value(&self) -> Value {
        json!({
            "uuid": self.id,
            "name": self.name,
            "cpu_name": self.cpu_name,
            "virtualization": self.virtualization,
            "arch": self.architecture,
            "cpu_cores": self.cpu_cores,
            "os": self.operating_system,
            "kernel_version": self.kernel_version,
            "gpu_name": "",
            "region": self.region,
            "public_remark": "",
            "mem_total": self.memory_total_bytes,
            "swap_total": self.swap_total_bytes,
            "disk_total": self.disk_total_bytes,
            "version": self.agent_version,
            "weight": 0,
            "price": 0,
            "billing_cycle": 0,
            "auto_renewal": false,
            "currency": "USD",
            "expired_at": "",
            "group": self.group,
            "tags": "",
            "hidden": false,
            "traffic_limit": 0,
            "traffic_limit_type": "sum",
            "created_at": format_timestamp(self.created_at_ms),
            "updated_at": format_timestamp(self.updated_at_ms)
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DashboardStatus {
    node_id: String,
    received_at_ms: u64,
    collected_at_ms: u64,
    uptime_seconds: u64,
    cpu_usage_percent: f64,
    load_one: f64,
    load_five: f64,
    load_fifteen: f64,
    memory_total_bytes: u64,
    memory_used_bytes: u64,
    swap_total_bytes: u64,
    swap_used_bytes: u64,
    disk_total_bytes: u64,
    disk_used_bytes: u64,
    network_receive_bytes_per_second: u64,
    network_transmit_bytes_per_second: u64,
    network_total_received_bytes: u64,
    network_total_transmitted_bytes: u64,
    process_count: Option<u64>,
    online: bool,
}

impl DashboardStatus {
    fn emerald_value(&self) -> Value {
        json!({
            "client": self.node_id,
            "time": format_timestamp(self.received_at_ms),
            "collected_time": format_timestamp(self.collected_at_ms),
            "clock_skew_ms": signed_difference(self.received_at_ms, self.collected_at_ms),
            "cpu": self.cpu_usage_percent,
            "gpu": Value::Null,
            "ram": self.memory_used_bytes,
            "ram_total": self.memory_total_bytes,
            "swap": self.swap_used_bytes,
            "swap_total": self.swap_total_bytes,
            "load": self.load_one,
            "load5": self.load_five,
            "load15": self.load_fifteen,
            "temp": Value::Null,
            "disk": self.disk_used_bytes,
            "disk_total": self.disk_total_bytes,
            "net_in": self.network_receive_bytes_per_second,
            "net_out": self.network_transmit_bytes_per_second,
            "net_total_up": self.network_total_transmitted_bytes,
            "net_total_down": self.network_total_received_bytes,
            "process": self.process_count,
            "connections": Value::Null,
            "connections_udp": Value::Null,
            "online": self.online,
            "uptime": self.uptime_seconds
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct DashboardNode {
    client: DashboardClient,
    status: Option<DashboardStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulse_protocol::PROTOCOL_VERSION;
    use tempfile::{TempDir, tempdir};

    fn sample(sample_id: String, collected_at: u64) -> SystemSnapshot {
        SystemSnapshot {
            protocol_version: PROTOCOL_VERSION,
            sample_id,
            collected_at_unix_ms: collected_at,
            host_name: Some("test-node".to_owned()),
            agent_version: "test".to_owned(),
            operating_system: "Linux".to_owned(),
            kernel_version: "6.0".to_owned(),
            architecture: "x86_64".to_owned(),
            cpu_name: "Test CPU".to_owned(),
            cpu_cores: 2,
            virtualization: "kvm".to_owned(),
            region: "SG".to_owned(),
            group: "test".to_owned(),
            uptime_seconds: 100,
            cpu_usage_percent: 12.5,
            load_one: 0.1,
            load_five: 0.2,
            load_fifteen: 0.3,
            memory_total_bytes: 1_024,
            memory_used_bytes: 512,
            swap_total_bytes: 256,
            swap_used_bytes: 0,
            disk_total_bytes: 4_096,
            disk_used_bytes: 1_024,
            network_receive_bytes_per_second: 10,
            network_transmit_bytes_per_second: 20,
            network_total_received_bytes: 100,
            network_total_transmitted_bytes: 200,
            process_count: None,
        }
    }

    fn request() -> EnrollmentRequest {
        EnrollmentRequest {
            protocol_version: PROTOCOL_VERSION,
            node_name: "test-node".to_owned(),
            agent_version: "test".to_owned(),
            region: "SG".to_owned(),
            group: "test".to_owned(),
        }
    }

    fn enrolled_storage() -> (TempDir, Storage, EnrollmentResponse) {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("pulse.db"), 64 * 1024 * 1024).unwrap();
        let enrollment = storage.create_enrollment(600, 10_000).unwrap();
        let credentials = storage
            .enroll(&request(), &hash_token(&enrollment.token), 10, 11_000)
            .unwrap();
        (directory, storage, credentials)
    }

    #[test]
    fn credentials_use_durable_wal_commits() {
        let (_directory, storage, _credentials) = enrolled_storage();
        let connection = storage.connection().unwrap();
        let synchronous: i64 = connection
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            synchronous, 2,
            "FULL synchronization is required for credential lifecycle durability"
        );
    }

    #[test]
    fn latest_status_lookup_does_not_scan_snapshot_history() {
        let (_directory, storage, credentials) = enrolled_storage();
        let mut connection = storage.connection().unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..1_000_i64 {
            insert_snapshot(
                &transaction,
                &credentials.node_id,
                &sample(Uuid::new_v4().to_string(), 1),
                20_000 + index * 5_000,
            )
            .unwrap();
        }
        transaction.commit().unwrap();
        let mut statement = connection.prepare(LATEST_STATUSES_SQL).unwrap();
        let received = statement
            .query_map([], |row| row.get::<_, i64>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(received, vec![5_015_000]);
        assert!(
            statement.get_status(rusqlite::StatementStatus::FullscanStep) < 10,
            "latest status must seek by node, not walk snapshot history"
        );
    }

    #[test]
    fn enrollment_is_expiring_single_use_and_revocable() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(&directory.path().join("pulse.db"), 64 * 1024 * 1024).unwrap();

        let expired = storage.create_enrollment(60, 1_000).unwrap();
        assert!(matches!(
            storage.enroll(&request(), &hash_token(&expired.token), 10, 62_000),
            Err(StorageError::InvalidEnrollment)
        ));

        let revoked = storage.create_enrollment(60, 2_000).unwrap();
        storage.revoke_enrollment(&revoked.id, 3_000).unwrap();
        assert!(matches!(
            storage.enroll(&request(), &hash_token(&revoked.token), 10, 4_000),
            Err(StorageError::InvalidEnrollment)
        ));

        let usable = storage.create_enrollment(60, 5_000).unwrap();
        storage
            .enroll(&request(), &hash_token(&usable.token), 10, 6_000)
            .unwrap();
        assert!(matches!(
            storage.enroll(&request(), &hash_token(&usable.token), 10, 7_000),
            Err(StorageError::InvalidEnrollment)
        ));
    }

    #[test]
    fn server_receive_time_controls_ordering_and_idempotency() {
        let (_directory, storage, credentials) = enrolled_storage();
        let token = hash_token(&credentials.agent_token);
        let duplicate_id = Uuid::new_v4().to_string();
        storage
            .ingest(
                &token,
                &sample(duplicate_id.clone(), 999_999_999),
                20_000,
                7,
            )
            .unwrap();
        storage
            .ingest(&token, &sample(duplicate_id, 1), 25_000, 7)
            .unwrap();
        storage
            .ingest(&token, &sample(Uuid::new_v4().to_string(), 1), 25_000, 7)
            .unwrap();

        let series = storage
            .history(&credentials.node_id, 1, 10, 30_000)
            .unwrap();
        assert_eq!(series.records.len(), 2);
        assert_eq!(series.records[0]["time"], json!(format_timestamp(20_000)));
        assert_eq!(series.records[1]["time"], json!(format_timestamp(25_000)));
        assert_eq!(
            series.records[1]["collected_time"],
            json!(format_timestamp(1))
        );
    }

    #[test]
    fn retention_uses_receive_time_not_agent_clock() {
        let (_directory, storage, credentials) = enrolled_storage();
        let token = hash_token(&credentials.agent_token);
        storage
            .ingest(
                &token,
                &sample(Uuid::new_v4().to_string(), u64::MAX / 2),
                10_000,
                1,
            )
            .unwrap();
        storage
            .ingest(
                &token,
                &sample(Uuid::new_v4().to_string(), 1),
                86_410_001,
                1,
            )
            .unwrap();
        let count: i64 = storage
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn retention_maintenance_expires_history_without_a_new_sample() {
        let (_directory, storage, credentials) = enrolled_storage();
        storage
            .ingest(
                &hash_token(&credentials.agent_token),
                &sample(Uuid::new_v4().to_string(), 10_000),
                10_000,
                1,
            )
            .unwrap();

        let removed = storage.prune_expired(86_410_001, 1).unwrap();
        let count: i64 = storage
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn pruning_before_insert_recovers_from_the_sqlite_page_limit() {
        let (_directory, storage, credentials) = enrolled_storage();
        let token_hash = hash_token(&credentials.agent_token);
        let mut received_at = 100_000_u64;
        for _ in 0..200 {
            storage
                .ingest(
                    &token_hash,
                    &sample(Uuid::new_v4().to_string(), received_at),
                    received_at,
                    365,
                )
                .unwrap();
            received_at += 5_000;
        }
        {
            let connection = storage.connection().unwrap();
            connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
            let page_count: i64 = connection
                .query_row("PRAGMA page_count", [], |row| row.get(0))
                .unwrap();
            connection
                .pragma_update(None, "max_page_count", page_count)
                .unwrap();
        }

        let full_at = (0..2_000)
            .find_map(|_| {
                let candidate_time = received_at;
                received_at += 5_000;
                match storage.ingest(
                    &token_hash,
                    &sample(Uuid::new_v4().to_string(), candidate_time),
                    candidate_time,
                    365,
                ) {
                    Ok(()) => None,
                    Err(StorageError::Database(rusqlite::Error::SqliteFailure(code, _)))
                        if code.code == rusqlite::ErrorCode::DiskFull =>
                    {
                        Some(candidate_time)
                    }
                    Err(error) => panic!("unexpected ingestion error: {error}"),
                }
            })
            .expect("database should reach its configured page limit");

        let recovered_at = full_at + 86_400_001;
        storage
            .ingest(
                &token_hash,
                &sample(Uuid::new_v4().to_string(), recovered_at),
                recovered_at,
                1,
            )
            .unwrap();
        let remaining: (i64, i64) = storage
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*), MAX(received_at_ms) FROM snapshots",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(remaining, (1, i64::try_from(recovered_at).unwrap()));
    }

    #[test]
    fn history_downsampling_covers_entire_window() {
        let (_directory, storage, credentials) = enrolled_storage();
        let token = hash_token(&credentials.agent_token);
        for index in 0..20_u64 {
            storage
                .ingest(
                    &token,
                    &sample(Uuid::new_v4().to_string(), 900_000 - index),
                    100_000 + index * 5_000,
                    7,
                )
                .unwrap();
        }
        let series = storage
            .history(&credentials.node_id, 1, 5, 200_000)
            .unwrap();
        assert!(series.coverage.downsampled);
        assert_eq!(series.coverage.source_points, 20);
        assert!(series.records.len() <= 5);
        assert_eq!(series.coverage.actual_start_unix_ms, Some(100_000));
        assert_eq!(series.coverage.actual_end_unix_ms, Some(195_000));
        assert_eq!(
            series.records.last().unwrap()["time"],
            json!(format_timestamp(195_000))
        );
    }

    #[test]
    fn node_token_rotation_revocation_and_deletion_are_audited() {
        let (_directory, storage, credentials) = enrolled_storage();
        let old_hash = hash_token(&credentials.agent_token);
        let replacement = storage
            .rotate_node_token(&credentials.node_id, 20_000)
            .unwrap();
        assert!(matches!(
            storage.ingest(
                &old_hash,
                &sample(Uuid::new_v4().to_string(), 20_000),
                20_000,
                7
            ),
            Err(StorageError::Unauthorized)
        ));
        storage
            .ingest(
                &hash_token(&replacement),
                &sample(Uuid::new_v4().to_string(), 21_000),
                21_000,
                7,
            )
            .unwrap();
        storage.revoke_node(&credentials.node_id, 30_000).unwrap();
        assert!(matches!(
            storage.ingest(
                &hash_token(&replacement),
                &sample(Uuid::new_v4().to_string(), 31_000),
                31_000,
                7
            ),
            Err(StorageError::Unauthorized)
        ));
        storage.delete_node(&credentials.node_id, 40_000).unwrap();
        let events = storage.audit_events(20).unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.action == "node.token.rotate")
        );
        assert!(events.iter().any(|event| event.action == "node.revoke"));
        assert!(events.iter().any(|event| event.action == "node.delete"));
    }

    #[test]
    fn lifecycle_changes_roll_back_when_audit_insertion_fails() {
        let (_directory, storage, credentials) = enrolled_storage();
        let enrollment = storage.create_enrollment(600, 12_000).unwrap();
        let original_token_hash = hash_token(&credentials.agent_token);
        let (enrollment_count, enrollment_expiry, node_updated_at) = {
            let connection = storage.connection().unwrap();
            let count = connection
                .query_row("SELECT COUNT(*) FROM enrollment_tokens", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap();
            let expiry = connection
                .query_row(
                    "SELECT expires_at_ms FROM enrollment_tokens WHERE id = ?1",
                    params![enrollment.id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap();
            let updated_at = connection
                .query_row(
                    "SELECT updated_at_ms FROM nodes WHERE id = ?1",
                    params![credentials.node_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER reject_audit_insert
                     BEFORE INSERT ON audit_events
                     BEGIN
                       SELECT RAISE(ABORT, 'injected audit failure');
                     END;",
                )
                .unwrap();
            (count, expiry, updated_at)
        };

        assert!(storage.create_enrollment(600, 20_000).is_err());
        assert!(storage.revoke_enrollment(&enrollment.id, 20_000).is_err());
        assert!(
            storage
                .rotate_node_token(&credentials.node_id, 20_000)
                .is_err()
        );
        assert!(storage.revoke_node(&credentials.node_id, 20_000).is_err());

        let connection = storage.connection().unwrap();
        let current_enrollment_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM enrollment_tokens", [], |row| {
                row.get(0)
            })
            .unwrap();
        let current_enrollment_expiry: i64 = connection
            .query_row(
                "SELECT expires_at_ms FROM enrollment_tokens WHERE id = ?1",
                params![enrollment.id],
                |row| row.get(0),
            )
            .unwrap();
        let (current_token_hash, current_updated_at, disabled_at): (String, i64, Option<i64>) =
            connection
                .query_row(
                    "SELECT token_hash, updated_at_ms, disabled_at_ms FROM nodes WHERE id = ?1",
                    params![credentials.node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
        assert_eq!(current_enrollment_count, enrollment_count);
        assert_eq!(current_enrollment_expiry, enrollment_expiry);
        assert_eq!(current_token_hash, original_token_hash);
        assert_eq!(current_updated_at, node_updated_at);
        assert_eq!(disabled_at, None);
    }

    #[cfg(unix)]
    #[test]
    fn database_and_backups_have_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let database = directory.path().join("pulse.db");
        let storage = Storage::open(&database, 64 * 1024 * 1024).unwrap();
        assert_eq!(
            fs::metadata(&database).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let backup = storage.backup().unwrap();
        assert_eq!(
            fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fs::set_permissions(&database, fs::Permissions::from_mode(0o644)).unwrap();
        drop(storage);
        assert!(Storage::open(&database, 64 * 1024 * 1024).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn online_backup_remains_consistent_during_external_writes() {
        use std::{
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            thread,
        };

        let directory = tempdir().unwrap();
        let database = directory.path().join("pulse.db");
        let storage = Storage::open(&database, 64 * 1024 * 1024).unwrap();
        {
            let connection = storage.connection().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE backup_invariant (
                        id INTEGER PRIMARY KEY,
                        value INTEGER NOT NULL,
                        padding BLOB NOT NULL
                     );
                     INSERT INTO backup_invariant VALUES (1, 0, zeroblob(1048576));
                     INSERT INTO backup_invariant VALUES (2, 0, zeroblob(1048576));
                     CREATE TABLE backup_padding (payload BLOB NOT NULL);
                     WITH RECURSIVE counter(value) AS (
                        SELECT 1
                        UNION ALL
                        SELECT value + 1 FROM counter WHERE value < 1000
                     )
                     INSERT INTO backup_padding(payload)
                     SELECT zeroblob(4096) FROM counter;",
                )
                .unwrap();
        }

        let completed_writes = Arc::new(AtomicUsize::new(0));
        let writer_counter = Arc::clone(&completed_writes);
        let writer_database = database.clone();
        let writer = thread::spawn(move || {
            let mut connection = Connection::open(writer_database).unwrap();
            connection.busy_timeout(Duration::from_secs(2)).unwrap();
            for value in 1..=50_i64 {
                let transaction = connection.transaction().unwrap();
                transaction
                    .execute(
                        "UPDATE backup_invariant SET value = ?1 WHERE id = 1",
                        params![value],
                    )
                    .unwrap();
                thread::sleep(Duration::from_millis(1));
                transaction
                    .execute(
                        "UPDATE backup_invariant SET value = ?1 WHERE id = 2",
                        params![value],
                    )
                    .unwrap();
                transaction.commit().unwrap();
                writer_counter.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(1));
            }
        });
        while completed_writes.load(Ordering::SeqCst) == 0 {
            thread::yield_now();
        }
        let writes_before_backup = completed_writes.load(Ordering::SeqCst);
        let backup_result = storage.backup();
        writer.join().unwrap();
        assert!(completed_writes.load(Ordering::SeqCst) > writes_before_backup);

        let backup = backup_result.unwrap();
        let backup_connection = Connection::open(backup).unwrap();
        let values: (i64, i64) = backup_connection
            .query_row(
                "SELECT
                    (SELECT value FROM backup_invariant WHERE id = 1),
                    (SELECT value FROM backup_invariant WHERE id = 2)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let padding_rows: i64 = backup_connection
            .query_row("SELECT COUNT(*) FROM backup_padding", [], |row| row.get(0))
            .unwrap();
        let integrity: String = backup_connection
            .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
            .unwrap();
        assert_eq!(values.0, values.1);
        assert_eq!(padding_rows, 1_000);
        assert_eq!(integrity, "ok");
    }

    #[cfg(unix)]
    #[test]
    fn existing_database_larger_than_configured_limit_fails_closed() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("pulse.db");
        drop(Storage::open(&database, 64 * 1024 * 1024).unwrap());

        let error = Storage::open(&database, 1)
            .err()
            .expect("oversized database should be rejected")
            .to_string();
        assert!(error.contains("PULSE_MAX_DATABASE_BYTES"));
    }

    #[cfg(unix)]
    #[test]
    fn v1_database_is_backed_up_and_migrated_forward() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let directory = tempdir().unwrap();
        let database = directory.path().join("pulse.db");
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&database)
            .unwrap();
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE nodes (
                    id TEXT PRIMARY KEY, token_hash TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                    agent_version TEXT NOT NULL, operating_system TEXT NOT NULL DEFAULT '',
                    kernel_version TEXT NOT NULL DEFAULT '', architecture TEXT NOT NULL DEFAULT '',
                    cpu_name TEXT NOT NULL DEFAULT '', cpu_cores INTEGER NOT NULL DEFAULT 1,
                    virtualization TEXT NOT NULL DEFAULT '', region TEXT NOT NULL DEFAULT '',
                    node_group TEXT NOT NULL DEFAULT '', memory_total_bytes INTEGER NOT NULL DEFAULT 0,
                    swap_total_bytes INTEGER NOT NULL DEFAULT 0, disk_total_bytes INTEGER NOT NULL DEFAULT 0,
                    created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL, last_seen_at_ms INTEGER
                 );
                 CREATE TABLE enrollment_tokens (
                    token_hash TEXT PRIMARY KEY, consumed_at_ms INTEGER NOT NULL,
                    node_id TEXT NOT NULL UNIQUE REFERENCES nodes(id) ON DELETE CASCADE
                 );
                 CREATE TABLE snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
                    collected_at_ms INTEGER NOT NULL, received_at_ms INTEGER NOT NULL,
                    uptime_seconds INTEGER NOT NULL, cpu_usage_percent REAL NOT NULL,
                    load_one REAL NOT NULL, load_five REAL NOT NULL, load_fifteen REAL NOT NULL,
                    memory_total_bytes INTEGER NOT NULL, memory_used_bytes INTEGER NOT NULL,
                    swap_total_bytes INTEGER NOT NULL, swap_used_bytes INTEGER NOT NULL,
                    disk_total_bytes INTEGER NOT NULL, disk_used_bytes INTEGER NOT NULL,
                    network_receive_bytes_per_second INTEGER NOT NULL,
                    network_transmit_bytes_per_second INTEGER NOT NULL,
                    network_total_received_bytes INTEGER NOT NULL,
                    network_total_transmitted_bytes INTEGER NOT NULL,
                    process_count INTEGER NOT NULL, UNIQUE(node_id, collected_at_ms)
                 );
                 INSERT INTO nodes (id, token_hash, name, agent_version, created_at_ms, updated_at_ms)
                 VALUES ('node-1', 'hash', 'legacy', 'v1', 1, 1);
                 INSERT INTO enrollment_tokens VALUES ('enrollment', 2, 'node-1');
                 INSERT INTO snapshots (
                    node_id, collected_at_ms, received_at_ms, uptime_seconds, cpu_usage_percent,
                    load_one, load_five, load_fifteen, memory_total_bytes, memory_used_bytes,
                    swap_total_bytes, swap_used_bytes, disk_total_bytes, disk_used_bytes,
                    network_receive_bytes_per_second, network_transmit_bytes_per_second,
                    network_total_received_bytes, network_total_transmitted_bytes, process_count
                 ) VALUES ('node-1', 3, 4, 5, 6, 1, 1, 1, 10, 5, 0, 0, 10, 5, 1, 1, 1, 1, 0);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let storage = Storage::open(&database, 64 * 1024 * 1024).unwrap();
        let connection = storage.connection().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        let process: Option<i64> = connection
            .query_row("SELECT process_count FROM snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(process, None);
        drop(connection);
        let backups: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("backup-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            backups[0].metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
