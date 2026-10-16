# Senulun runtime identity cutover

Senulun is the successor name for Pulse. The public name changed first; this
release changes the executable, environment, service and state identities;
the independent catalog changes its application IDs in a separate publication.
It is **not** an in-place binary replacement for a running Pulse
installation. Do not start it against the old data directory or credentials.

## New runtime contract

- Binaries: `senulun-service` and `senulun-agent`.
- Environment variables: `SENULUN_*` only. Old `PULSE_*` variables are not
  accepted as fallback configuration.
- Service data: `/var/lib/senulun`, with `senulun.db` and
  `senulun.metrics.db`.
- Agent state: `/var/lib/senulun-agent`; standalone default state is
  `$XDG_STATE_HOME/senulun`.
- Standalone systemd units: `senulun-service.service` and
  `senulun-agent.service`; Vastora uses its own managed unit names.
- Release archives: `senulun-<tag>-linux-<arch>.tar.gz`.

The monitoring wire protocol and database schema retain their existing
versioning. Changing filesystem names must not create a new monitoring node,
administrator account or authentication credential.

## Existing installations

Stop both old processes before copying their data. Back up the matched control
and metrics SQLite files, Agent credentials and configuration, then verify
their digests. Move a verified copy to the new protected paths, preserve file
permissions and node identity, and start only the new units. Never run old and
new writers against the same database or credential file. Check node count,
pairing, traffic totals and health before retiring old units or storage. Keep
the old stopped resources as recoverable evidence until acceptance.

Vastora-managed installations require a separate, explicitly invoked
maintenance migration. Merely refreshing the signed application catalog does
not perform the cutover. A failed copy, ownership check or health check must
leave the instance blocked; do not repair it by re-enrolling the Agent or
silently reinstalling the Service.

Historical release assets, image digests, signatures and signed catalog
revisions remain immutable. An old `petauron/pulse` asset is never presented
as if it were built and attested by `petauron/senulun`.
