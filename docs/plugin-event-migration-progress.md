# Phenix event-driven plugin migration — execution log

Updated: 2026-10-25. Working stack: #1144 → #1146 → #1147 → #1148 → #1149 → #1150.
Detailed all-plugin inventory: [plugin-event-restructure-matrix.md](plugin-event-restructure-matrix.md).

## Operating rules

Complete groups in dependency order. Each segment records actual commits, tests,
blockers and ergonomic findings. **A draft source change is not a merged or
verified implementation.** Kernel events remain small and fixed; domain events
belong to their declaring plugins; directed effects stay typed contracts.
Use only the existing M1 graph, ABI, authority, cancellation and state watches.

## Progress

| Group | Source/plugin scope | State | Executed implementation | Validation | Next gate |
| --- | --- | --- | --- | --- | --- |
| G0 – Kernel / event SDK | kernel graph, contract graph, SDK, macros, Basic/ACP host | IMPLEMENTED, verification pending | #1147 intrinsic kernel events; #1148 in-root emission; #1149 owner settlement, typed emit, Basic/ACP begin/shutdown and dependency-ordered plugin-loaded notifications | Formatting and generated workflow passed on recent heads; Nix native shards remain gate | Exact-head native product green; cancellation/failure/pending tests; reconcile parents |
| G1 – Provider manager and declarations | providers, provider-openai, provider-standards, provider-builtins, provider-openai-api, provider-chatgpt | PARTIAL / two-contributor product pending | #1146–#1149 shared manager/owner settlement; #1150 second plain-data registration hook (five standards), typed register_all, six-provider native catalog check | Rustfmt passed earlier after corrections; full native product pending | Cut over ACP consumers then remove duplicate OAuth/key/catalog and legacy pull registrar |
| G2 – Tool discovery and execution | basic-tools, tool-fixture | IMPLEMENTED / native acceptance pending | #1151: typed tools.populate/ready, owner onBegin→settled, attested private registry, reverse-tool hook; legacy catalog and execute contracts preserved | Rustfmt/ACP frontend passed on recent heads; native tool-population product pending | Exact-head native graph test and production compatibility, then remove redundant list boilerplate |
| G3 – Agent orchestration and sessions | basic-agent, basic-session, advanced-preparer-graph, advanced-model-adapter | PARTIAL / Basic success notice implemented | #1152 agent-owned run-finished notice after pending submit graph node, with session/run IDs and nonsecret counters; staged graph and Session CAS unchanged | Rustfmt, generated workflow and ACP-v2 passed on recent heads; native Basic/ACP acceptance pending | Verify graph post-submit notification and native graph composition; design staged successful terminal event and post-commit session notices |
| G4 – Optional semantic changes | workspace-selection, basic-memory, compaction-checkpoints, event-journal, evidence-store | NOT STARTED | Candidate events and authoritative owners inventoried | Not applicable yet | Validate subscriber need; publish only on committed state |
| G5 – Keep contract-oriented | storage, credentials, model transport, model fixtures, evidence-journal, environment, project-storage | REVIEWED / NO FORCED CONVERSION | Explicit 'no event' exclusions recorded in matrix | Existing contract conformance remains gate | Avoid new events for reads, CAS, tokens, or secrets |

### Segment log

- **S01 (G0)**: #1149 fixes Basic loop kernel move while `GraphExecution<'_>`
  was still in scope. Explicitly releases graph borrow before shutdown
  (`crates/phenix-basic-runtime/src/main.rs`, commit `36eeb42f`).
  Previously observed error: Rust E0505 in an ACP/Nix build.
- **S02 (G0–G5 audit)**: inventoried **31 plugin crates**, source roles,
  event fit, current registration methods, preserved contracts, risk,
  follow-up tasks, and acceptance gates. Matrix `docs/plugin-event-restructure-matrix.md`
  (commit `09db59f`). 9 high, 7 medium, 9 low, 6 no conversion.
- **S03 (G0 verification)**: at `09db59f`, Rustfmt, generated flake CI,
  ACP v2, and cache isolation reported success. Full Nix candidate and
  official ACP interoperability remained in progress. Do **not** treat as
  native provider product acceptance.

- **S04 (G0 load events)**: #1149 now delivers intrinsic `plugin-loaded`
  notifications in actual dependency-resolved startup order, before
  `phenix.kernel.begin`, only when subscribed
  (`phenix-kernel/src/runtime.rs`, Basic/ACP host, commits `0b4d9fa` and
  `1ab720c`). Existing kernel begin/shutdown event dispatch remains unchanged.
- **S05 (G1 declarations)**: draft #1150 adds `phenix-plugin-provider-standards`
  as an ordinary five-provider data/hook bundle separate from
  `phenix-plugin-provider-openai`; both import the same registry. Native
  product test selects both, asserts four graph nodes and exact six canonical
  provider IDs, with OpenAI API and ChatGPT connections. Legacy pull registrar
  is retained pending consumer cutover.
- **S06 (G1 ergonomic refinement)**: typed
  `Providers::register_all(iter)` reduces declarative bundle boilerplate;
  shared `standard_provider_definitions()` centralizes the transitional
  non-OpenAI partition. No second registry or new ABI added. Native CI pending.

- **S07 (A25 / G5 model-http review)**: inspected `phenix-plugin-model-http`
  and its transport, pending hooks and configuration state. `#[pending]`,
  `#[poll]` and `#[cancel]` already represent asynchronous model work;
  token chunks append through the authorized EventJournal contract. **No
  model-token graph events** or new parallel event system. Session-scoped
  `HttpModelSession::install` remains a directed contract; selected
  configuration consumes once and cannot fall back to environment defaults.
  Current kernel shutdown refuses nonquiescent native work and retains its
  dependency closure, so worker teardown must remain coordinated through
  pending cancellation rather than post-unload callbacks.
- **S08 (G1 CI)**: PR #1150 at `fe79be9` passed Rustfmt, generated workflow,
  ACP v2, cache isolation, deterministic session history and official ACP
  interoperability checks. Full Nix candidate remained in progress; no native
  product acceptance or merge claim.
- **S09 (G1 scope + SDK ergonomics)**: manager event callbacks now share
  `current_scope(&Host)` backed by the same authorized host resource check;
  `StaticGraphNode::on(id, triggers, operation)` was added to the canonical
  M1 graph declarations and used for both OpenAI and standard contributor
  hooks. This removes repeated selector/after/writes/join/failure fields
  without inventing a second event mechanism (commits `39625d7`,
  `8195f2e`, `16b4d29`, `d02c286`). Native tests pending.
- **S10 (G2 interface boundary review)**: external tool
  `ToolRegistrations` combines `list` and `execute`. Moving declarations
  to an event must preserve the **authenticated selected execution binding**,
  not accept an untrusted `provider_id` or `index` claimed in data. Do not
  ship a metadata-only registration event wired to unrelated execution
  routing. Track ERG-18 as a prerequisite for the G2 migration.


- **S11 (G2 implementation)**: draft #1151 now implements the actual
  manager-owned `phenix.kernel.begin → phenix.tools.populate →
  settled:phenix.tools.populate → phenix.tools.ready` flow. `basic-tools`
  owns its registry state, validates each externally offered tool against
  resolver-selected `ToolRegistrations::list` metadata, and pins the same
  selected import index for execution. `tool-fixture` subscribes and calls
  the typed registry contract while retaining its own executor.
- **S12 (G2 reentrancy fix)**: the manager now gathers all selected tool
  catalogs **before** emitting populate, not by invoking `list` from inside
  the contributing plugin's active registration handler. Without this,
  nested invocation would reenter a poisoned native instance. The same
  attested catalog is checked again during owner settlement. New native
  `phenix-tool-population-test` tests the complete graph followed by
  authenticated reverse-tool execution, and the legacy pull product test
  retains selected import grants. Acceptance still pending.
- **S13 (G3 assessment)**: Basic Agent already has a prompt-triggered
  graph and watched pending-model-request state; Sessions operations remain
  selected transactional contracts. A mandatory emitted event in a
  direct `Session::checkpoint` call would fail without a live graph root,
  because `Host::emit_event` is intentionally same-root-only. Do not
  introduce a second emitter or silently swallow this failure; log a
  generic deferred semantic-notification requirement before migration.

- **S14 (G3 Basic Agent implementation)**: draft #1152 declares
  `phenix.agent.run-finished` as a plugin-owned typed event and adds an
  explicit graph node ordered **after** the existing pending
  `phenix.basic.agent/submit` operation. The agent saves only the successful
  ephemeral `RunResult` receipt; it checks session scope and emits the
  event after durable checkpoint/journal completion. The event contains only
  session/run identities and model/tool counts. Direct Model, Tool, Session,
  Journal, and Storage contracts are unchanged, and the staged-graph
  reprepare path emits no fabricated final answer.
- **S15 (G3 existing state-reactive paths)**: Advanced preparation already
  watches authoritative `pending_model_request` and coordinates through
  graph state-change triggers; no duplicate event replaces that contract.
  Session open/checkpoint/reconcile remain CAS-checked direct operations.
  Emitting a semantic Session event outside a live GraphExecution still needs
  an explicit generic post-commit mechanism (ERG-21); leave session
  mutations unchanged until this is implemented.
- **S16 (G2/G3 validation)**: #1151 native tool-population Nix shards
  running; #1152 Rustfmt, generated CI and ACP-v2 frontend checks passed on
  its latest checked head. Exact-head full Nix and official ACP interop
  remain acceptance gates. All PRs draft/unmerged.

## Ergonomics register

Each item is observed in source. 'Target' describes the desired author-facing
interface; it is not automatically implemented merely by being logged.

| ID | Severity | Surface / current friction | Proposed target / owner | Tracking state |
| --- | --- | --- | --- | --- |
| ERG-01 | High | Provider author must manually construct `StaticGraphContribution` node, literal `triggers`, `operation`, plus `#[provide(ProviderPopulate::register)]` and `graph(super::GRAPH)` (`phenix-plugin-provider-openai/src/lib.rs`) | SDK `#[phenix::on(ProvidersPopulate)]` compiles into **existing** graph node and typed handler, no new dispatcher | Open; after G0 |
| ERG-02 | High | Settled handler uses magic `"settled:phenix.providers.populate"` and a separately defined empty Portable struct/contract (`phenix-plugin-providers`, `phenix-provider-contracts`) | Generic `#[phenix::on_settled(ProvidersPopulate)]` or typed `on(ProvidersPopulate::Settled)`; generate request schema/import | Open; G0 SDK |
| ERG-03 | High | `Providers::new(host, "Registry", &event.scope).register(OPENAI)` needs manually synchronized slot and scope names | Typed injected registry handle from selected imports, with resource scope derived/checked by SDK; retain explicit grants | Open; G1 SDK |
| ERG-04 | High | Legacy OpenAI API/ChatGPT plugins each own full catalog/auth interfaces alongside `phenix-plugin-providers`; duplicated OAuth/protocol/discovery complexity | Declarative provider plugin holds data + hook; all generic strategies in one shared providers manager (or supporting Rust libraries) | Open; G1 cutover |
| ERG-05 | Medium | Manager manually decodes `phenix.providers.<scope>` for both begin and settlement, and stores a second `selected` vector in addition to registry phase | Central shared `ProviderScope` validation; single authoritative registry published snapshot, not two state owners | Open; G1 |
| ERG-06 | Medium | Typed `Host::emit_event` exists, but `#[event]` subscriptions still require trigger-ID strings in static graphs | Typed subscriptions infer event ID/schema from the Rust event type; compatibility with existing graph metadata | Open; G0 |
| ERG-07 | High | Basic/ACP controllers duplicate finite polling loops for lifecycle admission; pending startup waves are rejected after arbitrary pump budget | Generic lifecycle controller/pump API with async progress, cancellation and shutdown semantics; kernel remains generic | Open; G0 |
| ERG-08 | Medium | Pull contributor `ProviderRegistrations::list` coexists with event registration and can select a legacy catalog before `kernel.begin` when invoked early | One explicit catalog readiness barrier and phased migration; no silent competing authoritative sources | Open; G1 |
| ERG-09 | Medium | Tool contributors manually provide both list and execute; tool catalog fetches contributors rather than generic population | Typed tool data registration via manager-owned tools.populate; only tool-specific execution in implementation | Open; G2 |
| ERG-10 | Medium | Domain/error boilerplate and `CallResult` wrappers repeated in small contributor plugins | SDK typed handler signature/error adapter where portable errors are inferred, without weakening domain/Kernal separation | Open; G1/G2 |
| ERG-11 | High | Full startup events can end with a domain failure and pending manager staging; host must guarantee teardown and no catalog fallback | Standard graph-lifecycle transaction/abort ownership and explicit failure outcome; ensure next generation remains isolated | Open; G0/G1 |
| ERG-12 | Medium | Graph static event IDs are strings and namespace ownership is verified at runtime emission | Infer typed event ownership and handler identity at compile-time where possible; still validate dynamic/native metadata offline | Open; G0 |

| ERG-13 | Medium | Provider declaration bundles need a manual registration loop with repetitive per-item error propagation (`phenix-plugin-provider-standards`) | Shared `Providers::register_all(iter)` invokes the same typed contract; does not bypass validation or grant authority | **Implemented in #1150**; native test pending |
| ERG-14 | High | OpenAI remains defined in both old `defaults::PROVIDERS` and new `phenix-plugin-provider-openai`; registration bundle must explicitly exclude its identity | Complete cutover: one definition owner per provider; remove OpenAI from old legacy defaults and delete retired pull contributor once ACP consumers migrate | Open; G1 de-duplication |
| ERG-15 | Medium | Standard declaration hook still requires manual `graph(super::GRAPH)`, empty plugin State and `#[provide(...)]` for a one-operation registration | Generate a generic typed event subscriber from the Rust handler; keep graph/ABI metadata as the compiled target | **Partially fixed** via generic `StaticGraphNode::on` in #1150; typed annotation still open |
| ERG-16 | Medium | Product assertions assumed provider index zero was OpenAI, while the registry correctly sorts canonical provider IDs | Author-facing provider lookup by stable ID, never by position; product test verifies exact deterministic ID set | **Fixed in #1150**; native test pending |

| ERG-17 | Medium | Long-lived model transport work currently relies on separate pending cancel/poll and the host's physical-lease shutdown gating; generic lifecycle pumping cannot safely force early unload (`phenix-plugin-model-http/src/lib.rs`, `phenix-kernel/src/runtime.rs`) | A shared async lifecycle controller that drains or cancels outstanding tickets *before* kernel.shutdown, preserving worker ownership; no token-level graph events | Open; generic G0, not model-http event conversion |

| ERG-18 | High | Tool catalog's imported `ToolRegistrations::{list,execute}` associates routing with resolver-selected slot identity, which would be lost if a registration payload supplied an arbitrary executor name (`phenix-plugin-basic-tools`) | Preserve an attested execution capability/selected binding when moving tool descriptions into an event-driven registry; never trust contributor-supplied route indices | Open; G2 security gate |

| ERG-19 | High | Tool subscriber's active handler cannot be reentered by manager calling its list contract; live registration previously queried the selected tool's list synchronously | Pin preselected contributor catalogs in manager onBegin, verify event registration against the snapshot, and preserve the imported route | **Fixed in #1151**, native product pending |
| ERG-20 | Medium | Tool authors still define both `ToolRegistrations::list` and `ToolPopulate::register` to provide exactly the same immutable metadata, solely for authenticated routing | Generic SDK-supplied attested executor capability binding in a registration payload; stop requiring a duplicated pull list once provenance can be preserved | Open; G2 cleanup |
| ERG-21 | High | Direct selected session/storage contracts can run outside any `GraphExecution`, but `Host::emit_event` is permitted only within one active graph root | Generic post-commit semantic notification integration from the owning graph/controller, with no nested root or silent event loss | Open; G3 prerequisite |
| ERG-22 | Medium | Several manager/startup plugins repeat manual graph declarations even with the common `StaticGraphNode::on` constructor | Optional typed `#[phenix::on(Event)]`/on-settled sugar generated over existing M1 contracts and graph descriptors | Open; shared SDK |

| ERG-23 | Medium | A graph successor receives its triggering event payload, not the finished predecessor's typed result, so Basic Agent currently retains an ephemeral completed RunResult for its ordered notification node | Add type-checked predecessor-result binding to the existing graph SDK (not a general event bus), or generated `#[on_completed(BasicInput::submit)]` lowering; preserve authority and cancellation | Open; G3 SDK ergonomics |
| ERG-24 | Medium | Agent's non-staged submit and staged-model continuation have different terminal steps; blindly emitting run-finished on each reprepare wave would fabricate completion | Introduce a terminal-only semantic completion continuation for staged graph based on `TurnAdvanceOutcome::Complete`, never `Reprepare` | Open; G3 staged migration |

### New finding policy

For each ergonomic issue: attach a source path, why the author needs to write
it, smallest usage-side goal, shared owner, and whether it can be fixed
without changing M1 ABI/authority. Avoid adding bespoke provider/tool macros
where a generic SDK hook works. Do not silently defer defects to ergonomics.
