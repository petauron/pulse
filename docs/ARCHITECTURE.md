# Architecture

Pulse is one repository with four bounded components.

## Agent

The Agent reads host metrics on a bounded interval. On first start it exchanges a short-lived, single-use enrollment token for a per-node credential stored in a mode-0600 file inside a mode-0700 directory on Unix. It then initiates authenticated outbound HTTPS requests to the Service and never listens for commands. Failed samples are discarded in favor of a fresh sample at the next interval, so there is no unbounded retry queue or response buffer.

ICMP support will use the narrow `CAP_NET_RAW` capability on Linux. The service account must not retain unrestricted root privileges.

## Protocol

`pulse-protocol` owns versioned request and response types. Protocol changes must be deliberate and covered by compatibility tests once the first stable protocol ships.

## Service

The Service owns enrollment, authentication, node metadata, ingestion, retention, and read APIs. SQLite is configured with a 4 MiB page cache, WAL journaling, indexed per-node history, an operator-defined maximum page count, and explicit time-based retention. Expired rows are pruned in bounded transactions before inserts and by startup/hourly maintenance, including when no Agent is active. Server receipt time—not the Agent wall clock—is authoritative for online state, ordering, and retention. Complete requested windows are downsampled in SQLite to a bounded response rather than truncated to the newest raw rows. Schema upgrades create a mode-0600 backup before running a forward-only transaction and fail closed on unknown newer schema versions.

The public dashboard and administrative API remain distinct authorization surfaces.

## Web

The Web interface is a Vue static client derived from Komari Emerald and embedded in the Service binary. Pulse supplies its read model through the versioned native API plus a narrow Emerald data adapter. The current slice presents cards, grouped list view, node details, and historical load charts. Probe latency/loss remains a later slice and is not represented as collected data yet.

## Current data flow

```text
host metrics -> Pulse Agent -> authenticated HTTPS -> Pulse Service -> SQLite
                                                           |
                                                           +-> embedded Emerald dashboard
```

Public dashboard reads are separate from authenticated Agent ingestion. The default bind address is loopback; an operator may deliberately expose it behind an HTTPS reverse proxy.

## Privacy boundary

Pulse has no mandatory cloud dependency. It does not send analytics, crash reports, host metrics, or identifiers anywhere except the administrator-configured Service.

## Resource constraints

- Agent collectors operate on fixed intervals and bounded buffers.
- The Service does not keep complete time-series histories in memory.
- SQLite cache, maximum file size, retention, node count, request concurrency, body size, response size, and history point count have explicit limits.
- Long-running tasks expose cancellation and shutdown paths.
- Resource budgets are verified against release binaries, not inferred from language choice.
