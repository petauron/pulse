# Changelog

All notable changes to Pulse will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) after its first stable release.

## [Unreleased]

## [0.1.0-alpha.4] - 2026-09-12

### Changed

- Split the Service into paired control (`pulse.db`, schema v4) and metrics (`pulse.metrics.db`, schema v1) databases. Snapshots, probe history, traffic counters and last-seen state no longer share credential/configuration pages.
- Update node metadata only when it changes, cache ingestion statements, remove snapshot AUTOINCREMENT writes, and schedule bounded ingestion-path retention cleanup at most once per minute.
- Align snapshot and probe indexes with actual node/time/ID ordering; use WITHOUT ROWID for single-row traffic and last-seen state.
- Preserve raw sample precision and commit-before-acknowledgment durability with FULL synchronization on both files. No lossy sample buffer is enabled.
- Backups now return a private directory containing both databases and a manifest, with standalone copies that require no WAL sidecars.

### Fixed

- Preserve original credentials, sessions, TOTP state and metric fields during a backed-up, verified, forward-only split migration; fail closed on incomplete, mismatched or newer database pairs.
- Recover interrupted node/probe history cleanup and reserve both writers for consistent paired backups.
- Finalize backup copies for portable read-only access and never remove another pre-existing temporary backup on failure.

### Validation

- 90 workspace tests, strict Clippy and formatting checks passed before release preparation. Real temporary-Service upgrade checks retained original sessions, Agent credentials and WAL-backed history.
- A local macOS six-minute synthetic-HTTP comparison measured 10.53% fewer process-accounted write bytes at one-second intervals and 12.20% fewer at three-second intervals. All acknowledged samples survived test-process SIGKILL. This is not a Linux production benchmark or a power-loss simulation; see `docs/STORAGE-BENCHMARK.md`.

### Upgrade notes

- Back up and stop the old Service before upgrading. Allow additional disk space for the pre-upgrade single-file backup, metrics database, rollback journals and verification work.
- Persist the entire state directory. New backups must be restored as a matching pair; never create an empty replacement metrics file. The CLI `backup` output is now a directory, not a single file.
- `PULSE_MAX_DATABASE_BYTES` now caps each database independently: its default permits 2 GiB per file, up to 4 GiB across the pair, excluding WAL files/backups/migration scratch space.
- Do not downgrade a migrated database. Restore the single-file pre-upgrade backup with the matching older binary for rollback; retain the split pair separately.
- Existing Agent credentials and protocol v2 remain valid; an Agent upgrade is not required for this storage change. Publishing does not deploy Service instances or update Vastora's separate application catalog.

## [0.1.0-alpha.3] - 2026-09-12

### Added

- Built-in administrator setup/login, private monitoring by default, session revocation, TOTP, and explicitly configured GitHub OAuth.
- Emerald administration for node metadata, visibility, enrollment, credentials, billing-period traffic, asset fields, and site settings.
- Bounded ICMP/TCP/HTTP probes with real latency/loss history, alert rules, incident state, and durable notifications through administrator-enabled HTTPS webhooks.
- Optional process/socket/GPU metrics and automatic country discovery with manual-region priority and an explicit disable option.
- Authentication and monitoring operator guides, a private-bootstrap Compose example, and regression coverage for desktop/mobile administration and real Agent probes.

### Changed

- The default reporting interval is three seconds; administrators can configure 1–300 seconds centrally.
- SQLite schema v3 uses a backed-up, forward-only migration; the Agent snapshot/enrollment protocol remains v2.
- Webhook delivery stays disabled unless the administrator explicitly configures and enables a destination. Payloads exclude IP addresses, credentials and raw monitoring snapshots.

### Fixed

- Keep OAuth network requests outside the session-change lock while rechecking revoked credentials before session issuance.
- Apply the same strict database byte ceiling during growth and restart, including non-page-aligned limits.
- Prevent stale administrator refreshes during logout and preserve real history across supported monitoring views.

### Upgrade notes

- Back up the Service database before upgrading. Configure the exact HTTPS `PULSE_PUBLIC_URL` and a private `PULSE_SETUP_TOKEN_FILE`, then create the first administrator at `/login`. An upgraded dashboard is private until an administrator deliberately enables public reads.
- Do not downgrade a migrated schema v3 database. Restore the pre-upgrade backup with the matching older binary if rollback is necessary.
- Existing enrolled Agents retain their credentials. Upgrade Agents to use automatic country discovery, extended metrics, centrally configured reporting, and network probes.
- When no manual country is set, GeoJS lookup is enabled by default and reveals public egress IP and Agent version, but no Pulse credentials or metrics. Set `PULSE_GEOIP_PROVIDER=disabled` before starting/upgrading to opt out.
- Publishing Pulse does not update or deploy Vastora applications. The matching Vastora executor integration and signed catalog publication are separate steps.

## [0.1.0-alpha.2] - 2026-09-09

The Alpha.1 release was stopped before publication after runtime verification found an Agent glibc incompatibility. Its existing source tag is retained without rewriting history.

### Added

- Initial Rust workspace and open-source project foundation.
- End-to-end node enrollment, authenticated snapshot ingestion, and retained SQLite history.
- Outbound-only Agent loop for CPU, load, memory, swap, disk, network, uptime, and platform metrics.
- Embedded Komari Emerald dashboard with card, list, detail, online state, and load-history views.
- Local Iconify subsets and packaged flags so the dashboard does not require runtime asset CDNs.
- Short-lived database-managed enrollment tokens, Agent credential rotation, node revocation/deletion, and local audit records.
- Production systemd/container assets, backup and rollback procedures, release provenance, checksums, and SBOM generation.

### Changed

- Replaced Emerald's finance-oriented fields with monitoring status while retaining its interface structure and styling.
- Made Service receipt time authoritative and replaced newest-row history truncation with bounded full-window downsampling and coverage metadata.

### Fixed

- Built and smoke-tested both Linux release binaries against a pinned Debian 12 runtime baseline instead of inheriting the newer CI runner's glibc requirements.
- Resolved packaged flag filenames consistently on case-sensitive Linux filesystems across cards, lists, details, and globe overlays.
- Made offline chart gaps explicit and excluded stale offline samples from current usage totals.
- Restored automatic dashboard initialization after connection failures and keyboard access to node status summaries.
- Added safe credential recovery after incomplete enrollment and accepted IPv6 loopback Service URLs.
- Made credential lifecycle commits durable, coordinated online backups with writers, and bounded latest-status queries by node count.
- Corrected deterministic asset checks, release cleanup on failure/cancellation, and third-party license delivery for Rust binaries and containers.
