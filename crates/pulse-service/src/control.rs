//! Bounded monitoring configuration, probe history, asset metadata and alerts.

use std::collections::HashSet;

use pulse_protocol::{AgentRuntimeConfig, ProbeBatch, ProbeKind, ProbeTask, SystemSnapshot};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use time::{Date, Month, OffsetDateTime};
use uuid::Uuid;

use crate::storage::{Storage, StorageError, hash_token};

const MAX_CONFIG_ROWS: usize = 128;
const MAX_CHANNELS: usize = 16;
const MAX_NODE_ASSIGNMENTS: usize = 10_000;
const MAX_PROBE_HISTORY: usize = 1_000;
const MAX_INCIDENTS: usize = 1_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub site_name: String,
    pub private_site: bool,
    pub agent_interval_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            site_name: "Pulse".into(),
            private_site: true,
            agent_interval_seconds: 3,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct NodeOptions {
    pub id: String,
    pub name: String,
    pub region: String,
    pub group: String,
    pub weight: i32,
    pub hidden: bool,
    pub tags: String,
    pub public_remark: String,
    pub price: f64,
    pub currency: String,
    pub billing_cycle_days: u32,
    pub expired_at_unix_ms: Option<u64>,
    pub auto_renewal: bool,
    pub traffic_limit_bytes: u64,
    pub traffic_limit_type: String,
    pub traffic_reset_day: u8,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProbeDefinition {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: ProbeKind,
    pub target: String,
    pub interval_seconds: u64,
    pub timeout_seconds: u64,
    pub enabled: bool,
    pub node_ids: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Channel {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AlertRule {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub node_ids: Vec<String>,
    pub metric: String,
    pub threshold: f64,
    pub duration_seconds: u64,
    pub cooldown_seconds: u64,
    pub channel_ids: Vec<String>,
    pub enabled: bool,
}

pub(crate) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE monitoring_settings (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
         INSERT INTO monitoring_settings VALUES(1, '{\"site_name\":\"Pulse\",\"private_site\":true,\"agent_interval_seconds\":3}');
         CREATE TABLE node_options (node_id TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE, data TEXT NOT NULL);
         CREATE TABLE traffic_periods (node_id TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
             cycle_start_ms INTEGER NOT NULL, raw_up INTEGER NOT NULL, raw_down INTEGER NOT NULL,
             used_up INTEGER NOT NULL, used_down INTEGER NOT NULL);
         CREATE TABLE probe_tasks (id TEXT PRIMARY KEY, data TEXT NOT NULL);
         CREATE TABLE probe_results (id INTEGER PRIMARY KEY, node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
             task_id TEXT NOT NULL REFERENCES probe_tasks(id) ON DELETE CASCADE, sample_id TEXT NOT NULL,
             collected_at_ms INTEGER NOT NULL, received_at_ms INTEGER NOT NULL, latency_ms REAL,
             success INTEGER NOT NULL, error TEXT, UNIQUE(node_id, sample_id));
         CREATE INDEX probe_node_task_time ON probe_results(node_id,task_id,received_at_ms DESC);
         CREATE INDEX probe_received ON probe_results(received_at_ms);
         CREATE TABLE alert_channels (id TEXT PRIMARY KEY, data TEXT NOT NULL);
         CREATE TABLE alert_rules (id TEXT PRIMARY KEY, data TEXT NOT NULL);
         CREATE TABLE alert_incidents (id TEXT PRIMARY KEY, node_id TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
             rule_id TEXT NOT NULL REFERENCES alert_rules(id) ON DELETE CASCADE, status TEXT NOT NULL,
             message TEXT NOT NULL, opened_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
             last_sent_at_ms INTEGER NOT NULL DEFAULT 0, UNIQUE(node_id,rule_id));
         CREATE INDEX alert_incidents_rule ON alert_incidents(rule_id,node_id);
         CREATE INDEX alert_incidents_updated ON alert_incidents(updated_at_ms DESC);
         CREATE TABLE alert_deliveries (id TEXT PRIMARY KEY, incident_id TEXT NOT NULL REFERENCES alert_incidents(id) ON DELETE CASCADE,
             channel_id TEXT NOT NULL REFERENCES alert_channels(id) ON DELETE CASCADE, payload TEXT NOT NULL,
             attempts INTEGER NOT NULL DEFAULT 0, next_attempt_ms INTEGER NOT NULL, created_at_ms INTEGER NOT NULL,
             UNIQUE(incident_id,channel_id));
         ALTER TABLE snapshots ADD COLUMN extensions TEXT NOT NULL DEFAULT '{}';",
    )
}

fn encode<T: Serialize>(value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|_| StorageError::InvalidInput("invalid configuration"))
}

fn decode<T: DeserializeOwned>(value: &str) -> Result<T, StorageError> {
    serde_json::from_str(value)
        .map_err(|_| StorageError::InvalidInput("stored configuration is invalid"))
}

fn text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}

fn integer(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
fn unsigned(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn list<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    limit: usize,
) -> Result<Vec<T>, StorageError> {
    // Table names are static internal constants, never request values.
    let mut statement = db.prepare(&format!("SELECT data FROM {table} ORDER BY id LIMIT ?1"))?;
    let rows = statement.query_map([i64::try_from(limit).unwrap_or(128)], |row| {
        row.get::<_, String>(0)
    })?;
    rows.map(|row| decode(&row?)).collect()
}

fn save<T: Serialize>(
    db: &Connection,
    table: &str,
    id: &str,
    data: &T,
    limit: usize,
) -> Result<(), StorageError> {
    let tx = db.unchecked_transaction()?;
    let count: i64 = db.query_row(
        &format!("SELECT count(*) FROM {table} WHERE id != ?1"),
        [id],
        |row| row.get(0),
    )?;
    if count >= i64::try_from(limit).unwrap_or(i64::MAX) {
        return Err(StorageError::InvalidInput("configuration limit reached"));
    }
    tx.execute(&format!("INSERT INTO {table}(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data"), params![id,encode(data)?])?;
    configuration_audit(&tx, "configuration.save", &format!("{table}:{id}"))?;
    tx.commit()?;
    Ok(())
}

fn configuration_audit(
    tx: &Transaction<'_>,
    action: &str,
    subject: &str,
) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO audit_events(happened_at_ms,action,subject) VALUES(?1,?2,?3)",
        params![integer(crate::storage::unix_time_ms()?), action, subject],
    )?;
    tx.execute("DELETE FROM audit_events WHERE id IN (SELECT id FROM audit_events ORDER BY id DESC LIMIT 10000 OFFSET 10000)",[])?;
    Ok(())
}

fn options_or_none(db: &Connection, id: &str) -> Result<Option<NodeOptions>, StorageError> {
    let value: Option<String> = db
        .query_row(
            "SELECT data FROM node_options WHERE node_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    value.map(|value| decode(&value)).transpose()
}

fn verify_nodes(db: &Connection, ids: &[String]) -> Result<(), StorageError> {
    if ids.len() > MAX_NODE_ASSIGNMENTS || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(StorageError::InvalidInput("invalid node selection"));
    }
    for id in ids {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=?1 AND disabled_at_ms IS NULL)",
            [id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::NotFound);
        }
    }
    Ok(())
}

impl Storage {
    pub(crate) fn settings(&self) -> Result<Settings, StorageError> {
        let db = self.connection()?;
        decode(&db.query_row::<String, _, _>(
            "SELECT data FROM monitoring_settings WHERE id=1",
            [],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn save_settings(&self, value: &Settings) -> Result<(), StorageError> {
        if value.site_name.trim().is_empty()
            || !text(&value.site_name, 128)
            || !(1..=300).contains(&value.agent_interval_seconds)
        {
            return Err(StorageError::InvalidInput("invalid site settings"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE monitoring_settings SET data=?1 WHERE id=1",
            [encode(value)?],
        )?;
        configuration_audit(&tx, "settings.update", "site")?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn admin_state(&self) -> Result<Value, StorageError> {
        let settings = self.settings()?;
        let db = self.connection()?;
        let mut statement = db.prepare("SELECT id,name,region,node_group,disabled_at_ms FROM nodes ORDER BY created_at_ms LIMIT 10000")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })?;
        let mut nodes = Vec::new();
        for row in rows {
            let (id, name, region, group, disabled) = row?;
            let options = options_or_none(&db, &id)?.unwrap_or(NodeOptions {
                id: id.clone(),
                name,
                region,
                group,
                currency: "USD".into(),
                traffic_limit_type: "sum".into(),
                traffic_reset_day: 1,
                ..NodeOptions::default()
            });
            let mut value = serde_json::to_value(options)
                .map_err(|_| StorageError::InvalidInput("invalid node metadata"))?;
            value["disabled"] = json!(disabled.is_some());
            let (up, down) = period_usage(&db, &id)?;
            value["traffic_used_up"] = json!(up);
            value["traffic_used_down"] = json!(down);
            nodes.push(value);
        }
        let mut failure_query=db.prepare("SELECT happened_at_ms,subject FROM audit_events WHERE action='notification.failed' ORDER BY id DESC LIMIT 100")?;
        let failures=failure_query.query_map([],|r|Ok(json!({"happened_at_unix_ms":r.get::<_,i64>(0)?,"subject":r.get::<_,String>(1)?})))?.collect::<Result<Vec<_>,_>>()?;
        Ok(json!({"settings":settings,"nodes":nodes,
            "probes":list::<ProbeDefinition>(&db,"probe_tasks",MAX_CONFIG_ROWS)?,
            "channels":list::<Channel>(&db,"alert_channels",MAX_CHANNELS)?,
            "alert_rules":list::<AlertRule>(&db,"alert_rules",MAX_CONFIG_ROWS)?,
            "incidents":incidents(&db)?,"notification_failures":failures}))
    }

    pub(crate) fn save_node_options(
        &self,
        id: &str,
        options: &mut NodeOptions,
    ) -> Result<(), StorageError> {
        id.clone_into(&mut options.id);
        if options.name.trim().is_empty()
            || !text(&options.name, 128)
            || !text(&options.region, 16)
            || !text(&options.group, 128)
            || !text(&options.tags, 512)
            || !text(&options.public_remark, 2048)
            || !options.price.is_finite()
            || !(0.0..=1_000_000_000.0).contains(&options.price)
            || options.currency.len() != 3
            || !options.currency.bytes().all(|c| c.is_ascii_uppercase())
            || options.billing_cycle_days > 36500
            || options.traffic_limit_bytes > i64::MAX as u64
            || !(1..=28).contains(&options.traffic_reset_day)
            || !["sum", "up", "down", "min", "max"].contains(&options.traffic_limit_type.as_str())
            || options
                .expired_at_unix_ms
                .is_some_and(|v| v > 32_503_680_000_000)
        {
            return Err(StorageError::InvalidInput("invalid node metadata"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        verify_nodes(&tx, &[id.to_owned()])?;
        tx.execute("INSERT INTO node_options(node_id,data) VALUES(?1,?2) ON CONFLICT(node_id) DO UPDATE SET data=excluded.data",params![id,encode(options)?])?;
        configuration_audit(&tx, "node.metadata.update", id)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn reset_traffic(&self, id: &str) -> Result<(), StorageError> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        verify_nodes(&tx, &[id.to_owned()])?;
        tx.execute(
            "UPDATE traffic_periods SET used_up=0,used_down=0 WHERE node_id=?1",
            [id],
        )?;
        configuration_audit(&tx, "node.traffic.reset", id)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn save_probe(&self, task: &mut ProbeDefinition) -> Result<(), StorageError> {
        if task.id.is_empty() {
            task.id = Uuid::new_v4().to_string();
        }
        if Uuid::parse_str(&task.id).is_err()
            || task.name.trim().is_empty()
            || !text(&task.name, 128)
            || !text(&task.target, 2048)
            || !(5..=3600).contains(&task.interval_seconds)
            || !(1..=30).contains(&task.timeout_seconds)
            || task.timeout_seconds > task.interval_seconds
        {
            return Err(StorageError::InvalidInput("invalid probe task"));
        }
        validate_target(task.kind, &task.target)?;
        let db = self.connection()?;
        verify_nodes(&db, &task.node_ids)?;
        let tasks = list::<ProbeDefinition>(&db, "probe_tasks", MAX_CONFIG_ROWS)?;
        // Empty node selection applies to every node; never silently truncate assignments.
        let mut nodes = HashSet::new();
        for t in tasks.iter().chain(std::iter::once(&*task)) {
            nodes.extend(t.node_ids.iter().cloned());
        }
        nodes.insert(String::new());
        for id in nodes {
            let count = tasks
                .iter()
                .filter(|t| {
                    t.id != task.id
                        && t.enabled
                        && (t.node_ids.is_empty() || t.node_ids.contains(&id))
                })
                .count()
                + usize::from(
                    task.enabled && (task.node_ids.is_empty() || task.node_ids.contains(&id)),
                );
            if count > pulse_protocol::MAX_PROBE_TASKS {
                return Err(StorageError::InvalidInput(
                    "at most 16 probes may apply to one node",
                ));
            }
        }
        save(&db, "probe_tasks", &task.id, task, MAX_CONFIG_ROWS)
    }

    pub(crate) fn save_channel(&self, channel: &mut Channel) -> Result<(), StorageError> {
        if channel.id.is_empty() {
            channel.id = Uuid::new_v4().to_string();
        }
        if Uuid::parse_str(&channel.id).is_err()
            || channel.name.trim().is_empty()
            || !text(&channel.name, 128)
            || channel.kind != "webhook"
        {
            return Err(StorageError::InvalidInput("invalid notification channel"));
        }
        validate_webhook(&channel.url)?;
        let db = self.connection()?;
        save(&db, "alert_channels", &channel.id, channel, MAX_CHANNELS)
    }

    pub(crate) fn save_alert_rule(&self, rule: &mut AlertRule) -> Result<(), StorageError> {
        if rule.id.is_empty() {
            rule.id = Uuid::new_v4().to_string();
        }
        if Uuid::parse_str(&rule.id).is_err()
            || rule.name.trim().is_empty()
            || !text(&rule.name, 128)
            || !["offline", "cpu", "memory", "disk", "traffic", "expiry"]
                .contains(&rule.metric.as_str())
            || !rule.threshold.is_finite()
            || !(0.0..=3650.0).contains(&rule.threshold)
            || rule.duration_seconds > 86400
            || !(60..=604_800).contains(&rule.cooldown_seconds)
            || rule.channel_ids.len() > MAX_CHANNELS
            || rule.channel_ids.iter().collect::<HashSet<_>>().len() != rule.channel_ids.len()
        {
            return Err(StorageError::InvalidInput("invalid alert rule"));
        }
        if ["cpu", "memory", "disk", "traffic"].contains(&rule.metric.as_str())
            && rule.threshold > 100.0
        {
            return Err(StorageError::InvalidInput(
                "percentage threshold must be between 0 and 100",
            ));
        }
        let db = self.connection()?;
        verify_nodes(&db, &rule.node_ids)?;
        for id in &rule.channel_ids {
            let found: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM alert_channels WHERE id=?1)",
                [id],
                |row| row.get(0),
            )?;
            if !found {
                return Err(StorageError::InvalidInput("unknown alert channel"));
            }
        }
        save(&db, "alert_rules", &rule.id, rule, MAX_CONFIG_ROWS)
    }

    pub(crate) fn delete_monitoring_config(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<(), StorageError> {
        let table = match kind {
            "probes" => "probe_tasks",
            "channels" => "alert_channels",
            "alert-rules" => "alert_rules",
            _ => return Err(StorageError::NotFound),
        };
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let changed = tx.execute(&format!("DELETE FROM {table} WHERE id=?1"), [id])?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        if kind == "channels" {
            for mut rule in list::<AlertRule>(&tx, "alert_rules", MAX_CONFIG_ROWS)? {
                if rule.channel_ids.iter().any(|channel| channel == id) {
                    rule.channel_ids.retain(|channel| channel != id);
                    tx.execute(
                        "UPDATE alert_rules SET data=?2 WHERE id=?1",
                        params![rule.id, encode(&rule)?],
                    )?;
                }
            }
        }
        configuration_audit(&tx, "configuration.delete", &format!("{table}:{id}"))?;
        tx.commit()?;
        // The control deletion is authoritative if cleanup is interrupted.
        // Startup also removes results whose task no longer exists.
        if kind == "probes" {
            db.execute("DELETE FROM metrics.probe_results WHERE task_id=?1", [id])?;
        }
        Ok(())
    }

    pub(crate) fn agent_runtime_config(
        &self,
        token: &str,
    ) -> Result<AgentRuntimeConfig, StorageError> {
        let settings = self.settings()?;
        let db = self.connection()?;
        let id = authenticated_node(&db, token)?;
        let probes = list::<ProbeDefinition>(&db, "probe_tasks", MAX_CONFIG_ROWS)?
            .into_iter()
            .filter(|t| t.enabled && (t.node_ids.is_empty() || t.node_ids.contains(&id)))
            .map(|t| ProbeTask {
                id: t.id,
                name: t.name,
                kind: t.kind,
                target: t.target,
                interval_seconds: t.interval_seconds,
                timeout_seconds: t.timeout_seconds,
                enabled: t.enabled,
            })
            .collect();
        Ok(AgentRuntimeConfig {
            schema_version: 1,
            interval_seconds: settings.agent_interval_seconds,
            probes,
        })
    }

    pub(crate) fn ingest_probes(
        &self,
        token: &str,
        batch: &ProbeBatch,
        now: u64,
        retention_days: u32,
    ) -> Result<(), StorageError> {
        if batch.schema_version != 1 || batch.results.len() > 16 {
            return Err(StorageError::InvalidInput("invalid probe batch"));
        }
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let node = authenticated_node(&tx, token)?;
        let tasks = list::<ProbeDefinition>(&tx, "probe_tasks", MAX_CONFIG_ROWS)?;
        let pruned = self.prune_before_ingest(&tx, now, retention_days)?;
        for result in &batch.results {
            let task = tasks
                .iter()
                .find(|t| {
                    t.id == result.task_id
                        && t.enabled
                        && (t.node_ids.is_empty() || t.node_ids.contains(&node))
                })
                .ok_or(StorageError::InvalidInput(
                    "probe not assigned to this node",
                ))?;
            if Uuid::parse_str(&result.sample_id).is_err()
                || result.collected_at_unix_ms > i64::MAX as u64
                || result
                    .latency_ms
                    .is_some_and(|v| !v.is_finite() || v < 0.0 || v > 30_000.0)
                || result.success != result.latency_ms.is_some()
                || (result.success && result.error.is_some())
                || result.error.as_ref().is_some_and(|e| {
                    ![
                        "timeout",
                        "dns",
                        "connect",
                        "http_status",
                        "response_read",
                        "response_too_large",
                        "unavailable",
                        "probe_failed",
                        "invalid_target",
                        "response_parse",
                    ]
                    .contains(&e.as_str())
                })
            {
                return Err(StorageError::InvalidInput("invalid probe result"));
            }
            let duplicate: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM probe_results WHERE node_id=?1 AND sample_id=?2)",
                params![node, result.sample_id],
                |row| row.get(0),
            )?;
            if duplicate {
                continue;
            }
            // A slow probe followed by a fast one may legitimately arrive together.
            // Bound the receive-time window without trusting the Agent clock.
            let recent:u32=tx.query_row("SELECT count(*) FROM probe_results WHERE node_id=?1 AND task_id=?2 AND received_at_ms>?3",params![node,task.id,integer(now.saturating_sub(task.interval_seconds*1000))],|row|row.get(0))?;
            if recent >= 2 {
                return Err(StorageError::RateLimited);
            }
            tx.prepare_cached("INSERT INTO probe_results(node_id,task_id,sample_id,collected_at_ms,received_at_ms,latency_ms,success,error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)")?.execute(params![node,result.task_id,result.sample_id,integer(result.collected_at_unix_ms),integer(now),result.latency_ms,result.success,result.error])?;
        }
        // Hard per-node cap, independent of retention or sample frequency.
        tx.execute("DELETE FROM probe_results WHERE id IN (SELECT id FROM probe_results WHERE node_id=?1 ORDER BY received_at_ms DESC,id DESC LIMIT 10000 OFFSET 100000)",[node])?;
        tx.commit()?;
        self.record_ingest_prune(pruned, now);
        Ok(())
    }

    pub(crate) fn probe_history(
        &self,
        node: &str,
        hours: u32,
        now: u64,
    ) -> Result<Value, StorageError> {
        let db = self.connection()?;
        if !visible(&db, node)? {
            return Err(StorageError::NotFound);
        }
        let tasks: Vec<_> = list::<ProbeDefinition>(&db, "probe_tasks", MAX_CONFIG_ROWS)?
            .into_iter()
            .filter(|t| t.node_ids.is_empty() || t.node_ids.iter().any(|id| id == node))
            .collect();
        let start = integer(now.saturating_sub(u64::from(hours.clamp(1, 8760)) * 3_600_000));
        let mut statement=db.prepare("SELECT task_id,collected_at_ms,received_at_ms,latency_ms,success,error FROM probe_results WHERE node_id=?1 AND task_id IN (SELECT id FROM probe_tasks) AND received_at_ms>=?2 ORDER BY received_at_ms DESC,id DESC LIMIT ?3")?;
        let records=statement.query_map(params![node,start,i64::try_from(MAX_PROBE_HISTORY).expect("probe history limit fits SQLite")],|row|Ok(json!({"task_id":row.get::<_,String>(0)?,"collected_at_unix_ms":row.get::<_,i64>(1)?,"received_at_unix_ms":row.get::<_,i64>(2)?,"latency_ms":row.get::<_,Option<f64>>(3)?,"success":row.get::<_,bool>(4)?,"error":row.get::<_,Option<String>>(5)?})))?.collect::<Result<Vec<_>,_>>()?;
        let mut statement=db.prepare("SELECT task_id,count(*),sum(CASE WHEN success=0 THEN 1 ELSE 0 END)*100.0/count(*),avg(latency_ms) FROM probe_results WHERE node_id=?1 AND task_id IN (SELECT id FROM probe_tasks) AND received_at_ms>=?2 GROUP BY task_id")?;
        let summary=statement.query_map(params![node,start],|row|Ok(json!({"task_id":row.get::<_,String>(0)?,"samples":row.get::<_,i64>(1)?,"loss_percent":row.get::<_,f64>(2)?,"avg_latency_ms":row.get::<_,Option<f64>>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
        // Public readers need names/types, never private probe targets or node assignments.
        let tasks:Vec<_>=tasks.into_iter().map(|t|json!({"id":t.id,"name":t.name,"kind":t.kind,"interval_seconds":t.interval_seconds})).collect();
        Ok(
            json!({"tasks":tasks,"records":records,"summary":summary,"limit":MAX_PROBE_HISTORY,"history_order":"newest_first"}),
        )
    }
}

fn authenticated_node(db: &Connection, token: &str) -> Result<String, StorageError> {
    db.query_row(
        "SELECT id FROM nodes WHERE token_hash=?1 AND disabled_at_ms IS NULL",
        [hash_token(token)],
        |row| row.get(0),
    )
    .optional()?
    .ok_or(StorageError::Unauthorized)
}

pub(crate) fn visible(db: &Connection, node: &str) -> Result<bool, StorageError> {
    let active: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE id=?1 AND disabled_at_ms IS NULL)",
        [node],
        |row| row.get(0),
    )?;
    Ok(active && !options_or_none(db, node)?.is_some_and(|o| o.hidden))
}

/// A configured slow reporting interval must not manufacture offline flapping.
pub(crate) fn effective_offline_ms(db: &Connection, configured: u64) -> Result<u64, StorageError> {
    let raw: String = db.query_row("SELECT data FROM monitoring_settings WHERE id=1", [], |r| {
        r.get(0)
    })?;
    let settings: Settings = decode(&raw)?;
    Ok(configured.max(settings.agent_interval_seconds.saturating_mul(3000)))
}

pub(crate) fn remove_node_assignments(
    tx: &Transaction<'_>,
    node: &str,
) -> Result<(), StorageError> {
    for mut task in list::<ProbeDefinition>(tx, "probe_tasks", MAX_CONFIG_ROWS)? {
        if task.node_ids.iter().any(|id| id == node) {
            task.node_ids.retain(|id| id != node);
            // Empty means global; deleting the last explicit target must never widen scope.
            if task.node_ids.is_empty() {
                task.enabled = false;
            }
            tx.execute(
                "UPDATE probe_tasks SET data=?2 WHERE id=?1",
                params![task.id, encode(&task)?],
            )?;
        }
    }
    for mut rule in list::<AlertRule>(tx, "alert_rules", MAX_CONFIG_ROWS)? {
        if rule.node_ids.iter().any(|id| id == node) {
            rule.node_ids.retain(|id| id != node);
            if rule.node_ids.is_empty() {
                rule.enabled = false;
            }
            tx.execute(
                "UPDATE alert_rules SET data=?2 WHERE id=?1",
                params![rule.id, encode(&rule)?],
            )?;
        }
    }
    Ok(())
}

fn validate_target(kind: ProbeKind, target: &str) -> Result<(), StorageError> {
    if target.trim() != target || target.is_empty() {
        return Err(StorageError::InvalidInput("invalid probe target"));
    }
    match kind {
        ProbeKind::Http => {
            let url = reqwest::Url::parse(target)
                .map_err(|_| StorageError::InvalidInput("invalid HTTP probe URL"))?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(StorageError::InvalidInput("invalid HTTP probe URL"));
            }
        }
        ProbeKind::Tcp => {
            let url = reqwest::Url::parse(&format!("tcp://{target}"))
                .map_err(|_| StorageError::InvalidInput("TCP target must be host:port"))?;
            if url.host_str().is_none()
                || url.port().is_none_or(|port| port == 0)
                || !url.username().is_empty()
                || url.password().is_some()
                || !url.path().is_empty()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(StorageError::InvalidInput("TCP target must be host:port"));
            }
            validate_host(url.host_str().unwrap_or("").trim_matches(['[', ']']))?;
        }
        ProbeKind::Icmp => validate_host(target)?,
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<(), StorageError> {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    if host.is_empty()
        || host.len() > 253
        || host.trim_end_matches('.').split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(StorageError::InvalidInput(
            "target must be a valid hostname or IP",
        ));
    }
    Ok(())
}

pub(crate) fn validate_webhook(raw: &str) -> Result<reqwest::Url, StorageError> {
    if raw.len() > 2048 {
        return Err(StorageError::InvalidInput("webhook URL too long"));
    }
    let url =
        reqwest::Url::parse(raw).map_err(|_| StorageError::InvalidInput("invalid webhook URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(StorageError::InvalidInput(
            "webhook must be HTTPS without user information",
        ));
    }
    Ok(url)
}

pub(crate) fn period_start(now: u64, day: u8) -> u64 {
    let Ok(date) = OffsetDateTime::from_unix_timestamp(integer(now / 1000)) else {
        return now;
    };
    let (mut year, mut month) = (date.year(), date.month());
    if date.day() < day {
        if month == Month::January {
            year -= 1;
            month = Month::December;
        } else {
            month = month.previous();
        }
    }
    Date::from_calendar_date(year, month, day.clamp(1, 28)).map_or(now, |d| {
        unsigned(d.midnight().assume_utc().unix_timestamp()) * 1000
    })
}

pub(crate) fn update_traffic(
    tx: &Transaction<'_>,
    node: &str,
    snapshot: &SystemSnapshot,
    now: u64,
) -> Result<(), StorageError> {
    let day = options_or_none(tx, node)?.map_or(1, |o| o.traffic_reset_day);
    let cycle = integer(period_start(now, day));
    let old:Option<(i64,i64,i64,i64,i64)>=tx.query_row("SELECT cycle_start_ms,raw_up,raw_down,used_up,used_down FROM traffic_periods WHERE node_id=?1",[node],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let up = integer(snapshot.network_total_transmitted_bytes);
    let down = integer(snapshot.network_total_received_bytes);
    let (used_up, used_down) = old.map_or(
        (0, 0),
        |(previous, raw_up, raw_down, total_up, total_down)| {
            let delta_up = if up >= raw_up { up - raw_up } else { up };
            let delta_down = if down >= raw_down {
                down - raw_down
            } else {
                down
            };
            (
                (if previous == cycle { total_up } else { 0_i64 }).saturating_add(delta_up),
                (if previous == cycle { total_down } else { 0_i64 }).saturating_add(delta_down),
            )
        },
    );
    tx.prepare_cached("INSERT INTO traffic_periods VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(node_id) DO UPDATE SET cycle_start_ms=excluded.cycle_start_ms,raw_up=excluded.raw_up,raw_down=excluded.raw_down,used_up=excluded.used_up,used_down=excluded.used_down WHERE cycle_start_ms IS NOT excluded.cycle_start_ms OR raw_up IS NOT excluded.raw_up OR raw_down IS NOT excluded.raw_down OR used_up IS NOT excluded.used_up OR used_down IS NOT excluded.used_down")?.execute(params![node,cycle,up,down,used_up,used_down])?;
    Ok(())
}

fn period_usage(db: &Connection, node: &str) -> Result<(u64, u64), StorageError> {
    let day = options_or_none(db, node)?.map_or(1, |o| o.traffic_reset_day);
    let cycle = integer(period_start(crate::storage::unix_time_ms()?, day));
    Ok(db
        .query_row(
            "SELECT used_up,used_down FROM traffic_periods WHERE node_id=?1 AND cycle_start_ms=?2",
            params![node, cycle],
            |row| Ok((unsigned(row.get(0)?), unsigned(row.get(1)?))),
        )
        .optional()?
        .unwrap_or_default())
}

pub(crate) fn decorate_client(
    db: &Connection,
    node: &str,
    value: &mut Value,
) -> Result<(), StorageError> {
    if let Some(o) = options_or_none(db, node)? {
        value["name"] = json!(o.name);
        if !o.region.is_empty() {
            value["region"] = json!(o.region);
        }
        value["group"] = json!(o.group);
        value["weight"] = json!(o.weight);
        value["hidden"] = json!(o.hidden);
        value["tags"] = json!(o.tags);
        value["public_remark"] = json!(o.public_remark);
        value["price"] = json!(o.price);
        value["currency"] = json!(o.currency);
        value["billing_cycle"] = json!(o.billing_cycle_days);
        value["auto_renewal"] = json!(o.auto_renewal);
        value["expired_at"] = json!(
            o.expired_at_unix_ms
                .map(crate::storage::format_timestamp)
                .unwrap_or_default()
        );
        value["traffic_limit"] = json!(o.traffic_limit_bytes);
        value["traffic_limit_type"] = json!(o.traffic_limit_type);
    }
    let raw:Option<String>=db.query_row("SELECT extensions FROM snapshots WHERE node_id=?1 ORDER BY received_at_ms DESC,id DESC LIMIT 1",[node],|r|r.get(0)).optional()?;
    if let Some(raw) = raw {
        let data: Value = decode(&raw)?;
        value["gpu_name"] = json!(
            data["gpus"]
                .as_array()
                .map(|gpus| gpus
                    .iter()
                    .filter_map(|g| g["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default()
        );
    }
    Ok(())
}

pub(crate) fn decorate_status(
    db: &Connection,
    node: &str,
    value: &mut Value,
) -> Result<(), StorageError> {
    let (up, down) = period_usage(db, node)?;
    value["net_total_up"] = json!(up);
    value["net_total_down"] = json!(down);
    let extensions:Option<String>=db.query_row("SELECT extensions FROM snapshots WHERE node_id=?1 ORDER BY received_at_ms DESC,id DESC LIMIT 1",[node],|r|r.get(0)).optional()?;
    if let Some(raw) = extensions {
        let data: Value = decode(&raw)?;
        apply_extensions(&data, value);
    }
    Ok(())
}

pub(crate) fn apply_extensions(data: &Value, value: &mut Value) {
    value["tcp_connection_count"] = data["tcp_connection_count"].clone();
    value["udp_connection_count"] = data["udp_connection_count"].clone();
    value["connections"] = data["tcp_connection_count"].clone();
    value["connections_udp"] = data["udp_connection_count"].clone();
    value["gpus"] = data["gpus"].clone();
    value["gpu"] = data["gpu"].clone();
    value["temp"] = data["temp"].clone();
    if let Some(gpus) = data["gpus"].as_array() {
        let (usage_sum, usage_count) = gpus
            .iter()
            .filter_map(|g| g["usage_percent"].as_f64())
            .fold((0.0, 0_u32), |(sum, count), usage| (sum + usage, count + 1));
        value["gpu"] = if usage_count == 0 {
            Value::Null
        } else {
            json!(usage_sum / f64::from(usage_count))
        };
        value["temp"] = gpus
            .iter()
            .filter_map(|g| g["temperature_celsius"].as_f64())
            .reduce(f64::max)
            .map_or(Value::Null, |t| json!(t));
    }
}

fn incidents(db: &Connection) -> Result<Vec<Value>, StorageError> {
    let mut statement=db.prepare("SELECT id,node_id,rule_id,message,status,opened_at_ms,updated_at_ms FROM alert_incidents ORDER BY updated_at_ms DESC LIMIT ?1")?;
    Ok(statement.query_map([i64::try_from(MAX_INCIDENTS).expect("incident limit fits SQLite")],|r|Ok(json!({"id":r.get::<_,String>(0)?,"node_id":r.get::<_,String>(1)?,"rule_id":r.get::<_,String>(2)?,"message":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,"opened_at_unix_ms":r.get::<_,i64>(5)?,"updated_at_unix_ms":r.get::<_,i64>(6)?})))?.collect::<Result<Vec<_>,_>>()?)
}
