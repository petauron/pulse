# Changelog

All notable changes to Pulse will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) after its first stable release.

## [0.1.0-alpha.18](https://github.com/petauron/senulun/compare/v0.1.0-alpha.17...v0.1.0-alpha.18) (2026-10-16)


### Bug Fixes

* align Emerald monitoring and Komari TCP latency ([a136fef](https://github.com/petauron/senulun/commit/a136fef44f879cb320f34b2466a230cb593e4237))
* **deps:** align and validate the updated build toolchain ([#57](https://github.com/petauron/senulun/issues/57)) ([f61d86e](https://github.com/petauron/senulun/commit/f61d86e70811829f164eaf2047877b0329dde4bb))
* **deps:** patch brace-expansion denial of service ([f482ff3](https://github.com/petauron/senulun/commit/f482ff36d1a72a981cde0ec184823df5e161caba))
* **deps:** patch shell-quote command injection ([7a36757](https://github.com/petauron/senulun/commit/7a367570a714bf815d76a08e49ecac13d2b40a71))
* **deps:** update Argon2 and reqwest with authentication regressions ([#18](https://github.com/petauron/senulun/issues/18)) ([617a975](https://github.com/petauron/senulun/commit/617a97584bf26ff6778fed8057e521af1d8d8f0f))

## [0.1.0-alpha.17](https://github.com/petauron/senulun/compare/v0.1.0-alpha.16...v0.1.0-alpha.17) (2026-10-12)


### Bug Fixes

* migrate canonical source and release identity to senulun ([6b63e42](https://github.com/petauron/senulun/commit/6b63e424259b2d0300494c2527e5d6fc2bd0532c))

## [0.1.0-alpha.16](https://github.com/petauron/pulse/compare/v0.1.0-alpha.15...v0.1.0-alpha.16) (2026-10-12)


### Bug Fixes

* **release:** recover immutable drafts with scoped permissions ([#49](https://github.com/petauron/pulse/issues/49)) ([24f944a](https://github.com/petauron/pulse/commit/24f944a598059d3741c89342e7d1b906a5abaac8))
* **release:** use supported GitHub workflow provenance ([#50](https://github.com/petauron/pulse/issues/50)) ([f0491a4](https://github.com/petauron/pulse/commit/f0491a4a88d2fb69f497ca05da8994d686c7df15))
* **web:** remove approximation prefix from remaining value ([fa09883](https://github.com/petauron/pulse/commit/fa0988390899d3991dceb2e2e8f6bb758da0da3b))

## [0.1.0-alpha.15](https://github.com/petauron/pulse/compare/v0.1.0-alpha.14...v0.1.0-alpha.15) (2026-10-11)


### Features

* rebrand Pulse as Senulun with backed-up title migration ([#44](https://github.com/petauron/pulse/issues/44)) ([1c6bd20](https://github.com/petauron/pulse/commit/1c6bd20e958ec020df54f14937ce9f79b301872f))


### Bug Fixes

* **deps:** use patched tinypool in the formatter toolchain ([#48](https://github.com/petauron/pulse/issues/48)) ([7b85b23](https://github.com/petauron/pulse/commit/7b85b236ec485eee35aa7aeffb508355d2ee2731))
* **release:** keep workspace license notices version independent ([#47](https://github.com/petauron/pulse/issues/47)) ([363f452](https://github.com/petauron/pulse/commit/363f452dd959f3ad6176b37da69ccd8008aecb06))
* **release:** support inherited Rust workspace versions ([#45](https://github.com/petauron/pulse/issues/45)) ([76a20c6](https://github.com/petauron/pulse/commit/76a20c6bbcb2f7efabd40c9aeebab81d80783628))

## [Unreleased]

## [0.1.0-alpha.13] - 2026-10-05

### Fixed

- Restore remaining-value dashboard summary with total value, monthly cost and currency details. Estimates explicitly use bundled reference exchange rates; no external requests are made.

## [0.1.0-alpha.12] - 2026-10-05

### Fixed

- Restore pricing and expiry information on dashboard cards and lists, replacing the redundant online last-report row. Missing expiry dates are no longer labeled expired.

## [0.1.0-alpha.11] - 2026-10-05

### Changed

- Align carrier latency panels and history charts with Emerald-Cazi spacing, colors, sample bars and tooltips while retaining explicit probe failure semantics.

## [0.1.0-alpha.10] - 2026-10-05

### Added

- Bounded carrier latency previews on node cards and lists, with visible failure rates and stale-data states.
- Audited CLI correction of current-cycle traffic, retaining subsequent increments and rejecting stale or mismatched counter baselines.

### Changed

- Network latency history now opens on a one-hour chart with time-range tabs, automatic refresh and collapsible detail tables.

## [0.1.0-alpha.9] - 2026-10-05

### Fixed

- Rebaseline decreasing traffic counters without adding their remaining lifetime totals to monthly usage; preserve unaffected upload/download increments.
- Document provider-facing interface selection and explicit handling of previously inflated usage totals.

## [0.1.0-alpha.8] - 2026-10-05

### Changed

- Administrator passwords now only require a non-empty value; setup and password changes no longer impose a separate password length policy. Authentication requests retain their existing size bound.
- Remove password length restrictions and helper text from the setup, login, and account forms.

## [0.1.0-alpha.7] - 2026-10-04

### Changed

- First administrator setup now requires only a username and password. The first successful registration closes setup; concurrent or later attempts cannot replace the administrator. Origin, CSRF, password, and session protections remain in place.
- Remove the setup-token configuration and deployment secret. Complete initial setup before sharing a new public instance. Existing administrator accounts are unchanged.

## [0.1.0-alpha.6] - 2026-10-03

### Added

- Read-only enrollment inspection to recover the original managed monitoring identity without creating a duplicate node.
- Authenticated node reporting inspection using the current credential on stdin, with rotation and last accepted report timestamps for recovery verification.

### Upgrade notes

- Preserve the existing Service data and collector credentials. This release supplies recovery evidence APIs; upgrading alone does not perform node recovery or prove that a replacement collector is reporting.
- Existing collectors remain separate upgrade targets. Country discovery settings and manual regions are unchanged.

## [0.1.0-alpha.5] - 2026-09-26

### Added

- Administrator-installed Komari theme ZIP packages, explicit activation, persisted theme settings, bounded extraction, and built-in Emerald fallback for authentication and administration.
- LuminaPlus v1.3.5 compatibility for dashboards, node histories, numeric Ping task bindings, assets, theme settings and mobile layouts. A reproducible Pulse-adapted theme ZIP and its upstream MIT license are included in the release.
- Optional, default-disabled IP information: registered public IPv4/IPv6, geography, ASN/network ownership, native/broadcast/anycast classification, six-location global latency, and CSRF-protected refresh in the original LuminaPlus interface.

### Fixed

- Upgrade rustls to 0.23.45 to address TLS handshake encryption-level validation advisory RUSTSEC-2026-0285.
- JavaScript-safe numeric probe identifiers and cross-node Ping history compatibility, settings-save CSRF support, and the LuminaPlus administration link.
- Nullable optional hardware samples no longer discard valid load or traffic history in the adapted theme.
- IP information APIs require administrator authentication even under the compatibility `/public/` path, validate node/address binding on every query, and never expose registered addresses to anonymous viewers.

### Upgrade notes

- Back up the entire Service state directory before upgrading. This release retains the alpha.4 database schemas and Agent protocol v2; existing Agent credentials remain valid and an Agent upgrade is not required for the theme/IP features.
- Install the bundled `LuminaPlus-v1.3.5-pulse.zip` through Admin → Themes, then activate it. Export existing theme settings before replacing an installed package; installation does not silently overwrite or activate a theme.
- IP information is disabled by default. Enable it in site settings and register node addresses explicitly. This permits Service-side requests containing the selected IP to Net.Coffee; no Pulse credentials or host metrics are sent. Mainland-China nodes are excluded. See `docs/THEMES.md` for quotas, cache bounds and third-party availability limits.
- LuminaPlus's default asset exchange-rate calculation permits browser requests to Frankfurter only while that theme is active. Custom external media/rate sources require explicit HTTPS settings.
- Theme packages are trusted administrator-installed browser code. This is not a general Komari plugin runtime, remote shell, file manager, or Komari Agent replacement.
- Publication does not deploy A1 or update Vastora's separate application catalog.

### Validation

- 106 Rust workspace tests, 24 management frontend tests and 379 LuminaPlus tests passed before release preparation, with strict Clippy and frontend lint checks.
- Real isolated Service/Agent browser verification covered desktop/mobile settings, registered addresses, real IP lookup, six-location latency, CSRF refresh, anonymous isolation and disabling the feature. No production nodes were used.

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
