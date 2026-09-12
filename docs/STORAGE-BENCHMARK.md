# Split-storage verification — 2026-09-12

## Scope

Local macOS x86_64 comparison of the released schema-3 Service and the schema-4 split-storage implementation. Both binaries were built from source with Rust/Cargo 1.98.0 in the debug profile and the same lockfile. No production databases, remote hosts, real Agents or external notification services were used.

The baseline source is `f81a58a8cbb6a4111376152d58d4e3c5b1f8cc8c`; its Service sources, Cargo.toml and Cargo.lock match released `b6b2fde9cc9df252626c023212a9ac7fd1b565f9`. At measurement time, the candidate was the uncommitted `kuddy/split-metrics-storage` worktree before alpha.4 release preparation.

## Write comparison

Four loopback-only Service processes each monitored one synthetic node: baseline/candidate at 1-second and 3-second intervals. Each received three warmup samples before a simultaneous 360-second measurement. Registration and initial account setup were excluded. Samples changed CPU/memory/network counters while keeping host metadata stable. The normal WAL checkpoint settings and FULL synchronization were retained.

The measure is the Service process's `ri_diskio_byteswritten` delta from macOS `proc_pid_rusage` flavor 2. It is OS write accounting, not database growth, NAND wear, or an Agent resource measurement. Each interval has equal before/after sample counts; the 1-second scheduler produced 358 samples over the six-minute wall-clock window, not a claimed 360.

| Interval | Accepted samples per version | Baseline bytes | Split bytes | Baseline MiB | Split MiB | Reduction |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 second | 358 | 13,262,848 | 11,866,112 | 12.6484 | 11.3164 | 10.53% |
| 3 seconds | 120 | 4,096,000 | 3,596,288 | 3.9063 | 3.4297 | 12.20% |

This is one local before/after run, not a statistical bound or a prediction for Linux production hosts. In particular, these numbers are not directly comparable to the earlier Linux Agent/Komari benchmark. No claim of Komari-like minute-aggregation write volume is made: every accepted raw Pulse sample is still persisted immediately.

Measurement binary SHA-256 values:

- Baseline: `f93673b594ecc6cbcae83e3ca19ffbcc881014af2adcd8eed74888a54f7f9a77`
- Candidate: `4db32bd520a333aec3a4d7acd430d230d44a9c483eaa5c060ad40d6f7a1c8c54`

After measurement, duplicate submissions remained idempotent, authenticated history counts matched every accepted sample, and the nodes remained online. SIGKILL was then sent to each owned test Service. SQLite recovery/integrity checks found 361 samples for each 1-second process and 123 for each 3-second process, including the three warmup samples. No acknowledged sample was lost. SIGKILL is a process-crash test, not a power-loss simulation.

## Migration and safety verification

The final implementation additionally passed a real stopped-Service upgrade using a later candidate binary (`1daacd460d05a289efff8aebb348ea376b910cf0f6039a2c6804532bc06a2e90`). Changes after the measurement concerned orphan cleanup synchronization, deleted-probe visibility, backup finalization and tests, not the ordinary snapshot write path.

The old Service created an account/session, enrolled one node and accepted three samples. It was killed without clean shutdown, leaving WAL state behind. The new Service opened the same temporary state, created a readable schema-3 pre-upgrade backup containing all three samples, migrated to the pair, accepted the original session cookie and Agent credential, retained history, and accepted another sample. Its live CLI backup returned a complete paired directory. After another SIGKILL, all four samples remained intact.

Regression coverage includes:

- Forward migration from released schemas 1, 2 and 3; credentials, sessions, TOTP data, nullable metrics, extensions and traffic values retained.
- Full or occupied destination, late migration failure, unknown/mismatched/missing database state fail closed without replacing source data.
- Main database data-version remains unchanged during routine snapshot ingestion; real metadata changes persist.
- Both database size bounds, FULL synchronization, indexed latest/history reads, retention/page-limit recovery, and probe/traffic/alert regressions.
- Consistent paired backup under external writes, private permissions, standalone read-only backup files, and preservation of pre-existing partial files.

Full workspace tests (90 passing), strict Clippy and formatting checks passed separately from the timing harness. All temporary databases, credentials and benchmark processes are removed when the scripts finish. Build artifacts remain under the ignored `target/` directory; no runtime databases or credentials are retained in the worktree.

## Reproduce

Use separately verified binaries from the same build profile. The script checks the actual schema version before measuring, preventing accidental reuse of a stale Cargo artifact. Use separate Cargo target directories when rebuilding different worktrees.

```bash
python3 scripts/benchmark-storage.py --baseline /absolute/path/to/schema3-service --candidate /absolute/path/to/schema4-service --duration 360
python3 scripts/benchmark-storage.py --baseline /absolute/path/to/schema3-service --candidate /absolute/path/to/schema4-service --upgrade-only
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

For SQLite's journal behavior, see the primary documentation on [read-only WAL databases](https://sqlite.org/wal.html#read_only_databases) and [multi-file atomic commit](https://sqlite.org/lang_attach.html).
