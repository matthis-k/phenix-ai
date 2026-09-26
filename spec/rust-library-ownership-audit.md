---
status: specification-only
---

# Rust library ownership audit

Status: reference audit, not an implementation specification.

Audit snapshot: `main` at `67c46f292ae66461f9aac63753c3532aa1043ab8`.

This audit applies the repository rule that Phenix should own semantics, not generic Rust mechanics. Current code and deterministic tests remain authoritative when this document becomes stale.

## Decision rule

Use this order when adding or reviewing a Rust abstraction:

1. Use an upstream type directly when its semantics match.
2. Add a local extension trait when the upstream type is semantically correct but lacks Phenix-specific ergonomics.
3. Use a derive, proc-macro, or helper crate for mechanical behavior.
4. Keep a transparent Phenix newtype only when nominal identity or an invariant matters.
5. Write a Phenix-owned datatype, macro, algorithm, or state machine only when existing crates cannot express the required semantics cleanly.

A wrapper that contains exactly one upstream type and only adds convenience methods should normally become an extension trait. A type that changes which values are valid, prevents distinct concepts from being mixed, or defines a Phenix protocol/domain state remains a Phenix type.

## Recommended crate set

| Concern | Prefer | Phenix ownership |
| --- | --- | --- |
| Validated IDs and strings | `nutype` | Names and validation rules |
| Error implementations | `thiserror` | Error taxonomy and messages |
| Builders | `bon` | Required semantic fields |
| Simple conversions/formatting | `derive_more` | Semantic conversions only |
| Enum strings/iteration | `strum` | Variant meaning |
| Closed enum sets | `enumset` | Feature meaning |
| Secrets | `secrecy` | Credential policy and persistence boundary |
| HTTP primitives | `http`, `bytes` | Provider policy and interpretation |
| Graph algorithms | `petgraph` | Node/edge meaning, provider and authority policy |
| Cancellation | `tokio-util::sync::CancellationToken` | Propagation policy |
| Synchronous locking | `parking_lot` | State invariants |
| Non-empty collections | `vec1` or equivalent | Why emptiness is invalid |
| Relative paths | `relative-path` where wire/config paths are relative | Path meaning |
| Proc-macro attributes | `darling` + `syn` | Phenix DSL semantics and generated ABI |
| Proc-macro crate lookup | `proc-macro-crate` | None |
| Proc-macro UI tests | `trybuild` | Expected diagnostics |
| Serde boundary diagnostics | `serde_path_to_error`, `serde_with` | Domain parsing rules |
| SQLite migration bookkeeping | `rusqlite_migration` where compatible | Schema and migration semantics |
| MCP | official `rmcp` | Callable mapping, authority, lifecycle |
| ACP | official ACP SDK | Phenix mapping and application semantics |
| OAuth | `oauth2` and standard HTTP/JWT helpers | Provider config, credential UX and policy |

## File audit

Verdicts:

- **Yes, appropriate Phenix type**: the type carries real Phenix semantics and should remain owned by Phenix.
- **Keep type, replace mechanics**: nominal identity/invariant matters, but derives/library types should replace hand-written mechanics.
- **Replace + extension trait**: use the upstream representation directly and add Phenix-specific ergonomic behavior through a trait if useful.
- **Replace**: the local abstraction is generic infrastructure with no useful Phenix ownership.

| File | Existing definitions or mechanics | Verdict | Recommended change |
| --- | --- | --- | --- |
| `rust/crates/phenix-core/src/identity.rs` | Many string ID newtypes, `InterfaceId`, local ID macro, manual serde/format/parse/codec | Keep type, replace mechanics | Retain nominal IDs. Replace ID macro and ordinary impls with `nutype`; keep only special validators and Phenix codec integration. |
| `rust/crates/phenix-domain/src/lib.rs` | Second domain-ID macro; session/execution/model/orchestration DTOs; secret strings | Mixed | DTOs are **Yes, appropriate Phenix type**. IDs use the same validated-newtype infrastructure. Secrets use `secrecy`. Remove duplicate identity ownership where possible. |
| `rust/crates/phenix-core/src/artifact.rs` | `ArtifactRevision(String)`, SHA-256 parsing/formatting/error | Keep type, replace mechanics | Keep content-revision semantics. Delegate validated representation, digest encoding, derives, and error mechanics. |
| `rust/crates/phenix-core/src/authority.rs` | `Authority` over open capability IDs | **Yes, appropriate Phenix type** | Keep. Open capability namespaces should not become a closed bitset. |
| `rust/crates/phenix-core/src/contract.rs` | `Type`, `PhenixValue`, keys/refs, projection markers, custom `Bytes`, ref macro | Mixed | `Type`, `PhenixValue`, projections and capability refs are **Yes, appropriate Phenix type**. Use `bytes::Bytes`; validated newtypes for simple keys/IDs; reduce ref macros to derives/generic typed refs. |
| `rust/crates/phenix-core/src/contract_wire.rs` | Contract-aware validated deserialization | **Yes, appropriate Phenix type** | Keep because re-parsing against the contract is semantic. |
| `rust/crates/phenix-core/src/std_value.rs` | Primitive/map/set Phenix codecs and integer macros | **Yes, appropriate Phenix integration** | Keep unless a simpler generic codec implementation replaces the macros. Foreign primitive types cannot derive a Phenix-owned trait. |
| `rust/crates/phenix-core/src/structural_value.rs` | `ValueCodec` bridges for `Type` and `PhenixValue` | **Yes, appropriate Phenix integration** | Keep. |
| `rust/crates/phenix-core/src/infallible_value.rs` | `ValueCodec for Infallible` | **Yes, appropriate Phenix integration** | Keep. |
| `rust/crates/phenix-core/src/configuration.rs` | Config namespaces, contribution DTOs, merge policy, late nonzero/nonempty validation | Mixed | Config domain and merge policy are **Yes, appropriate Phenix type**. Use validated newtypes and `NonZeroU64` at the boundary; `thiserror` for errors. |
| `rust/crates/phenix-core/src/manifest.rs` | Plugin/component manifests and semantic enums | **Yes, appropriate Phenix type** | Keep. Represent positive versions with `NonZeroU32` if zero is invalid. |
| `rust/crates/phenix-core/src/composition_metadata.rs` | Metadata DTOs, many late checks, closed enum sets | Mixed | Metadata is **Yes, appropriate Phenix type**. Use invariant-bearing fields, `EnumSet`, and `thiserror`. |
| `rust/crates/phenix-core/src/component.rs` | Resolved graph/provider plans plus hand-written DAG traversal | Mixed | Resolved graph and provider/authority policy are **Yes, appropriate Phenix type**. Use `petgraph` for topology and optionally a `ComponentGraphExt` trait for Phenix-specific queries. |
| `rust/crates/phenix-core/src/provider_resolution.rs` | Provider selection/fallback policy | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/resolver.rs` | `ResolvedHarness`, generation identity, many `resolve_with_*` entry points | Mixed | Resolution is **Yes, appropriate Phenix type**. Use `bon` for assembly ergonomics and `petgraph` for generic graph work. |
| `rust/crates/phenix-core/src/activation.rs` | Activation state and errors | **Yes, appropriate Phenix type** | Keep; derive errors. |
| `rust/crates/phenix-core/src/reconciliation.rs` | Graph/component/resource diffs and actions | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/reconciliation_inspection.rs` | Inspection DTOs | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/live_reconciliation.rs` | Live reconciliation state/errors | **Yes, appropriate Phenix type** | Keep semantics; use `thiserror`. |
| `rust/crates/phenix-core/src/metadata_input.rs` | Metadata lowering/input model | **Yes, appropriate Phenix type** | Keep; parse into stronger field types earlier. |
| `rust/crates/phenix-core/src/metadata_inspection.rs` | Resolved metadata inspection | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/metadata_reconciliation.rs` | Metadata changes/diffs | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/frontend_metadata.rs` | Frontend metadata resolution/errors | **Yes, appropriate Phenix type** | Keep; derive errors. |
| `rust/crates/phenix-core/src/events.rs` | Event bus, causal ordering, receipts, hand dependency graph, std poisoned locks, positive capacity assertion | Mixed | Event semantics are **Yes, appropriate Phenix type**. Use `petgraph`, `NonZeroUsize`, `parking_lot`, and `thiserror`. Do not replace EventBus with a generic channel. |
| `rust/crates/phenix-core/src/tasks.rs` | Custom cancellation token plus Phenix task scopes/runtime | Mixed | Task ownership/scope semantics are **Yes, appropriate Phenix type**. Replace custom cancellation primitive with `tokio_util::sync::CancellationToken`; add an ext trait only for Phenix-specific helpers. |
| `rust/crates/phenix-core/src/capability.rs` | Capability registry/handler/shared lock/error | **Yes, appropriate Phenix type** | Keep registry semantics. Use `parking_lot` and `thiserror`. |
| `rust/crates/phenix-core/src/invocation.rs` | Invocation outcomes/failures/classes | **Yes, appropriate Phenix type** | Keep taxonomy; derive mechanics. |
| `rust/crates/phenix-core/src/plugin_context.rs` | Plugin context, kernel access, SDK contract/object types | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/typed_component.rs` | Typed component interfaces/schema compatibility | **Yes, appropriate Phenix type** | Keep; derive error mechanics. |
| `rust/crates/phenix-core/src/sdk.rs` | SDK contributions and value/resource representation | **Yes, appropriate Phenix type** | Keep structural SDK ABI. |
| `rust/crates/phenix-core/src/agent.rs` | Agent/tool/skill/context contracts | **Yes, appropriate Phenix type** | Keep; use canonical byte type. |
| `rust/crates/phenix-core/src/management.rs` | Plugin management requests/results/policy/errors | **Yes, appropriate Phenix type** | Keep; use `thiserror` and builders only for assembly. |
| `rust/crates/phenix-core/src/inspection.rs` | Harness/listener inspection records | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-core/src/registry.rs` | Kernel config/bindings/service chains/errors | **Yes, appropriate Phenix type** | Keep. Replace lock/error mechanics. |
| `rust/crates/phenix-core/src/runtime.rs` and `runtime/*.rs` | Kernel/plugin/runtime/provenance state and sync machinery | **Yes, appropriate Phenix type** | Keep runtime semantics. Use `parking_lot` for synchronous locks where poisoning has no semantic meaning. |
| `rust/crates/phenix-core/src/persistence.rs` | Feature enum/set, schema versions, transactions, SQLite persistence, migration planning | Mixed | Persistence contracts are **Yes, appropriate Phenix type**. Use `EnumSet`, nonzero versions, `thiserror`, and migration tooling where compatible. |
| `rust/crates/phenix-core/src/persistence_bootstrap.rs` | Store/provider identities, feature sets, hand dependency cycles | Mixed | Bootstrap semantics are **Yes, appropriate Phenix type**. IDs/formats become validated types; features use `EnumSet`; dependency traversal uses `petgraph`. |
| `rust/crates/phenix-core/src/persistence_provider.rs` | Provider preparation/candidate interfaces | **Yes, appropriate Phenix type** | Keep; derive error mechanics. |
| `rust/crates/phenix-core/src/persistence_value.rs` | Persistence structural codecs | **Yes, appropriate Phenix integration** | Keep domain codecs; prefer derive over new local macros. |
| `rust/crates/phenix-core/src/prepared_mutation.rs` | Opaque handle, random hex encoding, serde/codec, custom RAII guard, mutex maps | Mixed | Mutation capability/scope is **Yes, appropriate Phenix type**. Delegate validated handle mechanics/encoding/locking; use `scopeguard` only if clearer than the local semantic guard. |
| `rust/crates/phenix-core/src/plugin_build.rs` | Validated-string macro, executable/arg/env/source/path wrappers, possibly-empty steps | Mixed | Build plan is **Yes, appropriate Phenix type**. Use `nutype`, relative-path tooling and a non-empty collection; `bon` for authoring. |
| `rust/crates/phenix-core/src/plugin_build_execution.rs` | Executor/store/output/evidence/failure | **Yes, appropriate Phenix type** | Keep semantics; standardize paths/bytes/nonempty fields. |
| `rust/crates/phenix-core/src/observable/*.rs` | Observable IDs, numeric wrappers, path model, subscriptions, store and transaction state machine | Mixed | Observable semantics are **Yes, appropriate Phenix type**. Delegate ID/newtype/error/lock mechanics. Existing `SmallVec` use is appropriate. |
| `rust/crates/phenix-core/src/lib.rs` | Reexports plus one-variant policy enums | Mixed | Reexports stay. Reconsider one-variant `SourceIdentityRule`, `SourceRevisionRule`, and `StableMaterializationRule`; constants/metadata may be clearer until real variants exist. |
| `rust/crates/phenix-domain/src/attempts.rs` | Attempt records | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-domain/src/failures.rs` | Failure decisions/records | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-domain/src/debug.rs` | Debug bundle/serializer/errors | **Yes, appropriate Phenix type** | Keep semantic bundle; use `thiserror`. |
| `rust/crates/phenix-domain/src/workspace/*.rs` | Execution authority, workspace/read-set/conflict model, objective/plan/decision/language types | **Yes, appropriate Phenix type** | Keep domain model. IDs use shared validated-newtype machinery. Replace raw hash strings with typed digest/revision where applicable. |
| `rust/crates/phenix-backend/src/lib.rs` | Backend/session/tool contracts and capabilities | Mixed | Backend contracts are **Yes, appropriate Phenix type**. Closed presentation set can use `EnumSet`; error mechanics use `thiserror`. |
| `rust/crates/phenix-backend-acp/src/lib.rs` | ACP backend/session/cancellation forwarding | Mixed | Keep adapter semantics; use official ACP SDK types and standard cancellation primitives. |
| `rust/crates/phenix-backend-acp/src/mcp_bridge.rs` | Hand-written MCP protocol/state machine | **Replace** | Use official `rmcp`. Keep only callable mapping, authority/policy and ACP attachment integration. |
| `rust/crates/phenix-backend-native/src/lib.rs` | Native backend/session state | **Yes, appropriate Phenix adapter type** | Keep provider-independent state. |
| `rust/crates/phenix-backend-native/src/providers.rs` | Provider/genai mapping | **Yes, appropriate Phenix adapter type** | Keep thin mapping. |
| `rust/crates/phenix-backend-native/src/schema_adapter.rs` | Manual `PhenixSchema` to JSON Schema translation | **Yes, appropriate Phenix translation** | Keep semantic translation, but prefer typed JSON Schema representation. A reusable `PhenixSchemaJsonExt` is appropriate if multiple adapters need it. |
| `rust/crates/phenix-backend-native/src/oauth.rs` | Hand PKCE/state/exchange/refresh/JWT/callback HTTP/random generation | **Replace** | `oauth2` and standard HTTP/JWT helpers own protocol mechanics. Keep `CodexOAuth` as thin provider-specific policy/UX adapter. |
| `rust/crates/phenix-backend-native/src/credentials.rs` | JSON credential store, XDG discovery, permissions/atomic replacement, raw secrets | **Replace heavily** | Prefer standard credential/keyring facilities. If file fallback remains, use path-discovery, atomic-write and secrecy crates. `StoredCredential` variant meaning remains Phenix/provider semantics. |
| `rust/crates/phenix-provider-sdk/src/types.rs` | Custom HTTP method/header/request/response, secret/token/env/endpoint wrappers | Mixed | Use `http::{Method, HeaderName, HeaderMap, Request, Response, StatusCode}` and `bytes::Bytes` directly. Add extension traits for Phenix rate-limit/provider interpretations. Keep only validated semantic endpoint/config types. |
| `rust/crates/phenix-provider-sdk/src/runtime.rs` | Converts custom HTTP model to reqwest | **Replace** | Mostly disappears after canonical `http` types are used. |
| `rust/crates/phenix-provider-sdk/src/protocol.rs` | Hand OpenAI/Anthropic JSON requests and responses | **Replace heavily** | Generate private wire types from pinned upstream schemas. Keep Phenix normalization and provider-policy mapping. |
| `rust/crates/phenix-provider-sdk/src/auth.rs` | Provider auth mechanics | Mixed | Provider policy stays. OAuth/secrets/HTTP mechanics use external crates. |
| `rust/crates/phenix-provider-sdk/src/store.rs` | Provider storage/errors | Mixed | Keep semantic registry/store boundary; derive errors and externalize secrets. |
| `rust/crates/phenix-client/src/lib.rs` | Client protocol DTOs plus custom frontend provider ID | Mixed | Protocol DTOs are **Yes, appropriate Phenix type** while this wire exists. ID mechanics use `nutype`; bytes use canonical `Bytes`. |
| `rust/crates/phenix-client-acp/src/lib.rs` | ACP client and shared state | Mixed | Keep Phenix client semantics; use official ACP SDK wire types and standard sync primitives. |
| `rust/crates/phenix-adapter-acp/src/*.rs` | ACP dispatch/callback/update/elicitation translations and extension IDs | **Yes, appropriate Phenix translation** | Keep mappings only. Remove duplicate protocol models; generate repetitive extension dispatch from canonical descriptors rather than a local macro list. |
| `rust/crates/phenix-acp-stdio/src/lib.rs` | ACP stdio process/application glue | Mixed | Keep assembly; let ACP SDK own framing/transport mechanics. |
| `rust/crates/phenix-application-interface/src/descriptor.rs` | Canonical app capability/operation/event/callback descriptors | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-application-interface/src/catalog.rs` | Local catalog macros | Replace mechanics | Prefer one declarative/static descriptor source plus derives over a second macro DSL. |
| `rust/crates/phenix-application-interface/src/generate.rs` | Rust source generation | Mixed | Keep descriptor-to-binding semantics; use codegen/template helpers for source emission. |
| `rust/crates/phenix-application-interface/src/client.rs` | Application client abstraction | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-application-interface/src/types/*.rs` | Canonical application DTOs plus local `record!`/`variants!` macros | Mixed | DTOs are **Yes, appropriate Phenix type**. Prefer normal structs/enums with derives over local declaration macros. |
| `rust/crates/phenix-binding-generator/src/lib.rs` | Manual Lua concatenation, escaping, indentation, casing | Replace mechanics | Keep descriptor-to-Lua semantics. Use source-generation/template and case-conversion crates; derive errors. |
| `rust/crates/phenix-binding-lua/src/*.rs` | mlua bridge and conversion/error machinery | **Yes, appropriate Phenix binding** | Keep semantic translation. Prefer `mlua`'s existing value/serde facilities where shapes match; `thiserror` for errors. |
| `rust/crates/phenix-sdk/src/providers.rs` | `ProviderSdkExt` and provider facade | **Yes, appropriate extension trait** | Keep. This already follows the intended pattern. |
| `rust/crates/phenix-sdk/src/public_projection.rs` | Public projection descriptors | **Yes, appropriate Phenix type** | Keep. |
| `rust/crates/phenix-sdk/src/api.rs` | SDK command types plus repetitive interface marker types | Mixed | Commands are **Yes, appropriate Phenix type**. Generate repetitive interface-marker mechanics centrally. |
| `rust/crates/phenix-sdk/src/authoring/*.rs` | Plugin/component/import/resource/lifecycle authoring descriptors and aliases | **Yes, appropriate Phenix ABI type** | Keep semantic descriptors; derive/generate construction and dispatch boilerplate. |
| `rust/crates/phenix-sdk/src/contracts/options.rs` | Option IDs, local codec macro, closed scope set and option contracts | Mixed | Contract DTOs are **Yes, appropriate Phenix type**. IDs use `nutype`; delete local codec macro; use `EnumSet` for scopes. |
| `rust/crates/phenix-sdk/src/contracts/*.rs` | Context/execution/frontend/jobs/memory/models/planning/sessions/workspace contracts | **Yes, appropriate Phenix type** | Keep. Standardize common bytes/IDs and closed sets. |
| `rust/crates/phenix-sdk-macros/src/lib.rs` | Phenix derive generation over `syn` | **Yes, appropriate Phenix macro**, mechanics replaceable | Keep derives; use `darling`, `proc-macro-crate` and `trybuild` for generic macro infrastructure. |
| `rust/crates/phenix-sdk-macros/src/component_attr.rs` | Manual component attribute parsing | Replace mechanics | Use `darling` for attribute data; retain semantic checks/codegen. |
| `rust/crates/phenix-sdk-macros/src/component_runtime_attr.rs` | Runtime structural wrapper parsing | Mixed | Externalize parsing mechanics; keep ABI semantics. |
| `rust/crates/phenix-sdk-macros/src/expose_attr.rs` | Manual syntax/attribute parsing | Mixed | Externalize generic attribute parsing; retain syntax transformation. |
| `rust/crates/phenix-sdk-macros/src/interface_attr.rs` | Interface marker generation | Mixed | Keep semantic generation; externalize parsing/path lookup. |
| `rust/crates/phenix-sdk-macros/src/plugin_attr_core.rs` | Large manual plugin attribute parser | Replace mechanics | Strong `darling` target. Keep only plugin lifecycle/import/export semantics and ABI generation. |
| `rust/crates/phenix-sdk-macros/src/plugin_attr.rs` | Plugin macro dispatcher | **Yes, appropriate Phenix macro entrypoint** | Keep tiny. |
| `rust/crates/phenix-sdk-macros/src/plugin_attr_legacy.rs` | Legacy plugin macro path | **Replace/delete** | Remove when no compatibility gate requires it. Do not maintain parallel macro generations. |
| `rust/crates/phenix-sdk-macros/src/resource_attr.rs` | Resource attribute generation | Mixed | Keep resource semantics; externalize parsing. |
| `rust/crates/phenix-plugin-options/src/lib.rs` | Duplicates option IDs/scopes/values/definitions already exposed by SDK contract | **Replace/delete duplicate types** | Import canonical option contract types rather than owning a second copy. |
| `rust/crates/phenix-plugin-command-toolbelt/src/implementation.rs` | `CliName` wrapper and CLI implementation state | Mixed | `CliName` uses validated-newtype machinery. Toolbelt behavior remains plugin semantics. |
| `rust/crates/phenix-plugin-command-toolbelt/src/component.rs` | Local interface macro | Replace mechanics | Generate or derive `ComponentInterface` markers centrally. |
| `rust/crates/phenix-plugin-execution/src/configuration.rs` | Local string codec macro plus execution config enums | Mixed | Enums are **Yes, appropriate Phenix type**. Use `strum` plus Phenix derive instead of local codec macro. |
| `rust/crates/phenix-plugin-execution/src/*.rs` | Agent loop, scheduling, component and execution implementation | **Yes, appropriate Phenix type/semantics** | Keep scheduling and execution policy; delegate graph/cancellation/bytes mechanics. |
| `rust/crates/phenix-plugin-context/src/*.rs` | Context/prompt/component implementation | **Yes, appropriate Phenix type/semantics** | Keep; use canonical bytes and shared IDs. |
| `rust/crates/phenix-plugin-memory/src/*.rs` | Memory component/retrieval/persistence/errors | **Yes, appropriate Phenix type/semantics** | Keep memory policy/model. Delegate errors and generic ranking/vector/storage mechanics where mature crates fit. |
| `rust/crates/phenix-plugin-models/src/*.rs` | Model component/routing/catalog | **Yes, appropriate Phenix type/semantics** | Keep selection policy; protocol/client mechanics stay upstream. |
| `rust/crates/phenix-plugin-hooks/src/*.rs` | Legacy hook dispatcher | **Transitional** | Keep only for compatibility; lower synchronous hooks to kernel Service Layers and observations to Events, then remove. |
| `rust/crates/phenix-plugin-jobs/src/*.rs` | Job lifecycle contracts/implementation | **Yes, appropriate Phenix type/semantics** | Keep; use standard cancellation/async primitives. |
| `rust/crates/phenix-plugin-artifacts/src/*.rs` | Artifact contracts/implementation | **Yes, appropriate Phenix type/semantics** | Keep; reuse canonical digest/revision type and generic storage/hash crates. |
| `rust/crates/phenix-plugin-planning/src/*.rs` | Planning contracts/implementation | **Yes, appropriate Phenix type/semantics** | Keep. |
| `rust/crates/phenix-plugin-sessions/src/*.rs` | Session state/storage semantics | **Yes, appropriate Phenix type/semantics** | Keep; converge on one canonical SessionId. |
| `rust/crates/phenix-plugin-session-tree/src/*.rs` | Hierarchical session model | **Yes, appropriate Phenix type/semantics** | Keep hierarchy semantics; use graph/tree algorithms only where they replace generic traversal. |
| `rust/crates/phenix-plugin-workspace/src/*.rs` | Workspace contracts/implementation | **Yes, appropriate Phenix type/semantics** | Keep native `Path`/`PathBuf`; delegate generic temp/atomic/walk mechanics. |
| `rust/crates/phenix-plugin-repository-workers/src/*.rs` | Worker contracts/scheduling | **Yes, appropriate Phenix type/semantics** | Keep; use runtime queue/cancellation/timer primitives. |
| `rust/crates/phenix-plugin-frontend/src/*.rs` | Frontend service contracts/implementation | **Yes, appropriate Phenix type/semantics** | Keep. |
| `rust/crates/phenix-plugin-debug/src/*.rs` | Debug contracts/export | **Yes, appropriate Phenix type/semantics** | Keep semantic bundle; use serde machinery directly. |
| `rust/crates/phenix-plugin-providers/src/lib.rs` | Provider plugin composition | **Yes, appropriate Phenix integration** | Keep thin. |
| `rust/crates/phenix-plugin-api/src/lib.rs` | Public reexports | Appropriate as-is | Prefer reexports to duplicate wrappers. |
| `rust/crates/phenix-plugin-catalog/src/lib.rs` | Plugin catalog/reexports | Appropriate as-is | Keep package-set/catalog surface. |
| `rust/crates/phenix-plugin-basic-*/src/lib.rs` | Minimal default plugins | **Yes, appropriate Phenix integration** | Keep thin. Agent Skills parsing/validation should use canonical tooling rather than a local standard parser. |
| `rust/crates/phenix-harness/src/runtime_config.rs` | Product config wire/lowering DTOs | Mixed | Product config schema is **Yes, appropriate Phenix type**. Use `serde_path_to_error` and invariant-bearing fields at deserialization. |
| `rust/crates/phenix-harness/src/basic_suite.rs` | Default suite composition | **Yes, appropriate Phenix product composition** | Keep; declarative builder where assembly is wide. |
| `rust/crates/phenix-harness/src/persistence.rs` | Persistence product composition | **Yes, appropriate Phenix integration** | Keep thin. |
| `rust/crates/phenix-harness/src/lib.rs` | Harness assembly | **Yes, appropriate Phenix type** | `bon` is suitable where construction has many independent arguments. |
| `rust/crates/phenix-harness/src/main.rs` | CLI entrypoint | Appropriate as-is | `clap` derive is already the right pattern. |
| `rust/crates/phenix-conductor/src/lib.rs` | Conductor/build error | **Yes, appropriate Phenix type** | Keep conductor; derive build error. |
| `rust/crates/phenix-conductor/src/main.rs` | Process entrypoint | Appropriate as-is | Keep thin. |
| `rust/crates/phenix-plugin-language/src/*.rs` | Language component/implementation | Mixed | Language semantics are appropriate Phenix/plugin types. Use parser/index/query libraries for generic language mechanics. Resolve the crate's workspace-packaging status separately. |

## Highest-value replacements

| Priority | Change | Expected effect |
| ---: | --- | --- |
| 1 | Replace hand-written MCP bridge with `rmcp` | Removes protocol state machine and future version maintenance. |
| 2 | Replace OAuth mechanics with `oauth2` and standard helpers | Removes PKCE/token/refresh boilerplate and RFC maintenance. |
| 3 | Standardize all validated IDs/newtypes on `nutype` | Deletes multiple local macros and repeated serde/parse/display implementations. |
| 4 | Adopt `thiserror` across subsystem errors | Removes repeated `Display`/`Error` plumbing without changing error semantics. |
| 5 | Use canonical `http` + `bytes` types in provider SDK | Deletes duplicate HTTP type system and conversion layer. |
| 6 | Replace generic DAG/cycle traversal with `petgraph` | Centralizes graph mechanics while retaining Phenix edge semantics. |
| 7 | Replace custom cancellation primitive with Tokio cancellation | Removes local synchronization/cancellation code. |
| 8 | Migrate proc-macro parsing to `darling` and crate lookup to `proc-macro-crate` | Shrinks custom proc-macro infrastructure while preserving the Phenix DSL. |
| 9 | Remove duplicated options domain types | Eliminates parallel semantic ownership. |
| 10 | Replace code-generation string mechanics with generation/template helpers | Reduces escaping/formatting/casing implementation surface. |

## LOC estimate

This is a planning estimate, not a measured diff.

| Area | Removed | Replacement glue | Net |
| --- | ---: | ---: | ---: |
| MCP, OAuth and provider protocol mechanics | 700-1,100 | 250-500 | -450 to -700 |
| IDs, validated strings and invariant wrappers | 700-1,100 | 200-350 | -500 to -750 |
| Manual error implementations | 400-700 | 100-200 | -300 to -500 |
| Duplicate option/domain definitions | 250-450 | 20-80 | -230 to -370 |
| Generic graph algorithms | 250-450 | 100-180 | -150 to -270 |
| Cancellation/task mechanics | 150-300 | 40-100 | -110 to -200 |
| HTTP/header/bytes wrappers | 200-350 | 70-140 | -130 to -210 |
| Proc-macro parsing infrastructure | 400-800 | 200-400 | -200 to -400 |
| Binding/code-generation plumbing | 250-500 | 150-300 | -100 to -200 |
| Builders, lock boilerplate and misc invariants | 200-400 | 80-180 | -120 to -220 |
| **Total** | **3,500-6,150** | **1,200-2,400** | **about -2,300 to -3,750** |

Use about **3,000 fewer hand-maintained Rust LOC** as the planning midpoint. Generated provider wire models can increase raw repository LOC while still reducing human-maintained code.

## Implementation guardrails

Do not flatten semantic types merely to reduce LOC. Preserve Phenix-owned types for authority, capability semantics, plugin/component contracts, execution/session state, observable transaction semantics, provider selection, application descriptors and structural compatibility.

Do not add a wrapper when an upstream type plus an extension trait is enough. Examples include `HeaderMap` plus a Phenix rate-limit interpretation trait, a `petgraph` graph plus Phenix dependency/provider query traits, and `CancellationToken` plus Phenix propagation helpers.

When migrating a type to a crate, preserve wire format, deterministic ordering, validation behavior, public error contracts and redaction. Library adoption is not justification for compatibility fallbacks or parallel APIs.
