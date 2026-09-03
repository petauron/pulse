## Summary

Describe the user-visible change and why it belongs in Pulse.

## Security and resource impact

Describe new privileges, network access, persistent data, external services, memory allocations, caches, queues, or background tasks. Write `None` when there is no change.

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --all-targets`
- [ ] Documentation and changelog updated when needed
- [ ] No credentials, production data, or generated runtime state included
