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
fixtures provide the acceptance gate. A temporary lock-resolution workflow
produced `sse-parse` 0.1.0, `futures-core` 0.3.34 and
`pin-project-lite` 0.2.17 plus the feature-corrected lock entries. The
exact 66-package lock resolution is now committed. The exact-head canonical conformance, flake matrix and generated DAG checks passed at #823 commit `393b814c`.

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

The three exact-head CI workflows passed for the Storage migration at #815 commit `51faf20c`. The separate full ACP SDK host/codec migration remains open within R1.

## Plugin-by-plugin crate ownership

Audited against the active M1 plugin manifests on 14 October 2026. The third
column names the library that should own generic mechanics. It does not assign
Phenix's authority, contract or graph semantics to that library.

| Plugin or host | Generic implementation | Decision and Phenix-owned behavior |
| --- | --- | --- |
| `phenix-plugin-storage-sqlite` | `rusqlite` 0.37, SQLite transactions, bindings and row decoding | **Adopted, verified in #815.** Keep the typed Storage provider, session isolation and revision CAS. No raw `sqlite3_*` bindings. |
| `phenix-plugin-storage-fixture` | Rust `BTreeMap` for deterministic in-memory tests | **Keep as a fixture**, not a second production database implementation. It exercises Storage semantics independently of SQLite. |
| `phenix-plugin-model-http` | `curl` 0.4 for synchronous HTTP/TLS/timeout/cancellation, `sse-parse` 0.1 for SSE, `serde_json` for model messages | **Mostly adopted.** Replace `request::Config::load`'s handwritten URL authority parser with `url`, in a separate lockfile-verified PR. Preserve the strict Model streaming grammar, bounds, JSON tool-call mapping and cancellation. The OpenAI-compatible payload mapping remains provider code, not a general HTTP stack. |
| `phenix-plugin-model-scripted` | `serde_json` plus ordinary Rust collections | **Keep as a deterministic test model**, not another transport or model API implementation. |
| `phenix-plugin-basic-frontends` | `serde_json` for JSONL, official `agent-client-protocol` 3.3.0 schema/codec for ACP | **ACP migration required.** Keep only typed Basic Input/Output mapping, session grant checks and journal-to-progress mapping. Delete redundant manual ACP envelope validation after SDK migration; Terminal has no external protocol. |
| `phenix-basic-runtime` | Official ACP SDK's connection and stdio transport; `std::fs::File::try_lock` for local locks; `std::io::BufRead` for bounded text input | **ACP migration required.** Preserve one kernel controller, graph-root ownership, per-session locks, durable owner markers, bounded input, backpressure and cancellation. The SDK must not move GraphExecution into an unrelated async task. A separate kernel-free ACP protocol adapter is acceptable if needed. |
| `phenix-plugin-basic-tools` | `serde_json` for argument decoding | **Keep the present three tools as examples only.** The catalog filter, cursor revision and counter are test-domain behavior. Production tool plugins should wrap maintained command/process, filesystem, HTTP or other task-specific crates rather than grow a generic built-in tool engine. Add a JSON Schema validator only when a provider actually validates schemas; declaring a schema is not validation. |
| `phenix-plugin-basic-memory` | Typed Portable encoding via SDK and the Storage contract | **Keep a small Skills/Context adapter.** The load/CAS/write logic implements Phenix's declared revision semantics. Separate future discovery adapters should use an existing Agent Skills parser or standard manifest parser rather than inventing a second skill format. No implicit filesystem scan in the kernel. |
| `phenix-plugin-basic-session` | Storage provider for durable records; SDK Portable values | **Keep Phenix-specific state transitions**, resume and interrupted-tool reconciliation. Avoid embedding another key-value database or generic persistence stack. |
| `phenix-plugin-event-journal` | Storage provider for persistent append/compare-and-swap and SDK Portable values | **Keep typed event sequence, cursor and observer semantics.** The library already supplies database transaction mechanics through Storage. A second event-log engine would duplicate persistence and could change at-least-once replay behavior. |
| `phenix-plugin-basic-agent` | SDK graph and typed Model, Tool, Skills, Context, Session, Journal interfaces | **Keep the Basic graph topology and turn policy.** No duplicated transport, JSON-RPC, filesystem discovery, scheduling or persistence implementation belongs here. |

### Review of the existing P1 PR stack

The following entries are migration requirements for code that already exists,
not optional advice for new plugins.

| Existing PRs | Review finding | Required disposition |
| --- | --- | --- |
| #818, #821, #832, #833, #834, #835, #836, #837 and #838 | `phenix-basic-runtime` and the ACP Frontend still manually route JSON-RPC requests, frame replies and map protocol events. Those patches repair live P1 correctness, but expand a host that duplicates part of the official SDK. | Keep the verified behavior and independent fixtures as regression tests. Implement an official `agent-client-protocol` v1 adapter, then remove the handwritten ACP router and codec before R1/P1 acceptance. Do not keep two production ACP protocol implementations. |
| #822 and #831 | Official ACP SDK is used as an **independent client test**, not as the host codec. | Keep the external SDK tests; their success is compatibility evidence, not proof that the server is SDK-backed. |
| #815 | Direct SQLite FFI was replaced with `rusqlite`. | Accepted library reuse. Preserve single Storage provider for Session, Memory and EventJournal; do not fork a second SQLite implementation. |
| #823 | SSE framing was replaced by `sse-parse`; JSON delta and ToolCall checks remain the Model provider's typed contract. | Accepted framing reuse. Future HTTP variants should use this transport/parser or a maintained provider-specific model SDK rather than new SSE decoders. |
| `phenix-plugin-model-http::request::Config` | Handwritten endpoint authority detection duplicates URL parsing. | Migrate to `url` in #840's implementation branch, with one canonical parsed URL handed to libcurl. The exact `Cargo.lock` was resolved by a temporary GitHub Actions job and adds 27 registry packages. The temporary job has been removed. Native compile, integration and full Nix acceptance remain unverified until exact-head CI. |

The `url` 2.5.8 option adds 27 packages to the current 66-package
Cargo.lock, mainly to support IDNA through ICU4X. That is a meaningful
compile and cache cost. It also buys a maintained WHATWG URL parser, IPv6,
Unicode hosts, normalized paths and parsed userinfo. Keep `curl` as the
sole HTTP/TLS transport; no second HTTP client is warranted. Reconsider
the choice only if an equally maintained parser with matching validation
semantics has a materially smaller dependency cost.

**Gate for the existing stack:** a green test of handwritten ACP protocol
handling proves behavior, not R1 completion. R1 closes only after the SDK
adapter replaces that code and all current malformed-frame, cancellation,
streaming, restart, tool-event and EOF fixtures pass without a parallel
handwritten implementation.

### Official ACP v1 schema migration

The first implementation slice on `impl/m1-r1-acp-schema-frontend`
feature-gates `agent-client-protocol-schema` 1.11.0 to the native ACP
Frontend plugin. `acp_decode` now deserializes the SDK's stable-v1
`PromptRequest`, `CancelNotification` and `ContentBlock` payloads
instead of selecting `params.sessionId` and `params.prompt` by hand.
It renders journal output with the official v1 `SessionNotification`,
`SessionUpdate`, `ContentChunk`, `ToolCall` and `ToolCallUpdate` types.
The schema omits `Pending` tool status by default; ACP consumers infer the
standard default. Phenix still checks the selected session and limits input
to Basic-supported content. The host's JSON-RPC envelope framing remains
legacy code until the SDK-backed connection migration.

The resolved lockfile grows from 93 to 169 packages: this is a real
dependency cost of the official schema, including its `serde_with` and
related packages. The temporary GitHub Actions resolver was removed.
All ACP native checks, Rustfmt, ABI, streaming and independent SDK
fixtures must pass before accepting the typed-input slice.

The next slice must use the official SDK's stable-v1 `Agent.builder()`
handlers, `Stdio` transport and typed notifications. SDK dispatch
must not own Phenix's GraphExecution: a bounded typed command/event
channel hands protocol requests to the existing single-thread kernel
controller. Responses and journal progress return to the SDK writer.
A cancellation notification targets the scoped graph root; a client
disconnect must trigger cancellation and durable cleanup. The SDK
is responsible for JSON-RPC syntax, request IDs and wire serialization.

**Removal gate:** delete the old production `parse_acp_frame` and
manual `session/update` encoding after the typed host passes the
independent official SDK client and negative HTTP/stdio fixtures.
Client-side interoperability alone is insufficient. The new host
must also define how ACP's baseline `ResourceLink` prompt content
is handled, rather than silently claiming complete ACP compliance.

### SDK-backed host integration (R1-ACP2)

The implementation branch `impl/m1-r1-acp-sdk-host-v1` introduces a
`phenix-basic-runtime::acp_sdk` adapter using
`agent-client-protocol` 3.3.0, with `stdio` and draft
`unstable_protocol_v2` enabled. The v1 adapter registers official
`Agent.builder()` request/notification handlers and sends typed replies,
while the protocol transport uses the SDK's `Stdio`. It is selected with
`PHENIX_BASIC_USE_ACP_SDK=1` until independent fixtures pass and the
legacy `run_acp` implementation can be removed.

The first remote Cargo check exposed that the Phenix Kernel intentionally
contains non-`Send` state. The adapter keeps Controller, Kernel,
GraphExecution and the durable session lock on the caller thread; only
SDK wire dispatch moves to a transport thread. A 64-entry controller queue
admits requests, and a session-scoped atomic cancellation signal bypasses
the queue during a live graph. SDK notifications are typed before writing.
The current SDK adapter temporarily serializes the existing Frontend
render output back into `SessionNotification`; remove that translation
when R1-ACP3 changes the Frontend render contract.

**R1-ACP2 is not accepted yet.** Independently test the new SDK path for
initialize, new/load, durable replay, prompt, streamed text, tool updates,
queue saturation, cross-session cancellation and stdin EOF. Preserve a
bounded physical stdio frame policy: the SDK's standard `Stdio` reader
must be checked for equivalent limits or wrapped with bounded SDK
`Lines`/transport primitives before replacing the legacy host. Verify
graceful close and exactly-once owner marker cleanup. Then make the SDK
path the default and delete `run_acp`, `drive_acp_prompt` and
`parse_acp_frame`, including their legacy-specific tests. Do not retain
both paths as selectable production implementations.

The `unstable_protocol_v2` feature currently only makes SDK v2 APIs
available at build time. Serving and negotiating v2 requires a separate
typed v2 session/prompt lifecycle adapter (R1-ACP5); that is not implied
by v1 conformance or enabling the Cargo feature.

### R1 migration order

1. **ACP codec and transport.** Replace the handwritten host and frontend JSON-RPC implementation with the official `agent-client-protocol` stable-v1 API. The independent SDK client already passes against the custom host. Compare both implementations on invalid envelopes, sessions, live progress, tool events, FIFO requests, EOF, cancellation and crash recovery before deleting the old path.
2. **HTTP endpoint URL parsing.** Use `url` to parse absolute HTTP(S) URLs and reject credentials, fragments, non-web schemes, invalid hosts and controls. Keep libcurl as the only HTTP/TLS transport. Pin and rebuild the workspace Cargo.lock. Test IPv6, query, ports, malformed authority and userinfo.
3. **Future Skills/Tools adapters.** Pick a parser or domain library when introducing a real filesystem Skills provider or concrete tool. The three demo tools and static catalog are acceptance fixtures, not reasons to add a second framework.
4. **Reduce duplicate domain glue.** Compare Storage record read/CAS helpers across Memory, Session and Journal. Consolidate a helper only when their revision, authorization and corruption behavior are identical. Keep distinct operations where those semantics differ.

### Review gate for new plugins

A new plugin PR should name the underlying maintained crate or standard-library
API for every generic responsibility: transport, parser, database, process,
filesystem, serialization and scheduling. If it writes a protocol parser or
database wrapper instead, record why an existing library cannot preserve the
required behavior. Include the crate version and license, feature/dependency
cost, cancellation/async model, failure and size bounds, and a real integration
fixture. Keep `phenix-kernel` unaware of the external library.

Thin adapters are the default for providers. The kernel's graph/authority
mechanisms and the Basic agent's turn policy remain Phenix-specific code.
Avoid installing a framework solely to wrap a dozen lines of Rust collections
or a typed domain transition.
