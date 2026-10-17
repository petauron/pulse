# Senulun legacy-name audit

The source audit covers Senulun, Vastora, catalog, Glivela and the website.
Before cleanup, case-insensitive matches appeared in 69, 112, 14, one and zero
tracked files respectively, excluding changelogs and dependency lockfiles.
Matching-file counts include history and immutable identities, not only defects.

## Remediated source and future runtime

- Canonical Rust crate/directory/dependency names, Cargo lock entries and
  version-updater selectors; no manual version change.
- Executables, usage text, User-Agent values, cookie names, new token prefixes,
  build/runtime environment variables and default configuration/data paths.
- Docker and systemd packaging, new release archives, SBOM/checksum/provenance
  consumers and future verified catalog release materialization.
- Vastora monitoring package, types/functions/files and frontend helpers, plus
  their Glivela consumer; existing task JSON and IDs remain stable.
- Traffic-key instructions, Web API/event/cache names, current installation
  and publication documentation, and focused CI regressions.
- Explicit offline configuration conversion and backed-up, identity-verified
  paired database migration; not an automatic parallel installation.

## Deliberately retained evidence, not runtime naming fallbacks

- Released SQL migrations, legacy migration fixtures, immutable catalog IDs and
  persisted task fields; changing these would break queued work or ownership.
- Published assets/tags, signed catalog snapshots, provenance and old source
  coordinates. New assets use new names; old bytes are never overwritten.
- The existing CSRF hash-domain constant, third-party license attributions,
  Tailwind `animate-pulse`, and compatibility-patch history.
- Existing verified resource names/paths and private originals/backups. They
  remain attached to the same application UUID rather than creating new empty
  volumes or credentials. New installations use canonical defaults.
- Unrelated local checkouts/worktrees and other sessions. Their relocation is
  not part of a source PR; no Codex history was modified.

See [the migration runbook](technical-brand-migration.md) for rollout order,
standalone path/account conversion and acceptance criteria. This audit contains
no private endpoints, credentials or production identifiers. Production has not
been changed by these source PRs.
