# Monitoring administration

This document describes Alpha.3 and later, not the older Alpha.2 image.
Complete [administrator setup](AUTH.md) before using `/admin`. The administration
area reuses the existing Emerald components and visual tokens.

## Nodes and access

The node editor controls name, country, group, ordering, tags, public remark,
hidden status, price/currency, billing cycle, expiry and automatic-renewal metadata.
These values are stored separately from Agent facts, so a later snapshot cannot
overwrite administrator edits. A blank country resumes the Agent's discovered
country. Hidden nodes remain in administration but are excluded from all monitoring
lists, totals, histories and probe reads, including authenticated dashboard views.
Revoked nodes stop authenticating; deleting a node removes its stored monitoring
data. Both actions require confirmation in the UI.

Enrollment tokens are single-use, valid for 60–86400 seconds, and shown once.
Existing Agents retain a distinct long-lived credential. Rotation immediately
invalidates the old credential and displays the replacement once: securely update
the Agent credential file and restart that Agent yourself. Pulse does not silently
deliver the replacement over the invalidated connection.

## Traffic and assets

Traffic is computed from positive differences between cumulative Agent counters.
The first observed sample establishes a baseline; pre-enrollment traffic cannot be
recovered. When counters decrease after a reboot/interface reset, the current value
starts the next delta. Monthly UTC boundaries use a chosen day from 1 through 28.
Intervals crossing a boundary are assigned to the new period. Missing reports and
interface-selection changes limit accounting accuracy: this is monitoring, not an
authoritative billing meter.

The current dashboard totals show the current billing period. Historical network
totals remain raw Agent counters. Resetting traffic clears only current-period
usage and keeps the raw baseline, so the next sample does not re-add old traffic.
Limits can count upload, download, sum, minimum or maximum. A zero limit is unlimited.
Asset billing-cycle and auto-renewal fields are descriptive; Pulse does not perform
payments, renew servers, or modify provider subscriptions.

## Probes

Administrators configure only ICMP, TCP and HTTP probes. An empty node selection
means all active nodes, including future enrollments. Each Agent runs at most 16
tasks with at most four concurrent probes. Intervals are 5–3600 seconds; timeouts
are 1–30 seconds and cannot exceed the interval. Configuration refreshes every
30 seconds and is authenticated with the Agent credential.
Deleting the last explicitly assigned node disables its remaining probe/rule
definition; it never silently widens that definition to all nodes. Deleting a
notification channel removes it from rules without granting any new destination.

Targets are explicit destinations selected by the administrator. Probes never
carry Pulse credentials, execute shell text, follow HTTP redirects, or use system
HTTP proxy environment settings. Private/local destinations are allowed because
monitoring internal services is a legitimate administrator action. ICMP requires
a supported system ping executable and OS permission for the service account.
Do not grant unrestricted root privileges to enable it. Unavailable probes report
failure categories, not a fabricated zero latency.

HTTP success requires a 2xx response whose complete body is received within the
timeout and 64 KiB bound; redirects, error statuses and larger responses fail.
HTTP latency includes DNS, connection, TLS and body transfer; TCP latency includes
DNS and connection; ICMP reports the system ping RTT.
The detail page shows the newest 1000 results and an aggregate for the requested
window. Server receipt time controls ordering and retention; Agent collection time
is diagnostic. Probe storage has a hard 100000-row per-node cap in addition to
retention. Missing samples are not counted as received failures: node-offline
alerts cover an absent Agent separately.

## Alerts and notifications

Rules monitor offline state, CPU/memory/disk utilization, traffic-limit percentage
or days until expiry. A rule has a duration and a notification cooldown. Missing
or stale utilization cannot prove recovery; pending duration is reset until fresh
data arrives. Empty channel selection keeps the incident inside Pulse.
The offline timeout is the larger of `PULSE_OFFLINE_AFTER_SECONDS` and three
centrally configured report intervals, so a 300-second interval does not falsely
toggle offline every 90 seconds. Dashboard status and alert evaluation use the
same effective timeout.

Notification channels are generic HTTPS JSON webhooks, disabled by default.
Only an administrator's explicit enabled channel receives node name/ID, rule
name/ID, incident ID, status, time and a short reason. IP addresses, credentials,
actual measured values and raw snapshots are not sent. URL tokens are sensitive:
only the administrator can view them; do not place them in public node remarks.
This is not a native Telegram/SMTP integration; use a webhook receiver that
understands this payload if forwarding to another service.

The engine evaluates every five seconds. It stores pending/firing/resolved state,
with at most 128000 node/rule pairs (1000 nodes × 128 rules). The UI shows the latest
1000 states, not an unlimited event journal. The durable delivery queue holds at
most 256 items; up to four send concurrently, each with a five-second timeout,
no redirects or proxy inheritance, and no more than five attempts. Queue pressure
keeps incident state and retries enqueueing later. Recovery supersedes an unsent
alert for the same incident/channel. Delivery failures appear in administration
and the audit log without exposing URLs or payload secrets.

## Additional host metrics

Linux extended collection counts processes and TCP/UDP sockets in the Agent's
current process/network namespace using bounded streaming reads. TCP includes
listening and non-established sockets; UDP includes unconnected sockets. Running
the Agent in a container does not turn container metrics into host metrics.

AMD GPU metrics use readable sysfs fields; NVIDIA uses a fixed `nvidia-smi` command
with bounded output/time. Hardware, drivers, permissions and hardened systemd
device isolation may make GPU data unavailable. Missing capabilities produce
null/N/A. Virtualization auto-detection is best-effort; explicit configuration wins.

Set `PULSE_EXTENDED_METRICS=disabled`, `PULSE_GPU_METRICS=disabled`, or
`PULSE_AUTODETECT_VIRTUALIZATION=disabled` to disable the corresponding collection.
No full-process cache or arbitrary command facility is maintained. The default
host-reporting interval is three seconds, adjustable centrally within 1–300 seconds.
Size retention and database capacity for the chosen interval before production use.

## Upgrade boundary

Database schema v3 is forward-only and requires a successful private SQLite backup
before migration. Enrollment/credential protocol remains v2; config and probes are
separately versioned schema v1. Upgrade the Service before Agents, after configuring
the public URL and bootstrap token. A rollback to a schema-v2 Service requires
stopping writers and restoring its pre-migration backup, not opening the v3 database
with the old executable. Do not publish this branch until its added local/CI
regressions and regenerated Web/license assets have been validated.
