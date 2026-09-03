# Architecture

Pulse is one repository with four bounded components.

## Agent

The Agent reads host metrics and executes explicitly assigned probe tasks. It initiates authenticated outbound connections to the Service and never accepts remote shell commands. Persistent queues and buffers must be bounded.

ICMP support will use the narrow `CAP_NET_RAW` capability on Linux. The service account must not retain unrestricted root privileges.

## Protocol

`pulse-protocol` owns versioned request and response types. Protocol changes must be deliberate and covered by compatibility tests once the first stable protocol ships.

## Service

The Service owns enrollment, authentication, node metadata, probe assignment, ingestion, retention, aggregation, and read APIs. SQLite is the initial storage engine. Released schema changes are forward-only migrations that fail closed.

The public dashboard and administrative API remain distinct authorization surfaces.

## Web

The Web interface is a static client built separately and embedded in the Service release artifact. It presents compact node cards, node details, historical metrics, and separate latency/loss rows for assigned probe tasks.

## Privacy boundary

Pulse has no mandatory cloud dependency. It does not send analytics, crash reports, host metrics, or identifiers anywhere except the administrator-configured Service.

## Resource constraints

- Agent collectors operate on fixed intervals and bounded buffers.
- The Service does not keep complete time-series histories in memory.
- SQLite cache and retention have explicit configuration limits.
- Long-running tasks expose cancellation and shutdown paths.
- Resource budgets are verified against release binaries, not inferred from language choice.
