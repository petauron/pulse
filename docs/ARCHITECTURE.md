# Architecture

Pulse is one repository with four bounded components.

## Agent

The Agent reads host metrics on a bounded interval. On first start it exchanges a short-lived, single-use enrollment token for a per-node credential stored in a mode-0600 file inside a mode-0700 directory on Unix. It then initiates authenticated outbound HTTPS requests to the Service and never listens for commands. Failed samples are discarded in favor of a fresh sample at the next interval, so there is no unbounded retry queue or response buffer.

ICMP uses only a fixed system ping executable with validated host arguments, one packet and a bounded deadline/output. It requires the OS to permit ping for the service account. The service account must not retain unrestricted root privileges; absence of that capability is reported as unavailable, never as zero latency.

Default-enabled, opt-out country discovery runs in the Agent, where a self-IP lookup observes the
monitored host's egress rather than a reverse proxy in front of the Service. It
reuses reqwest, serde, and Tokio; no new dependency, IP storage, protocol field, or
database migration is needed. The existing snapshot `region` field carries the
result into the existing country-cluster rendering. Manual region wins; an unset
or empty `PULSE_GEOIP_PROVIDER` selects GeoJS, and `disabled` prevents requests.
This documented GeoIP exception does not authorize other default third-party calls. One cancellable task
retains one country value in a watch channel, with one five-second/8 KiB request
at a time, a 24-hour success refresh, and a 15-minute failure retry. Lookups never
block the metric loop, and failures keep the latest successful country in memory.

## Protocol

`pulse-protocol` owns versioned request and response types. Protocol changes must be deliberate and covered by compatibility tests once the first stable protocol ships.

## Service

The Service owns enrollment, authentication, node metadata, ingestion, retention, and read APIs. It attaches a dedicated metrics database to the control connection: account/configuration/credential state stays in the control file; snapshots, probes, traffic and last-seen state live in the metrics file. Both use WAL and FULL synchronization; ingestion returns success only after commit. There is no sample buffer or storage-time downsampling. The connection has bounded caches (1 MiB control, 3 MiB metrics, 32 prepared statements); each file has an operator-defined maximum page count. Ordinary reports do not update control rows unless static metadata changes. Snapshot IDs use INTEGER PRIMARY KEY without AUTOINCREMENT, avoiding a sequence-page write. Latest/history indexes include the actual receipt-time and ID ordering, while separate time indexes support bounded retention. Probe task/time and node/time queries have dedicated indexes. Single-row traffic and last-seen tables use WITHOUT ROWID to avoid a redundant primary-key tree.

Expired rows are pruned in bounded transactions at most once per minute during ingestion and by startup/hourly maintenance, including when no Agent is active. Server receipt time—not the Agent wall clock—is authoritative for online state, ordering, and retention. Complete requested windows are downsampled in SQL to a bounded response rather than truncated to the newest raw rows. Schema upgrades create a mode-0600 backup and fail closed on unknown newer versions. The split migration temporarily uses rollback journals for SQLite's atomic multi-file commit; runtime WAL transactions do **not** provide cross-file crash atomicity. Control-plane deletes commit first, with explicit metrics cleanup and startup orphan recovery; accepted reports remain individually durable. Pair IDs reject an unrelated metrics file. Backups reserve both writers and publish a complete private directory with both databases and a manifest; a normal single-file copy is insufficient. See [Operations](OPERATIONS.md#backup-and-restore).

The dashboard and administrative API are distinct authorization surfaces. Browser sessions use an administrator account, Argon2id, optional TOTP and explicit GitHub OAuth configuration. Reads are private by default, with Origin/CSRF protection on all browser mutations. Schema v3 adds auth/configuration, probe history, monthly traffic baselines and durable alert state in a forward migration. Snapshot/enrollment protocol remains v2 with optional metrics; probe/config documents independently use schema v1.

## Web

The Web interface is a Vue static client derived from Komari Emerald and embedded in the Service binary. Pulse supplies its read model through the versioned native API plus a narrow Emerald data adapter. The same components and visual tokens power login and administration. Node details show actual ICMP/TCP/HTTP results with a latest-1000 result limit and whole-requested-window summary, separately from downsampled host history. Missing metrics are unavailable, never synthesized measurements.

## Current data flow

```text
host metrics -> Pulse Agent -> authenticated HTTPS -> Pulse Service -> SQLite
                                                           |
                                                           +-> embedded Emerald dashboard
```

Public dashboard reads are separate from authenticated Agent ingestion. The default bind address is loopback; an operator may deliberately expose it behind an HTTPS reverse proxy.

## Privacy boundary

Pulse has no mandatory cloud dependency or analytics. Agent metrics go only to the administrator-configured Service. Administrator-configured probes contact only their specified targets with no Pulse credentials. Optional HTTPS webhook channels are disabled until explicitly enabled; payloads contain only node/rule identity, status, time and a short reason, not IPs, credentials or raw snapshots. At most four deliveries run at once, each with a five-second timeout and five durable attempts. Exhausted delivery failures are audited; monitoring collection never waits for notifications.

Unless disabled or overridden by a manual region, the GeoIP provider observes
the Agent's public egress IP and HTTP user-agent/version, but never receives Pulse
credentials or host metrics. The response's IP and city are discarded. GeoIP uses
a separate HTTPS client with redirects, automatic retries, and HTTP proxies
disabled; there is no automatic fallback to a different third party.

## Resource constraints

- Agent collectors operate on fixed intervals and bounded buffers.
- The Service does not keep complete time-series histories in memory.
- SQLite cache, maximum file size, retention, node count, request concurrency, body size, response size, and history point count have explicit limits.
- Long-running tasks expose cancellation and shutdown paths.
- Resource budgets are verified against release binaries, not inferred from language choice.
