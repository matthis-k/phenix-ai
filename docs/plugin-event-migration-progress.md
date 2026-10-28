# Phenix event-driven plugin migration — execution log

Updated: 2026-10-25. Working stack: #1144 → #1146 → #1147 → #1148 → #1149.
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
| G0 – Kernel / event SDK | kernel graph, contract graph, SDK, macros, Basic/ACP host | IMPLEMENTED, verification pending | #1147 intrinsic kernel events; #1148 generic in-root emission; #1149 owner-only settlement, typed emit, Basic/ACP begin/shutdown wiring | Formatting, generated workflow, ACP-v2 and cache isolation passing on 09db59f; Nix shards and official ACP SDK still running | Exact-head native product green; cancellation/failure/pending tests; reconcile parents |
| G1 – Provider manager and declarations | providers, provider-openai, provider-builtins, provider-openai-api, provider-chatgpt | PARTIAL / legacy cutover blocked | #1146/#1148/#1149: one shared manager, staged registry, OpenAI declaration hook, onBegin→populate→settled→ready | Unit tests added; native provider population check not yet confirmed green | Migrate other declarations; remove duplicated OAuth/key/catalog implementations after consumers switch |
| G2 – Tool discovery and execution | basic-tools, tool-fixture | NOT STARTED | Current typed pull-based ToolRegistrations and ToolExecute audited | Existing baseline tests only | Declare tools.populate/ready, preserve ToolExecute contract, prove external native contribution |
| G3 – Agent orchestration and sessions | basic-agent, basic-session, advanced-preparer-graph, advanced-model-adapter | NOT STARTED | Existing graph prompt and property watches inventoried (already event/reactive) | Existing baseline tests only | Add semantic turn/session events only with clear independent consumers |
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

### New finding policy

For each ergonomic issue: attach a source path, why the author needs to write
it, smallest usage-side goal, shared owner, and whether it can be fixed
without changing M1 ABI/authority. Avoid adding bespoke provider/tool macros
where a generic SDK hook works. Do not silently defer defects to ergonomics.
