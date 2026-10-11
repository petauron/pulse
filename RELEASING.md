# Version releases

Release Please v5 is pinned to 45996ed1f6d02564a971a2fa1b5860e934307cf7.
Like Vastora, it uses only the built-in GITHUB_TOKEN; no extra release secret or
personal token is needed. Repository settings must allow Actions to create pull
requests. Existing branch protection and required CI remain mandatory.

Senulun is one release unit: its virtual Rust workspace uses
`workspace.package.version`, and all three crates inherit that version. The
pinned action bundles release-please 17.6.0, whose native Rust strategy expects
literal `package.version` values and cannot update this layout. Use the native
`simple` strategy with `version.txt` and native TOML/JSON extra-file updaters to
keep Cargo.toml, the three workspace Cargo.lock entries, and Web package files
in sync with the generated manifest. Do not replace inherited crate versions
with independently maintained versions. Cargo.lock selectors use `name.value`
because this updater queries tagged TOML values; third-party crates are excluded.

CI runs `scripts/check-release-please.mjs` against that pinned engine. It builds
a candidate version PR in memory using the repository configuration and checks
all version updates, inherited crate manifests, and unchanged dependencies.
It has no GitHub client or write credentials and does not create a PR or tag.
When updating the action SHA, review its bundled release-please version and
update the regression engine pin together. The candidate version's ordinary
CI still checks the actual generated files with Cargo's `--locked` checks.

After a push to main, the release workflow waits (at most 30 minutes) for the
exact commit's normal CI to succeed. It then creates the release, finishes
publication, and maintains the next version PR. Review and merge that PR rather
than bumping versions or creating tags manually.

GITHUB_TOKEN-created tags do not start another release workflow, and generated
PR workflows can require manual approval. The release workflow explicitly
dispatches ci.yml on the version PR branch to run the existing checks.

Publication runs in the same release.yml, gated by release_created, using the
Release Please output tag and immutable source SHA. The source must equal the
workflow commit. Builds and provenance checks remain required; only successful
publication makes the draft public. Tags are not a second release entry point.
On a build failure, use GitHub's Re-run failed jobs for that run; do not move tags
or overwrite published releases. A stale run skips preparation when main moved.


Local workflow edits alone do not activate releases. Push/review the changes,
then verify the first version PR CI and release run before calling the migration
complete. No local builds or tests are implied by this configuration change.

Ordinary CI keeps no downloadable artifacts. Release archives and SBOMs are
staged directly in the GitHub draft release, which downstream jobs read using
GITHUB_TOKEN; there is no duplicate Actions artifact copy. Docker build record
upload is disabled. Dependency/build caches remain available for CI speed.
Manual maintenance workflows follow the same no-artifact policy. The redundant
source-patch download workflow has been removed; ordinary CI retains its format,
generated-icon and frontend checks. Coverage reports are the only permitted
downloadable CI artifact, and require explicit approval and short retention.
