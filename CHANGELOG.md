# Changelog

All notable changes to Pulse will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) after its first stable release.

## [Unreleased]

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

- Made offline chart gaps explicit and excluded stale offline samples from current usage totals.
- Restored automatic dashboard initialization after connection failures and keyboard access to node status summaries.
- Added safe credential recovery after incomplete enrollment and accepted IPv6 loopback Service URLs.
- Made credential lifecycle commits durable, coordinated online backups with writers, and bounded latest-status queries by node count.
- Corrected deterministic asset checks, release cleanup on failure/cancellation, and third-party license delivery for Rust binaries and containers.
