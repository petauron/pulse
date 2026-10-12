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

The Rust license template keeps workspace package attributions without the
product version; registry/git dependency versions and all license texts remain
included. A version-only PR must not stale the third-party notice bundle. CI
still regenerates and compares the complete bundle with pinned cargo-about.

After a push to main, the release workflow waits (at most 30 minutes) for the
exact commit's normal CI to succeed. It then creates the release, finishes
publication, and maintains the next version PR. Review and merge that PR rather
than bumping versions or creating tags manually.

GITHUB_TOKEN-created tags do not start another release workflow, and generated
PR workflows can require manual approval. The release workflow explicitly
dispatches ci.yml on the version PR branch to run the existing checks.
CI and dependency review use the same repository/branch concurrency identity for
PR and dispatch events, so overlapping runs cancel the older duplicate. A later
dispatch after completion is still a deliberate rerun. CI checks out the exact
PR head (or dispatch/push SHA); both event paths execute all existing checks.

Publication runs in the same release.yml, gated by release_created, using the
Release Please output tag and immutable source SHA. For new releases the source
must equal the workflow commit. Builds and provenance checks remain required;
only successful publication makes the draft public. Tags are not a second
release entry point. A stale push skips preparation when main moved.

## Recover an unpublished draft

On a transient build failure, use GitHub's Re-run failed jobs for that run. If
the workflow itself needs a fix, merge the fix through ordinary PR checks first.
Then dispatch the existing Release workflow **on main**, with `release_tag` set
to the exact unfinished tag, for example:

```sh
gh workflow run release.yml --ref main -f release_tag=v0.1.0-alpha.15
```

Recovery does not invoke Release Please to recreate the release. It requires an
existing bot-created draft whose target is a full commit SHA. It preserves that
source, checks main ancestry, any existing tag, successful source CI and all
version files, then runs the same binary, container and publication jobs. The
workflow revision must itself be current checked main. Published releases and
mutable targets are rejected. Never delete/recreate the draft, move its tag or
overwrite a published release to work around a failure.

Push-triggered preparation preserves pending drafts and does not create the next
version PR while one remains unfinished. Draft readers need job-scoped
`contents: write`: GitHub hides drafts from tokens without push-level access,
even when the job only checks metadata or downloads assets. Ordinary CI stays
read-only. No personal token, new secret or protection bypass is needed.

## Release provenance

The pinned `actions/attest` action signs a SLSA v1 predicate using the standard
`https://actions.github.io/buildtypes/workflow/v1` build type accepted by GitHub.
The workflow dependency resolves `refs/heads/main` to its checked SHA; an
additional `release-source` dependency identifies the immutable product source,
which can differ during recovery. Recovery records `release_tag` in dispatch
inputs. The workflow path, trigger and run attempt identify the builder
invocation, following the GitHub Actions workflow schema. It makes no additional
SLSA level claim. The publishing job binds the invocation to its actual run
attempt, including failed-job retries that reuse preparation outputs.
Checksums and SBOM generation remain mandatory. CI exercises
draft rejection, immutable identity, provenance and workflow safeguards without
GitHub writes or publication.


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

## CI dependency caches and compilation

The pinned Swatinem/rust-cache action retains Cargo downloads and compiled
dependencies, prunes unused crates and artifacts older than one week, and excludes
workspace binaries and incremental outputs. Ubuntu checks and browser tests have
separate job keys; security caches only dependency sources. Native Bookworm builds
share a key between CI and release jobs, scoped by CPU architecture, Rust toolchain
and the pinned builder script. Ubuntu outputs never enter this cache. The Docker
builder mounts the restored Cargo home and runs with the runner's UID/GID.
Dependency changes retain the action's partial restore behavior. Product source
and embedded Web assets are compiled again for the checked source SHA.

Within CI, the container job compiles the Debian 12 binaries once, verifies both
binaries in the restricted non-root Debian runtime, then packages those same
binaries with Dockerfile.release and checks the final container's identity and
health. Clippy, Rust tests, Web checks, security, browser flows and the required
`rust` aggregate gate remain mandatory. Release builds still compile their own
immutable source for both supported architectures; only dependencies are reused.

Caches stay within GitHub's repository cache quota and normal eviction policy;
there are no per-commit cache keys or downloadable CI artifacts. PR caches cannot
seed main, so the first run on a new scope is cold. Compare actual job/step times
and cache hit logs separately from runner queue time when measuring improvements.
