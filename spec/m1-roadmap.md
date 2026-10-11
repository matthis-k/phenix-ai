# Phenix M1 implementation roadmap: contract-first reactive values and DAGs

Status: S1a portable gate accepted; remaining M1 gates planned
Updated: 2026-10-11
Baseline: [clean-start RFC](clean-start-rfc.md), accepted M0.0-M0.4 in PR #771
Normative semantics: [M1 value/reference/DAG RFC](m1-values-reactive-dag-rfc.md)
This file is the single task/evidence tracker for M1. Do not duplicate completion percentages in PR bodies.

## 1. Branch and PR ownership

PR #771 remains the unchanged M0 implementation and validation baseline. This specification PR is stacked on `rewrite/minimal-kernel`, includes documentation only, and must not amend M0 acceptance claims or rewrite implementation history.

Each numbered stage below owns one **focused implementation PR** after the specification is accepted. Do not open empty placeholder PRs. Use the agreed base branch and add actual executable changes before opening. Mark implementation PRs ready for review to trigger full CI; keep them unmerged until the requested integration decision. Stage-dependent PRs may be stacked, but their dependency and base branch must be explicit.

| ID | Proposed PR scope | Depends on | Exit gate | Status |
| --- | --- | --- | --- | --- |
| S0 | M1 RFC, this roadmap and M0-to-M1 links | #771 head | Consistent contract and file-level scope | Spec authored; review pending |
| S1a | Portable schema/ref identities and explicit v2 wire profile | S0 | Independent Python/Rust vectors and M0 regression gates | PROVEN at `2682be5f` in CI 38125329024 |
| S1b | Definition-only contracts, property/import metadata and negotiated state ABI | S1a | Metadata-only package and independent C/native ABI vectors | Not started |
| S2 | Kernel-owned ValueRef cells, CAS, subscriptions, transactions | S1b | External native state and security fixtures | Not started |
| S3 | Typed Rust value SDK and property/contract proc macros | S2 | External SDK-only annotated plugin | Not started |
| S4 | Contributed graph descriptor, resolve, validation, identity | S3 | Independent plugin contributions compile deterministically | Not started |
| S5 | Serial readiness execution and reactive on-change integration | S4 | Correct multi-root DAG partial order and cancellation | Not started |
| S6 | Parallel-ready tasks and multi-admission concurrency | S5 | Independent branches run concurrently, safe instance lifetimes | Not started |
| A1 | Agent contract definitions, default service plugins and agent loop | S5; S6 only if product needs parallel execution | Working replaceable agent profile | Deferred, not kernel scope |

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
| Definition-only descriptors, slots/properties and state-host ABI | NOT STARTED | S1b gate remains separate |

## 3. S2: kernel-owned live cells

Owner: one S2 implementation PR, no reactive DAG.

| File | Required change |
| --- | --- |
| `crates/phenix-kernel/src/value_state.rs` (new) | Cell ownership, revision, snapshot, CAS and atomic multi-cell commit |
| `crates/phenix-kernel/src/lib.rs` | Export narrowly scoped trusted APIs, not unfenced raw cell access |
| `crates/phenix-kernel/src/runtime.rs` | Reuse central dispatcher for read/write/watch and authenticated caller scopes |
| `crates/phenix-kernel/src/pending.rs` | Subscription tickets, cancellation, coalesced notification cursors and leases |
| `crates/phenix-contract/src/scope.rs` | State resource restrictions if operation/resource grants cannot express them |
| `crates/phenix-kernel/src/generation.rs` | Owner-incarnation and retirement behavior for live refs |
| `examples/reactive-provider/` (new) | Independently compiled provider with typed cells |
| `examples/reactive-client/` (new) | Independent authorized/unauthorized accessor and subscriber |

Gate: two refs clone the same identity; old snapshots remain unchanged; racing CAS commits produce one winner; atomic read+subscribe has no lost update; batch publish is all-or-nothing; stale owner and forged ref reject; denying write yields zero provider effects; abandoned subscriptions release leases. Run M0 tests unchanged.

## 4. S3: Rust SDK, macros and ergonomic plugin state

Owner: one S3 implementation PR.

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

## 8. Agent follow-on, not kernel scope

After S5, define `Model`, `ToolSource`, `ToolCatalog`, `Skills`, `Memory`, `Compaction`, and `Agent` in definition-only plugin contracts. Build provider, tools/skills/memory/compaction, CLI/ACP/MCP, and agent-loop plugins as ordinary selected implementations. Use `ValueRef` for exposed state and graph hooks only where it adds value. Agent plugin work MUST NOT become a dependency of any M1 kernel gate.

## 9. Evidence, closure, and PR discipline

Each stage updates only its own source files, fixtures, the relevant RFC addendum, and this matrix. The owning PR body records exact head, targeted acceptance tests, unresolved blockers, and relation to the previous stage. Mark tasks `PROVEN` only with executable evidence from the exact head; use `NOT STARTED`, `IMPLEMENTED`, `TESTED`, and `PROVEN` consistently. A stage cannot claim acceptance based on green CI alone or unit tests that mirror declarations.

Run focused tests while implementing, full `nix flake check -L` and release evidence before marking a stage accepted. Verify M0 tests, external independent artifacts, denied-effect counters, negative ABI cases, state/ref lifecycle, and canonical identity vectors. Nix builds remain declarative and CI read-only.

Rules for restructuring stages: record a concrete dependency and file ownership before creating another PR. Do not reintroduce the old kernel, a second scheduler, temporary compatibility paths, or agent code into the kernel. Remove an obsolete code path only after its replacement has independent executable proof.

