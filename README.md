# Pulse

Pulse is a focused, self-hosted server health monitor built for low and predictable memory usage.

The project consists of a Rust Service, a Rust Agent, a shared versioned protocol, and a static Web interface. Its visual direction is inspired by compact infrastructure dashboards, while its scope intentionally excludes general server administration.

> **Status:** pre-alpha. The protocol and storage schema are not yet released.

## Goals

- Show CPU, load, memory, swap, disk, network throughput, uptime, and platform details.
- Display independent latency and packet-loss histories for multiple probe tasks, including China Telecom, China Unicom, and China Mobile.
- Keep the Agent outbound-only, narrowly privileged, and suitable for small VPS instances.
- Keep Service memory bounded and store historical data in SQLite with explicit retention.
- Provide reproducible systemd Agent installs and containerized Service deployments.
- Operate without mandatory third-party accounts, analytics, or telemetry.

## Non-goals

- Remote shells or arbitrary command execution
- File management
- Application orchestration
- Proxy or VPN management
- AI-based network scoring
- Unlock testing or visitor fingerprinting

## Repository layout

```text
crates/pulse-protocol  Shared wire types and protocol version
crates/pulse-service   HTTP API and monitoring service
crates/pulse-agent     Low-memory host metrics collector
web/                   Static Web application (next vertical slice)
docs/                  Architecture and roadmap
```

## Current bootstrap slice

The Service exposes `GET /healthz` on `127.0.0.1:8080` by default. Set `PULSE_LISTEN` to select another listen address.

The Agent currently emits one local system snapshot as JSON. Enrollment and authenticated upload are intentionally deferred until their persistent security model is implemented.

```bash
cargo run -p pulse-service
cargo run -p pulse-agent
```

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and the [Code of Conduct](CODE_OF_CONDUCT.md) before contributing.

## License

Licensed under the [Apache License 2.0](LICENSE).
