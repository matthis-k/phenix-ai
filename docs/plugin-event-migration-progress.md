# Phenix event-driven plugin migration — execution log

Updated: 2026-10-25. Working stack: #1144 → #1146 → #1147 → #1148 → #1149 → #1150 → #1151 → #1152.
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
| G1 – Provider manager and declarations | providers, provider-openai, provider-standards, provider-builtins, provider-openai-api, provider-chatgpt | PARTIAL / immutable registry snapshot consolidated; native product pending | #1150 typed registration; #1152 removes duplicate selected manager vector, leaving a single registry-owned immutable catalog for event and legacy pull | Unit tests added; native CI pending | Cut over ACP consumers then remove duplicate OAuth/key/catalog and legacy pull registrar |
| G2 – Tool discovery and execution | basic-tools, tool-fixture | IMPLEMENTED / native acceptance pending | #1151: typed tools.populate/ready, owner onBegin→settled, attested private registry, reverse-tool hook; legacy catalog and execute contracts preserved | Rustfmt/ACP frontend passed on recent heads; native tool-population product pending | Exact-head native graph test and production compatibility, then remove redundant list boilerplate |
| G3 – Agent orchestration and sessions | basic-agent, basic-session, advanced-preparer-graph, advanced-model-adapter | PARTIAL / both terminal paths implemented | #1152 agent-owned run-finished notice after pending submit graph node, with session/run IDs and nonsecret counters; staged advance emits only after terminal checkpoint (not on Reprepare); Session CAS unchanged | Rustfmt, generated workflow and ACP-v2 passed on recent heads; native Basic/ACP acceptance pending | Verify graph post-submit notification and native graph composition; verify native staged async completion and generic post-commit session notices (ERG-21) |
| G4 – Optional semantic changes | workspace-selection, basic-memory, compaction-checkpoints, event-journal, evidence-store | REVIEWED / conditional only | Audited existing selected contracts; no concrete independent default subscriber yet establishes a need for new broadcasts; do not add per-CAS or per-token events | Source-level audit only; no new code or native gate | Implement owner-scoped, post-commit notices only when a concrete consumer is identified and ERG-21 is solved |
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
  `phenix.basic.agent/submit` operation. The agent saves only a
  redacted ephemeral `AgentRunFinished` notice (no answer or transcript);
  it checks session scope and emits that notice after durable
  checkpoint/journal completion. The event contains only
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

- **S17 (G3 staged completion)**: #1152 now emits the same redacted
  `phenix.agent.run-finished` event from pending `BasicTurnAdvance::advance`
  only on its terminal `Complete` branch, after Session checkpoint.
  Reprepare, pending, failure and cancellation never emit. Emission failure
  surfaces explicitly; native asynchronous graph-root acceptance is pending.
  The obsolete staged no-events unit assertion was corrected.
- **S18 (shared graph ergonomics)**: added const `StaticGraphNode::after()`
  builder and reused it for the Basic completion successor node. Added
  contract unit coverage; no new scheduler, wire ABI or authority surface.

- **S19 (G4 source/consumer review)**: `Workspace::set`, `Context::set`,
  `CompactionStore::put`, `EvidenceStore::put` and `EventJournal::append`
  are directed, authorized operations with their own persistence/CAS or
  stream semantics. The searched default consumers did not establish an
  independent multicast notification requirement. No speculative events
  added. Session-scoped semantic notices remain gated by generic post-commit
  graph delivery (ERG-21); per-token journal append is expressly excluded.

- **S20 (G0 canonical graph declarations)**: static M1 graph metadata now
  canonicalizes authored events, actions, handlers and dependency keys in
  a const encoder, without requiring source-order sorting. The kernel
  separately resolves event-to-handler subscriptions and explicit
  handler-to-handler edges after collecting contributions. SDK permutation
  byte-equivalence and kernel DAG-order tests were added. CI formatting
  fixes committed; native acceptance remains open (ERG-25).
- **S21 (G1 single authoritative provider catalog)**: the shared provider
  manager no longer stores an independent selected vector. Registry owns
  Idle/Collecting/Published(snapshot)/Aborted, and both successful population
  and temporary legacy pull selection use the same immutable snapshot.
  Rejected, aborted, or active graph waves prohibit fallback selection.
  Added registry unit tests; no contract/ABI or credential data moved.
- **S22 (all 31 plugin progress rows)**: updated the detailed source matrix
  with implementation/draft/review status for every audited plugin, the
  actual PR stack, remaining native gates and explicit no-conversion cases.
  High-benefit next gates: G1 ACP/provider cutover, G2 contributor provenance
  deduplication, and G3 native pending/cancellation plus ERG-21.

- **S23 (shared typed event subscriptions)**: added SDK
  `on_event!(node_id, EventType, operation)` macro that derives its trigger
  ID from the event's `GraphEventPayload` implementation. It lowers to the
  existing `StaticGraphNode::on` and requires no runtime bus or ABI change.
  Applied to provider-openai, provider-standards and tool-fixture so event
  renames cannot silently desynchronize registration hooks. Added SDK
  metadata equivalence unit coverage; exact-head native CI still pending.

- **S24 (G2 native acceptance repairs, owning #1151)**: replaced the
  selected-contributor route validation's repeated catalog scans with a
  single two-element iterator check; preserved exact metadata equality,
  unique selected executor and no contributor-selected route indexes.
  Removed the redundant borrow at manager settlement. Both were previously
  rejected by `-D warnings`. Propagated offline preparation diagnostics
  into `tool-contributors-product.py` rather than returning an opaque `None`.
  Latest checked #1151 head `d9c878e6` passed Rustfmt, generated workflow,
  ACP-v2, cache isolation and deterministic history; native Nix matrix and
  official ACP remained in progress at observation. No merge.
- **S25 (G1 legacy/event bridge, owning #1150)**: the provider manager
  stages verified legacy selected `ProviderRegistrations::list` pages in its
  own private generation registry at `kernel.begin`, then emits the standard
  populate event; all contributors settle into the same publication path.
  Failures abort staging; legacy and event duplicate identities remain
  subject to the same catalog validation. Trusted ACP provider composition
  now grants only the manager's declared startup/settlement operations.
  Also repaired the contract-definition Nix assertion and added native
  provider-builtins offline failure diagnostics. No automatic authority
  grants were added in the kernel.
- **S26 (stack integration blocker)**: #1150–#1152 report merge conflicts
  against their updated bases after parent PR amendments. New head native
  checks cannot be claimed green until stack synchronization/rebase and
  exact-head CI. Existing source fixes are committed on their owning PRs;
  do not merge any lane while this remains unresolved.

- **S27 (stack reconstruction)**: #1150, #1151 and #1152 now descend
  from their respective current parents and all three PRs are mergeable.
  Each original head has a `backup/` ref; guarded ref updates were used.
  The #1152 tree retains trusted provider ACP lifecycle grants, provider
  legacy declaration staging, the one-authority catalog, and G2 selected
  tool attestation fixes. This does **not** mean merged or native-verified.
- **S28 (latest validation)**: after stack reconstruction, CI exposed
  rustfmt-only differences in #1152's provider Registry unit tests and
  the typed on_event SDK test; corrected them in commits `0d50f731`
  and `c639beb6`. The latest generated-workflow validation passed,
  but exact-head native/Nix and ACP acceptance are still running.
  All PRs remain draft until those gates succeed.

- **S29 (G0 typed operation subscriptions, draft next lane)**: SDK
  `on_event!` now accepts an `Operation` marker type as its third argument,
  deriving the event ID from `GraphEventPayload::ID` and the exact operation
  from `Operation::KEY`. The legacy literal operation variant is retained
  unchanged for compatibility; typed bindings compile to the same static
  descriptor and kernel authority path. Migrated provider-openai,
  provider-standards, and tool-fixture, and added a test asserting exact
  canonical metadata byte equivalence. Native acceptance pending.

- **S30 (G2 one authoritative tool catalog, follow-up draft)**: removed
  Basic Tools' parallel `State.snapshot` and `ToolSnapshot`. The owner-
  private registry now transitions `Idle → Collecting → Published(Catalog)`
  (or Aborted), holding the canonical tool definitions, resolver-attested
  executor routes and cursor revision in one immutable generation snapshot.
  Both kernel.begin event settlement and temporary direct pull calls use
  the same registry begin/register/publish transition; no event payload
  can supply an executor index. After graph publication, the manager drops
  the no-longer-needed attested contributor pages. Registry unit tests cover
  publication visibility, route preservation, terminal abort and no reentry.
  Native B3, tool-population, ACP and Nix acceptance pending.

- **S31 (native grant and hook graph acceptance)**: #1151 now owns the
  B5 conformance grants for ToolStartup/ToolSettlement; #1152 continues
  to own the AgentRunFinished grant and ordered success/failure assertions.
  The external tool fixture's ToolRegistry dependency is Optional at
  startup because its handler is graph-triggered after initialization.
  Invoking the selected registry remains scoped and authorized; the
  resolver still rejects genuinely required dependency cycles.
- **S32 (G2 failure-atomic compatibility selection)**: the shared tool
  Registry now exposes `from_selected` for the transitional pull path.
  It prevalidates all resolver-selected contributors into a temporary
  candidate and publishes it only after the whole catalog passes.
  Transient unavailable imports or duplicate/invalid declarations cannot
  strand the manager in Collecting or expose a partial snapshot.
  Includes rejection and successful-route unit tests.
- **S33 (G1 provider native boundary)**: native host API-key product's
  explicit owner-scoped startup and settlement graph grants are now
  committed in #1150 and inherited through #1154. No kernel permission
  inference was added. All five PRs remain draft pending native Nix
  acceptance on the exact revised heads.

## Ergonomics register

Each item is observed in source. 'Target' describes the desired author-facing
interface; it is not automatically implemented merely by being logged.

| ID | Severity | Surface / current friction | Proposed target / owner | Tracking state |
| --- | --- | --- | --- | --- |
| ERG-01 | High | Provider author must manually construct `StaticGraphContribution` node, literal `triggers`, `operation`, plus `#[provide(ProviderPopulate::register)]` and `graph(super::GRAPH)` (`phenix-plugin-provider-openai/src/lib.rs`) | SDK `#[phenix::on(ProvidersPopulate)]` compiles into **existing** graph node and typed handler, no new dispatcher | Open; after G0 |
| ERG-02 | High | Settled handler uses magic `"settled:phenix.providers.populate"` and a separately defined empty Portable struct/contract (`phenix-plugin-providers`, `phenix-provider-contracts`) | Generic `#[phenix::on_settled(ProvidersPopulate)]` or typed `on(ProvidersPopulate::Settled)`; generate request schema/import | Open; G0 SDK |
| ERG-03 | High | `Providers::new(host, "Registry", &event.scope).register(OPENAI)` needs manually synchronized slot and scope names | Typed injected registry handle from selected imports, with resource scope derived/checked by SDK; retain explicit grants | Open; G1 SDK |
| ERG-04 | High | Legacy OpenAI API/ChatGPT plugins each own full catalog/auth interfaces alongside `phenix-plugin-providers`; duplicated OAuth/protocol/discovery complexity | Declarative provider plugin holds data + hook; all generic strategies in one shared providers manager (or supporting Rust libraries) | Open; G1 cutover |
| ERG-05 | Medium | Manager manually decodes `phenix.providers.<scope>` for both begin and settlement, and stores a second `selected` vector in addition to registry phase | Central shared `ProviderScope` validation; single authoritative registry published snapshot, not two state owners | **Partly fixed in #1152**: one published registry snapshot, scope validation remains manual |
| ERG-06 | Medium | Typed `Host::emit_event` exists, but `#[event]` subscriptions still require trigger-ID strings in static graphs | Typed subscriptions infer event ID/schema from the Rust event type; compatibility with existing graph metadata | **Partially implemented in #1152**: typed SDK on_event! used by 3 contributors; handler operation still explicit |
| ERG-07 | High | Basic/ACP controllers duplicate finite polling loops for lifecycle admission; pending startup waves are rejected after arbitrary pump budget | Generic lifecycle controller/pump API with async progress, cancellation and shutdown semantics; kernel remains generic | Open; G0 |
| ERG-08 | Medium | Pull contributor `ProviderRegistrations::list` coexists with event registration and can select a legacy catalog before `kernel.begin` when invoked early | One explicit catalog readiness barrier and phased migration; no silent competing authoritative sources | Open; G1 |
| ERG-09 | Medium | Tool contributors manually provide both list and execute; tool catalog fetches contributors rather than generic population | Typed tool data registration via manager-owned tools.populate; only tool-specific execution in implementation | Open; G2 |
| ERG-10 | Medium | Domain/error boilerplate and `CallResult` wrappers repeated in small contributor plugins | SDK typed handler signature/error adapter where portable errors are inferred, without weakening domain/Kernal separation | Open; G1/G2 |
| ERG-11 | High | Full startup events can end with a domain failure and pending manager staging; host must guarantee teardown and no catalog fallback | Standard graph-lifecycle transaction/abort ownership and explicit failure outcome; ensure next generation remains isolated | Open; G0/G1 |
| ERG-12 | Medium | Graph static event IDs are strings and namespace ownership is verified at runtime emission | Infer typed event ownership and handler identity at compile-time where possible; still validate dynamic/native metadata offline | Open; G0 |

| ERG-13 | Medium | Provider declaration bundles need a manual registration loop with repetitive per-item error propagation (`phenix-plugin-provider-standards`) | Shared `Providers::register_all(iter)` invokes the same typed contract; does not bypass validation or grant authority | **Implemented in #1150**; native test pending |
| ERG-14 | High | OpenAI remains defined in both old `defaults::PROVIDERS` and new `phenix-plugin-provider-openai`; registration bundle must explicitly exclude its identity | Complete cutover: one definition owner per provider; remove OpenAI from old legacy defaults and delete retired pull contributor once ACP consumers migrate | Open; G1 de-duplication |
| ERG-15 | Medium | Standard declaration hook still requires manual `graph(super::GRAPH)`, empty plugin State and `#[provide(...)]` for a one-operation registration | Generate a generic typed event subscriber from the Rust handler; keep graph/ABI metadata as the compiled target | **Partially fixed** via generic `StaticGraphNode::on` in #1150; typed event ID now inferred via on_event!; generating registration/handler from annotation still open |
| ERG-16 | Medium | Product assertions assumed provider index zero was OpenAI, while the registry correctly sorts canonical provider IDs | Author-facing provider lookup by stable ID, never by position; product test verifies exact deterministic ID set | **Fixed in #1150**; native test pending |

| ERG-17 | Medium | Long-lived model transport work currently relies on separate pending cancel/poll and the host's physical-lease shutdown gating; generic lifecycle pumping cannot safely force early unload (`phenix-plugin-model-http/src/lib.rs`, `phenix-kernel/src/runtime.rs`) | A shared async lifecycle controller that drains or cancels outstanding tickets *before* kernel.shutdown, preserving worker ownership; no token-level graph events | Open; generic G0, not model-http event conversion |

| ERG-18 | High | Tool catalog's imported `ToolRegistrations::{list,execute}` associates routing with resolver-selected slot identity, which would be lost if a registration payload supplied an arbitrary executor name (`phenix-plugin-basic-tools`) | Preserve an attested execution capability/selected binding when moving tool descriptions into an event-driven registry; never trust contributor-supplied route indices | Open; G2 security gate |

| ERG-19 | High | Tool subscriber's active handler cannot be reentered by manager calling its list contract; live registration previously queried the selected tool's list synchronously | Pin preselected contributor catalogs in manager onBegin, verify event registration against the snapshot, and preserve the imported route | **Fixed in #1151**, native product pending |
| ERG-20 | Medium | Tool authors still define both `ToolRegistrations::list` and `ToolPopulate::register` to provide exactly the same immutable metadata, solely for authenticated routing | Generic SDK-supplied attested executor capability binding in a registration payload; stop requiring a duplicated pull list once provenance can be preserved | Partially implemented in follow-up draft: one published catalog; duplicate ToolRegistrations::list and ToolPopulate declarations remain cleanup |
| ERG-21 | High | Direct selected session/storage contracts can run outside any `GraphExecution`, but `Host::emit_event` is permitted only within one active graph root | Generic post-commit semantic notification integration from the owning graph/controller, with no nested root or silent event loss | Open; G3 prerequisite |
| ERG-22 | Medium | Several manager/startup plugins repeat manual graph declarations even with the common `StaticGraphNode::on` constructor | Optional typed `#[phenix::on(Event)]`/on-settled sugar generated over existing M1 contracts and graph descriptors | **Partially implemented in #1152** via typed on_event! now also infers Operation::KEY in follow-up draft; on-settled/handler generation still open |

| ERG-23 | Medium | A graph successor receives its triggering event payload, not the finished predecessor's typed result, so Basic Agent currently retains an ephemeral completed RunResult for its ordered notification node | Add type-checked predecessor-result binding to the existing graph SDK (not a general event bus), or generated `#[on_completed(BasicInput::submit)]` lowering; preserve authority and cancellation | Open; G3 SDK ergonomics |
| ERG-24 | Medium | Agent's non-staged submit and staged-model continuation have different terminal steps; blindly emitting run-finished on each reprepare wave would fabricate completion | Introduce a terminal-only semantic completion continuation for staged graph based on `TurnAdvanceOutcome::Complete`, never `Reprepare` | **Implemented in #1152**, native acceptance pending |

| ERG-25 | High | Const metadata formerly required lexicographic graph member order, leaking canonical encoding into plugin authoring and causing E0080 on valid out-of-order DAG nodes | Canonicalize all authored graph members at const-encoding time; resolve execution only after collecting edges | **Implemented in #1152**, SDK+kernel permutation tests added; native CI pending |
| ERG-26 | High | Installing the provider host's graph startup/settlement nodes requires declared, scoped operation grants in legacy ACP/product compositions; without them offline preparation rejects the graph before plugin execution | Update trusted composition/deployment builders to grant only owner lifecycle hooks under the exact resource; retain fail-closed kernel admission | **Partially fixed in #1150**, builtins product/ACP management composition updated; remaining native compositions and exact-head acceptance pending |

### New finding policy

For each ergonomic issue: attach a source path, why the author needs to write
it, smallest usage-side goal, shared owner, and whether it can be fixed
without changing M1 ABI/authority. Avoid adding bespoke provider/tool macros
where a generic SDK hook works. Do not silently defer defects to ergonomics.
