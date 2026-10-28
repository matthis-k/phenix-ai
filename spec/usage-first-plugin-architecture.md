# Usage-first plugin APIs and derived architecture

Status: **dated design proposal with partially implemented slices**. Based on the 24 October 2026 tree `00598cedf0a345934098d950881a702110001f83`; verify current code and merged PRs before reusing examples. The graph-v2 migration target is [Graph Architecture v2.4](graph-architecture-v2.4.md), not this pre-v2.4 design. This document **starts with user and author usage**, then derives architecture. The first priority is to preserve existing M1 guarantees and successful P1 auth, graph, and interruption behavior, not to add plugin count.

Related: [plugin inventory](plugin-responsibility-audit.md), [state ownership](plugin-state-ownership.md), [M1 authoring](m1-contract-authoring-contract.md), [P2 preparation contract](../crates/phenix-advanced-contracts/src/model_preparation.rs).

## 1. Contract with the human first: what success looks like

Each story is an acceptance requirement, not evidence that the behavior already works.

| Persona | Action | Expected observable result | Must not require |
| --- | --- | --- | --- |
| User | Run :Phenix auth, choose OpenAI API, select environment variable or paste key | One named authorized credential profile becomes usable; failure is typed and stays on that account | Set transport-specific env vars, choose a vault location, hand-edit a composition |
| User | Run :Phenix auth, choose ChatGPT OAuth and complete browser login | Same provider-management interface reports the verified named profile | A second provider-specific auth UI or manually persisting tokens |
| User | Run :Phenix model, choose a model, submit a prompt | Effective account+protocol+model is pinned for subsequent turns | A provider-specific Model client or changing the current in-flight turn |
| User | Restart and continue the session | Owner state is rehydrated from validated durable records; revoked accounts remain invalid | Treating persistence as an authority grant |
| Tool author | Add one uppercase tool with a typed input and handler | It appears in catalog and executes through ordinary ToolExecute | Editing a central string match or JSON parser |
| Model extension author | Add an opt-in compaction preparer | A verified, budgeted replacement request reaches the selected Model | Reimplementing the agent loop or journal |
| Frontend author | Add a new wire/presentation adapter | Same authorized turns/events, without changing the Basic agent | Duplicating model, provider, or session semantics |
| Evidence consumer | Write a project-scoped fact supported by source references | Current source and project membership are verified; withdrawn evidence cannot reappear | Selecting a SQLite connection or forging access from a project ID |

### 1.1 User journeys (client usage)

1. **New API-key account:** :Phenix auth -> registered provider -> API-key method -> environment name **or** masked direct input -> verification -> named profile status. A missing variable, revoked secret, and rejected remote key are distinct errors. Secure store is chosen by the installed composition, not the model or client.
2. **New OAuth account:** :Phenix auth -> provider+OAuth method -> browser challenge -> callback -> verified account/issuer/nonce/PKCE -> credential persistence -> completed attempt. Browser opening is not success. Cancel/expiry are terminal, and retry does not borrow another account.
3. **Model selection:** :Phenix model -> authorized connection -> profile -> admissible model -> later turn uses the checked binding. A stale catalog, revoked profile, or unavailable codec fails closed. No fallback to a sibling connection or account.
4. **Tool invocation:** Model emits registered tool ID and JSON arguments -> catalog/authority/schema checks -> selected implementation -> durable reply/reconciliation. A fabricated tool name, forged session, or malformed JSON cannot reach the handler.
5. **Context pressure:** Basic publishes original request -> selected preparer proposes a source-bound change -> source/CAS validation -> exact selected token measurement -> Model call -> Basic commits reply before tools. Failed preparation leaves the original request intact or denies the turn, never runs an unaudited truncated prompt.
6. **Concurrent sessions:** two independent sessions and two named provider accounts cannot mix profile state, tool state, session snapshots, checkpoint provenance or selection revisions.

## 2. Plugin-author experience (target sketches)

These examples are **illustrative proposed ergonomic syntax**, not currently compiled SDK methods. The architecture below identifies how existing M1 operations can realize them without inventing a new kernel registry opcode.

### 2.1 Provider: declaration should be the entire ordinary author experience

~~~rust
// Proposal: static SDK-authored declaration; not an imperative pre-dlopen hook.
providers.register(
    Provider::new("openai", "OpenAI")
        .connection("api", Connection {
            endpoint: "https://api.openai.com/v1",
            protocol: protocol("openai-responses"),
            auth: auth("api-key"),
            models: Some(catalog("openai.models", "https://api.openai.com/v1/models")),
        })
        .connection("chatgpt", Connection {
            endpoint: "https://api.openai.com/v1",
            protocol: protocol("openai-responses"),
            auth: auth("chatgpt-oauth"),
            models: Some(catalog("chatgpt.models", "https://api.openai.com/v1/models")),
        }),
);
~~~

The minimal required fields for one connection remain **endpoint, protocol, authentication**; IDs and display labels are surrounding registry identity. Model discovery, alternate protocol endpoints, optional account hints, presentation (Bearer vs header), issuer validation, model-token budgets and feature flags are **typed optional extensions**, not mandatory values.

**Already available approximation:** phenix-provider-builtins contains ProviderDeclaration { endpoint, protocol, auth }, ConnectionExtensions and explicit model discovery; do not replace that working shape merely for a prettier DSL. The SDK can generate a provider-registration plugin whose only executable operation returns immutable declarations when startup composition invokes it. No provider author implements ProviderAuth, ProviderCatalog or OAuth state handling.

An unusual provider MAY register a different authentication or wire adapter as a **separately selected, scoped implementation**, with an explicit typed contract and owner. The simple path should not require writing such an adapter.

### 2.2 Tool: implementation code is the only non-declarative part

~~~rust
// Proposal: SDK expansion produces one ToolRegistration description
// and one existing ToolExecute operation for the selected implementation.
#[phenix_tool(
    id = "text.uppercase",
    description = "Uppercase a string",
    input = UppercaseArgs,
    output = UppercaseResult,
    idempotent = true
)]
fn uppercase(state: &mut State, args: UppercaseArgs) -> Result<UppercaseResult, ToolError> {
    state.calls += 1;
    Ok(UppercaseResult { text: args.text.to_uppercase() })
}
~~~

The SDK checks that the JSON arguments schema offered to the Model matches the typed decode boundary; Portable and JSON Schema are **not automatically equivalent**. The macro must either generate a verified JSON Schema or require a schema with a type-level consistency test. The tool manager publishes the catalog, performs conflict checks and dispatches using selected **preauthorized import slots**, not arbitrary IDs supplied by the model. The handler's State stays inside its own plugin; model/tool quotas are owned by Basic Agent and selected policy, not by global tool registry state.

Present fixture code: phenix-plugin-basic-tools contains three hardcoded names and a match dispatch; the migration acceptance test is to add a fourth standalone plugin **without modifying** Basic Tools or Basic Agent.

### 2.3 Agent: no special API for default versus Advanced

~~~rust
// Conceptual user-facing flow, not an SDK function:
agent.submit(prompt);
  // graph: prepare -> optional preparation -> validate -> model
  //     -> record assistant reply -> tool(s) -> prepare next request
  //     -> answer and durable terminal result
~~~

Basic Agent owns AgentRun: session, run identity, pending model import, turn/tool budget, history/call recovery state. It exposes typed prepare/advance/cancel/snapshot operations and read-only watched pending requests. The kernel owns graph scheduling, authority and cancellation. No caller selects a parallel "default loop" versus "staged loop" algorithm. No Model plugin writes a Basic assistant tool-call receipt.

**Baseline at the original audit commit:** BasicInput::submit and BasicTurnPrepare/BasicTurnAdvance were separate implementations; staged-graph was feature-selected. Converge on one canonical two-wave flow only after product parity and restart/cancel/duplicate-effect verification.

### 2.4 Request preparation and model execution

~~~rust
// Proposed author ergonomics are a thin wrapper around existing
// ModelPreparation::prepare(ModelPreparationInput).
preparation.register(
    "context.compact",
    prepare = |input: ModelPreparationInput| -> ModelPreparationProposal {
        // Return original or source-bound candidate+checkpoint proposal.
        // No Model call, journal mutation or authority grant here.
        propose_verified_compaction(input)
    },
);

// Selected Model remains the ordinary existing interface:
Model::complete(ModelRequest) -> Result<ModelReply, BasicError>;
~~~

A preparation proposal is *untrusted*. The existing ModelPreparationInput carries the original canonical request SHA; proposal validation must preserve session/run, tools, skills, capabilities and limits. Changed history/context requires an independently source-verified checkpoint; an unchanged candidate is allowed without a checkpoint. Apply selected-model-specific exact token measurement **after** the final candidate is validated and before Model::complete. Measurement and Model must refer to the same pinned model/configuration; the request bytes cannot change between measurement and dispatch.

Summarizer, checkpoint store, provenance verification and token meter remain separate capabilities. The Advance/Basic Agent stage records an assistant reply in the durable journal **before** executing tool effects. Recovery must deduplicate the same assistant message and pending tool call; a storage or journal error cannot be treated as a successful tool receipt. Do not assume a cross-plugin atomic transaction without proving it in the selected persistence composition.

### 2.5 Plugin-owned state and optional persistence

~~~rust
// Current SDK direction (illustrative syntax around supported attributes).
pub struct State {
    #[expose]
    #[watch]
    selected: Selection,
    credentials: PrivateAuthState, // never expose tokens
}

fn select(state: &mut State, host: &Host, proposal: Selection) -> Result<(), Error> {
    authorize_and_validate(&proposal)?;
    let revision = record_update_cas(host, &proposal)?;
    // Publish only a committed owner-held revision; never a writable mirror.
    state.publish_selected(host, proposal)?;
    Ok(())
}
~~~

The concrete publication/update ordering must follow spec/plugin-state-ownership.md; generated try_update_FIELD ergonomics are still tracked separately and must not be assumed complete. A proposed library VersionedRecord<T> may encapsulate encoding, revision checks, CAS acknowledgments, retries/invalidation and error mapping. It must **not** own live state, decide domain validity, grant resource authority or replace durable Session, Journal, credential and evidence semantics.

### 2.6 Frontend and evidence author usage

A frontend implements the **existing** Frontend::decode/render operations for its native protocol. It may share a checked SDK wrapper for session/resource validation; cancellation is an **authorized graph-root host action**, not a simulated Frontend::cancel success. Wire adapters do not manage provider accounts or session journals.

Evidence consumer invokes EvidenceStore::put/get/search with typed source references. Internal helpers may share revision/ledger validation, but membership checks must preserve the Project Storage provider's existing atomic membership-and-data SQLite snapshot. SourceEvidence remains a separate verification contract.

## 3. Derive the minimal implementation architecture from those usages

**No new kernel domain concepts:** the kernel still understands only define / require / provide contracts, canonical schemas, authority, selection, graph admission, cancellation and pending generations. It never learns "provider", "tool", "OAuth", "compaction" or "frontend" special cases.

### 3.1 Registries: normal selected plugins, not kernel tables

| Responsibility | Provider manager | Tool manager |
| --- | --- | --- |
| Startup imports | require(ProviderRegistrations, slot="Registrations", many(min=0)) | require(ToolRegistrations, slot="Registrations", many(min=0)) |
| Runtime exports | existing ProviderCatalog, ProviderAuth | existing ToolCatalog, ToolExecute |
| Owner-held state | registration snapshot; profiles; attempts; model offers; revision | installed tool identities, routing table, catalog revision |
| Contributor | tiny provider declaration plugin | tool declaration plus executable ToolExecute implementation |
| Dispatch | selected auth strategy, discovery strategy, credential backend, Model session | selected ToolExecute import for corresponding tool identity |
| Security | account, credential and method resource grants | registered tool ID, arguments, selected handler and session grant |

`ProviderRegistrations` (`phenix.provider.registrations@1`) and `ToolRegistrations` (`phenix.basic.tool-registrations@1`) **already exist on `main`**. They are not proposed new contracts. The provider contribution path entered with merged [#1128](https://github.com/matthis-k/phenix-ai/pull/1128), and builtin tools were consolidated in merged [#1129](https://github.com/matthis-k/phenix-ai/pull/1129). The current `ToolRegistrations` interface includes both `list` and `execute`; its proposed list-only form below is obsolete. The one-manager/event-driven contribution redesign remains subject to its open implementation PRs. Do not infer it ships from this proposal. Keep their payloads typed and immutable; each contributor's list operation is invoked **only after** the kernel has verified descriptor/package identity, selected imports and applicable authority. Multiple contributors use the already supported M1 many(min=0) import and deterministic slot indexing (see the working imports::all::invoke_at fixture in examples/authoring-client/src/lib.rs); duplicate IDs, ambiguous owner routes and unsupported adapter configurations fail at composition/registry setup. For tools, pair each registration with its preselected executable ToolExecute binding by verified contributor identity and an explicit composition-approved route; never trust a contributor-supplied artifact path or handler index. If a registrant and executor cannot be matched unambiguously, the tool is not offered. A registration is data, never a grant.

The author syntax may look like providers.register(...) but implementation should lower to a generated **normal contributor** with a read-only registration-list operation; it is not an implicit executable callback before dlopen and not a fourth kernel declaration role. If all registrations can be expressed in currently verified static package data without a new ABI, prefer that as an optimization, not as a prerequisite.

### 3.2 Runtime ownership matrix

| Layer | Owns | Must not own |
| --- | --- | --- |
| Kernel | Artifact validation, slot binding, authority, graph, cancellation/generation | Tool/provider IDs, OAuth token protocol, session/compaction rules |
| Provider manager plugin | Registered providers, account status, auth/OAuth/API-key lifecycle, selected discovery strategy, profile state | Kernel grants, HTTP wire parser, plaintext credential persistence |
| Credential backend plugin | Protected durable secret bytes and CAS/revoke | Proof of authenticated account or model entitlement |
| Protocol libraries / model transport | Endpoint validation, HTTP codecs, streaming, cancellation of I/O worker | Provider catalog or account selection |
| Tool manager plugin | Catalog merge, schema/identity routing, selected ToolExecute dispatch | Tool-specific mutable data or tool effects |
| Tool implementation plugin | Actual tool handler and private tool state | Central catalog routing, kernel grants |
| Basic Agent plugin | One run state machine, reply commit/reconciliation, tool execution progression | Transport codecs, OAuth, tokenizing/compaction |
| ModelPreparer plugin | Candidate transformation policy and optional selected summarizer use | Executing Model or granting checkpoint validity |
| Checkpoints / Evidence / Session / Journal | Their domain-authoritative durable records | Arbitrary mutable properties of other plugins |
| Storage plugin | Revisioned durable records/transactions | Capability-domain semantics and session-level auth |
| Frontend plugin | Wire decode/encode and protocol-native rendering | Agent turns, provider auth or graph cancellation policy |

**Exception:** ProjectStorage::read and membership/source verification share a SQLite snapshot today. Avoid "clean" separation that would introduce TOCTOU checks across two different databases.

### 3.3 Public contract deltas should stay small

| Interface | Status | Proposed work |
| --- | --- | --- |
| ProviderCatalog, ProviderAuth, ProviderCredentials | Existing | Rebind exports to one manager; preserve request/error schemas initially |
| ProviderDeclaration/AdapterBinding | Existing library values | Reuse as contributor payload, add immutable registration wrapper only if needed |
| ProviderRegistrations | Implemented (merged #1128) | `list() -> Result<ProviderPage, ProviderError>`; registration grants no authority |
| ToolCatalog, ToolExecute | Existing | Keep external API; ToolExecute implementations selected through prebound routes |
| ToolRegistrations | Implemented (on current main) | `list()` and `execute(ToolRequest)`; separate future manager/event hook remains proposed |
| BasicInput, BasicTurnPrepare, BasicTurnAdvance | Existing | Converge behavior, decide public revision cleanup only after parity |
| ModelPreparation, ModelPreparationChange | Existing | Reuse, make changed candidates admissible only after source/epoch/CAS validation |
| ModelTokenMeter | Existing | Bind to pinned selected Model; enforce final immutable request bytes |
| Model, HttpModelSession | Existing | Accept one validated installation, no implicit legacy fallback |
| Storage, Session, EventJournal, EvidenceStore | Existing | Keep; use shared internal Rust record helper, no new universal storage contract |
| Frontend | Existing | Reuse decode/render; deprecate redundant cancel only in separately tested revision |

The two registration contracts discussed here are **already implemented** in their current forms. Future proposed graph and manager contracts must be justified against the current source and [v2.4 target design](graph-architecture-v2.4.md). Do not add a generic Registry<T> kernel contract, a new tool transport protocol, or a universal plugin state service.

### 3.4 Initialization and lifecycle rules

1. Launcher/Nix composition explicitly selects verified registration contributors, manager(s), strategies, credential store, Model transport and grants. The client never supplies artifact paths or grants via a registration request.
2. Kernel verifies descriptors, schemas, artifact origins, import graph and authority before loading executable plugin code.
3. Each registry manager reads the allowed many-slot contributors **once per initialized generation**. It validates bounded sizes, duplicate IDs and deterministic ordering; failure aborts registry readiness, not silently fallbacks.
4. Manager hydrates only permitted durable metadata/credentials; verifies scopes and revisions. Its Rust State remains the authoritative **live** account/registry state; storage is not a direct writable state mirror.
5. Operations (auth, model listing, tools) dispatch only through already selected scoped imports. No dynamic plugin ID from a request becomes a kernel locator, and no declaration grants access.
6. Hot reload / composition replacement creates a new validated generation with isolated state and routes. In-flight calls retain original generation semantics until cancellation/completion. Reload never reinterprets a revoked account as authenticated.
7. For account refresh/revoke and durable CAS, remote uncertainty yields an explicit state; tokens stay within selected encrypted/permissioned credential boundaries and never enter diagnostics/ACP/Nix or ordinary Storage journals.

### 3.5 Consequence: smaller, more consistent imports

- Provider contributor: no runtime auth/callback/model/catalog implementation. Only one generated registration operation.
- Tool contributor: declaration and **one real executable ToolExecute handler**. No extra catalog/dispatch implementation.
- Model provider: one Model (and optional associated ModelTokenMeter) with its own transport/worker; no session or tool receipt writing.
- Context preparer: one ModelPreparation implementation; no second Basic agent loop.
- Memory/session/workspace providers: one domain interface each; Rust state + authorized optional persistence library.
- UI adapter: decode/render, plus client-side/native host cancellation; never routes model calls.

## 4. Design questions that usage resolves (and decisions to freeze)

1. **Is declaration execution allowed during startup?** Yes, after verified load and explicit slot selection. No plugin code before validation.
2. **Do auth/protocol strategies belong to the provider manager?** The manager owns the authoritative lifecycle and chooses generic implementations. Reusable libraries provide common algorithms; an exceptional external adapter is an independently authorized selected plugin, not a kernel branch.
3. **Does a registration imply an authenticated profile?** No. Only checked current state can claim that.
4. **Who owns tool execution?** The selected tool plugin. The manager owns catalog/dispatch, not arbitrary tool state.
5. **Can a preparer rewrite tools, scopes or limits?** No; the existing proposal contract denies those changes. Changed history/context requires checked provenance.
6. **Does the Model adapter own journal state?** No. Basic Agent owns reply reconciliation and source events; its commit ordering and crash recovery must be proved before removing the old receipt path.
7. **Does the persistence helper own domain state?** No; only Rust library mechanics, invoked inside the owning plugin.
8. **Do model+meter need the same selected identity?** Yes, the binding is part of the composition/selection proof, not inferred from a returned model name.
9. **Can a tool model choose a plugin target?** No; model supplies only registered tool name+arguments, manager chooses an already-authorized fixed import.
10. **Are fixture plugins stale?** Not merely because they aren't shipped. Deterministic tests remain a supported use case.
11. **Must an external plugin opt into every capability?** No. A minimal plugin can implement a single provide contract or generated registration and own plain Rust state.
12. **Do we need a new ABI or protocol?** No for this design. Only add domain contracts and SDK ergonomic code if the existing M1 selected import route cannot express the use case.

## 5. Implementation sequence driven by usage tests

| Slice | First test written | Minimal change | Gate / regression |
| --- | --- | --- | --- |
| A: provider contributor | Add a fictional provider via a one-screen declaration and see it in :Phenix auth; zero granted profiles | ProviderRegistrations + manager catalog merge, reuse current descriptors | Duplicate/conflicting IDs fail; built-in metadata-only provider remains safe |
| B: API-key and OAuth | Same :Phenix auth UI exercises both independent methods and two named accounts | Move strategy lifecycles into manager and shared libraries; keep verified issuer rules | Existing API-key product test, OAuth callback/expiry/restart/revoke, no account fallback |
| C: tool contributor | Add external uppercase tool, catalog and execute without editing existing tool plugin | ToolRegistrations + Tool manager + selected ToolExecute routing | JSON schema, cardinality, session authority, duplicate tool ID, cancel/denial |
| D: agent unification | Run same transcript/model/tool/cancel cases with default and advanced graph, compare byte-level committed state | One staged prepare/advance path; retire direct loop after parity | No duplicate tool effects; restart and pending-ticket cancellation; correct journal receipts |
| E: preparation | Test unchanged, changed-valid-checkpoint and changed-invalid-checkpoint proposals, exact meter mismatch | Shift compaction/summary orchestration out of Model adapter; Basic admits source-bound output | No model call when source/budget/identity invalid, no accepted forged checkpoint |
| F: persistence helper | Same CAS semantics across Memory, Session and Workspace; intentionally race two instances | Normal Rust library, not a plugin or new Storage contract | Conflict invalidation, rollback/publication ordering, restart; no record mixing |
| G: transport and presentation | One selected model works API-key/OAuth plus Terminal/JSONL/ACP; native cancel | Remove old preset and frontend wrappers after parity | No legacy fallback; errors remain typed, no credentials in ACP |
| H: evidence | Put/read/search project+session evidence with membership changes | Internal evidence backend adapter, atomic membership/read stays in Project Storage | No TOCTOU or withdrawn-source resurrection |

Slices A/C/F can prototype concurrently. B depends on A. D enables E. G depends on B and D. H is independent but low priority. Avoid migrating two distinct failure boundaries in the same PR.

## 6. Explicit acceptance criteria and negative tests

- An external provider contributor makes exactly one registration-visible contribution and contains **no** ProviderAuth, ProviderCatalog, credential-store or HTTP transport logic.
- An external tool contributor adds a tool without changing Basic Tools' central list/match; it retains private mutable state.
- The *same* registered provider and tool names yield deterministic revisioned catalogs regardless of load order; conflicts reject instead of shadowing.
- Missing registration, missing implementation, unsafe URLs, unsupported protocols, revoked or different-account credentials, schema mismatch and forbidden grant all fail **before effectful invocation**.
- Default and staged Basic transitions generate the same relevant journal, session and tool effects (allowing explicitly versioned graph-receipt metadata) and are interruption-safe.
- Summarization is a proposal; changed requests require original-hash, source-span, checkpoint lineage, CAS/readback and exact final ModelTokenMeter binding.
- A successful state mutation is observable through owner-read/watch only after the operation's durability and publication guarantees; ambiguous writes invalidate caches.
- Test-only fixtures stay outside default production package closure; regular users do not configure internal credential/transport variables just to authenticate.

## 7. Non-goals

No universal capability registry plugin, policy engine in Core, dynamic Rust ABI, generic behavioral descriptors, arbitrary model-selected plugin routing, cross-plugin implementation imports, direct writable copies of plugin state, opaque Nix-store secrets, removal of deterministic fixtures, or a frontend-specific provider protocol.

## 8. Existing implementation anchors

- crates/phenix-provider-builtins/src/lib.rs: three-field ProviderDeclaration and typed extensions.
- crates/phenix-provider-contracts/src/lib.rs: ProviderCatalog/Auth/credentials/model selection.
- crates/phenix-basic-contracts/src/lib.rs: Model, ToolCatalog, ToolExecute, Basic turn stages, Frontend, Storage, Session, Journal.
- crates/phenix-advanced-contracts/src/model_preparation.rs: source-bound proposal and invariants.
- crates/phenix-advanced-contracts/src/model_tokens.rs: selected exact-token measurement contract.
- crates/phenix-sdk/src/live.rs and examples/authoring-client/src/lib.rs: many(min=0), slot_count and selected imports.
- crates/phenix-plugin-basic-tools/src/lib.rs: three-name hardcoded fixture dispatcher.
- crates/phenix-plugin-basic-agent/src/lib.rs: direct/staged loops and source journal.
- crates/phenix-plugin-advanced-model-adapter/src/lib.rs: compaction and receipt logic to migrate.
- crates/phenix-plugin-basic-frontends/src/lib.rs: three frontend codecs and repeated wrappers.
- crates/phenix-plugin-evidence-store/src/lib.rs and crates/phenix-plugin-project-storage-sqlite/src/lib.rs: dual-scope evidence and atomic membership check.
- spec/plugin-state-ownership.md: merged owner-held state and pending transactional update ergonomics.

**Verification level:** code-derived design, not a proof of API compilation. All proposed examples use target syntax, not implemented macros. No build/test or production code changes are claimed by this document.
