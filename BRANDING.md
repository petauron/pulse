# Senulun product identity and upgrade

Senulun is the new product name of Pulse. It is the same service and collector,
not a fresh deployment. The rename updates the dashboard, default site title,
messages and documentation.

## Permanent machine contracts

Keep the existing `pulse-service` / `pulse-agent` executables, `PULSE_*`
environment keys, systemd unit/user names, `/etc/pulse` and `/var/lib/pulse`
paths, database and credential files, catalog IDs `pulse` / `pulse-agent`,
protocol fields and release asset names. These are stable
installation identities, not compatibility aliases. There is no parallel
Senulun binary, dual environment lookup or fallback implementation.

Do not delete or re-enroll collectors to change branding. Keep node IDs, agent
credentials, administrator credentials, sessions, probe history, traffic totals,
alert settings, storage identities, OAuth redirect URLs and mounted volumes.
Existing TOTP registrations remain usable; only newly generated enrollment
labels use Senulun.

## Forward-only data migration

Control schema 5 changes only `monitoring_settings.data.site_name` when its
exact value is `Pulse`. Custom titles (including titles containing that word)
are retained. Both control and metrics databases are backed up before upgrading
an existing schema-4 store. A missing/mismatched pair or backup/migration error
blocks startup. Never run an older service on the migrated database; recovery
requires the matched pre-upgrade backup pair while the service is stopped.
Fresh installations follow the same migration path and default to Senulun.

## Release boundary

The canonical source repository is `petauron/senulun`. Future releases publish
the Service image to `ghcr.io/petauron/senulun` and bind provenance to the new
repository identity. The catalog source registry must use that same identity.
Previously published `ghcr.io/petauron/pulse` images, archive names, tags and
attestations remain immutable; a repository rename does not re-sign old assets
or upgrade installed applications. Do not accept an old signer as a fallback
for a new-repository release.

Use the existing Release Please version PR and checked immutable commit. Keep
the `pulse` release component, Rust crate names and existing version history.
After release, use the protected catalog publication and an explicitly approved
Vastora managed upgrade. Preserve owned resources; never install a second app
or replace live containers by hand. Source PRs do not deploy production.
