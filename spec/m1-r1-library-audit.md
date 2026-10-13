# R1 library and protocol audit

13 October 2026

Scope: the active M1 rewrite. [The roadmap](m1-roadmap.md) owns milestone status. This file records the implementation decisions and the remaining checks.

| Code | Standard mechanism | Choice | Required Phenix behavior |
| --- | --- | --- | --- |
| `crates/phenix-plugin-storage-sqlite/src/lib.rs` | SQLite connection, statement binding, rows and transaction lifecycle | Use `rusqlite` 0.37 with the system SQLite library. Remove manual `sqlite3_*` declarations and unsafe pointer handling. | Missing or disallowed database paths fail closed. Keep 5-second busy timeout, FULL synchronous mode, immediate write transactions, atomic multi-key CAS, revision overflow refusal and session checks. |
| `crates/phenix-plugin-model-http/` | HTTP, TLS and request transport | Keep the maintained `curl` crate. Review the custom SSE parser in `src/stream.rs` against established event-stream parsers. | Provider-selected limits, typed progress, fragmented event handling, redirect refusal, TLS trust, errors and physical cancellation draining. |
| `crates/phenix-basic-runtime/src/main.rs` and `crates/phenix-plugin-basic-frontends/src/lib.rs` | ACP and JSON-RPC framing | Evaluate the official ACP Rust SDK before expanding the custom message handling. Replace protocol code where the SDK fits the pinned ACP version. | Authenticated session selection, checked action admission, progress and cancellation mapping. Test through an independent ACP client. |
| `crates/phenix-plugin-basic-frontends/src/lib.rs` | JSONL serialization | Keep `serde_json`; do not add another parser. | Validate typed input, selected session and denied cross-session output. |
| Kernel, SDK and Basic contracts | Phenix wire identity, authority, graph admission and leases | Keep the canonical Phenix implementation. Generic libraries cannot define these semantics. | Independent wire, ABI, graph, denial and generation conformance. |

## SQLite migration acceptance

The Storage plugin now uses `rusqlite::Connection`, bound parameters and `TransactionBehavior::Immediate`. The `BEGIN IMMEDIATE` CAS boundary remains at the same point. A failed edit returns a domain error and rolls back the entire transaction. The plugin remains a separately packaged native provider; the kernel and public Basic contract do not change.

Validate on the exact PR head:

1. Run the Rust build, format and Clippy checks.
2. Run the native B2 SQLite restart, concurrent CAS, rollback, corruption and session-denial fixtures.
3. Run the B5 pending HTTP product, Terminal/JSONL and ACP restart fixtures against the same Storage provider.
4. Run all Nix shipping checks, including independent ABI and security negatives. Compare the accepted conformance reports with the parent commit.

Acceptance is pending until these checks pass. The separate SSE and ACP reviews remain open within R1.
