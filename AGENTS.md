# Pulse Agent Rules

- Keep Pulse focused on server health monitoring. Do not add remote shells, file management, arbitrary command execution, or unrelated administration features.
- Optimize for predictable, low resident memory. Every persistent cache, background task, queue, and database buffer must have an explicit bound.
- Agents initiate outbound connections only. The Service must never gain a general-purpose command channel to an Agent.
- Do not add telemetry, crash uploads, analytics, or calls to third-party services unless an administrator explicitly configures them. The sole default-enabled exception is Agent country discovery through GeoJS: disclose it in installation/upgrade instructions, retain an explicit disable option and manual-region priority, and never send Pulse credentials or host metrics. Other third-party integrations still require explicit configuration.
- Released database schemas use tested, forward-only migrations. Back up before migration and fail closed on migration errors.
- Keep protocol changes versioned and explicit. Remove superseded pre-release implementations instead of maintaining hidden compatibility paths.
- Prefer memory-safe Rust and mature crates. Any `unsafe` block requires a documented invariant and focused tests.
- Build in small end-to-end slices and keep Service, Agent, protocol, storage, and Web concerns separate.
- Never commit credentials, enrollment tokens, private keys, production data, or generated runtime state.

- Do not run local builds. Generate Web assets and compile release artifacts in GitHub Actions; never commit `web/dist`.

## GitHub releases and Actions storage

- Use Release Please v5, pinned to a reviewed full commit SHA, with the built-in
  `GITHUB_TOKEN`. Do not add a PAT or a separate release-token secret. Grant only
  the job permissions needed; never weaken branch protection to enable releases.
- Release through the generated version PR and the repository's Release Please
  config/manifest. Use Conventional Commits, including the final squash title;
  do not hide a releasable fix under a `ci:` or `chore:` title. Do not manually
  bump versions, move tags, or add a second tag-triggered release path.
- Required checks must represent real checks on the exact version PR head SHA.
  When token-created PRs do not trigger them, explicitly dispatch the existing
  workflows. Metadata validation is a separate check, not a substitute for CI
  or CodeQL; never manufacture successful required-check results.
- Publish only from an immutable, checked commit on protected `main`. Use the
  Release Please output SHA/tag throughout checkout, build, provenance and
  publication; fail closed on identity/version mismatches or failed checks.
  Do not assume a tag created with `GITHUB_TOKEN` triggers another workflow.
- Keep releases as drafts until all required builds, integrity/provenance checks
  and uploads succeed. Retain only the distribution files and metadata required
  by users or verified consumers. Retry failed jobs or an explicitly documented
  recovery flow; never overwrite an already published release or move its tag.
- CI and manually dispatched maintenance workflows do not retain downloadable
  Actions artifacts: no binaries, UI bundles, browser evidence, source patches,
  workspace copies or build records. Do not add upload/download-artifact steps
  or retention-based exceptions for these files. Only explicitly requested test
  coverage reports may be uploaded, with narrowly scoped contents and short,
  documented retention. Keep the actual tests and required CI gates.
- Set `DOCKER_BUILD_RECORD_UPLOAD=false` for Docker build actions. Keep bounded
  dependency/build caches for CI speed; caches are not release artifacts.
  Stage necessary cross-job release files directly in the draft Release rather
  than keeping duplicate Actions artifacts.
- Artifact cleanup must enumerate exact targets, skip active runs and artifacts
  required to recover failed releases, and preserve published Release assets and
  caches unless separately authorized. Never include credentials, production
  data, private host details or full subscription URLs in files or logs.
- Distinguish workflow edits, successful CI, successful publication and production
  deployment in completion reports. A green preparation job with skipped publish
  jobs is not proof of a release. Documentation edits do not authorize a push,
  merge, release, signing operation or production deployment.

- Read `RELEASING.md` before changing release workflows. Keep the Rust workspace,
  Cargo lockfile, Web package files and manifest on the same release version.
  Verify both supported Linux architectures and preserve required licenses,
  checksums, SBOMs and provenance in distribution assets.
