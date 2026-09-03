# Pulse Agent Rules

- Keep Pulse focused on server health monitoring. Do not add remote shells, file management, arbitrary command execution, or unrelated administration features.
- Optimize for predictable, low resident memory. Every persistent cache, background task, queue, and database buffer must have an explicit bound.
- Agents initiate outbound connections only. The Service must never gain a general-purpose command channel to an Agent.
- Do not add telemetry, crash uploads, analytics, or calls to third-party services unless an administrator explicitly configures them.
- Released database schemas use tested, forward-only migrations. Back up before migration and fail closed on migration errors.
- Keep protocol changes versioned and explicit. Remove superseded pre-release implementations instead of maintaining hidden compatibility paths.
- Prefer memory-safe Rust and mature crates. Any `unsafe` block requires a documented invariant and focused tests.
- Build in small end-to-end slices and keep Service, Agent, protocol, storage, and Web concerns separate.
- Never commit credentials, enrollment tokens, private keys, production data, or generated runtime state.
