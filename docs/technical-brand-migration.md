# Technical identity migration to Senulun

This is a forward-only, explicit migration, not an alias layer. It does not
authorize publishing a release, signing a catalog or upgrading an instance.

## New runtime contract

- Rust crates, executables and release archives use `senulun-*`.
- All runtime/build environment keys use `SENULUN_*`.
- Standalone units, accounts and defaults use Senulun names. New Service images
  run as the same numeric UID/GID (65532), mount `/var/lib/senulun` and use
  `/var/lib/senulun/store/senulun.db` with its sibling metrics file.
- The image explicitly runs `migrate-and-serve`: when the retained volume has
  the old database pair, it creates a matched private backup, verifies identity
  and integrity, and atomically publishes a complete canonical `store` directory.
  It does not rename tables, create nodes or regenerate credentials. Old files
  remain offline recovery evidence, never a second read/write runtime path.
- Every subsequent managed startup checks the receipt and both database
  identities before opening the writer. A missing or mismatched database fails
  closed; a leftover receipt cannot initialize an empty replacement. Ordinary
  startup reads only identity rows, not the full monitoring history.
- Existing data and external traffic credential keys are retained. Never create
  a new traffic key for encrypted sources; migrate only its configuration name.
- Browser cookie names are canonical. Users may need to sign in again; account
  credentials, TOTP registrations and database records are not reinitialized.

## Managed upgrades

The paired catalog PR changes future verified source distribution names and
materializes the new environment, executable and mount contract only alongside
a verified new release. It does not rewrite the old signed recipe or publish
new files under an old version.

The paired Vastora PR normalizes inspected old environment assignments once
before replacement, preserving OAuth, retention/limits, the traffic key path,
country, disabled discovery, custom intervals and interface choices. Conflicting
old/new keys fail preparation, before stopping the writer. A nonstandard database
path requires an explicit offline migration instead of silently using a fresh
default. Persistent storage and node credentials stay attached to the original
application UUID and verified resource receipt.

Immutable catalog app IDs, task wire IDs/JSON fields and owned legacy backing
resource names remain **identity evidence**, not aliases. They must not be
text-replaced: doing so would discard upgrade ownership or invalidate signatures.
Source identifiers and new runtime configuration no longer use them as branding.
CLI administration selects the inspected installed executable once, so an old
signed version can still be backed up/upgraded. It never retries a failed command
against a different executable.

## Standalone installations

Do this in a maintenance window with the old writer stopped. First inventory
custom file paths and back up configuration, key files and the matched database
pair. Do not start the new installer beside an old installation; its preflight
stops before changing anything when old configuration has not been converted.

1. Keep numeric ownership when moving the old account/group to the canonical
   names. Refuse an already occupied target user, group, config or state path.
2. Move the owned configuration/state directories to the canonical locations on
   the same filesystem; keep a private pre-migration backup. Do not copy or
   re-enroll Agent credentials into a fresh identity.
3. Run `python3 deploy/migrate-environment.py SOURCE TARGET` using distinct
   absolute paths. It retains comments, exact values and the private original,
   rejects conflicting assignments, and never sources shell syntax. Review any
   retained explicit paths and update only paths actually moved in step 2.
4. For the Service, run the verified new executable's
   `migrate-storage SOURCE_DATABASE NEW_STORE_DIRECTORY` command. Source and
   target store directory must share one owned parent. The result contains
   `senulun.db`, `senulun.metrics.db` and a private migration receipt. Point
   `SENULUN_DATABASE_PATH` at that control database only after success.
5. Install the canonical unit/binary with the converted environment. Check
   account login, stored settings, original Agent node IDs, traffic counters,
   history and fresh reporting before retiring the stopped old unit/link.

The standalone directory/account move is deliberately an explicit operator
step, not an arbitrary shell command delivered by a catalog recipe. Do not run
both versions on shared data. Do not automatically downgrade a migrated schema.

## Regression coverage

CI covers canonical crate/release naming, locked versions, Web and Rust tests,
offline environment conversion and the paired database migration. The latter
checks retained enrollment hashes, node credentials, traffic totals and reporting
history, idempotence, occupied destinations and missing metrics. Companion
Vastora tests cover secret/result delivery and retained upgrade environments;
catalog tests prove the baseline is untouched and new recipes retain app/storage
identities. Source/CI success is not production acceptance.
