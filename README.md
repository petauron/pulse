# Pulse

Pulse is a focused, self-hosted server health monitor built for low and predictable memory usage.

The project consists of a Rust Service, a Rust Agent, a shared versioned protocol, and a static Web interface derived directly from Komari Theme Emerald. Its scope intentionally excludes general server administration.

> **Status:** alpha. The basic monitoring path is implemented; pre-1.0 protocol and storage changes may still be breaking.

## Goals

- Show CPU, load, memory, swap, disk, network throughput, uptime, and platform details.
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
web/                   Embedded Emerald-based Web dashboard
docs/                  Architecture and roadmap
```

## Current basic monitoring slice

Pulse now provides an end-to-end monitoring path:

- one-time Agent enrollment and per-node credentials;
- outbound-only periodic host collection;
- authenticated and idempotent snapshot ingestion;
- bounded SQLite storage with configurable retention;
- online/offline state, node cards, list view, detail view, and load history;
- the Komari Emerald interface, built into the Service binary.

Alpha.3 adds administrator setup/login, private reads,
TOTP and optional GitHub OAuth, an Emerald management area, bounded network probes,
alerts, asset fields, monthly traffic accounting and optional extended metrics.
These additions are **not included in the published Alpha.2 artifacts**. See
[authentication setup](docs/AUTH.md) and [monitoring administration](docs/MONITORING.md).

### Run the Service

Create a short-lived, single-use enrollment token in the same database, then start the Service:

```bash
export PULSE_DATABASE_PATH=./pulse.db
export PULSE_PUBLIC_URL=http://127.0.0.1:8080
umask 077
openssl rand -hex 32 > ./setup-token
export PULSE_SETUP_TOKEN_FILE=./setup-token
cargo run -p pulse-service -- enrollment create
cargo run -p pulse-service -- serve
```

Open `http://127.0.0.1:8080/login`, supply the private setup token and create the administrator. The dashboard is private by default. After setup, remove the token file and its environment setting together. The Service defaults to `pulse.db`, seven days of history, a 90-second offline threshold, and a maximum of 100 nodes. See [.env.example](.env.example) for all current settings.

### Enroll and run an Agent

In another terminal, place the returned `token` in a mode-0600 file and start the Agent:

```bash
umask 077
printf '%s\n' 'replace-with-the-returned-token' > ./enrollment-token
PULSE_ENROLLMENT_TOKEN_FILE=./enrollment-token \
PULSE_NODE_NAME=example-node \
PULSE_NODE_REGION='' \
PULSE_NODE_GROUP=default \
  cargo run -p pulse-agent
```

One enrollment token enrolls exactly one Agent and expires after ten minutes by default. Remove the token file after enrollment. The Agent stores its per-node credential at `$XDG_STATE_HOME/pulse/agent-credentials.json` or `~/.local/state/pulse/agent-credentials.json`; subsequent starts do not need an enrollment token. Create another token with the admin CLI to enroll another node—no Service restart is needed. Set `PULSE_CREDENTIALS_PATH` to choose a different protected credential file.

Remote Service URLs must use HTTPS. Plain HTTP is accepted only for loopback development.

Automatic globe placement is enabled by default through GeoJS when
`PULSE_NODE_REGION` is empty. The provider sees the Agent's public egress IP and
Agent version, but receives no metrics or Pulse credentials. Set
`PULSE_GEOIP_PROVIDER=disabled` to opt out before starting/upgrading the Agent, or
`ipinfo` to select IPinfo. A manual country code (for example `US`) always takes
priority and prevents GeoIP requests. Existing explicit `disabled` settings are preserved.
See [node location](docs/OPERATIONS.md#node-location-and-globe-placement) for refresh,
failure behavior, privacy, and existing-node upgrades.

### Build the embedded Web interface

The generated `web/dist` assets are checked in so Rust release builds do not require Node.js. After changing the Web source, regenerate them with:

```bash
cd web
npm ci
npm run build
```

The visual implementation is derived from [Komari Theme Emerald](https://github.com/Tokinx/komari-theme-emerald) v1.0.11. See [web/UPSTREAM.md](web/UPSTREAM.md) and the bundled third-party license files.

## Production operation

Official Linux binary archives use a Debian 12 (glibc 2.36) runtime baseline. Both the Service and Agent must start in a clean Debian 12 runtime in CI and release jobs. For musl-based distributions or older runtimes, use the container or build from source.

Each release publishes archives containing both binaries, systemd units, and reversible install/rollback scripts, alongside checksums, provenance attestations, and SPDX SBOMs. A non-root Service container is also available. See [docs/OPERATIONS.md](docs/OPERATIONS.md) for deployment, TLS/access-control, enrollment, credential rotation/revocation, backup/restore, upgrade, rollback, and uninstall procedures.

Rust dependency license texts and original copyright/NOTICE files accompany each distribution in [RUST_THIRD_PARTY_LICENSES.html](RUST_THIRD_PARTY_LICENSES.html); [RUST_STDLIB_LICENSES.html](RUST_STDLIB_LICENSES.html) preserves the pinned standard library's notices. After updating `Cargo.lock` or the toolchain, install `cargo-about` 0.9.2 with its `cli` feature and the toolchain's `rust-docs` component, run `cargo fetch --locked`, then `node scripts/rust-licenses.mjs` and review the changes. CI runs the same generator with `--check` to reject missing or stale notices.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and the [Code of Conduct](CODE_OF_CONDUCT.md) before contributing.

## License

Licensed under the [Apache License 2.0](LICENSE).
