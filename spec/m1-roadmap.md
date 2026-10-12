# Phenix M1 implementation roadmap: contract-first reactive values and DAGs

Status: S1a, S1b, S2, S3a and S3b accepted; G1/G2 shipping CI passed with semantic gaps; G3 partial; G4 pending
Updated: 2026-10-12
Baseline: [clean-start RFC](clean-start-rfc.md), accepted M0.0-M0.4 in PR #771
Normative semantics: [M1 value/reference/DAG RFC](m1-values-reactive-dag-rfc.md)
This file is the single task/evidence tracker for M1. Do not duplicate completion percentages in PR bodies.

## Consolidation intervention

The 12 October decision makes M1 the sole supported product after consolidation.
[The consolidation contract](m1-consolidation-contract.md) supersedes historical
compatibility requirements for this transition. [The verified audit and file
matrix](m1-consolidation-audit.md) record corrections, executable changes and
remaining removal work. M0 acceptance below is historical evidence. C1 and C2
must replace descriptors and their consumers together, while migrating semantic
conformance to M1. S3b pending imports and G1 follow that coordinated removal.
PR #779 implements prepared native/cell effects, shared ordering and concise
stateless handlers. Its exact head `712b9898` passed full CI 38193609248.
The `impl/m1-only-runtime` transition now removes legacy schemas, descriptors,
wire/ABI profiles, macros, synthetic normalization and preparation input forms.
[Its frozen contract](m1-only-runtime-contract.md) owns the coordinated C1/C2
migration. Local M1 native, lifecycle, pending/import, generation, state, SDK,
authoring and agent suites pass. Independent wire, metadata and C ABI gates pass.
Consolidation passed exact-head full Nix CI 38197645494 at `367420e33b`.
S3b completion adds property discovery, sealed pending slots and adversarial
import tests. It passed full CI 38205255204 at `75a255b9`;
[its matrix](m1-contract-authoring-matrix.md) binds the downloaded artifact.
G1-G4 now follow the accepted consolidated kernel. Historical profiles and acceptance claims below describe
their pinned commits, not the active product.

## 1. Branch and PR ownership

PR #771 remains the unchanged M0 implementation and validation baseline. The S0 specification PR #772 is stacked on `rewrite/minimal-kernel` and includes documentation only. The separate implementation PRs below own their executable changes and must not amend M0 acceptance claims or rewrite implementation history.

Each numbered stage below owns one **focused implementation PR** after the specification is accepted. Do not open empty placeholder PRs. Use the agreed base branch and add actual executable changes before opening. Mark implementation PRs ready for review to trigger full CI; keep them unmerged until the requested integration decision. Stage-dependent PRs may be stacked, but their dependency and base branch must be explicit.

| ID | Proposed PR scope | Depends on | Exit gate | Status |
| --- | --- | --- | --- | --- |
| S0 | M1 RFC, this roadmap and M0-to-M1 links | #771 head | Consistent contract and file-level scope | Spec authored; review pending |
| S1a | Portable schema/ref identities and explicit v2 wire profile | S0 | Independent Python/Rust vectors and M0 regression gates | PROVEN at `2682be5f` in CI 38125329024 |
| S1b | Definition-only contracts, property/import metadata and negotiated state ABI | S1a | Metadata-only package and independent C/native ABI vectors | PROVEN at `51017e07` in CI 38128644908 |
| S2 | Kernel-owned ValueRef cells, CAS, subscriptions, transactions | S1b; stacked on #775 | External native state and security fixtures | PROVEN at `6ce818c4`, CI 38170397949; `m1-live-state-matrix.md` |
| S3a | Typed Rust value SDK and static schemas | S2 | Independent vectors, native host fixture and compile failures | PROVEN at `b47f7d95`, CI 38170898637; `m1-value-sdk-matrix.md` |
| S3b | Contract/property macros and generated ABI | S3a | External SDK-only annotated plugin | Accepted at `75a255b9`, CI 38205255204; generated native metadata, ABI 3, typed fields/slots, pending imports and property/security completion proven |
| S4 | Contributed graph descriptor, resolve, validation, identity | S3b | Independent plugin contributions compile deterministically | Refined into G1/G2: shipping CI passed, remaining acceptance cases open |
| S5 | Serial readiness execution and reactive on-change integration | S4 | Correct multi-root DAG partial order and cancellation | Refined into G3/G4: G3 partial, G4 pending |
| S6 | Parallel-ready tasks and multi-admission concurrency | S5 | Independent branches run concurrently, safe instance lifetimes | Not started |
| A0 | Basic synchronous agent plugin preflight | Accepted M0 services; S1b branch baseline | Replaceable model/tool imports and real CLI gate | PROVEN at `4d47e783` in full CI 38134340209; `spec/a0-agent-matrix.md` |
| Basic | Triggered agent, providers, durable sessions and frontends | G4 and Basic providers | Terminal, JSONL and ACP run the same agent | B1-B5, F1-F2 and P1 below; not started |

S1/S2/S3 may refine their Rust API shape based on conformance evidence, but MUST maintain the semantic boundaries in the M1 RFC. S6 is a distinct optimization/capability gate: S5 must implement the correct partial order without relying on parallelism.

## 2. S1: contract and wire freeze

S1a owns the [frozen portable reference wire contract](m1-reference-wire-contract.md), `crates/phenix-contract/src/reference.rs`, `crates/phenix-contract/src/value.rs`, the contract dependency/lockfile updates, `tests/m1-contract-vectors.json`, `tests/m1-wire-negative.py`, and the public-contract probe. `flake.nix` and the conformance workflow own its executable gate. This split proves canonical bytes before live state callbacks can use them.

S1b depends on S1a and owns definition-only metadata, property/import-slot descriptors and the separately negotiated state-host ABI. The table below names the combined S1 scope. Freeze each profile before runtime code.

| File | Required change |
| --- | --- |
| `crates/phenix-contract/src/lib.rs` | Stable `SchemaId`, `ValueId`, ref/error vocabulary and contract role definitions |
| `crates/phenix-contract/src/value.rs` | A version-negotiated reference-capable portable encoding; keep v1 semantics frozen |
| `crates/phenix-contract/src/descriptor/mod.rs` | Definition-only contracts and typed property/import-slot metadata with ownership |
| `crates/phenix-contract/src/descriptor/encode.rs` | Canonical descriptor bytes and deterministic contribution order |
| `crates/phenix-contract/src/abi.rs` | Versioned state-host API extension proposal with explicit length/ownership |
| `include/phenix.h` | Exact C ABI declaration for the negotiated extension |
| `crates/phenix-artifact/src/lib.rs` | Verify versioned metadata-only packages offline, with separate package and descriptor digests |
| `crates/phenix-artifact/src/bin/phenix-describe.rs` | Extract and inspect definition-only metadata without loading executable code |
| `examples/reactive-contracts/` | SDK-only contract definitions and generated offline metadata fixture |
| `tests/m1-contract-vectors.json` (new) | Independent canonical byte and identity fixtures |
| `tests/m1-wire-negative.py` (new) | Malformed, forged, noncanonical and incompatible encoding cases |
| `spec/m1-values-reactive-dag-rfc.md` | Record frozen discriminants and compatibility decision when proven |

Gate: independent decoder/encoder agrees on reference/descriptor vectors; a metadata-only contract package verifies without dlopen; malformed references and duplicate/conflicting property declarations reject. The M0 conformance profile still passes. Do not accept a new ABI merely because a Rust unit test round-trips.

### S1a implementation evidence

The frozen contract commit is `c2365e1199b8bf15184f8e7246f15d853f2fe366`. [PR #773](https://github.com/matthis-k/phenix-ai/pull/773) owns the implementation on `impl/m1-reference-wire`, stacked on `spec/m1-values-reactive-dag`.

S1a is PROVEN at implementation commit `2682be5f1b7293a8a3305d5b09186821b4167305`. [CI run 38125329024](https://github.com/matthis-k/phenix-ai/actions/runs/38125329024) passed all fifteen checks, including the fourteen unchanged M0 gates. The downloaded artifact's `kernel-commit.txt` matches that commit. Its wire report confirms 52 accepted and 148 rejected independent cases; the vector digest matches the frozen fixture. [CI evidence](evidence/m1-reference-wire-ci.json) records the exact Nix outputs and hashes of 27 JSON reports. [Local evidence](evidence/m1-reference-wire-local.json) records source hashes, isolated SDK tests and optimized wire tests.

The first CI attempt stopped on Clippy 1.96 `collapsible_match`. The corrected expression passes the pinned toolchain. Reference payloads use a Box so portable scalar storage stays 32 bytes on the frozen target; location data takes 112 bytes. This changes no wire bytes.

This acceptance covers portable shape, identities and compatibility. S1b owns contract metadata and the state-host ABI. S2 owns authenticated cells and their lifecycle.
| File or task | State | Evidence or remaining gate |
| --- | --- | --- |
| `crates/phenix-contract/src/reference.rs` | PROVEN | Distinct identity types, structural schema hashing, validated `DynValue`; independent schema and mismatch cases |
| `crates/phenix-contract/src/value.rs` | PROVEN | Explicit v2 selection, nested refs, v1 refusal and all reference truncations |
| Contract manifest and both Cargo lockfiles | PROVEN | Pinned SHA-256 dependency; workspace and external examples compile |
| `crates/phenix-contract/src/bin/phenix-value-probe.rs` | PROVEN | Public-contract process, no private kernel dependency |
| `tests/m1-contract-vectors.json` | PROVEN | Fixed wire bytes and schema identities agree with independent Python |
| `tests/m1-wire-negative.py` | PROVEN | 52 accepted and 148 rejected cases |
| `flake.nix`, `.github/workflows/minimal-kernel.yml` | PROVEN | Exact implementation-head CI passed all fifteen checks and uploaded release evidence |
| Owner/cell issuance, live authority and lifecycle | NOT STARTED | S2 owns these; decoded locations grant no access |
| Definition-only descriptors, slots/properties and state-host ABI | PROVEN | S1b accepted at `51017e07`; see independent CI evidence below |

### S1b implementation and acceptance gate

The [frozen S1b profile](m1-metadata-state-contract.md) is commit
`295067bbcc2ea78f13cb26c68351080cacc1d0a5`, created before implementation.
[PR #774](https://github.com/matthis-k/phenix-ai/pull/774) on
`impl/m1-metadata-state` is stacked on PR #773. S1b is PROVEN at implementation
commit `51017e07067bae786131775e18686044b78eb625`. [CI run 38128644908](https://github.com/matthis-k/phenix-ai/actions/runs/38128644908)
passed all sixteen checks, including all fourteen M0 gates and S1a.
The downloaded [artifact 11698043204](https://github.com/matthis-k/phenix-ai/actions/runs/38128644908/artifacts/11698043204)
records that exact commit, 1,544 independent cases (23 accepted, 1,521 rejected),
12 package cases, and native C/Rust interoperation with 17 rejected tables.
[CI evidence](evidence/m1-metadata-state-ci.json) records the realized Nix outputs,
archive digest and hashes of all 30 JSON reports. No live cell, provider
selection, field macro or graph execution is claimed.

Optimized standalone linkage needed ThinLTO. Both initial CI attempts then
exposed Cargo's separate target libraries and host proc-macro libraries. The
corrected search paths reproduce locally with Cargo's explicit target layout;
the final pinned Nix run proves the correction.

| File or task | State | Evidence or remaining gate |
| --- | --- | --- |
| `crates/phenix-contract/src/descriptor/metadata.rs`, `descriptor/mod.rs` | PROVEN | Typed definitions, named cardinality slots and plugin-owned properties; canonical ordering and duplicate rejection |
| `crates/phenix-contract/src/state.rs`, `lib.rs` | PROVEN | Exact state request opcodes, v2 envelopes, sorted batch identities and domain error shapes |
| `crates/phenix-contract/src/abi.rs`, `include/phenix.h` | PROVEN | ABI 3 header-first negotiation, initialization callback, v2 owned replies and unchanged M0 decoding |
| `crates/phenix-artifact/src/lib.rs`, `bin/phenix-describe.rs` | PROVEN | Offline definition-only verification, independent descriptor/package digests, containment and no native artifact |
| `crates/phenix-artifact/src/bin/phenix-metadata-probe.rs` | PROVEN | Process boundary for independent malformed descriptors and state envelopes |
| `examples/reactive-contracts/`, examples manifest/lockfile | PROVEN | Public SDK only; reversed authored declaration order produces canonical Python bytes |
| `tests/m1-metadata-vectors.json`, `tests/m1-metadata-negative.py` | PROVEN | Frozen independent vectors, metadata/schema/cardinality/property negatives, all truncations, state requests/errors and 12 package cases |
| `tests/m1-state-abi.c`, `tests/m1-state-abi.rs` | PROVEN | C producer/Rust host interoperate; 4 host callbacks, 10 C/3 Rust releases, 17 rejected tables and incompatible reply bodies |
| `flake.nix`, conformance workflow | PROVEN | All sixteen gates passed in CI 38128644908; both M1 products uploaded |
| `descriptor/encode.rs` | UNCHANGED | Frozen const v1 encoder stays intact; canonical M1 encoding belongs to the owned metadata module; macro lowering is S3 |
| Native metadata packages and runtime admission | NOT STARTED | S2 integrates ABI 3 with the existing native loader/resolver; this gate accepts offline definition packages only |
| `spec/evidence/m1-metadata-state-local.json` | PROVEN | Source hashes and debug, optimized and explicit-target layout reports |
| `spec/evidence/m1-metadata-state-ci.json` | PROVEN | Exact commit and run, independent reports, realized Nix outputs and 30 report hashes |

## 3. S2: kernel-owned live cells

Owner: one S2 implementation PR, no reactive DAG. Native package verification,
ABI 3 admission and selected slot normalization are prerequisites for its external
state fixtures and belong to S2. S4 extends that same prepared representation
with graph contributions; it must not introduce another resolver. The S2
addendum must freeze initialization outcomes, state grant/resource mapping,
owner incarnation issuance and subscription/ticket ownership before mechanisms.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/value_state.rs` (new) | Cell ownership, revision, snapshot, CAS and atomic multi-cell commit |
| `crates/phenix-artifact/src/lib.rs` | Verify native M1 packages and bind canonical metadata to exact executable bytes before loading |
| `crates/phenix-kernel/src/native.rs` | Negotiate ABI 3 in the existing loader; create then initialize before admission, preserving instance/stop ownership |
| `crates/phenix-kernel/src/resolve.rs` | Normalize M1 definitions and selected named slots into the existing resolver/dispatcher; no parallel metadata resolver |
| `crates/phenix-conformance/src/bin/phenix-m1-state.rs` (new) | Independent native state effects, denied-effect counters, transaction and retirement traces |
| `crates/phenix-kernel/src/lib.rs` | Export narrowly scoped trusted APIs, not unfenced raw cell access |
| `crates/phenix-kernel/src/runtime.rs` | Reuse central dispatcher for read/write/watch and authenticated caller scopes |
| `crates/phenix-kernel/src/pending.rs` | Subscription tickets, cancellation, coalesced notification cursors and leases |
| `crates/phenix-contract/src/scope.rs` | State resource restrictions if operation/resource grants cannot express them |
| `crates/phenix-kernel/src/generation.rs` | Owner-incarnation and retirement behavior for live refs |
| `examples/reactive-provider/` (new) | Independently compiled provider with typed cells |
| `examples/reactive-client/` (new) | Independent authorized/unauthorized accessor and subscriber |

Gate: two refs clone the same identity; old snapshots remain unchanged; racing CAS commits produce one winner; atomic read+subscribe has no lost update; batch publish is all-or-nothing; stale owner and forged ref reject; denying write yields zero provider effects; abandoned subscriptions release leases. Run M0 tests unchanged.

### S2 implementation gate

The frozen live profile is commit `80a40cc86f41d3e3c464685699251e861e4f13e8`.
The consistent epoch snapshot import was frozen separately at
`0690c37d578782fe6b865235132f6bb29938e797` before its mechanism.
`impl/m1-live-state` is based on the accepted A0 head so the basic loop gates stay
in every regression run; its functional prerequisite remains accepted S1b.
The [S2 matrix](m1-live-state-matrix.md) owns file-level evidence and remaining
acceptance. Native ABI 3 callbacks use the same loader, operation/resource
policy check, resolver, generation manager and pending tickets as M0. The
[Basic parity checklist](basic-agent-parity.md) records the original group target.

## 4. S3: Rust SDK, macros and ergonomic plugin state

S3 is split before implementation: S3a owns typed values, static schemas and
SDK state access; S3b owns generated contract/property authoring and ABI glue.
The dependency and file ownership are frozen in `m1-value-sdk-contract.md`.
Neither split changes the S4/S5 order or the complete S3 acceptance gate.

| File | Required change |
| --- | --- |
| `crates/phenix-sdk/src/lib.rs` | Typed `Value<T>`, `ValueRef<T>`, `Snapshot<T>`, `Edit<T>` and async/ref host methods |
| `crates/phenix-macros/src/lib.rs` | Canonical `#[plugin]`, `#[contract]`, `define/require/provide` and field-helper parsing |
| `crates/phenix-contract/src/descriptor/mod.rs` | Only schema changes required by actual generated contracts; no duplicate registration |
| `examples/reactive-contracts/` (new) | Public API crate with generated property symbols |
| `examples/reactive-provider/` | `#[get]`, `#[set]`, `#[watch]` on authored plain Rust fields |
| `examples/reactive-client/` | `Value<T>` trait ergonomics, remote refs and denied field access |

Gate: `Value<T>: Clone + Deref + AsRef<T> + From<T>` where bounds allow; `ValueRef<T>: Clone`, but no `Deref` or `DerefMut`; `Edit<T>` supports ordinary `Vec::push` and explicit, fallible commit. Macro-generated state storage has no silent direct write path. Standalone SDK builds without private kernel sources. Compile-fail fixtures check duplicate exported IDs, invalid field types, mismatched contracts, undeclared imports and forbidden mutable ref deref. Definition-only plugins need no state or export.

## 5. S4: multi-source graph preparation

Owner: one S4 implementation PR. Graphs remain inert until S5.

| File | Required change |
| --- | --- |
| `crates/phenix-contract/src/descriptor/mod.rs` | Graph node, edge, trigger, join/failure declaration schemas |
| `crates/phenix-kernel/src/graph.rs` (new) | Composition, activation-independent DAG validation, stable node IDs and conflict reports |
| `crates/phenix-kernel/src/resolve.rs` | Resolve graph service imports through the existing selected binding map |
| `crates/phenix-kernel/src/generation.rs` | Include canonical graph inputs, bindings and policy in generation identity |
| `crates/phenix-macros/src/lib.rs` | Generate static hook/edge contributions from `#[on_change]` and `#[after]` |
| `tests/m1-graph-vectors.json` (new) | Permutation-invariant deterministic graph identities |
| `examples/graph-search/`, `examples/graph-context/` (new) | Contributions from independent plugin artifacts |

Gate: `A -> C <- B` and disconnected `A -> C, B -> D` compose identically regardless of contribution enumeration; duplicates, stale anchors, ambiguous bindings, cycles, unauthorized edges, conflicting writes, and undeclared operations fail before graph activation. Dependency order is validated separately from plugin start/stop order. No plugin callback runs during graph resolution.

## 6. S5: serial DAG execution and reactive propagation

Owner: one S5 implementation PR. Reuse one kernel runtime and central dispatcher.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/graph.rs` | Compute affected/activated set and per-epoch predecessor counters |
| `crates/phenix-kernel/src/runtime.rs` | Execute ready nodes using the existing authorized dispatcher under one admitted root |
| `crates/phenix-kernel/src/pending.rs` | Associate pending tickets with nodes, failure/cancellation, and join settlement |
| `crates/phenix-kernel/src/value_state.rs` | Publish committed epoch notifications into selected graph executions |
| `crates/phenix-sdk/src/lib.rs` | Typed change context and on-change invocation access |
| `examples/graph-search/`, `examples/graph-context/` | Executable hook/consumer end-to-end tests |
| `tests/m1-graph-order.py` (new) | External trace assertions for ordering and dormant predecessor behavior |

Gate: A and B complete before C if both active; no global barrier for `A->C`, `B->D`; only A active does not wait for B; `after` alone does not trigger a node; transactional updates trigger one epoch; dependent failure never masquerades as successful completion; cancellation and retirement observe existing leases; no duplicate executor. Serial topological order is an allowed implementation, not a cross-branch promise.

## 7. S6: concurrency and independent branches

Owner: one S6 implementation PR. Do not fold the runtime ownership refactor into S5.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/runtime.rs` | Replace serial root/thread constraints with synchronized admission and execution state |
| `crates/phenix-kernel/src/pending.rs` | Wake/cancel/ticket ownership safe under multiple concurrent tasks |
| `crates/phenix-kernel/src/native.rs` | Explicit plugin-instance concurrency/reentrancy policy and lifetime safety |
| `crates/phenix-kernel/src/generation.rs` | Concurrent root pins, promotion/rollback, safe physical retirement |
| `crates/phenix-contract/src/abi.rs`, `include/phenix.h` | Versioned cross-thread call semantics as required; do not repurpose M0 ABI |
| `tests/m1-concurrency.py` (new) | Independent overlap, join ordering, cancellation and old-generation residency fixtures |

Gate: independent A and B execute concurrently when both providers advertise safe concurrency; C waits for both; unrelated `A->C` and `B->D` branches do not serialize due to a global root lock. Stateful serial-only providers remain serialized, with explicit diagnostics. Concurrency never bypasses root-scope authentication, cancellation, resource grants, or physical leases. Noncooperative trusted native work still has no forced-termination guarantee.

## 8. Working Basic agent roadmap

The 11 October Basic roadmap replaces A1a/A1b's direct-call product plan.
A0 remains an accepted synchronous model-tool-model fixture. It does not prove
triggered execution, full history, durable sessions or frontend parity.

Frontends submit typed events. Nodes declare triggers. The kernel admits events,
resolves the selected graph and invokes nodes through its existing dispatcher.
Frontends observe ordered output events. Product startup never calls a special
`Agent::run()` entry. The kernel owns authority, generation selection, readiness,
cancellation and native lifetimes. Plugins own agent and frontend semantics.

Status is an acceptance checklist: `[x]` means proven on an exact implementation head; `[ ]` means further acceptance work. Passing CI is necessary but does not alone close a semantic gate.

| Stage | Depends on | Concrete ownership | Acceptance | State |
| --- | --- | --- | --- | --- |
| S3b | S3a | `phenix-macros/src/`, `phenix-sdk/src/live.rs`, `phenix-contract/src/descriptor/`, `phenix-artifact/src/lib.rs`, `phenix-kernel/src/native.rs`, `pending.rs`, `runtime.rs`, external examples | SDK annotations generate complete native metadata, fields, typed imports and ABI 3 hooks; independently built provider/decorator/field-only/pending caller | [x] PROVEN at `75a255b9`, CI 38205255204 |
| G1 | S3b | Contract descriptors, macros, SDK trigger contexts, narrow graph inspection API | Offline typed event/trigger declarations and discoverable action IDs, schemas, descriptions and required authority; duplicate IDs, invalid schemas and undeclared handlers reject | [ ] IMPLEMENTED, NOT ACCEPTED (#782): shipping CI 38220130161 passed at `1615ed52`; definition-only contributors and negative semantic gates remain |
| G2 | G1 | New `phenix-kernel/src/graph.rs`, existing `resolve.rs`, `generation.rs`, contract descriptors | Static multi-source multi-root DAG; discovery-order-independent graph; cycles, missing anchors, ambiguous bindings, denied contributions and conflicting writes reject before activation; preparation executes no plugin code | [ ] IMPLEMENTED, NOT ACCEPTED (#782): native prepared graph and shipping CI passed; remaining schema/write and definition-only contribution gates |
| G3 | G2 | `graph.rs`, existing `runtime.rs`, `pending.rs`, SDK trigger contexts | Authenticated event activates two external nodes in declared order and a third reads the resulting state; frontend never invokes handlers directly | [ ] PARTIAL (#783): A/B commit typed properties, C reads both, scoped admission and fail-root cases added at `a838de05`; CI 38223182503 and output/failure semantics remain |
| G4 | G3 | Existing tickets, cancellation, leases and live values through one same-root control pump | Slow pending model emits progress; cancel event stops its child before tool effects; retained physical leases drain safely; committed value changes use the same DAG | [ ] NOT STARTED |
| B1 | G1 | New `phenix-basic-contracts/` | Independently compiled Model, ToolCatalog, ToolExecute, Skills, Context, Session, EventJournal, Storage, AgentInput and AgentOutput contracts; no kernel agent dependency | [ ] NOT STARTED |
| B2 | B1 | New `phenix-plugin-basic-session/`, `phenix-plugin-event-journal/`, `phenix-plugin-storage-sqlite/` | Restart restores history, call IDs/results, checkpoints and observer offsets; two observers replay independently; cross-session access rejects; interrupted non-idempotent tools remain indeterminate until reconciled | [ ] NOT STARTED |
| B3 | B1, B2 | New Basic tools, skills, context, executor and optional workspace-backed shell plugins | Selectable independent tools; denied execution has no effect; query-bound pagination and revisions reject stale cursors; skills/context persist; catalog discovery grants no execution permission | [ ] NOT STARTED |
| B4 | B1 | New `phenix-plugin-model-scripted/`, `phenix-plugin-model-http/` | Same portable contract carries complete history, text, structured calls, usage and failures; local HTTP fixture verifies streaming, malformed replies, provider errors and cancellation | [ ] NOT STARTED |
| B5 | G4, B2-B4 | New `phenix-plugin-basic-agent/` | Prompt trigger starts a multi-turn run with complete history, multiple call IDs, skill/context use, tool-error recovery, usage, partial output, session continuation and cancellation; limits are caller-selected | [ ] NOT STARTED |
| F1 | G3, B1 | Terminal and JSONL frontend plugins | Input decoding submits discovered actions; observers render progress and completion without agent-specific kernel entry points | [ ] NOT STARTED |
| F2 | F1 contracts | ACP frontend plugin and protocol integration fixtures | ACP drives the same input/output contracts and passes session, progress and cancellation integration tests | [ ] NOT STARTED |
| P1 | B5, F1, F2 | Basic package composition, executable product fixtures and release evidence | Start terminal, submit prompt, discover/execute tools across turns, stream answer, restart/continue session; JSONL and ACP use unchanged agent/kernel logic | [ ] NOT STARTED |
| K1 | S3b | Kernel generation and prepared composition APIs in `phenix-kernel` | Authorized stage/load/replace/unload/trial/promote/retire of verified plugins; preserve grants, active roots, rollback and leases; builds stay outside kernel | [ ] PLANNED: generation lifecycle exists, management API incomplete |
| K2 | K1, G2 | Kernel desired-state diff/preview/reconciliation using existing resolver, DAG compiler and generation manager | No-effect preview; deterministic diff; atomic promotion or rollback; unchanged roots and safe draining; no second registry or dispatcher | [ ] PLANNED: generation transition exists, desired-state reconciliation absent |
| K3 | G3, G4 | Kernel diagnostic event emission and authenticated internal inspection API | Structured execution/lifecycle/state/denial facts with correlation; authorized consistent snapshots; dropped diagnostic events detectable; no consumer affects execution | [ ] PLANNED: basic inspect methods exist, diagnostic emitter missing |
| L1 | K3 | `phenix-plugin-logging` | Independent optional consumer of kernel diagnostics; logging formats, filtering, sinks, export and retention owned by plugin; zero logging code in kernel | [ ] NOT STARTED |
| I1 | K3 | `phenix-plugin-introspection` | Independent optional typed, discoverable contract projection over authorized kernel query APIs; no direct unrestricted host handles, no dependency on L1 | [ ] NOT STARTED |

G1-G4 refine S4/S5 using one resolver and dispatcher. S6 remains outside the Basic critical path. B1 can proceed after G1, F1 after G3, and P1 requires G4. K1-K3 and L1/I1 are independent operations milestones, not Basic product blockers.

G1/G2: [PR #782](https://github.com/matthis-k/phenix-ai/pull/782) owns typed graph declarations, ELF verification and the prepared multi-root DAG. Full exact-head shipping CI 38220130161 passed at `1615ed52`, but definition-only and negative semantic evidence remain before acceptance. [Its contract](m1-graph-declarations.md) owns the gates.

G3: [PR #783](https://github.com/matthis-k/phenix-ai/pull/783) now includes typed scoped admission and a native state-backed join: A/B commit independent properties and C reads both. Exact-head CI for `a838de05` is run 38223182503. G3 still needs verified output observation, failure policy and acceptance evidence.

### Event admission and graph rules

The typed envelope identifies the event, contract/schema, payload, resource,
causation/correlation and admitted generation. Authority derives from the
host/root admission, never from an event's self-claimed origin. Discovery exposes
the prepared graph's action IDs, schemas, descriptions and authorization needs.
Freeze this contract before adding G1 code or changing the native wire format.

An `after` edge constrains ordering; it does not trigger its successor.
If A and B activate, C waits for both active predecessors. If only A activates,
inactive B cannot block C. Disconnected roots remain valid. Follow-up events
advance model turns in new epochs, so the static graph stays acyclic.

The journal retains every accepted semantic event in order. Value watches can
coalesce and cannot replace journal cursors. During pending I/O, one same-root,
non-reentrant control pump admits cancellation and publishes progress.

### Basic product boundaries

The agent imports prepared selected providers. Model requests contain previous
assistant calls and every tool result. Session/storage plugins retain history,
checkpoints and run status. A crash during an external tool effect preserves an
indeterminate result rather than blindly repeating the call.

A native shell plugin needs a selected workspace execution provider. Kernel
import mediation alone does not implement an OS sandbox. The product reports
only the isolation its selected provider actually supplies. Model endpoint,
model identity and credentials are configured outside CI. Deterministic scripted
and local HTTP fixtures exercise the same contracts used by a live provider.

The final product gate covers start, progress, finish, failure and cancellation;
multi-turn history; multiple tools; typed tool-error recovery; loaded skills and
context; replaceable providers; usage; streaming; durable session continuation;
caller-selected limits; and denied imports with zero tool effects. Planning,
compaction and MCP remain later plugin work.

## 9. Kernel operations and independent plugins

These stages add kernel management and observation without turning the kernel
into a logger, inspection contract provider, storage backend or agent runtime.
Their only acceptance checklist is the stage table above.

### K1-K3: kernel-owned mechanisms

**K1: dynamic plugin management.** Reuse `Prepared`, `Generations`,
the verifier, root admission and retirement. The kernel accepts authorized
composition changes and verified artifacts, stages candidates, trials, promotes,
rolls back and drains removed instances. Plugin compilation, artifact fetching,
CLI/UI policy and build tools remain external. Plugin callers cannot grant
their own management authority.

**K2: desired-state reconciliation.** Diff the desired composition against
the active prepared generation. Preview without effects. Apply through the same
resolver, graph compiler, authority checks and generation lifecycle. Retain
already admitted roots and physical leases during replacement. No separate
plugin registry, resolver, reconciliation executor or task manager.

**K3: diagnostic events and raw inspection.** The kernel emits versioned
structured facts about dispatch, lifecycle, state, scheduling, authorization
and generations. Include stable correlation, root, call, plugin and generation
identities where available. These are *diagnostic* events, separate from the
triggered application events in G1-G4 and from B2's durable semantic event
journal. Emission uses bounded, non-reentrant delivery; absent, slow or
failing consumers do not affect execution. Report overflow and gaps rather
than promising lossless diagnostic history.

The kernel owns a narrow, authenticated host query API. It returns consistent
snapshots of prepared bindings, selected graphs, live values, calls, tickets,
generations and authority-filtered metadata. Read access does not mutate state.
Callers receive only information authorized by their root/resource scopes.
The API is an internal kernel boundary, not a public unchecked ABI.

### L1/I1: independent optional plugins

**L1: logging plugin.** `phenix-plugin-logging` consumes diagnostic events.
It owns filtering, formatting, sinks, retention and optional file or telemetry
export. The kernel holds no log store, formatter, sink configuration or tracing
backend. The plugin can be absent or replaced without changing kernel outcomes.

**I1: introspection plugin.** `phenix-plugin-introspection` calls authorized
kernel query APIs, then projects selected results through ordinary versioned,
typed Phenix contracts. It owns contract schemas, pagination, presentation and
consumer-specific redaction. Agents and frontends receive only separately
granted read operations, not a raw kernel handle. It queries the current kernel
state rather than reconstructing it from logs.

L1 and I1 each depend on K3, never on one another. Introspection is not a
logging plugin; logging is not a contract API. B2's event journal remains the
durable semantic replay mechanism for sessions, not a diagnostic sink.

### Deferred plugin-owned work

Durable storage and migrations belong to storage plugins, building on B2
where appropriate. Remote or external language runtimes and OS isolation
providers belong to plugins. Kernel authority must apply to all their effects;
none may expand its own grants. None introduces a second kernel executor.

### Acceptance evidence

K1/K2 require denied-management, unchanged-root, rejected-candidate,
no-effect-preview, rollback and pending-retirement fixtures. K3 requires
no-consumer and slow-consumer equivalence, explicit gap reporting, consistent
snapshots and denied-read tests. L1 and I1 must build independently against
the public SDK. Removing either leaves the other and all kernel tests
unchanged. Restricted introspection grants must not expose secret state
or plugin-management authority.

## 10. Evidence, closure, and PR discipline

Each stage updates only its own source files, fixtures, the relevant RFC addendum, and this matrix. The owning PR body records exact head, targeted acceptance tests, unresolved blockers, and relation to the previous stage. Mark tasks `PROVEN` only with executable evidence from the exact head; use `NOT STARTED`, `IMPLEMENTED`, `TESTED`, and `PROVEN` consistently. A stage cannot claim acceptance based on green CI alone or unit tests that mirror declarations.

Run focused tests while implementing, full `nix flake check -L` and release evidence before marking a stage accepted. Verify M0 tests, external independent artifacts, denied-effect counters, negative ABI cases, state/ref lifecycle, and canonical identity vectors. Nix builds remain declarative and CI read-only.

Rules for restructuring stages: record a concrete dependency and file ownership before creating another PR. Do not reintroduce the old kernel, a second scheduler, temporary compatibility paths, or agent code into the kernel. Remove an obsolete code path only after its replacement has independent executable proof.


S3b host support is scoped by `m1-contract-authoring-contract.md`: the existing
artifact extractor owns the bounded M1 section; normalization owns declared
maximum delegation; pending owns versioned command envelopes; runtime owns
cell-only control dispatch. These are generated-authoring prerequisites and
use existing boundaries, not new managers.
