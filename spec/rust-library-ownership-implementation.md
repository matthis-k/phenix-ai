---
status: implemented
source: rust-library-ownership-audit
snapshot: 5c3c0f2ee187c6dd38d569d2467055b8afcd067a
---

# Rust library ownership implementation

This file is the completion record for `spec/rust-library-ownership-audit.md`. The audit remains the design reference.

## Goal

Phenix owns domain semantics. Maintained Rust libraries own generic protocol, parsing, synchronization, HTTP, validation, error, and code-generation mechanics when they fit without weakening Phenix contracts.

Preserve wire formats, validation, deterministic ordering, redaction, authority, lifecycle, generation identity, transaction semantics, public error meaning, and source-compatible APIs unless the PR records an intentional contract change.

## Definition of done

- [x] Every ownership item is implemented, rejected with a current architectural reason, or superseded by a better existing mechanism.
- [x] No adopted upstream mechanism leaves a redundant local implementation behind.
- [x] No dependency is added only to save trivial code or hide Phenix semantics.
- [x] Intentional API or wire changes have deterministic regression coverage.
- [x] Exact-head Source, Rust, Clippy, Product, Integration, Docs, and Maintenance remain merge gates. Workflow evidence belongs in the PR body so recording it does not create a self-referential SHA change.
- [x] The final hand-maintained Rust LOC measurement uses one reproducible method and separates historical drift from the PR delta.

## Verified upstream baseline

Checked against current upstream documentation on 2026-09-13. Versions are evidence for this pass, not permanent pins.

| Mechanic | Current upstream | Decision |
| --- | --- | --- |
| ACP | `agent-client-protocol` 2.x | owns ACP protocol and MCP-over-ACP carrier |
| MCP | `rmcp` 3.3.x | owns typed MCP model, version knowledge, lifecycle/cancellation parameters |
| ACP ↔ rmcp | `agent-client-protocol-rmcp` 3.x exposes `rmcp` 2.x | defer until it matches the workspace rmcp major and removes more carrier glue |
| OAuth2 | `oauth2` 5.x | owns PKCE, authorization-code exchange, refresh, and token primitives |
| JWT | `jsonwebtoken` 9.x | extraction-only decoding for trusted token-exchange metadata |
| HTTP | `http` 1.x + `url` 2.x | canonical provider request/response types |
| bytes | `bytes` 1.x | canonical provider transport payload |
| secrets | `secrecy` 0.10.x | runtime secret storage and redacted debug |
| proc macros | `proc-macro-crate` 3.x + `trybuild` | consumer crate lookup and compiler-level external fixture coverage |
| graph algorithms | `petgraph` 0.8.x | rejected for current `ComponentGraph`; workspace-wide consolidation is #519 |

## Decisions

### Errors and synchronization

- `thiserror` replaces mechanical `Display`/`Error`/`From` implementations where it preserves exact taxonomy and messages. Manual implementations remain when formatting or public field names would change.
- `parking_lot` replaces synchronous mutexes where poisoning is not a domain contract. This includes ACP session/cancellation state, event delivery state, task runtime registries, observables, native backend state, runtime/plugin state, and ACP client ordering/extension metadata.
- Removing poisoning does not change public method contracts solely because a lock became infallible. `SessionUpdates::resume_at` therefore keeps its existing `Result<(), ClientError>` API and now returns `Ok(())` after the infallible `parking_lot` update. A regression test pins that source contract.

### IDs and invariants

- The duplicate domain `SessionId` is removed in favor of the canonical core `SessionId`. The resulting stricter character policy is intentional and covered by wire-deserialization tests.
- Remaining validated IDs use the shared `identifier!`, `domain_id_type!`, or `validated_string!` machinery. `nutype` would still need Phenix `ValueCodec`/schema glue and would change generated errors, so it is not adopted.
- Stable positive-integer parsing uses `NonZeroU32`, `NonZeroU64`, or `NonZeroUsize` where zero is structurally invalid. Existing public numeric fields retain boundary validation when changing their field types would widen this PR into an API migration.
- No non-empty collection crate is added. Current collections are either legitimately empty or already validated at their boundary.
- Filesystem paths stay `Path`/`PathBuf`; a relative-path crate would not remove domain mechanics.

### Option contracts

`phenix-plugin-options` imports and reexports the SDK-owned option IDs, scopes, values, definitions, commands, responses, service function, and `OptionsInterface`. Existing plugin import paths remain valid. The plugin retains persistence, precedence, validation policy, and resolution state.

The removed plugin-local `OptionsInterface` duplicate declared the service with `InterfaceSchema::of::<OptionCommand, OptionResponse>()`, even though the generated options component returns `Result<OptionResponse, String>` and therefore exports a fallible schema. The SDK-owned `OptionsInterface` already models that real component contract with `InterfaceSchema::fallible_of::<OptionCommand, OptionResponse, String>()`. Consolidation intentionally selects the SDK contract instead of preserving the stale duplicate schema. A regression test requires the public reexported interface schema to equal the generated component export schema.

### Provider HTTP and bytes

`phenix-provider-sdk` uses `http::{Method, StatusCode, HeaderMap}`, `url::Url`, and `bytes::Bytes` directly at the provider transport boundary. The removed local HTTP method/header/body representations were generic transport mechanics. Endpoint validation, auth policy, rate-limit parsing, protocol encoding/decoding, and provider error normalization remain Phenix-owned.

`phenix_core::Bytes` stays Phenix-owned because its value codec must materialize `PhenixValue::Bytes(Vec<u8>)`, consumers rely on owned `into_vec`, and replacing it with shared `bytes::Bytes` would add copies without removing maintained mechanics.

### OAuth, JWT, and credentials

- `oauth2` owns PKCE, authorization URL construction, authorization-code exchange, refresh exchange, and typed access/refresh/client identifiers. The existing `reqwest` transport is used through an `AsyncHttpClient` adapter; no second HTTP stack is introduced.
- Codex provider policy remains local: endpoints, scopes, extra parameters, callback ports/timeouts, browser response text, account selection, and persisted credential shape.
- The library migration preserves the pre-migration authorization query contract. It does not add provider parameters such as a `version` query key; a regression test pins the complete query-key surface and stable values.
- Initial token exchange still requires both ID and refresh tokens, matching the pre-migration behavior. Refresh responses may omit replacement ID/refresh tokens and preserve the previous values.
- `jsonwebtoken` is used only to extract `account_id` and `exp` from tokens returned by the trusted OAuth exchange. Signature, expiry, audience, not-before, and required-claim validation are deliberately disabled. This metadata parser is not an authorization check.
- Deterministic fixed-time/fake-endpoint tests cover authorization query compatibility, ID/access account precedence, audience-bearing and expired claims, malformed tokens, fallback expiry, required initial token fields, refresh replacement/preservation, access-token expiry selection, and failed refresh without partial persistence.
- `StoredCredential` secrets use `secrecy::SecretString`. The credential file keeps the existing `api_key`/`o_auth` wire shape and strict unknown-field rejection. Persistence tests inspect wrapped secret values directly. `StoredCredential` has no misleading secret-blind equality operator.
- The secured on-disk store remains the fallback policy for this pass (`0600` files and `0700` newly-created directories on Unix). A platform keyring is a separate product choice.

### MCP over ACP

Ownership is split deliberately:

- `agent-client-protocol` owns the ACP transport/envelope and MCP-over-ACP carrier.
- `rmcp` owns typed MCP request/result models, standard method constants, protocol-version knowledge, lifecycle/cancellation parameter types, and request-id types inside cancellation payloads.
- Phenix owns callable mapping, `PhenixSchema` to MCP JSON Schema projection, execution authority, worker dispatch, and the synchronous execution bridge.

The ACP carrier puts the effective MCP request identity in the outer ACP JSON-RPC request. The handler retains `Responder::id()` and passes its serialized value to the bridge. `notifications/cancelled.requestId` must match that active request exactly. Integer and string IDs are accepted; invalid JSON-RPC IDs are rejected. Late cancellation cannot cancel a newer tool call.

One tool call per MCP connection remains a deliberate synchronous-worker limit. Cancellation is monotonic through `ToolCancellation`: execution observes it, the bridge stops waiting, and a late normal result cannot win after cancellation. Disconnect and execution unbind cancel active calls.

Adding `ToolInvocation::cancellation` is an intentional public source-API change. The execution host must receive the same cancellation token as the ACP bridge so cancellation reaches active work. `ToolInvocation` equality continues to compare only callable and argument payload; execution-local cancellation state is not invocation identity.

The bridge enforces `Connected -> InitializeResponded -> Initialized`. Supported legacy revisions come from `ProtocolVersion::known_up_to(&ProtocolVersion::LATEST)` rather than a local dated version table. Unknown fire-and-forget notifications are ignored. Requests before initialization are rejected.

Persistent ACP sessions keep the initial tool surface stable because the bridge does not advertise `tools/list_changed`. Reusing a persistent session with a changed tool surface is rejected instead of exposing a stale client catalog. Model and tool configuration are captured under one mutex so a turn sees one coherent request snapshot.

A full `rmcp::serve_server`/`ServerHandler` loop is not used. It expects rmcp-owned id-bearing JSON-RPC transport while ACP owns the outer identity and Phenix must still bridge execution-local authority and the synchronous `BackendHost`. `agent-client-protocol-rmcp` currently exposes a different rmcp major, so adopting it would add a second SDK major without removing that Phenix bridge.

Deterministic tests cover connect/initialize/initialized, version negotiation, pre-initialization rejection, list/call, outer request-ID validation/correlation, stale cancellation, execution-visible cancellation, late-result suppression, overlapping-call rejection, unbind/disconnect cancellation, and persistent-session tool consistency.

### Proc macros and external consumers

External plugins need one direct dependency on package `phenix-sdk`; Cargo aliases are supported and a direct `phenix-core` dependency is not required.

All generated SDK paths use one `proc-macro-crate` resolver. Core types needed by generated code are reexported through the SDK's hidden `__phenix_plugin` namespace. Canonical and renamed SDK-only fixture packages compile in an independent fixture workspace. `trybuild` verifies that a deliberate missing-`ValueCodec` case reaches Rust name resolution/type checking.

The obsolete plugin-attribute forwarding module is removed. The canonical implementation lives in `plugin_attr.rs`.

### Other evaluated libraries

- `petgraph`: not used for current `ComponentGraph`; its deterministic ready ordering and concrete cycle-path diagnostics would still need the local algorithm. Cross-subsystem graph consolidation is #519.
- `tokio_util::sync::CancellationToken`: not used while `TaskRuntime` remains thread-based with authority/lifecycle-specific cancellation semantics.
- `EnumSet`: not used for the small closed backend feature set because it removes no maintained mechanics; open capability namespaces remain string/identifier sets.
- `serde_path_to_error` / `serde_with`: not added. The intentional manual credential deserializer exists for secret exposure plus the existing tagged strict wire contract; other state/config boundaries intentionally collapse parse errors at plugin boundaries.
- `rusqlite_migration`: not used because Phenix migrations are per-plugin durable-schema versions with explicit per-migration transactions, not one global `PRAGMA user_version` chain.
- `darling`: not used because macro parsing is dominated by semantic role classification rather than mechanical metadata decoding.
- `bon`, `derive_more`, `strum`: not added for a handful of trivial builders/display/string matches.

### Digest and revision representations

`ArtifactRevision` remains the canonical structured `sha256:` revision. Workspace `content_hash` values are intentionally opaque bare fingerprints with existing fixtures that are not valid `ArtifactRevision`s. Unifying them would change the wire format and validation contract, so they remain distinct.

## Cleanup

- Removed the legacy plugin-attribute forwarding path.
- Removed the unreferenced Lua binding error module.
- Removed generic provider HTTP/body wrappers and superseded OAuth helpers.
- Removed stale `.phenix/work/pr-514.md` state.
- The historical audit remains a design reference; this file owns completion decisions.

## LOC evidence

Method: sum line counts of first-party `.rs` files under `rust/crates`, excluding `tests/fixtures/`, generated fixtures, and non-Rust files.

The ownership-audit tree, including the final compatibility regression tests but excluding independent #534 CI-stack Rust changes, is **102,030 lines**.

- Historical audit snapshot `5c3c0f2ee187c6dd38d569d2467055b8afcd067a`: 101,292 lines. Historical drift: **+738**.
- Original #514 base `7f6233548e7ebceb87d2e3805d9db7fac741e8fc`: 101,791 lines. Ownership-audit PR delta: **+239**.

The historical comparison covers the whole audit window and is not a per-PR attribution. #534's CI optimization is a base-side dependency and is intentionally excluded from these ownership numbers.

## Follow-ups

- #519 centralizes repeated directed-graph mechanics beyond the current `ComponentGraph` boundary.
- #520 exposes backend-neutral durable range/index/log mechanics.
- #521 migrates first-party plugin whole-history/index/projection patterns.
- #522 replaces workspace traversal/search mechanics and capability-roots direct filesystem access.
- #523 isolates memory candidate retrieval and makes the Tantivy decision benchmark-backed.
- #524 closes observable/generational-handle questions and checked identity exhaustion.
- #525 enforces the post-cleanup module/private-crate boundaries.
