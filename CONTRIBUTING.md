# Contributing to Pulse

Thank you for helping improve Pulse.

## Before opening a change

1. Search existing Issues and Pull Requests.
2. Use an Issue for behavior changes or architecture proposals before writing a large patch.
3. Keep changes within Pulse's monitoring-only scope and document their memory impact.
4. Report vulnerabilities privately through GitHub Security Advisories rather than a public Issue.

## Development

Pulse uses the Rust version pinned in `rust-toolchain.toml`.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Commits must not contain secrets, production databases, machine identifiers, or Agent credentials.

## Pull Requests

- Keep each Pull Request focused on one coherent change.
- Explain the observable behavior, security implications, memory impact, and validation performed.
- Add or update tests for changed behavior.
- Update documentation and `CHANGELOG.md` when the public behavior changes.
- Accept maintainer edits and follow the repository Code of Conduct.
