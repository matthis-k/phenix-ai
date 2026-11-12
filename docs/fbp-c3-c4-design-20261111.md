# FBP issues C3–C4: retirement authority and preparation inspection

Status: Phase 0/1 design note, 2026-11-11. No C3 behavioral change is authorized by this note.

## C3 — Retired generations can still execute

### Phase 0: problem and constraints

Current `KernelHost::publish` retires the old producer inventory under the whole-drain lock but intentionally does not stop explicit `Generation::inject`, `run_steps`, `with_session`, or `with_kernel` calls. `generations.rs` explicitly asserts that retained old generations can run after replacement; `generation_retirement.rs` asserts accepted producer work remains runnable after retirement. Code leases remain alive until all old executable owners drop.

Possible harm: a caller retaining `Arc<Generation>` can start *new* work on superseded plugin code after reload, retaining old resources indefinitely. However, changing this without a policy would contradict existing tested behavior and can strand accepted work.

Invariants to preserve: publication is atomic and does not wait for old explicit sessions; a whole producer drain completes before retirement; no producer polls after retirement; exact code leases outlive all owners; no preemption of arbitrary running callbacks; no silent drop or rollback of accepted work.

### Phase 1: alternatives

| Alternative | Strength | Cost |
| --- | --- | --- |
| Baseline: keep archival execution | Existing tests, nonblocking publish, easy long-lived sessions | Post-retirement admission remains possible |
| Gate only public `Generation` methods | Small code diff | `with_kernel` and any held raw `Kernel` can bypass; races if only checked before a session lock |
| Revoke *all* admission/execution in `Kernel` | Strictest post-retire semantics | Strands accepted jobs, conflicts with existing tests, requires new cancellation/disposal contract and ordering proof |
| Separate read-only archival handle and explicit drain-only execution authority | Can forbid new work while completing accepted work | New capabilities/authority model and synchronization; in-flight admission races require serialized retirement boundary |

**Decision:** no implementation in this batch. Treat C3 as a primary-use-case policy blocker, *not* a resolved/accepted tradeoff. Any change needs a new design note with an explicit authority choice, request-lifecycle acceptance tests, and a concurrency interleaving proof. A documentation-only reclassification would be dishonest.

## C4 — Unrestricted compiled-graph inspection in preparation

### Phase 0: problem

A `Prepare` callback receives `CandidateContext { pub compiled: &CompiledNetwork, imports }`. A consumer may inspect every physical process, interface, claim, and path even though it can only import declared contract bindings. Compose/edit grants already limit **writes**, but the public field does not limit **reading** topology. No arbitrary read capability is necessary for the two current native import consumers: both call `import(id)?.bind(context.compiled)` and operate on the resulting declared `BoundComponent`.

### Phase 1: alternatives and selected minimal change

| Alternative | Benefit | Weakness |
| --- | --- | --- |
| Baseline | No migration | Unrestricted topology inspection |
| Add a new graph-authority API layer | Potential fine-grained reads | Adds concepts and carries a large audit surface |
| **Selected:** private compiled field plus `bind_import(id)` | Reuses existing bound contract view, scope, and type checks | Introspection outside an import is no longer available to Prepare |

Proposed API:
```rust
let provider = context.import("service")?; // Metadata only, if required.
let bound = context.bind_import("service")?; // Exact declared import only.
```

No new authority type, graph executor, transport, scheduler, or claim. Bind uses the existing `CompiledNetwork::bind` and the exact `ResolvedImport` stored in this context, not a caller-supplied import identity. Existing `ResolvedImport::bind(network)` can remain for code which independently owns an authorized compiled network; it does not grant access to the private context field.

Acceptance: undeclared imports reject, declared imports have the same shape/typing/projection behavior, no mutation access is exposed, a compile-fail doctest prevents `context.compiled` access, native tests and exact-head CI pass. Read access to a separately obtained CompiledNetwork and in-process Rust isolation are outside this change and are not misrepresented as sandbox guarantees.


## Phase 1 revision — full-graph export is an established capability

The implementation audit found two more uses: the independently compiled
`host_portable_consumer` fixture binds a declared import (direct migration), and
`code_leases::detached_compiled_getters_exports_factories_processes_and_sessions_pin_both_leases`
deliberately exports a detached `CompiledNetwork` from preparation. Blindly
removing the latter weakens the established code-lease test and violates the
protocol's default acceptance rule. **Withdraw the unconditional removal**.

Revised minimum: `BuildOptions` carries one *host-owned*, default-empty
`inspect_network: BTreeSet<Key>` grant list. Ordinary `CandidateContext`
still hides `compiled` and binds only declared imports. A distinct
`inspect_network()` method returns the complete immutable compiled network
*only* when the exact contribution `Key` was authorized by the host. Permission
is checked when constructing the context; plugins cannot self-grant.
The opt-in privileged path retains the former detached compiled export and
its exact lease test, with the test host explicitly granting this privilege.
The portable fixture migrates to `bind_import` like the other consumers.

Alternative comparison: unrestricted baseline violates default least privilege;
removal-only breaks established export/lifetime behavior; a new scoped graph
projection type requires substantial implementation and cannot preserve exporting
a full compiled snapshot; exact opt-in is a single additive host policy field and
no new trait, graph backend, or executor. This consciously preserves full
inspection for trusted callers. It is **not** in-process sandboxing.

Phase 3 gates: ungranted preparation cannot inspect the complete topology;
authorized preparation can still clone and export the compiled network; no
cross-contribution permission bleed; declared imports remain checked; existing
code lease test remains unchanged apart from explicit grant and API invocation;
portable fixture compiles and executes.
