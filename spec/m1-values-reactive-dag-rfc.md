# RFC: Kernel values, reactive references, and contributed execution DAGs

Status: **superseded as a future graph design** by [Graph Architecture v2.4](graph-architecture-v2.4.md). Historical M1 design context only; retained for traceability.
Target: M1 after accepted clean-start M0.4
Date: 2026-10-11
Baseline: PR #771, branch `rewrite/minimal-kernel`, head `1ece5161cfb75f96bed7cbff8f48b3e5690efacc`
Roadmap: [m1-roadmap.md](m1-roadmap.md)
Precedence: M0 evidence is historical. The existing M1 graph-v1 behavior is defined by current source/tests. This RFC no longer governs future graph-v2 implementation; use v2.4.

## 1. Decision

The kernel owns canonical portable values, live reference identities, revisions, authorized access, atomic updates, subscriptions, and generic graph composition and execution. The kernel MUST NOT interpret application type `T`, know agent/skills/tool semantics, or expose Rust pointers across native plugin ABIs. Plugins own their state schema, initialization values, operations, and hook behavior. The SDK generates descriptors and ABI glue from Rust declarations. All calls use the existing resolver, host dispatch, authority lineage, pending tickets, physical leases, and immutable generation manager.

A graph has zero or more source nodes. It may have disconnected components. A required edge `A -> C` establishes a happens-before dependency only when A participates in the same execution. Unconnected nodes have no imposed order. Two active incoming edges to C require both predecessors to settle according to C's join policy before C becomes runnable.

M1 adds no second resolver, authority store, plugin registry, native loader, root identity system, or independent execution engine. A service-only plugin continues to work without values or DAG contributions.

## 2. Terminology and ownership

| Name | Meaning |
| --- | --- |
| `Value<T>` | SDK-owned immutable local Rust snapshot, cheaply cloned with `Arc<T>` |
| `DynValue` | Schema-identified canonical portable value, including opaque bytes; no Rust `Any` crosses ABI |
| `ValueRef<T>` | Typed SDK handle to a kernel-owned live cell identity; not a memory pointer |
| `Snapshot<T>` | Atomic pair of revision and immutable `Value<T>` |
| `Edit<T>` | Owned mutable draft plus expected revision, committed explicitly |
| `ValueId` | Cell identity, distinct from contract, schema, generation, and revision identity |
| `SchemaId` | Stable portable schema identity, not `TypeId` |
| `Epoch` | One committed change batch and its reactive propagation instance |
| Graph contribution | Immutable plugin-owned node/edge/trigger/condition declaration |
| Graph source | Node with no active predecessors in a propagation; not a separately admitted kernel root |
| Kernel root | Authorized execution scope, pinned generation, ownership and cancellation tree |

Live cells belong to one authenticated plugin instance or an explicitly named durable namespace. Owning an ID does not grant access. A ref conveys location and type expectation, not authority. Generation pinning and owner incarnation prevent accidental rebinding to new plugin code or state.

## 3. Data and ABI invariants

`Value<T>` SHOULD be `Arc<T>` in the SDK. `Clone` shares a snapshot. `Deref<Target=T>`, `AsRef<T>`, and `From<T>` are supported. `into_arc()` is always cheap. `into_inner()` MAY return the `Arc<T>`; extracting an owned `T` is only guaranteed when uniquely owned or `T: Clone`.

`ValueRef<T>::clone()` duplicates a live identity. It MUST NOT implement `Deref`, `DerefMut`, or `AsRef<T>`: access is a potentially fallible, scoped host operation. `Snapshot<T>` can dereference to its immutable `T`. `Edit<T>` dereferences mutably to its private draft. Dropping an edit discards it. Publication never happens from `Drop`.

Ordinary `bool`, `String`, `Vec<T>`, `Option<T>`, maps, structs, and enums remain Rust authoring types. `Portable` schemas and generated codecs provide stable canonical serialization. Serde MAY simplify SDK authoring but MUST NOT replace the canonical schema or permit noncanonical encodings. The existing `phenix.value.v1` remains the M0 wire profile. M1 MUST freeze a new explicitly negotiated reference-capable wire/ABI profile before accepting `ValueRef` nested in portable data; no meaning of existing v1 tags may change. Implementers MUST NOT assume a raw reference ID serialized as a string is a validated live reference. M0 compatibility is retained only for its conformance profile, not as a permanent shadow runtime.

The portable representation of a ref contains version, namespace/owner incarnation, cell identity, schema identity, and any non-secret versioning fields required to locate it. The kernel authenticates callers and separately validates read, watch, and write grants at each access. It MUST NOT serialize capabilities, raw native pointers, `Arc`, `dyn Any`, `TypeId`, mutex guards, or credentials. The kernel MUST bound payloads, reference nesting, and graph fan-out by explicit validated protocol limits, not arbitrary execution timeouts.

The [S1a wire contract](m1-reference-wire-contract.md) fixes portable identities, schema hashing and the explicitly selected v2 encoding. The [S1b metadata/state contract](m1-metadata-state-contract.md) fixes offline definition packages, named slots/properties and native ABI 3.0 with explicitly selected v2 replies. Its portable C/Rust gate is accepted in PR #774 at `51017e07`; authenticated live state remains S2.

## 4. Live value semantics

A kernel cell holds a stable `ValueId`, schema, owner incarnation, monotonic revision, and immutable current snapshot. Value bytes MAY be internally shared by `Arc<[u8]>`. Type-specific Rust objects MAY be cached inside one plugin, but never used as an ABI contract.

- `read(ref)` returns a consistent snapshot and revision from one atomic read.
- `compare_set(ref, expected_revision, value)` validates caller, owner, schema, and current revision, then commits atomically or returns typed `Conflict`. Conflict produces no change.
- `watch(ref, from_revision)` atomically registers and observes the current revision, returning an initial snapshot or a cursor that cannot miss a commit between read and subscription.
- Change delivery MAY coalesce state snapshots to the latest revision; every notification carries a monotonic cursor. Every-event delivery requires a separate stream contract.
- `commit_batch(edits)` publishes several cells as one epoch or publishes none. Readers requesting a multi-cell consistent view use an epoch snapshot API; independent reads are only per-cell consistent.
- A write is committed before on-change hooks run. Hooks observe committed state; a hook write opens a new epoch, not a hidden recursive same-epoch call.
- Reference disposal, subscription cancellation, owner retirement, and generation rollback release subscriptions and leases safely. A stale live ref returns `OwnerGone` or `StaleReference` unless it retains an explicitly authorized owner lease. Immutable historical snapshots remain valid.

All mutations of exported live state MUST pass a tracked commit boundary. An ordinary `Vec::push` on a raw privately held vector cannot be observed magically. Macro-generated fields therefore lower to a controlled typed handle; an update uses `edit()/commit()` or a generated state mutation method. No unrestricted `DerefMut` on a live handle.

## 5. Plugin declarations and Rust ergonomics

The SDK has one declaration vocabulary: `define`, `require`, `provide`. Definitions carry independent contract IDs, versions, typed operations, and guarantees. Imports resolve per caller/import slot, including named and multi-provider slots; multiple matching providers without declared selection rules reject. A contract-only plugin has no executable state and needs no native entrypoint.

This is target syntax, not currently compiled code:

```rust
#[phenix::plugin(id = "phenix.skills.basic", version = "1.0.0")]
#[derive(Default)]
pub struct Skills {
    #[get]
    #[watch]
    skills: Vec<Skill>,

    #[get]
    #[set]
    enabled: bool,

    cache: HashMap<String, Skill>,
}
```

The outer `#[plugin]` macro reads field helper attributes, emits stable property-contract IDs and `get/watch/set` operations, generates canonical descriptors and bindings, and lowers exposed field storage to SDK-managed state handles. Unannotated fields remain plain Rust. `#[get]` does not implicitly authorize setting or watching. A field may use an explicit public `id` to survive Rust renaming. Named and versioned public field identities are contract ABI, not incidental Rust spelling.

Rust has no standalone field attribute procedural macro. Field annotations MUST be processed by the enclosing `#[plugin]` proc macro or a supported derive-helper macro. A macro on a struct cannot discover external `impl` blocks. The optional inline-module form collects `#[state]`, `#[provide]`, `#[on_change]`, and `#[after]` annotations in one deterministic descriptor. Compile-time generated schemas must be usable across independently compiled crates without macro source introspection of dependency crates.

Initialization MUST not issue host calls inside `Default`. The host creates and binds cells after an SDK-generated initialization handshake and before admitting calls/subscriptions. Changes need a negotiated native ABI extension; the existing M0 create/stop semantics stay unchanged until the M1 profile is accepted. A definition-only plugin MUST work without loading executable code.

Generic SDK API target:

```rust
let value: Value<Vec<Skill>> = Vec::new().into();
let clone = value.clone();          // shares immutable Arc
let count = clone.len();            // Deref
let reference: ValueRef<Vec<Skill>> = skills.ref_handle();
let before = reference.read().await?;
let mut edit = reference.edit().await?;
edit.push(skill);                   // DerefMut on the draft
edit.commit().await?;               // may return Conflict
```

`ValueRef<T>` is typed by a schema trait bound. An arbitrary `dyn Any` downcast is permitted only within a single Rust type universe, never as proof of ABI compatibility.

## 6. Graph contributions and reactive execution

A selected plugin can contribute node definitions or dependencies targeting a published property or hook slot. Contributions are static, owner-attributed, and inspectable before code execution. They name typed service operations, trigger refs/contracts, predecessor IDs, read/write effects, and explicit join/failure policy. No callback may run while discovering or resolving contributions.

Illustrative SDK syntax:

```rust
#[phenix::on_change(skills_api::SKILLS, id = "search.reindex")]
fn reindex(ctx: &HookContext, change: Change<Vec<Skill>>) -> HookResult { /* ... */ }

#[phenix::on_change(
    skills_api::SKILLS,
    id = "context.refresh",
    after = [search_api::REINDEX]
)]
fn refresh(ctx: &HookContext, change: Change<Vec<Skill>>) -> HookResult { /* ... */ }
```

`after` is an ordering edge, NOT a trigger. A node runs only when its declared trigger fires or its graph entry is explicitly admitted. A reactive propagation computes the affected set for one epoch, then the active predecessor set for each affected node. A node becomes ready when its required **active** predecessors have settled according to its join/failure policy. Dormant predecessors never block reactive propagation. An explicit workflow `All` join, by contrast, requires all declared participating branches of that execution.

- `A -> C <- B`, both active: C waits for A and B.
- `A -> C <- B`, only A active: C waits for A, then reads B's last committed state if needed.
- `A -> C` and `B -> D`: no cross-branch ordering guarantee.
- A disconnected graph has several sources. Source nodes are not implicitly joined.
- Failure/cancellation is a typed outcome; a failed predecessor does not count as successful completion. A node's declared policy chooses skip, propagate failure, or explicitly handle failure.
- Write conflicts without ordering or transactional conflict policy reject. Static same-epoch cycles reject, including declared write-trigger edges. Feedback across epochs needs an explicit delay/loop policy and traceable termination.
- Unrelated nodes MAY execute concurrently; conformance MUST first prove partial-order correctness under serial scheduling. Parallel execution requires a separately accepted host concurrency upgrade.

The kernel composes contributions from selected plugins, resolves provider bindings once, validates schemas, ownership, authority and cycles, and freezes graph topology in the candidate generation. Reference revisions never modify graph identity. Graph or provider changes stage a new generation. Old executions stay pinned. Callbacks invoke through the existing dispatcher with the same root authority, cancellation, ticket and lease rules.

## 7. Kernel reuse and non-goals

| Existing M0 component | Reused behavior | M1 change |
| --- | --- | --- |
| `phenix-contract/src/value.rs` | Canonical value encoding | Reference-capable versioned wire representation |
| `descriptor/*` | Schemas, imports, exports | Contract/property/hook contributions |
| `phenix-macros/src/lib.rs` | Compile-time descriptor/ABI generation | Trait and field macros plus graph annotations |
| `phenix-sdk/src/lib.rs` | Typed host imports | `Value<T>`, refs, snapshots, edits, subscriptions |
| `phenix-kernel/src/resolve.rs` | Resolver, import cycles, startup order | Named imports, selected contributions, graph validation |
| `runtime.rs` / `pending.rs` | Dispatch, roots, cancellation, tickets, leases | State access operations and readiness in the same host |
| `generation.rs` | Staging, pinned roots, rollback and retirement | Graph/ref schema identity and leases |

M0 supports one serial root per kernel generation and disallows reentrant stateful instance calls. M1 MUST NOT describe this as a parallel executor. M1 serial DAG conformance comes before runtime concurrency. Shared `Arc<dyn Any>` data is not a cross-library transport. The kernel does not implement agent loops, skills, model APIs, UI widgets, remote transports, or domain-specific workflows.

## 8. Acceptance

M1 acceptance requires independent non-agent native fixtures built against the public SDK that prove:

1. Immutable snapshot clone and typed `Deref/AsRef/From` behavior; no live-ref `Deref`.
2. Shared ref identity; revisions; one winner of competing expected-revision edits; no implicit retry or publication on drop.
3. Atomic read/subscription bootstrap, coalescing cursor, cancellation, and bounded memory.
4. Authorized read/write/watch, denied operation with no effect, schema mismatch, stale instance, and no capability escalation by cloning or serializing refs.
5. Generated `#[get]/#[set]/#[watch]` properties, plain Rust source fields, no untracked updates, no handwritten descriptors, and an independently compiled consumer.
6. Metadata-only contract definitions, `define/require/provide` annotations, independent schema identity, named imports, and multiple selected provider instances.
7. Two separately authored hook plugins producing `A -> C <- B` and disconnected `A -> C`, `B -> D` graphs; deterministic compilation; correct readiness; explicit failure policies.
8. No inactive predecessor deadlock; correct handling of simultaneous changes in one epoch; no read-subscribe gap.
9. Old generation pinned through trial and promotion; stale refs reject or maintain explicitly granted leases; retirement drains execution safely.
10. Existing M0 conformance still passes. No domain plugin or second resolver/executor enters the kernel production closure.

Any unsupported ABI profile, malformed graph, missing dependency, conflicting contribution, forged ref, or unbounded declared fan-out fails before activation or before effect.

## 9. Milestones

The ordered execution plan, acceptance gates, file-by-file ownership, and proof artifacts are in [m1-roadmap.md](m1-roadmap.md). Each implementation PR MUST include its stage's focused semantic tests and update the roadmap's evidence column. `PROVEN` requires tested executable behavior at the exact commit, not a checked box or green CI alone.

