# R1 library and protocol audit

13 October 2026

Scope: the active M1 rewrite. [The roadmap](m1-roadmap.md) owns milestone status. This file records the implementation decisions and the remaining checks.

| Code | Standard mechanism | Choice | Required Phenix behavior |
| --- | --- | --- | --- |
| `crates/phenix-plugin-storage-sqlite/src/lib.rs` | SQLite connection, statement binding, rows and transaction lifecycle | Use `rusqlite` 0.37 with the system SQLite library. Remove manual `sqlite3_*` declarations and unsafe pointer handling. | Missing or disallowed database paths fail closed. Keep 5-second busy timeout, FULL synchronous mode, immediate write transactions, atomic multi-key CAS, revision overflow refusal and session checks. |
| `crates/phenix-plugin-model-http/` | HTTP, TLS and SSE framing | Keep `curl` for blocking HTTP and use `sse-parse` 0.1.0 for incremental WHATWG SSE framing without an async runtime. | Bounded frames, strict Model UTF-8 and typed JSON/usage, fragmented input, redirect refusal, TLS trust and physically drained cancellation. |
| `crates/phenix-basic-runtime/src/main.rs` and `crates/phenix-plugin-basic-frontends/src/lib.rs` | ACP and JSON-RPC framing | Evaluate the official ACP Rust SDK before expanding the custom message handling. Replace protocol code where the SDK fits the pinned ACP version. | Authenticated session selection, checked action admission, progress and cancellation mapping. Test through an independent ACP client. |
| `crates/phenix-plugin-basic-frontends/src/lib.rs` | JSONL serialization | Keep `serde_json`; do not add another parser. | Validate typed input, selected session and denied cross-session output. |
| Kernel, SDK and Basic contracts | Phenix wire identity, authority, graph admission and leases | Keep the canonical Phenix implementation. Generic libraries cannot define these semantics. | Independent wire, ABI, graph, denial and generation conformance. |

## SSE parser decision

`impl/m1-r1-standard-sse-parser` replaces the hand-written SSE line and
data-field accumulation with `sse-parse` 0.1.0. Its sans-I/O
`SseParser::feed` and `next_event` APIs fit the current synchronous
`curl::easy` callbacks. The bounded parser enforces the portable size
cap and reports dropped oversized frames as failures. It handles CR/LF,
CRLF, comments, BOMs, multi-line data and byte fragmentation.

The Model plugin still validates its JSON deltas, cumulative token usage,
tool call envelopes, finish reasons and `[DONE]` marker. A short
incremental UTF-8 check preserves the existing fail-closed Model contract,
which is stricter than the SSE standard's replacement-character behavior.
No async executor or extra I/O stack is introduced.

The existing Model plugin unit tests and independent pending HTTP product
fixtures provide the acceptance gate. The new parser dependency needs a
locked Cargo resolution and exact-head Nix shipping proof before R1 SSE
framing is accepted.

## ACP v1 library decision

The official [ACP Rust SDK](https://github.com/agentclientprotocol/rust-sdk)
publishes `agent-client-protocol` 3.3.0 with a stable v1 client and agent API.
Its `agent-client-protocol-schema` companion supplies the protocol data model.
Use the maintained SDK instead of extending Phenix's handwritten JSON-RPC
router for additional protocol features. Keep kernel graph admission, resource
authority and the one-thread execution requirement owned by Phenix.

The current lightweight ACP host remains a P1 test implementation. It
handles stable-v1 initialization, session entry, journal progress and prompt
cancellation. A client requesting a newer protocol version receives the
supported version 1 so that the client may negotiate down. Malformed
JSON-RPC envelopes do not terminate an idle connection.

R1 acceptance includes a real official-SDK ACP client exercising the
native host over stdio. The isolated `tests/acp-sdk-client` crate pins the
SDK to 3.3.0 and uses its stable-v1 Client, AcpAgent process transport and
typed notifications. The separate `P1 official ACP SDK interop` workflow
launches the exact-head Nix ACP product wrapper, initializes the connection,
runs a prompt and requires a completed turn with session updates. This
check is independent of the existing hand-written JSON-RPC fixtures.

The integration fixture does not introduce the SDK into the kernel's Cargo
workspace. The first official-SDK integration run passed on commit
`9534a35d` and produced a 144-package test dependency resolution. That
`Cargo.lock` is now committed under `tests/acp-sdk-client`; the workflow
uses `cargo run --locked`. The fully locked test and canonical Nix shipping
checks must pass on the updated exact head before acceptance. SDK-backed
replacement of the handwritten ACP codec remains separate work. The
protocol driver must keep the kernel's active root on its owning thread.

## SQLite migration acceptance

The Storage plugin now uses `rusqlite::Connection`, bound parameters and `TransactionBehavior::Immediate`. The `BEGIN IMMEDIATE` CAS boundary remains at the same point. A failed edit returns a domain error and rolls back the entire transaction. The plugin remains a separately packaged native provider; the kernel and public Basic contract do not change.

Validate on the exact PR head:

1. Run the Rust build, format and Clippy checks.
2. Run the native B2 SQLite restart, concurrent CAS, rollback, corruption and session-denial fixtures.
3. Run the B5 pending HTTP product, Terminal/JSONL and ACP restart fixtures against the same Storage provider.
4. Run all Nix shipping checks, including independent ABI and security negatives. Compare the accepted conformance reports with the parent commit.

Acceptance is pending until these checks pass. The separate SSE and ACP reviews remain open within R1.
