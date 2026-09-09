# Roadmap

Pulse is developed as small end-to-end slices. Scope may change before the first stable release.

## 1. Secure ingestion

- [x] SQLite schema and forward migration foundation
- [x] One-time enrollment token exchange
- [x] Protected per-Agent credential storage
- [x] Authenticated, idempotent snapshot ingestion
- [x] Explicit protocol version negotiation
- [x] Short-lived enrollment creation and revocation
- [x] Agent credential rotation, node revocation, deletion, and audit trail

## 2. Metrics and retention

- [x] CPU, load, memory, swap, disk, network, uptime, OS, and architecture
- [x] Bounded fresh-sample retry behavior
- [x] Raw history with explicit retention
- [x] Complete-window bounded history downsampling with coverage metadata
- Rolled-up long-term history
- [x] Storage migration, permission, retention, and bound tests
- Release-binary memory budget regression test

## 3. Probe tasks

- ICMP latency and packet loss
- Per-node task assignment and ordering
- Three independent carrier rows by default
- Clear capability and privilege diagnostics

## 4. Dashboard

- [x] Emerald compact node cards and grouped list view
- [x] Node detail and historical load charts
- Independent latency/loss history for each probe task
- [x] Keyboard-operable Emerald card/list/detail flows and accessible history table

## 5. Operations

- [x] systemd Service/Agent install, update, rollback, and uninstall assets
- [x] Non-root Service container image and persistent-volume guidance
- [x] Backup, restore, credential lifecycle, and hardening documentation
- [x] Multi-architecture checksummed releases, provenance, and software bill of materials
