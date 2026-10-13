# Phenix M1 implementation roadmap: contract-first reactive values and DAGs

Status: S1a, S1b, S2, S3a and S3b accepted; G1/G2 accepted at fbe55d3d; G3 serial CI passed; G4 pending
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
| S5 | Serial readiness execution and reactive on-change integration | S4 | Correct multi-root DAG partial order and cancellation | G3/G4 slices implemented; committed-state triggers and full reactive acceptance remain |
| S6 | Parallel-ready tasks and multi-admission concurrency | S5 | Independent branches run concurrently, safe instance lifetimes | Not started |
| A0 | Basic synchronous agent plugin preflight | Accepted M0 services; S1b branch baseline | Replaceable model/tool imports and real CLI gate | PROVEN at `4d47e783` in full CI 38134340209; `spec/a0-agent-matrix.md` |
| Basic | Triggered agent, providers, durable sessions and frontends | G4 and Basic providers | Terminal, JSONL and ACP run the same agent | Partial implementation; B1-B5, F1-F2 and P1 below own evidence and remaining gates |

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
| G1 | S3b | Contract descriptors, macros, SDK trigger contexts, narrow graph inspection API | Offline typed event/trigger declarations and discoverable action IDs, schemas, descriptions and required authority; duplicate IDs, invalid schemas and undeclared handlers reject | [x] PROVEN at `fbe55d3d`, full CI 38231650549; definition-only, owner/schema/write and native active/dormant cases |
| G2 | G1 | New `phenix-kernel/src/graph.rs`, existing `resolve.rs`, `generation.rs`, contract descriptors | Static multi-source multi-root DAG; discovery-order-independent graph; cycles, missing anchors, ambiguous bindings, denied contributions and conflicting writes reject before activation; preparation executes no plugin code | [x] PROVEN at `fbe55d3d`, full CI 38231650549; definition-only, owner/schema/write and native active/dormant cases |
| G3 | G2 | `graph.rs`, existing `runtime.rs`, `pending.rs`, SDK trigger contexts | Authenticated event activates two external nodes in declared order and a third reads the resulting state; frontend never invokes handlers directly | [ ] TESTED LOCALLY: follow-up on #785 fixes resource grants, implements Skip and transitive suppression, observes correlated serial results, and bounds delegation by root resource grants; serial shipping CI 38228548634 passed at `672b7f09`; definition/native dormant CI 38231650549 passed; generation-aware admission context remains |
| G4 | G3 | Existing tickets, cancellation, leases and live values through one same-root control pump | Slow pending model emits progress; cancel event stops its child before tool effects; retained physical leases drain safely; committed value changes use the same DAG | [ ] PARTIAL: same-root pending graph coordination and cancellation tested locally on impl/m1-pending-graph; live state triggers and full shipping evidence remain |
| CI0 | Cross-cutting infrastructure; not a Basic dependency | Existing Nix checks, GitHub Actions shard orchestration and cache | Matrix must run all affected checks with exact-head evidence; full release/shipping parity remains mandatory before replacing serial acceptance | [ ] BENCHMARKED on #790: four read-only-cache shards passed; 11m10s matrix vs 25m18s serial during the same PR revision; 878 MB cache shared within the run. Full shipping and persistent cross-run fixture reuse pending |
| AI-CI1 | CI0; parallel with Basic | `flake.nix`, compatible example and Basic plugin builds | Share Cargo compilation across compatible fixtures, package each plugin independently, narrow Nix source sets; preserve isolated ABI, LTO/linker and malicious fixtures | [ ] PLANNED: same-head before/after benchmark; one fixture edit must not rebuild unrelated fixtures |
| AI-CI2 | AI-CI1 | Phenix AI Rust/Nix test profiles | Provide fast semantic checks and separate optimized shipping proofs without weakening native ABI, negative/security or production closure tests | [ ] PLANNED: compare exact check coverage, artifacts and failure cases for both profiles |
| FC1 | CI0 | `phenix-flake-ci` opt-in Nix check discovery | Derive exact-head check inventory and plan manifest from `checks.<system>`; new checks cannot be omitted by stale YAML | [ ] DESIGNED in [phenix-flake-ci #17](https://github.com/matthis-k/phenix-flake-ci/pull/17); implementation pending |
| FC2 | FC1 | `phenix-flake-ci` checked Nix source-impact planner | Extend existing Cargo reverse-dependency rules with provable Nix input impact; unknown or shared changes fall back to full checks | [ ] DESIGNED in #17; implementation pending |
| FC3 | FC1 | `phenix-flake-ci` check grouping, DAG and evidence gate | Schedule shared Nix outputs, shard without new compiles, account for every check and validate exact-head shipping evidence | [ ] DESIGNED in #17; implementation pending |
| FC4 | FC1; may parallelize FC2/FC3 | `phenix-flake-ci` trusted cache policy and CI metrics | PRs restore verified Nix outputs read-only; validated trusted runs publish cache; report wall time, build counts and transfer costs | [ ] DESIGNED in #17; implementation pending |
| AI-CI3 | AI-CI1, AI-CI2, FC1, FC3 | Phenix AI generated workflow migration | Adopt the Nix-aware planner; prove full check and shipping-evidence parity on the same head; retire redundant serial workflow | [ ] PLANNED: existing serial shipping gate remains authoritative until parity |
| AI-CI4 | AI-CI3, FC2; FC4 for cache rollout | Phenix AI selective PR checks and shared cache | Small edits run only proven affected checks; main/release/manual-full run complete inventory and shipping | [ ] PLANNED: cold/warm timing and unaffected-check evidence required |
| B1 | G1 | New `phenix-basic-contracts/` | Independently compiled Model, ToolCatalog, ToolExecute, Skills, Context, Session, EventJournal, Storage, AgentInput and AgentOutput contracts; no kernel agent dependency | [ ] IMPLEMENTED on impl/m1-basic-contracts: eleven typed public contracts and an independently built definition package; exact-head acceptance pending |
| B2 | B1 | New `phenix-plugin-basic-session/`, `phenix-plugin-event-journal/`, `phenix-plugin-storage-sqlite/` | Restart restores history, call IDs/results, checkpoints and observer offsets; two observers replay independently; cross-session access rejects; interrupted non-idempotent tools remain indeterminate until reconciled | [ ] PARTIAL: observer checkpoint slice proven at `f235d68f` by full conformance 38321349932 and matrix 38321349890. Storage migrations and remaining transaction fault cases stay open |
| B3 | B1, B2 | New Basic tools, skills, context, executor and optional workspace-backed shell plugins | Selectable independent tools; denied execution has no effect; query-bound pagination and revisions reject stale cursors; skills/context persist; catalog discovery grants no execution permission | [ ] IMPLEMENTING: typed native ToolCatalog/ToolExecute and a new Skills/Context plugin with selected Storage CAS, session checks and two-process SQLite restart fixture on impl/m1-basic-skills-context. Exact-head CI and complete B2 integration remain open |
| B4 | B1 | New `phenix-plugin-model-scripted/`, `phenix-plugin-model-http/` | Same portable contract carries complete history, text, structured calls, usage and failures; local HTTP fixture verifies streaming, malformed replies, provider errors and cancellation | [ ] IMPLEMENTING: scripted reference and pending HTTP/HTTPS provider with live journal deltas, typed usage, function calls, limits and draining on impl/m1-model-http. Exact-head CI and pending Basic-agent integration remain |
| B5 | G4, B2-B4 | New `phenix-plugin-basic-agent/` | Prompt trigger starts a multi-turn run with complete history, multiple call IDs, skill/context use, tool-error recovery, usage, partial output, session continuation and cancellation; limits are caller-selected | [ ] IMPLEMENTING on impl/m1-basic-agent: independently packaged BasicInput/BasicOutput plugin with prompt graph trigger, selected providers, persistent checkpoints, journal events, multiple tool calls and caller limits. Pending model streaming, cancellation, full continuation and CI acceptance remain |
| F1 | G3, B1 | Terminal and JSONL frontend plugins | Input decoding submits discovered actions; observers render progress and completion without agent-specific kernel entry points | [ ] IMPLEMENTING: independently packaged terminal and JSONL Frontend adapters from shared Rust codecs; native decode/render/session checks on impl/m1-basic-frontends. Live input/output control pump and cancellation remain |
| F2 | F1 contracts | ACP frontend plugin and protocol integration fixtures | ACP drives the same input/output contracts and passes session, progress and cancellation integration tests | [ ] PARTIAL: ACP v1 Frontend codec and multi-session stdio host on impl/m1-basic-acp; initialize/new/prompt/update smoke is defined. Nonblocking ACP progress and cancellation implemented on `impl/m1-p1-live-acp-controller`, with independent HTTP acceptance pending CI; the ACP protocol boundary now handles malformed idle frames and v1 fallback negotiation on `impl/m1-p1-acp-protocol-boundary`, pending CI. Official ACP SDK client check implementing on `impl/m1-p1-acp-sdk-interop`; exact-head verification remains |
| R1 | Cross-cutting; before P1 acceptance | Audit common infrastructure in plugins and hosts; select maintained Rust libraries or document narrow exceptions | Use established protocol, transport and database libraries; remove avoidable hand-written SQLite bindings and ACP protocol code; keep Phenix-specific semantics and ABI guarantees | [ ] IMPLEMENTING: SQLite Storage migration to `rusqlite` accepted on #815 after exact-head canonical, matrix and DAG checks. ACP SDK client gate is implementing on `impl/m1-p1-acp-sdk-interop`; SSE framing migration to a bounded sans-I/O parser is implementing on `impl/m1-r1-standard-sse-parser`; locked dependency and exact-head CI remain. Full ACP SDK-based codec migration remains. |
| P1 | B5, F1, F2, R1 | Basic package composition, executable product fixtures and release evidence | Start terminal, submit prompt, discover/execute tools across turns, stream answer, restart/continue session; JSONL and ACP use unchanged agent/kernel logic | [ ] PARTIAL: Basic agent, live Terminal/JSONL/ACP streams and cancellation, durable sessions and named session selection implemented on #816, #818, #819 and #820. Full shipping CI passed through #819; later exact heads and independent official ACP SDK interop passed once; locked exact-head CI remains under test. Fenced crash recovery and remaining R1 review remain open. |
| K1 | S3b | Kernel generation and prepared composition APIs in `phenix-kernel` | Authorized stage/load/replace/unload/trial/promote/retire of verified plugins; preserve grants, active roots, rollback and leases; builds stay outside kernel | [ ] PLANNED: generation lifecycle exists, management API incomplete |
| K2 | K1, G2 | Kernel desired-state diff/preview/reconciliation using existing resolver, DAG compiler and generation manager | No-effect preview; deterministic diff; atomic promotion or rollback; unchanged roots and safe draining; no second registry or dispatcher | [ ] PLANNED: generation transition exists, desired-state reconciliation absent |
| K3 | G3, G4 | Kernel diagnostic event emission and authenticated internal inspection API | Structured execution/lifecycle/state/denial facts with correlation; authorized consistent snapshots; dropped diagnostic events detectable; no consumer affects execution | [ ] PLANNED: basic inspect methods exist, diagnostic emitter missing |
| L1 | K3 | `phenix-plugin-logging` | Independent optional consumer of kernel diagnostics; logging formats, filtering, sinks, export and retention owned by plugin; zero logging code in kernel | [ ] NOT STARTED |
| I1 | K3 | `phenix-plugin-introspection` | Independent optional typed, discoverable contract projection over authorized kernel query APIs; no direct unrestricted host handles, no dependency on L1 | [ ] NOT STARTED |

G1-G4 refine S4/S5 using one resolver and dispatcher. S6 remains outside the Basic critical path. B1 can proceed after G1, F1 after G3, and P1 requires G4. K1-K3 and L1/I1 are independent operations milestones, not Basic product blockers.

### Library and protocol reuse policy (R1)

This rule applies across Phenix, including future plugins and frontends. Prefer a
well-maintained, widely used Rust library for standard protocols, database
drivers, transports, serialization, async I/O and common utilities. Optimize
for less maintained code and fewer custom failure modes, not the fewest crates.
Use `std` where it already solves the problem cleanly.

A PR that adds custom code for a standard mechanism must first compare
established libraries. Record a short exception when none fits, covering the
missing capability, safety and maintenance costs, and compatibility evidence.
Keep domain-specific contracts, resource checks, journal ordering, graph
semantics, and the frozen Phenix value/ABI encoding in Phenix. Existing custom
ABI code still requires independent conformance tests. A standard library does
not replace Phenix's security checks.

| Area | Default choice to evaluate | Phenix ownership |
| --- | --- | --- |
| HTTP model transport, TLS and connection pooling | Established clients such as `curl` or `reqwest`; an async runtime such as `tokio` only when needed | Model request/response mapping, limits, authorization, cancellation and usage |
| Streaming HTTP/SSE | A maintained event-stream parser integrated with the HTTP client | Typed model progress, ticket lifecycle and journal events |
| SQLite bindings and transactions | `rusqlite` instead of hand-written `sqlite3_*` FFI | Storage keys, revision-checked CAS and session isolation |
| ACP and JSON-RPC transport | Official `agent-client-protocol` Rust SDK if compatible with the required ACP version | ACP-to-typed-event mapping, session grants and cancellation admission |
| JSON and shared encoding utilities | Existing `serde` / `serde_json`; `std` for ordinary conversions | Canonical Phenix wire bytes and declared contract schemas |
| IDs, time, URI handling, retries and other common functions | Established, scoped crates where they remove meaningful code and edge cases | Domain policy and explicit caller-selected behavior |

[The concrete R1 library audit](m1-r1-library-audit.md) records the first migration
and open transport/protocol reviews. The table above remains the stage tracker.

R1 work, before P1 acceptance:

1. Inventory hand-written generic mechanisms in the current kernel, SDK,
   plugins and frontend host. Record the chosen dependency or justified
   exception for each relevant P1 implementation; audit new features during PR
   review so the list does not grow stale.
2. Replace the SQLite plugin's manual C binding layer with a maintained wrapper,
   unless a measured, documented constraint rules it out. Preserve the current
   inter-process CAS, rollback, restart, denial and corruption tests.
3. Review the HTTP Model provider in #803, which already uses the `curl` crate
   for HTTP/TLS. Evaluate its custom SSE framing against maintained parsers;
   retain custom code only with a documented compatibility reason. Verify
   fragmented streams, malformed replies, cancellation, timeouts and errors.
4. Evaluate the official ACP Rust SDK for the frontend and host. Use it for
   supported protocol framing, types and transport. Document any compatibility
   exception, then test against an independent ACP client.
5. Keep external I/O and protocol dependencies in the owning plugins or hosts,
   not in the generic kernel. Check lockfile, license, security, Nix build and
   independent ABI/security tests before closing R1.

R1 is a P1 acceptance dependency and a review rule for later work. It is not
a new runtime layer or a reason to replace proven custom Phenix semantics.

### CI performance and flake-aware planning

CI0 is the measured parallel matrix baseline, not completion of CI optimization.
At the 12 October exact-head comparison, the matrix took 11m10s versus
25m18s for the serial job. Later successful matrices took 9m33s to 14m03s.
The prior old-main selective CI took about 5-6 minutes with different coverage;
do not use it as a full-suite speedup claim. Logs show repeated independent
Cargo compilation of shared contract, SDK and macro crates.

[The M1 matrix rollout](m1-ci-matrix.md) records the current benchmark and
shipping gate. [The proposed generic flake-aware design](https://github.com/matthis-k/phenix-flake-ci/pull/17)
records FC1-FC4, owned by `phenix-flake-ci`. AI-CI1-AI-CI4 are consumer
integration milestones in this roadmap. The library schedules declared Nix
checks and proves selections; Phenix AI owns the derivations, compiler profiles,
fixture packaging and release semantics. Maintain one Nix check definition.

Prioritize AI-CI1 before adding further matrix shards. First remove redundant
Cargo builds and broad source invalidations. Next prove fast check profiles,
then use FC1-FC3 to generate a complete job plan. Keep the original shipping
job required until the generated aggregate verifies every check and exact-head
release report. FC2 and FC4 add selective PR execution and shared caches after
those guarantees hold. Only validated main or trusted revisions may publish
cache outputs; PRs consume them read-only.

Collect same-commit cold and warm runs, per-crate compilation counts, Nix
built/substituted derivations, cache bytes, wall time and billed runner-minutes.
Initial performance targets, not acceptance claims, are 2-4 minutes for a
small warm plugin PR and 5-8 minutes for warm full validation. A faster run
must preserve the same semantic, negative, ABI and shipping evidence.


G1/G2: [PR #782](https://github.com/matthis-k/phenix-ai/pull/782) owns typed graph declarations, ELF verification and the prepared multi-root DAG. Full exact-head shipping CI 38220130161 passed at `1615ed52`, but definition-only and negative semantic evidence remain before acceptance. [Its contract](m1-graph-declarations.md) owns the gates.

G3: [PR #783](https://github.com/matthis-k/phenix-ai/pull/783) introduced
scoped action admission and a state-backed join. CI 38223182503 failed because
the fixture omitted the property resource grants. #785 inherited that failure.
The `impl/m1-serial-graph-semantics` follow-up fixes the fixture and implements
[the frozen serial execution contract](m1-serial-graph-contract.md). Skip
suppresses active dependents transitively while independent nodes continue.
FailRoot stops the remaining action. Kernel failures always stop execution.
A trusted host observes admission, results, suppressed nodes and completion
under one correlated root. Root requirement resource grants bound delegation;
resource denial preserves cell epochs and revisions. The SDK-only native gate
checks four nodes from three independently built plugins. Local workspace tests,
Clippy and native execution pass with Rust 1.96. Full exact-head shipping CI 38228548634 passed at `672b7f09`. G3 serial semantics are proven; full stage acceptance awaits the G1/G2
definition-only follow-up; streaming, cancellation and state triggers remain G4.

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


### B2 SQLite restart slice

`impl/m1-basic-storage-sqlite` is stacked on the Session provider. The new
`phenix-plugin-storage-sqlite` implements the existing public Storage contract.
It uses SQLite transactions and a revision check under `BEGIN IMMEDIATE`.
The provider keeps no process-local record map. The product launcher sets
`PHENIX_BASIC_SQLITE_PATH` to the database file; a missing path fails rather
than creating an implicit in-memory store. This is a file-backed provider, not
an OS sandbox or a kernel persistence mechanism.

The `phenix-b2-sqlite` conformance gate executes two separate processes against
the same database. The first checkpoints a Running session with two pending
call IDs, appends journal events and tests a failed multi-key CAS rollback.
The second loads the persisted session, checks the Indeterminate transition,
reads the journal with two independent cursors and explicitly reconciles both
call IDs. The conformance and shipping checks include the resulting report.
The original in-memory fixture remains separate.

The observer slice below adds per-observer persistence and startup replay.
B2 remains unchecked until exact-head acceptance, migration/versioning checks
and the remaining transaction fault cases pass.

### B2 durable observer checkpoint slice

`impl/m1-observer-checkpoints` extends EventJournal and BasicOutput with typed
observer load and checkpoint operations. The Journal plugin stores one
revision-checked record per session and observer through its selected Storage
provider. BasicOutput forwards through the selected Journal import. The
[observer contract](basic-observer-contract.md) owns record validation, offset
bounds, CAS conflicts and crash semantics.

Terminal and JSONL restore separate named cursors at startup. The host writes
and flushes a rendered page before persisting its last sequence. Failure leaves
the page available for replay. A crash between delivery and commit can repeat
the page; delivery is at least once. ACP session/load retains its explicit
history replay and connection-local offsets.

The existing `b2-sqlite` gate now includes a five-process observer fixture with
independent cursors, abrupt exits before/after acknowledgement, stale revisions,
backward/future offset rejection, session denial and corrupt-record rejection.
The product gate includes restart and failed-pipe replay checks. The observer
slice passed [full conformance](https://github.com/matthis-k/phenix-ai/actions/runs/38321349932)
and [matrix CI](https://github.com/matthis-k/phenix-ai/actions/runs/38321349890)
at `f235d68f6b5d236328971b31654e19db69dec1e5`. Full B2 remains open for migrations
and the remaining transaction fault cases.

### B2 session/checkpoint slice

`impl/m1-basic-session` stacks on the journal branch and provides the public
`Session` contract from a separate native plugin. Snapshot open, revisioned
checkpoint and pending tool reconciliation all delegate to an explicitly
selected Storage implementation. A restarted provider's first open of a
previously Running snapshot with unresolved tool calls marks it Indeterminate
through a CAS transaction. It never silently repeats an indeterminate tool;
the caller must reconcile its recorded call ID. A native fixture seeds a
checkpoint in independently packaged Storage to verify the transition,
stale revisions and cross-session denial.

This is not process-restart or SQLite proof: the fixture is volatile and the
simulated reload seeds the stored checkpoint explicitly. SQLite durability,
journal-backed replay integration, true restart/resume, observer cursor storage
and fault-injection remain the B2 product acceptance gate.

### B2 semantic event journal slice

`impl/m1-basic-journal` is stacked on `impl/m1-basic-tools` and
implements an optional `EventJournal` provider using the existing typed
`Storage` import. The provider assigns contiguous sequence numbers only
after reading the revisioned persisted snapshot, and writes through one
Storage compare-and-swap transaction. Concurrent revision conflicts stay
visible to callers instead of being silently replayed. Read offsets are
independent and read-only. It refuses cross-session and malformed persisted
records.

A separately packaged in-memory Storage plugin is used **only as a native
conformance fixture**. It does not prove durability, process restart, SQLite
storage, independent observer checkpoint persistence, or session replay.
Those are still B2 acceptance requirements. No persistence or journal
logic enters the kernel.

### F2 ACP v1 adapter slice

`impl/m1-basic-acp` builds the same Frontend source crate with an exclusive
`acp` feature to yield an independent native package. It accepts ACP v1
JSON-RPC `session/prompt` text blocks and `session/cancel` notifications,
rejects mismatched session IDs and unsupported content, and emits
`session/update` notifications from journal events. The native F2 check loads
the actual selected plugin and verifies this codec.

The ACP connection host now handles initialize, scoped session/new,
session/load, session/prompt, journal-backed notifications and end_turn replies.
Each session selects the same artifacts with distinct kernel-authorized resource
grants. A Python ACP stdio fixture creates a session, sends a prompt, restarts
the host process, loads the persisted SQLite session, continues it and creates
a second isolated session. This still uses a synchronous scripted model, so it
cannot interrupt active model I/O. Streaming cancellation, richer ACP client
conformance and exact-head shipping remain before F2/P1 acceptance.

The branch includes the B5 nested Storage ceiling fix and removes direct agent
Storage grants from the product composition. Runtime and frontend formatting
pass locally. The native ACP codec and two-process stdio/reload fixture pass
with independently packaged native providers. Exact-head Nix CI remains pending.

### P1 terminal and JSONL host smoke

`impl/m1-basic-runtime` packages `phenix-basic-runtime` outside the kernel.
It loads one selected M1 composition with terminal and JSONL adapters,
independent model/tool/skill/context/session/journal/storage providers, and the
definition-only Basic contracts. The host asks the frontend plugin to decode
a prompt, submits `phenix.basic.submit` through the kernel graph and obtains
output by replaying the typed journal through BasicOutput and the selected
frontend renderer. Terminal and JSONL use the same kernel and Basic agent.

The `basic-host-smoke` Nix check submits a prompt and a multi-tool turn in
separate product processes against the same SQLite file. It verifies visible
terminal and JSONL answers, rather than proving only isolated plugin ABIs.
This initial launcher uses one preselected session resource, `s1`; it is not
yet a full multi-session host. The synchronous agent cannot accept live
cancellation while model I/O blocks. The full P1 gate remains unchecked until
pending/streaming, ACP, session/resource selection and all product acceptance
cases pass on the exact integrated head.

The product composition now follows the B5 nested Storage ceiling declaration.
It grants Storage operations only to the owning Session, Journal and memory
providers. The runtime passes Rust formatting. Local native Terminal and JSONL
smoke tests pass in separate processes against one SQLite database. These
checks cover the synchronous slice; canonical Nix CI is still required.

### P1 live terminal and JSONL controller

`impl/m1-p1-live-jsonl-controller` gives the host a dedicated stdin reader
while the controller thread owns the kernel and pending graph. It continues
calling `GraphExecution::pump` while handling follow-up and cancellation
packets. The host reads journal deltas through explicit, session-scoped grants
on the independently selected EventJournal provider; rendering and observer
checkpoints use the same existing Frontend and Storage-backed Journal plugins.
Cancellation waits for graph ticket draining, then checkpoints a cancelled
Session and appends a typed `Cancelled` journal event. The next prompt is
allowed to continue in that session. Prompts queue with a 32-entry limit.

The `basic-pending-http-product` fixture extends its real fragmented HTTP
test with live text, cancellation and same-session continuation. No new kernel
dispatcher, agent entry point or scheduler is introduced. These changes remain
unaccepted until the exact-head Nix shipping gate passes. The terminal and
JSONL launcher still uses a single selected session, and ACP cancellation
still requires its own nonblocking protocol controller. Model-only interrupted
runs also need ownership fencing for safe recovery: a fresh Session instance
cannot infer that another process's live model request has ended. An automatic
recovery attempt in closed PR #817 was rejected for this reason.

### R1 standard SSE framing

`impl/m1-r1-standard-sse-parser` replaces custom line/data accumulation
in the pending HTTP Model plugin with `sse-parse` 0.1.0. Existing `curl`
callbacks and the typed Model delta/usage decoder remain. The parser enforces
the canonical portable size bound; malformed UTF-8 remains a Model error.
The unit suite adds bytewise BOM/CRLF fragmentation, invalid UTF-8 across
chunks, and oversize rejection. The real pending-HTTP Basic and ACP fixtures
remain part of the shipping gate. Package resolution and exact-head CI are
pending.

### P1 independent ACP SDK integration

`impl/m1-p1-acp-sdk-interop` adds a separate official Rust ACP SDK 3.3.0
client. It launches the packaged `phenix-basic-acp` executable over stdio,
negotiates stable v1, creates a session, sends a prompt and counts typed
session updates. The dedicated CI job is distinct from the canonical
Phenix-wire tests. The test crate has its own Cargo workspace; neither the
kernel nor Basic runtime imports the SDK. The initial SDK-client job passed on `9534a35d`, and its transitive
dependencies are now locked. The updated exact-head `--locked` run and the
canonical shipping checks must still pass before P1 acceptance.

### P1 ACP initialization and JSON-RPC errors

`impl/m1-p1-acp-protocol-boundary` keeps malformed idle JSON-RPC from
terminating the connection. It returns the standard parse/invalid-request
errors. It rejects missing or invalid protocol versions without consuming
the initialization slot. When a client advertises a newer ACP version, the
v1-only agent responds with `protocolVersion: 1` for client-controlled
downgrade. These behaviors are tested through the real stdio product
fixture. The official-SDK client compatibility gate remains open.

### P1 live ACP controller

`impl/m1-p1-live-acp-controller` replaces the blocking ACP prompt path
with a reader thread and one kernel-owning control loop. A pending prompt
drives the existing `GraphExecution::pump`. Journal reads and Frontend
rendering reuse `GraphExecution::invoke_selected` while the root is live.
Cancellation is decoded by the ACP Frontend plugin and cancels the same
graph root. The host waits for physical draining, persists the cancelled
Session and emits a journal event before answering the prompt with
`stopReason: cancelled`. It then admits the next turn on the same session.

The connection retains other ACP requests in a bounded 64-entry FIFO until
the current prompt completes. This avoids a second kernel thread, an
additional scheduler and modification of the graph execution ABI. The
`acp-pending-http-product` fixture uses an independent chunked HTTP
server to test a streamed update, live cancellation and same-session
continuation. All acceptance remains pending exact-head shipping CI and an
independent ACP client compatibility check.

### P1 JSONL session selection

`impl/m1-p1-jsonl-multisession` routes each JSONL packet by its declared
session using the existing ACP `scoped_kernel` composition. Each session
gets independent resource authority and its own Storage-backed journal
observer checkpoint. The controller runs one graph at a time and queues
other sessions' prompts until the current root completes. A cancellation
packet can affect only its matching active session. Starting JSONL without
input still replays undelivered events for the default `s1` session.

The `jsonl-multisession-product` check interleaves two sessions, restarts
the host, checks independent output and journal offsets and verifies that
an idle cancellation cannot change another session. Concurrent execution
and multi-session Terminal support remain outside this slice. The check
has not yet passed canonical CI.

### P1 Terminal session selection

`impl/m1-p1-terminal-session-selection` adds an optional
`--session SESSION` argument to the Terminal runtime. Its default remains
`s1`. The launcher reuses ACP's `scoped_kernel` admission to bind the
selected session resource and applies the existing journal observer checkpoint
for that session. Terminal retains the same serial graph controller and the
same frontend plugin. No extra scheduler or ABI is introduced.

The `terminal-sessions-product` fixture switches between two sessions,
restarts the launcher, checks independent observer cursors and rejects an
invalid session identifier. Full P1 integration and external ACP compatibility
still require exact-head CI and independent verification.

### F1 terminal and JSONL frontend codecs

`impl/m1-basic-frontends` builds two standalone Frontend plugins from
one source crate with exclusive Cargo features. Both decode input into the
same typed `phenix.basic.submit` action payload and render journal events.
JSONL enforces its declared session against the authenticated client session.
The host controller owns active graph roots. A frontend's cancellation packet
signals that controller, while the direct `Frontend::cancel` method refuses to
report success without an active root to cancel.

The `phenix-f1-frontends` native check loads both artifacts separately and
checks typed prompt/cancel packets, event output and cross-session refusal.
The terminal event loop, JSONL host event loop, streaming/cancel dispatch and
ACP integration are still required for F1/F2/P1 acceptance.

### B5 graph-driven Basic agent slice

`impl/m1-basic-agent` declares a graph node triggered by the existing
`phenix.basic.prompt` event and implements `BasicInput` and `BasicOutput`
using public SDK calls. The frontend invokes the discovered
`phenix.basic.submit` action through the kernel graph. The plugin imports the
selected Model, ToolCatalog, ToolExecute, Skills, Context, Session and
EventJournal providers. The kernel contains no Basic agent handlers.

Before executing tool effects, the agent checkpoints assistant call IDs as
pending. It appends each tool result and clears its pending ID only after the
effect returns. A restart with unresolved IDs relies on the Session plugin's
Indeterminate state and explicit reconciliation. Journal records cover accepted
prompts, model calls, text updates, tool starts/results and terminal answers.
History, resource admission and caller limits travel through the typed imports.

The `phenix-b5-agent` native gate composes seven independently packaged
plugins with the definition package and submits a real graph action. This is a
scripted synchronous slice, not yet P1. It still needs pending streaming,
cancellation while model/tool I/O is in progress, HTTP-backed model tests,
reactive value triggers and shared terminal/JSONL/ACP product integration.
Exact-head CI acceptance remains required.

The B5 grant fix declares an optional Storage requirement to retain the
operation ceiling through nested Session, Journal and memory calls. The agent
has no selected Storage slot or direct Storage grants. Storage-owning providers
retain their explicit grants. Resolver and effect-admission rules are unchanged.
The native gate also rejects duplicate and unknown grants, then narrows root
Storage authority and verifies denial leaves the session idle with no history.
Local workspace tests, formatting and the packaged debug native gate pass.
Canonical Nix acceptance must still pass on the published commit.

### B3 persisted skills and context slice

`impl/m1-basic-skills-context` is stacked on the SQLite Storage slice.
`phenix-plugin-basic-memory` provides the existing typed Skills and Context
contracts and requires the selected Storage provider. Both contracts can be
selected independently. The plugin stores declarations under separate
session-scoped keys, rejects empty or duplicate IDs before writing, verifies
persisted session identity and propagates CAS conflicts. It does not scan the
filesystem or assemble model prompts.

The `phenix-b3-memory` gate executes two independent processes against
one SQLite database. It proves a new session starts empty, skills and context
survive process restart, unauthorized cross-session changes reject and invalid
updates leave the stored records unchanged. This is implementation evidence
until exact-head Nix CI, shipping and negative gates pass.

B3 still needs Basic-agent integration for selected skill/context use, tools
with durable counts and other Basic parity cases. Optional shell execution is
a separate plugin and requires a real selected workspace execution provider.

### B3 native tools slice

`impl/m1-basic-tools` is stacked on `impl/m1-basic-providers`. It
implements the independently selectable `ToolCatalog` and `ToolExecute`
providers with the existing public Basic contract and the current M1 native
dispatcher. The fixture checks query-bound cursor revisions, UTF-8 character
counts, JSON string arguments, independent session counts, denied executions
and no-effect rejection. The scripted B4 provider emits JSON arguments to
match the tool contract. This partial B3 slice intentionally does not claim
durable skills/context or a completed B2 dependency; storage-backed providers,
session replay and the triggered B5 agent remain separate stages.

### Development branch after the CI matrix

`impl/m1-basic-providers` branches from `impl/m1-ci-matrix` (PR #790)
without changing the CI branch. The matrix remains a cross-cutting
development accelerator, not an agent execution layer or a replacement for
the shipping acceptance gate. CI0's measured comparison is between the
same PR's head and generated merge commit; full shipping parity is open.

The first executable B4 slice provides a stateless scripted Model contract
implementation using the public Basic types. It validates transcript tool
call IDs, returns deterministic multiple calls and usage, honors caller
token limits, and checks the authenticated session resource at the native
ABI boundary. An independent package and native black-box gate exercise
these contracts. The HTTP provider below adds a separate pending implementation;
full B4 acceptance remains unchecked. B2 durable sessions, B3 tools/skills/context, G4 committed
state-change triggers, B5 graph-composed agent and F1/F2 remain in their
existing roadmap order. This slice does not add a new kernel mechanism.

### B4 pending HTTP model slice

`impl/m1-model-http` provides the existing Model contract as an independent
native package. It imports the selected Journal and declares an optional Storage
ceiling for nested journal writes. A libcurl worker owns HTTP/HTTPS transport,
certificate verification, complete-history requests and incremental SSE decoding.
It holds only owned data, a cancellation flag and an existing wake permit.
Authenticated poll callbacks append text deltas through the Journal import.

The [HTTP model contract](basic-http-model-contract.md) owns request mapping,
caller limits, reply validation and cancellation. Function arguments use an
API object envelope that unwraps into the unchanged ToolCall JSON arguments.
The worker retains its pending native lease until transport ends and it joins.
Cancelled cleanup discards progress and makes no journal writes.

The independent local HTTP/HTTPS fixture checks live progress before completion,
split UTF-8/SSE, multiple calls, complete history, skills/context, typed usage,
errors, truncated/malformed streams, caller limits, session denial, redirects,
CA trust and cancellation/draining. The new `b4-http` check participates in the
matrix and shipping evidence. Exact-head Nix CI is still required. The current
Basic agent remains synchronous; pending import integration and full frontend
streaming/cancellation remain B5/P1 requirements. Kernel and SDK code are unchanged.

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

### Definition-only graph follow-up

`impl/m1-graph-definition-packages` uses the existing contract package format
with an optional checksummed graph file. Generated definition macros expose its
canonical bytes without a native entry point. The existing resolver composes
its events and actions with native node declarations. The external fixture now
uses this independently compiled package, rejects forged digests, package-owner
mismatches and executable nodes before constructors, and proves that dormant B
does not block C and that an ordering edge alone does not activate D. Schema and
write-grant negatives remain in the same gate. The
[frozen definition contract](m1-definition-graph-contract.md) owns this scope.
Local native execution, Rust workspace tests, Clippy and formatting pass.
Full follow-up shipping CI 38231650549 passed at `fbe55d3d`; the downloaded artifact binds 30 native cases to that checkout. G1/G2 are accepted. G3 serial semantics are proven; admitted generation context remains before its full stage closure.

### Pending graph coordination follow-up

`impl/m1-pending-graph` replaces the serial graph loop with one readiness pump
around the existing Root and Ticket handles. Synchronous action submission uses
that same pump with pending admission disabled. Ready independent providers can
run while another source is pending; active joins wait for successful outcomes.
Cancellation and failures stop future node admission and cancel existing ticket
trees. Logical completion is distinct from retained physical draining.

The independent native fixture holds a worker file descriptor while A is
pending, completes B, and delays C. It proves immediate and delayed cancellation,
rejected cleanup writes, shutdown rejection during a retained lease, one terminal
update and ticket release. Domain and kernel failures in B cancel pending A.
The [frozen pending contract](m1-pending-graph-contract.md) owns this scope. Local
workspace tests, Clippy and the SDK-only native gate pass. Full follow-up shipping
CI and live committed-state triggers remain before G4 acceptance.

### Basic contracts

`phenix-basic-contracts` defines eleven provider/frontend contracts and the
`phenix.basic.submit` prompt action in one definition-only package. All schemas
derive from Rust types. Model requests carry full messages, assistant call IDs,
tool results, tools, skills, context and caller-selected limits. Storage uses
revision-checked transactions; journal/query, session replay and frontend types
remain plugin-owned. The shared Blob SDK type uses the existing byte value.

The Nix definition package builds from public SDK and Basic contracts only. The
existing verifier checks it without loading code. Definition packaging now uses
one shared Nix helper for the authored, graph and Basic packages. Shipping's
required evidence set includes the Basic contract gate. The
[Basic product contract](basic-agent-contract.md) owns provider semantics. Runtime
providers and the composed agent are the next implementation stages.
