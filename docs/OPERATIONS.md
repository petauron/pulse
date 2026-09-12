# Production operations

Pulse has two deployment roles: run `pulse-service` on the monitoring host, and run one outbound-only `pulse-agent` on every monitored Linux host. The Agent never accepts inbound commands. Run both under dedicated unprivileged accounts.

## Trust boundary

Alpha.3 includes administrator authentication and defaults to a private dashboard. Until the first administrator is created, all monitoring read APIs fail closed; afterward, only an administrator can deliberately enable public reads. Administrative mutations always require an authenticated session, exact Origin and CSRF protection. Configure the HTTPS browser origin and private bootstrap token before upgrading; see [AUTH.md](AUTH.md). Older Alpha.2 artifacts require external dashboard authentication. Keep the default loopback bind and an HTTPS reverse proxy; do not expose port 8080 directly to the Internet.

Remote Agents accept only HTTPS Service URLs. Pulse has no analytics, telemetry, runtime CDN, or mandatory third-party account. Metrics go only to the configured Service.

**Default outbound request:** Agents with no manual region use GeoJS to discover
their country. GeoJS sees the public egress IP and Agent version, not Pulse tokens
or metrics. To prevent this, set `PULSE_GEOIP_PROVIDER=disabled` in the Agent
environment **before starting or upgrading**. See [node location](#node-location-and-globe-placement).

## Verify a release

Download the archive for the host architecture together with `SHA256SUMS`, its SPDX SBOM, and the GitHub artifact attestation. Verify before extracting:

```bash
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify pulse-v0.1.0-alpha.3-linux-x86_64.tar.gz --repo petauron/pulse
```

Official Linux archives target `x86_64` and `aarch64` GNU/Linux with a Debian 12 (glibc 2.36) runtime baseline. Both binaries are built in the pinned Debian 12 Rust image and must start in a clean Debian 12 runtime before packaging. The same build/verification script runs in CI. Use the container or build from source for other environments, including musl-based distributions.

The release workflow validates binaries, SBOMs, and a multi-architecture container before creating a hidden draft. It then pushes and attests the image and publishes the GitHub Release only as the final step. If final publication fails or the run is cancelled, the workflow attempts to remove the GHCR package version carrying that release tag and leaves the draft for a safe rerun. Cleanup is limited to two minutes. If cleanup fails, times out, or the runner is forcibly terminated, the maintainer must inspect and remove that tagged GHCR package version before retrying; never publish the draft manually while cleanup is unresolved.

## systemd installation

The release archive contains the binaries and `deploy/`. Review the environment example, then install from the extracted, verified archive:

```bash
sudo ./deploy/install-systemd.sh service "$PWD/pulse-service"
sudoedit /etc/pulse/service.env
sudo systemctl start pulse-service
```

The installer creates a dedicated account and mode-0700 state directory, records the previous binary, installs a hardened unit, and enables it without starting an unconfigured first installation. An update restarts the unit only if it was already running.

Create an enrollment token locally against the Service database. The JSON response contains an ID, the only display of the token, and its expiry time:

```bash
sudo -u pulse env PULSE_DATABASE_PATH=/var/lib/pulse/pulse.db \
  /usr/local/bin/pulse-service enrollment create --ttl-seconds 600
```

On the Agent host:

```bash
sudo ./deploy/install-systemd.sh agent "$PWD/pulse-agent"
sudoedit /etc/pulse/agent.env
sudo install -o pulse-agent -g pulse-agent -m 0600 /path/to/securely-transferred-token \
  /etc/pulse/agent-enrollment-token
sudo systemctl start pulse-agent
```

The token file must contain only the token value. It is read only when the Agent has no stored credential. `systemctl start` does not wait for enrollment. Verify that `journalctl -u pulse-agent` reports `agent credentials saved` and that this node appears online before deleting the token:

```bash
sudo test -s /var/lib/pulse-agent/agent-credentials.json && \
  sudo rm -f /etc/pulse/agent-enrollment-token
```

## Node location and globe placement

The globe groups nodes by country/territory, not by group names or individual
city coordinates. Set `PULSE_NODE_REGION=US` (or another two-letter country code)
for manual placement. A nonempty manual region takes precedence and prevents all
GeoIP requests, even if a provider is configured.

Country discovery through GeoJS is enabled by default when no manual region is
set. Upgrade **each Agent binary** to a build containing this feature. The equivalent
explicit configuration in `/etc/pulse/agent.env` is:

```ini
PULSE_NODE_REGION=
PULSE_GEOIP_PROVIDER=geojs
```

Alternatively select `ipinfo`. The only accepted provider values are `disabled`,
`ipinfo`, and `geojs`; an unset or empty value selects GeoJS. Discovery makes
outbound HTTPS requests from that Agent to
`https://get.geojs.io/v1/ip/geo.json` or `https://ipinfo.io/json`, respectively.
There is no automatic fallback to a different third party. The provider sees the
Agent's public egress IP and Agent user-agent/version, but receives no host name,
node ID, enrollment token, Agent credential, or metrics. Only the country code is
retained and sent to Pulse; the IP, city, and other response fields are discarded.

Upgrades preserve existing configuration files: a previous explicit `disabled`
setting stays disabled, and a manual region continues to prevent lookups. An old
configuration with neither setting starts using GeoJS after the Agent upgrade.
The installer may restart an already-running Agent, so configure `disabled` before
running the installer if you do not want third-party requests. Fresh installations
remain stopped until configured and started by the administrator.

The dedicated GeoIP client does not follow redirects or use HTTP proxy environment
variables. This locates the Agent's direct network egress rather than the Service,
Cloudflare, or an HTTP proxy. The route may use IPv4 or IPv6; a VPN, transparent
proxy, NAT gateway, or IP database error can still yield a country different from
the physical host. Use manual placement to override it. No browser location
permission or visitor geolocation is involved.

After updating the binary and configuration:

```bash
sudo systemctl restart pulse-agent
```

Keep existing credentials; no new enrollment is required. For this combined
monitoring upgrade, update the Service first: Alpha.2 rejects the new optional
snapshot fields, and the new Service migrates its database to schema v3. GeoIP
itself reuses the existing region field. Discovery runs independently of metric delivery.
Each attempt has a five-second timeout and an 8 KiB response limit. A successful
result is refreshed after 24 hours; failures retry after 15 minutes and preserve
the last successful country in memory. The cache is not persisted across Agent
restarts. Before the first success, the region remains empty and the node can be
online without a globe marker. After success, the next accepted metric snapshot
updates the existing node's region. Two US nodes share one US cluster on the globe.

Check `journalctl -u pulse-agent` for `automatic node region updated`, then confirm
the country's flag on the node card and globe. Disabled discovery with an empty
manual region emits a warning. Network errors, rate limits, malformed responses,
and unknown countries leave monitoring operational. To disable further lookups,
set `PULSE_GEOIP_PROVIDER=disabled` and restart; set a manual region as well if the
node should remain positioned. These options are not implemented in Alpha.2;
changing only the environment of an old binary will not enable discovery.

Provider response formats: [GeoJS](https://www.geojs.io/docs/v1/endpoints/geo/)
and [IPinfo](https://github.com/komari-monitor/komari/blob/main/utils/geoip/ipinfo.go).

## Containerized Service

The image runs as UID/GID 65532 and stores its mode-0600 SQLite database in `/var/lib/pulse`. The Agent should still run directly on each monitored host so its metrics describe that host rather than a container.

```bash
sudo install -d -o 65532 -g 65532 -m 0700 /srv/pulse
# Generate a private bootstrap file securely; make it readable only by UID 65532.
sudo install -o 65532 -g 65532 -m 0600 /path/to/private-setup-token /srv/pulse-setup-token
docker run --detach --name pulse --restart unless-stopped \
  --publish 127.0.0.1:8080:8080 \
  --volume /srv/pulse:/var/lib/pulse \
  --volume /srv/pulse-setup-token:/run/secrets/setup-token:ro \
  --env PULSE_PUBLIC_URL=https://pulse.example.com \
  --env PULSE_SETUP_TOKEN_FILE=/run/secrets/setup-token \
  ghcr.io/petauron/pulse:v0.1.0-alpha.3
```

Alpha.3 contains these authentication changes; Alpha.2 does not. Even though the process listens on all interfaces inside the container, publish it to loopback and terminate TLS at the reverse proxy. Open `/login` for initial setup; afterward remove both the setup-token mount and environment setting when recreating the container, then remove its source file. The [Compose example](../deploy/docker-compose.yml) provides the same explicit configuration. Run local administration with `docker exec`, using the same database path already present in the image environment.

## Credential lifecycle

Enrollment tokens are single-use, expire after 60 to 86,400 seconds, and can be revoked before use:

```bash
pulse-service enrollment revoke ENROLLMENT_ID
```

To rotate an Agent credential without exposing it in a command-line argument:

1. Stop `pulse-agent` on that node.
2. On the Service host, run `pulse-service node rotate NODE_ID` and redirect its one-time output to a mode-0600 file.
3. Transfer that file over an authenticated encrypted channel.
4. On the Agent host, pipe the file to `sudo -u pulse-agent env PULSE_CREDENTIALS_PATH=/var/lib/pulse-agent/agent-credentials.json pulse-agent credentials replace`.
5. Delete both temporary copies, start the Agent, and confirm a new snapshot arrives.

Rotation invalidates the previous token immediately. If a node is lost or compromised, use `pulse-service node revoke NODE_ID`; this retains its historical rows but excludes it from active views. `pulse-service node delete NODE_ID` permanently deletes the node and its snapshots. Review up to 1,000 local lifecycle events with `pulse-service audit 1000`. These commands must be run with the Service database environment and account.

If initial enrollment succeeds but the Agent cannot save its credential (or the response is lost), stop the Agent and fix the state directory's ownership, mode, or disk capacity. The single-use enrollment token cannot be reused. Find the node ID in the Agent error, dashboard, or Service `audit` enrollment event, then rotate that node's token as above. On a host with no credential file, import the rotated token from stdin instead of using `replace`:

```bash
sudo -u pulse-agent env PULSE_SERVICE_URL=https://pulse.example.com \
  PULSE_CREDENTIALS_PATH=/var/lib/pulse-agent/agent-credentials.json \
  pulse-agent credentials import NODE_ID < /path/to/mode-0600-rotated-token
```

Import refuses to overwrite an existing credential. Set the exact original Service URL, delete the temporary token copies, restart the Agent, and verify fresh samples arrive for the same node. The Service uses SQLite WAL with `synchronous=FULL` so confirmed credential changes are durable after a host crash, subject to the storage device honoring fsync.

## Backup and restore

Run the online backup command as the Service account:

```bash
sudo -u pulse env PULSE_DATABASE_PATH=/var/lib/pulse/pulse.db \
  /usr/local/bin/pulse-service backup
```

Pulse uses SQLite's online backup API to coordinate a consistent snapshot with live writers. It writes to a uniquely named mode-0600 temporary file, runs SQLite's integrity check, fsyncs it, and only then makes the final backup path visible beside the database. Copy the printed file to encrypted storage and test restoration periodically. A forward schema migration also creates and validates a backup and fails closed if the backup or migration cannot complete.

Restore only while the Service is stopped:

1. Stop `pulse-service` and take one final filesystem copy of the state directory.
2. Replace `pulse.db` with a verified backup; do not mix old `-wal` or `-shm` files into the restore.
3. Set ownership to `pulse:pulse`, directory mode to 0700, and database mode to 0600.
4. Start the Service and verify `/healthz`, the dashboard, Agent ingestion, and recent history.

Pulse performs forward-only schema migrations and does not automatically downgrade a database. Restore a pre-upgrade backup before rolling back across a schema change.

## Upgrade, rollback, and uninstall

Verify and extract the new release, then rerun `install-systemd.sh`. Back up the Service first. The installer keeps the previous executable target:

```bash
sudo ./deploy/install-systemd.sh service "$PWD/pulse-service"
sudo ./deploy/install-systemd.sh agent "$PWD/pulse-agent"
```

For a binary-only rollback with a compatible database and protocol:

```bash
sudo ./deploy/rollback-systemd.sh service
sudo ./deploy/rollback-systemd.sh agent
```

For an incompatible schema, stop the Service, restore the documented pre-upgrade backup, then roll back both Service and Agents. Never point an older Service at a newer database.

Uninstall binaries and units while preserving configuration, accounts, credentials, and data:

```bash
sudo ./deploy/uninstall-systemd.sh agent
sudo ./deploy/uninstall-systemd.sh service
```

Take a backup before manually deleting preserved paths under `/var/lib/pulse`, `/var/lib/pulse-agent`, or `/etc/pulse`.

## Capacity and metric selection

`PULSE_RETENTION_DAYS`, `PULSE_MAX_NODES`, and `PULSE_MAX_DATABASE_BYTES` form hard Service bounds. The defaults are 7 days, 100 active nodes, and 2 GiB. Expired snapshots are removed in bounded batches before new inserts, once at startup, and hourly even when no Agent is reporting, so a database at its page limit can reclaim expired pages before accepting another sample. The Service also limits request concurrency, request bodies, history responses, and the SQLite page cache. Monitor filesystem free space independently and alert before the configured database limit is reached.

The Agent interval is bounded to 1–300 seconds, defaulting to three seconds. The administrator can change it centrally; each Agent refreshes authenticated configuration every 30 seconds. Faster intervals increase disk usage; adjust capacity and retention together. By default, disks are deduplicated by device identity and loopback network interfaces are excluded. Container mounts, aliases, bridges, or virtual interfaces can still make aggregate metrics misleading; set exact comma-separated `PULSE_DISK_MOUNT_POINTS` and `PULSE_NETWORK_INTERFACES` allowlists when required.

## Diagnostics

Use `systemctl status`, `journalctl -u pulse-service`, and `journalctl -u pulse-agent`. Logs intentionally omit enrollment and Agent tokens. The health endpoint reports only process health and version. History responses include requested/actual coverage, source count, returned count, bucket width, and whether the window was downsampled. Agent collection timestamps and clock skew remain diagnostic fields; server receipt time controls online state and retention.
