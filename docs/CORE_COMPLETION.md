# Core monitoring completion

Implementation scope: authenticated private dashboard and administration, alert
rules and delivery, bounded ICMP/TCP/HTTP probes and history, editable node/asset
metadata and billing-period traffic, additional host metrics, faster reporting,
and preservation of region configuration through Vastora upgrades. Reuse Emerald;
remote shells, file management and arbitrary commands remain out of scope.

The core implementation was merged through protected PR #12. Alpha.3 publication
was subsequently authorized and prepared on a separate release branch. Required
CI and release checks still gate publication; no production restart is implied.

## Work slices

- [x] Authentication, first administrator, sessions, private reads and protected administration (source)
- [x] Node metadata, billing-period traffic and persistent settings (source)
- [x] Alert rules, bounded delivery and incident history (source)
- [x] ICMP/TCP/HTTP probe scheduling, authenticated results and history (source)
- [x] Additional host metrics and faster bounded reporting (source)
- [x] Emerald login, management, probes and alerts UI (source)
- [x] Vastora region/GeoIP configuration preservation (isolated worktree)
- [x] Vastora Service bootstrap/public-origin integration (source; private file, not a read-only mount)
- [x] Independent source review, regression cases and operator documentation
- [x] Required local Web static checks, unit tests, type check and production build
- [x] Required local Rust formatting, Clippy and complete workspace regression tests
- [x] Regenerate checked-in Web distribution and Rust license bundle, then verify
- Browser tests and protected GitHub CI are required before merge; see the
  [integration PR checks](https://github.com/petauron/pulse/pull/12/checks) for the
  final revision's live result.
- Alpha.3 publication is tracked on the [release page](https://github.com/petauron/pulse/releases/tag/v0.1.0-alpha.3).
- [ ] Separately completed Vastora program integration and signed catalog publication

The Web license check, lint, 24 unit tests, Vue type check and production build
have passed. Rust formatting, strict Clippy and all 73 workspace tests have
passed. Production Web dependency auditing reported no vulnerabilities. The
checked-in distribution has been rebuilt with the reproducible `dev` revision;
the Rust notice bundle was regenerated and checked for 167 Linux dependencies.
The PR checks remain authoritative for the final submitted revision, including
Linux-only code and container builds. No live webhook destination was configured.

The follow-up independent review's capacity-rounding and OAuth lease findings
were fixed and re-reviewed. Regression tests cover non-page-aligned byte limits,
administrator access during delayed OAuth identity lookup, and rejecting a stale
OAuth flow after credential revocation.

Vastora changes remain isolated in [PR #405](https://github.com/petauron/vastora/pull/405);
its unrelated working-tree changes are not part of Pulse's release. The new
initialization/configuration semantics require updated compiled Vastora executor
contracts and a compatible program release, followed by the independently signed
catalog with verified Pulse artifact pins. A catalog refresh only advertises
versions; it never upgrades an installed application automatically.
