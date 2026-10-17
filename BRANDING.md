# Senulun product and runtime identity

Senulun is the current name of the former Pulse project. New source, Rust
crates, executables, release archives, environment variables, standalone units,
accounts and default paths use Senulun names. There are no binary aliases or
dual environment-variable readers.

Read [the technical migration](docs/technical-brand-migration.md) before
upgrading a retained installation. Back up the matched databases and private
configuration/key files, convert configuration before switching the program,
and retain node identity, custom settings, traffic totals and history. Never
re-enroll a probe merely to rename it. Service and Agent upgrades are separate.

The old default-site-name schema migration remains forward-only and preserves
custom titles, even titles containing the old word. Historical tags, signed
manifests, trust metadata and assets stay immutable. Original catalog IDs,
persisted task wire IDs and verified backing-resource names remain ownership
evidence, not parallel implementations or public product branding.

New releases bind exactly `petauron/senulun` and `ghcr.io/petauron/senulun`.
Do not accept a former signer as fallback, replace published release bytes,
renumber the release history or manually bump a version. Use Release Please
and the protected catalog publication flow. PR creation does not publish or
deploy anything.
