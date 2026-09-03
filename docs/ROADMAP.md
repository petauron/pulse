# Roadmap

Pulse is developed as small end-to-end slices. Scope may change before the first stable release.

## 1. Secure ingestion

- SQLite schema and forward migrations
- One-time enrollment token exchange
- Per-Agent credential storage and rotation
- Authenticated, idempotent snapshot ingestion
- Explicit protocol version negotiation

## 2. Metrics and retention

- CPU, load, memory, swap, disk, network, uptime, OS, and architecture
- Bounded batching and retry behavior
- Raw and rolled-up history with explicit retention
- Storage and memory budget tests

## 3. Probe tasks

- ICMP latency and packet loss
- Per-node task assignment and ordering
- Three independent carrier rows by default
- Clear capability and privilege diagnostics

## 4. Dashboard

- Compact node cards and grouped list view
- Node detail and historical charts
- Independent latency/loss history for each probe task
- Responsive and accessible layout

## 5. Operations

- systemd Agent installer, update, rollback, and uninstall
- Service container image and persistent-volume guidance
- Backup and restore documentation
- Multi-architecture signed releases and software bill of materials
