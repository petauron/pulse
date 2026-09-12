# Core monitoring completion

Implementation scope: authenticated private dashboard and administration, alert
rules and delivery, bounded ICMP/TCP/HTTP probes and history, editable node/asset
metadata and billing-period traffic, additional host metrics, faster reporting,
and preservation of region configuration through Vastora upgrades. Reuse Emerald;
remote shells, file management and arbitrary commands remain out of scope.

Work is kept on feature branches. Existing GeoIP work is preserved. Submission
and PR merge were explicitly requested; repository-required checks are being run
as part of that workflow. No production restart or release is implied.

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
- [ ] Complete browser tests and protected GitHub CI gates
- [ ] Separately authorized publication and catalog artifact update

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

Pulse is on `kuddy/agent-auto-region`. Vastora changes are isolated on
`kuddy/pulse-monitoring-config`; the original Vastora `main` worktree and its
unrelated changes were left alone.
The catalog still points to Alpha.2 artifacts; do not deploy that source change
without a corresponding new Pulse release and normal catalog update.
