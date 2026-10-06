# Rust ecosystem baseline

Research baseline: 2026-08-26.

Use this as a shortlist for common roles. It is not a mandate. Project constraints and current evidence win.

| Need | Default candidate | Boundary |
| --- | --- | --- |
| Serialization framework | `serde` | Default format-agnostic serialization layer. Pair with a format crate such as `serde_json` or `toml`. |
| JSON | `serde_json` | Normal JSON choice when using Serde. |
| OS threads | `std::thread` | Default starting point for a small number of long-lived blocking workers. |
| Basic channels | `std::sync::mpsc` | Start here for simple typed worker/event channels; research richer channel crates only when the semantics require them. |
| Async runtime | `tokio` | Standard choice when a project explicitly uses async Rust and the surrounding ecosystem is Tokio-compatible. Not the default concurrency mechanism. |
| Future/stream utilities | `futures` | Common combinators and traits for projects that intentionally expose async/future abstractions. |
| Structured diagnostics | `tracing` | Default for structured logging and spans; usable independently of whether domain code is async. |
| Library/domain errors | `thiserror` | Derive typed `Error`, `Display`, and conversion boilerplate while preserving variants. |
| Application/tool errors | `anyhow` | Good at executable/tooling boundaries where callers do not need structured variants. |
| CLI argument parsing | `clap` | Default full-featured CLI parser. |
| HTTP client | `reqwest` | Strong high-level HTTP client. Use its blocking API only when its dependency/runtime footprint fits the project; research a blocking-native client when the project intentionally excludes async runtimes. |
| HTTP types | `http` | Shared request/response/header/status types without choosing an implementation. |
| HTTP server | `axum` | Strong default for new Tokio HTTP services. Only relevant when the project intentionally chooses async server architecture. |
| SQLite, synchronous | `rusqlite` | Direct SQLite API with broad SQLite feature access. Strong fit for local synchronous persistence. |
| SQL, async or multi-database | `sqlx` | Postgres/MySQL/SQLite and compile-time checked query support without requiring an ORM. Choose when its async model and database scope fit. |
| SQL ORM | `diesel` | Prefer when strict typed query/ORM semantics are the actual requirement. |
| Regular expressions | `regex` | De facto default when backtracking-specific features are unnecessary. |
| UUIDs | `uuid` | Standard UUID generation/parsing/interchange crate. |
| Temporary files/directories | `tempfile` | Safer lifecycle management than hand-built temp-path logic. |
| Gitignore-aware tree walking | `ignore` | Good default for repository/code searches that should respect ignore files. |
| Basic recursive walking | `walkdir` | Smaller choice when ignore semantics are unnecessary. |
| Parallel iterators | `rayon` | Default for CPU-bound data parallelism over ordinary iterators. Do not treat it as a pool for long-lived blocked I/O workers. |
| Snapshot tests | `insta` | Established snapshot-testing choice when snapshots fit the assertion model. |
| Faster test runner | `cargo-nextest` | Tooling choice for running Rust test suites; keep `cargo test` compatibility unless project policy says otherwise. |
| Dependency vulnerabilities | `cargo-audit` | Checks `Cargo.lock` against RustSec. |
| Dependency policy | `cargo-deny` | Advisories, licenses, sources, duplicate versions, and policy checks. |

## Roles without one hard default

Some common areas have several strong choices. Research the actual requirements instead of forcing one package.

| Need | Candidates | Decision axis |
| --- | --- | --- |
| Date/time | `jiff`, `time`, `chrono` | API model, timezone needs, ecosystem integration, MSRV. |
| Blocking HTTP client | `ureq`, `reqwest::blocking`, others | Whether the project permits hidden/runtime async machinery, dependency size, TLS/streaming needs, existing stack. |
| Richer channels | `crossbeam-channel`, others | Select/multi-consumer semantics, boundedness, performance, ecosystem fit. |
| Unix/system calls | `rustix`, `nix` | API coverage, platform scope, compatibility needs. |
| Concurrent maps | `dashmap`, `papaya` | Workload shape and API requirements. |

## Research sources

Use several signals. Popularity is supporting evidence, not the decision rule.

1. Official crate docs and upstream repository for API, maintenance, MSRV, feature flags, and release state.
2. crates.io for ownership, versions, downloads, reverse dependencies, and metadata.
3. RustSec for vulnerabilities, unsoundness, malicious packages, and unmaintained advisories.
4. Blessed.rs as a curated map of commonly trusted ecosystem choices.
5. Project evidence: existing dependencies, interoperability requirements, compile/build constraints, runtime policy, and architecture.

## Selection rule

Prefer the established crate when it cleanly owns the mechanism the project needs. Keep local code for project-specific semantics, policy, and invariants. A crate should remove generic mechanism, not become a second owner of domain truth.

For concurrency, start from the project's runtime model. Threads/channels are a complete concurrency model for many applications. Add async only for a concrete requirement, not because waiting or streaming exists.