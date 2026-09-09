# Production operations

Pulse has two deployment roles: run `pulse-service` on the monitoring host, and run one outbound-only `pulse-agent` on every monitored Linux host. The Agent never accepts inbound commands. Run both under dedicated unprivileged accounts.

## Trust boundary

The dashboard read APIs are intentionally unauthenticated. Anyone who can reach the Service can read node names, operating-system and architecture details, online state, and health metrics. Keep the default loopback bind and place an authenticated HTTPS reverse proxy in front whenever this information is not public. Do not expose port 8080 directly to the Internet. Restrict it with the host firewall or a private network.

Remote Agents accept only HTTPS Service URLs. Pulse has no analytics, telemetry, runtime CDN, or mandatory third-party account. Metrics go only to the configured Service.

## Verify a release

Download the archive for the host architecture together with `SHA256SUMS`, its SPDX SBOM, and the GitHub artifact attestation. Verify before extracting:

```bash
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify pulse-v0.1.0-alpha.2-linux-x86_64.tar.gz --repo petauron/pulse
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

## Containerized Service

The image runs as UID/GID 65532 and stores its mode-0600 SQLite database in `/var/lib/pulse`. The Agent should still run directly on each monitored host so its metrics describe that host rather than a container.

```bash
sudo install -d -o 65532 -g 65532 -m 0700 /srv/pulse
docker run --detach --name pulse --restart unless-stopped \
  --publish 127.0.0.1:8080:8080 \
  --volume /srv/pulse:/var/lib/pulse \
  ghcr.io/petauron/pulse:VERSION
```

Even though the process listens on all interfaces inside the container, publish it to loopback as shown and terminate authenticated TLS at the reverse proxy. Run local administration with `docker exec`, using the same database path already present in the image environment.

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

The Agent interval is bounded to 5–300 seconds. By default, disks are deduplicated by device identity and loopback network interfaces are excluded. Container mounts, aliases, bridges, or virtual interfaces can still make aggregate metrics misleading; set exact comma-separated `PULSE_DISK_MOUNT_POINTS` and `PULSE_NETWORK_INTERFACES` allowlists when required.

## Diagnostics

Use `systemctl status`, `journalctl -u pulse-service`, and `journalctl -u pulse-agent`. Logs intentionally omit enrollment and Agent tokens. The health endpoint reports only process health and version. History responses include requested/actual coverage, source count, returned count, bucket width, and whether the window was downsampled. Agent collection timestamps and clock skew remain diagnostic fields; server receipt time controls online state and retention.
