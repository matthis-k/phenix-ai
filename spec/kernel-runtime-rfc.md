# RFC: Phenix kernel contract and execution model

status: specification-only
review: target architecture, not implemented behavior
date: 2026-10-09
scope: kernel, contracts, plugin host, execution IR, composition, authority, lifecycle
depends_on: none
related: #726, #728, #729, #730, #731, #734

## 1. Decision and scope

Phenix Core is a domain-neutral host for independently authored plugins. **Plugins author behavior. Core validates, composes, and executes one canonical representation of that behavior.** Core has one resolver, one execution scheduler and dispatch mechanism, one authority lineage, and one generation lifecycle.

An ordinary typed service needs no workflow, contribution-kind provider, graph patch, profile, or domain framework. Direct service calls and plan-backed roots share Core dispatch, permissions, cancellation, and generation leases.

This RFC specifies the target after the microkernel redesign. It is a design contract. It does not claim that current Core or any open PR meets these rules. Implementation changes must retain the behavioral-parity and migration gates in their owning PRs.

### 1.1 The kernel does not own

The kernel does not know AI agents, turns, models, tools, memories, skills, prompts, UI controls, retries, compaction, or product-specific context. Those concepts are ordinary versioned interfaces and implementations defined by plugins. The kernel also does not interpret Nix semantics, decide domain policies, compile domain scripting languages, or implement an interpreter for Lua, Wasm, or JavaScript.

### 1.2 Success criteria

A third-party author can supply a typed service using one interface, one implementation, and a provide annotation. An agent-free fixture can run a plan with a cycle, bounded dynamic fan-out, a generic IR patch, and generation replacement. Both paths execute through the same resolver and host without importing Basic, Full, or any agent contract.

## 2. Precise vocabulary

| Term | Meaning |
| --- | --- |
| Interface | Independently versioned operation names, request/result schemas, and behavioral guarantees |
| Provider | Plugin-owned implementation of an Interface export |
| Import | Consumer requirement for an interface, required or optional |
| Concrete dependency | Exact plugin selection, different from an interface requirement |
| Contribution | Immutable, versioned, plugin-owned data passed to preparation and resolution |
| Kind provider | Plugin defining the schema and declarative lowering for a contribution kind |
| Plan | Versioned, plugin-owned ExecutionPlan definition with stable step, slot, and frame identities |
| Entry binding | Resolved association of a typed trigger or command with a PlanId |
| IR patch | Immutable edit against a known plan revision expressed in Core IR terms |
| Layer | Ordered around-call behavior with a one-shot continuation |
| Listener | Observation of an event after its publication; cannot veto the completed operation |
| Frame | Typed serializable data visible to plan steps; it never contains Core capability handles |
| Root | Admission-scoped execution with pinned generation, authority, environment and owned tasks |
| Generation | Immutable resolved plugin composition, bindings, policy and compiled plan identities |
| Lease | Generation occupancy retained while root-owned execution or callbacks may still run |
| Environment pin | Host-defined resolved binding that generation selection must preserve |
| Durable namespace | Independently versioned data owner, schema epoch, access modes and migration policy |
| Native adapter | Independently loaded native ABI plugin implementing a guest runtime contract |
| Guest plugin | Separate logical Plugin hosted by an adapter, with its own identity and authority |

A logical module does not necessarily require a separate Rust crate. A separate artifact or plugin identity is justified by distinct authority, compatibility, or lifecycle.

## 3. Modules and dependency direction

| Module | Owns | Why separate | Forbidden responsibility |
| --- | --- | --- | --- |
| Contract/value representation | Canonical typed values, identities, schemas, compatibility metadata | Cross-language contracts must not depend on Rust implementation details | Provider selection or domain defaults |
| SDK authoring | Typed annotations, metadata generation, ABI wrappers | Authors need a small default path | Runtime registration callbacks or a second resolver |
| Contribution preparation | Manifest ownership, kind validation, frozen lowering output | Portable plugin sources must enter the same resolver | Starting providers or changing an active generation |
| Canonical resolver | One provider selection, authority and layer ordering, plan selection, patch conflicts | Every selected binding must have one explanation | Running plugin callbacks as resolver logic |
| IR composer/compiler | Subplan expansion, IR patch composition, schema and flow checks, executable dispatch plan | One trusted plan validation boundary | Domain node types or workflow-specific engines |
| Root execution host | Invocation, cooperative scheduling, fork/join, event correlation, cancellation, lease tracking | One runtime lifetime and call identity | Domain retries or application strategy |
| Generation manager | Prepare, stage, inspect, trial, promote, retire | Hot replacement without changing existing roots | Implicit promotion on artifact discovery |
| PluginHost/native loader | Stable ABI, scoped host imports, guest adapter bootstrap | Independent native artifacts need one transport-neutral host boundary | Built-in Lua, Wasm or JS cases |
| Durable resource mechanism | Namespace ownership, transactional conflicts, migration fences | Executable revisions are not storage schema epochs | Automatic migration on plugin replacement |
| Workflow authoring plugin | Domain-friendly workflows compiled to Core IR | Different domains require different authoring vocabulary | Executor, scheduler or provider resolver |
| Kind providers | Domain kinds such as Tool and Skill, declarative templates | Extend semantics without Core-specific enum arms | Live graph mutation or hidden registration |
| Basic and Full profiles | Replaceable default plugin selections and optional Layers | Products are choices, not kernel dependencies | Nix-owned provider decisions |

Dependencies point from SDK and plugins to the small contract representation, then to Core's public host capabilities. Core does not depend on agent or first-party product crates. The host may load plugins but does not import their domain schemas to implement control flow.

## 4. Composition phases and invariants

`Author declarations -> Validate ownership/schema -> Lower -> Freeze canonical contributions -> Resolve selections -> Compose IR -> Validate/compile -> Stage generation -> Admit root.`

- Static typed declarations, portable JSON and guest manifests normalize into the same versioned, owned canonical data.
- Declarative kind lowering is total, bounded, pure and non-Turing-complete.
- Exceptional procedural preparation runs outside canonical resolution, with no ambient host authority and bounded immutable input/output. Its validated output, digest and provenance are frozen before resolution. Capturing output does not prove the preparation process reproducible.
- **The resolver never executes arbitrary plugin author code.** It never calls plugin callbacks to discover providers, patches or graph topology.
- Plan definitions, entry bindings, patches, and provider selections are ordinary contributions with stable owner provenance.
- New plan revisions or changes in selected bindings create a candidate generation. An active graph cannot be patched in place.
- Identical canonical selected inputs and policy produce identical generation identities regardless of discovery order. Include relevant contract, IR, artifact, lowering and policy revisions. Never include secret bytes.
- Missing required imports, invalid owners, incompatible schemas, ambiguity, cycles in required provider dependencies, unauthorized bindings or unresolved conflicts reject candidate activation.

## 5. Minimal ExecutionPlan IR

Core recognizes exactly four step forms: `Invoke`, `Fork`, `Join`, `Exit`. A plan also carries `PlanId`, plan revision, an independently versioned IR semantic version, one typed entry, input/output schemas, typed frame schema and revision, typed transitions, cancellation scopes, authority requirements and stable extension slots.

`Invoke` addresses a declared service *import*, never a concrete provider. Its result projection maps only declared normal result discriminants to typed outgoing edges. Service runtime failures and cancellation are not normal outcome edges.

`Fork` admits either static named branches or a bounded map over a finite typed collection. Its map form declares collection schema, one reusable branch entry and a maximum child count. Admission validates the bound and effective authority before allocating children.

`Join` references its `Fork` and selects from a closed, versioned policy set: `All(CollectAll)`, `All(FailFast)`, `FirstCompleted`, `FirstSuccess`, `Quorum(k)`. Core evaluates settlement and deterministic tie ordering for simultaneous observations. Non-winning children remain root-owned, receive cancellation where appropriate, and retain generation leases until settled or safely terminated.

`Exit` projects a validated plan output. Branch and frame values are data, not live capabilities.

### 5.1 Cycles and concurrency

Runtime control-flow cycles are allowed. Every cycle must reach `Invoke` or another scheduler-recognized suspension point; a cycle made solely of immediate control operations is invalid. Core checks cancellation on back edges and schedules ready scopes fairly. Runtime loops do not create new roots or choose new generations. Limits and deadlines are opt-in unless deployment authority imposes a security resource bound.

A service may use Core's structured internal task API. Those tasks retain scope and lease ownership but need not appear as public IR steps. The IR describes *between-service* topology, not a provider's private implementation.

### 5.2 Frame and outcome boundaries

Frame slots contain typed serializable data only. Unforgeable capability, credential, continuation and native pointer handles remain host-side. Branch frames are isolated copy-on-write snapshots. Join combines only declared typed data and resumes with the parent's authority, never the union of child grants.

Immutable `DataRef` values may identify large data without copying it. A data reference is a locator, not an authorization token: the reader needs a separately authorized host import checked against schema, root, storage scope and Environment. Frame serialization cannot carry process pointers.

Recoverable domain errors are **declared normal result variants** in the interface schema. The envelope distinguishes `Completed(DeclaredResult)`, `Failed(ExecutionFailure)` and `Cancelled`. Core never translates missing providers, transport errors, cancellation, schema rejection or authority denial into ordinary result variants.

## 6. Plan contributions, entry choice and subplans

`ExecutionPlanDefinition` is an immutable plugin-owned contribution that includes plan identity/revision, IR version, frame-schema revision, stable step and slot IDs, scopes, typed entry/output, imported contracts and provenance. Multiple conflicting plan definitions require explicit resolution.

An `EntryBinding` associates one typed command, trigger, or explicit application entry with exactly one selected plan and compatible revision, input projection and admission authority. A plan-backed root resolves its entry before execution, selects a resident generation and pins the compiled plan. A service-only root invokes an authorized import without requiring any plan. Neither path permits runtime fallback plan discovery.

**Subplan reuse is compile-time inlining**, before IR patch composition. An inclusion site has a stable ID. The compiler prefixes child step, edge, scope, slot and private frame-field identities with that inclusion ID; input and output mappings are typed, and child `Exit` is rewritten as a handoff. Published child slots remain addressable through qualified identities. Recursive inclusion or unbounded expansion rejects preparation. Runtime `CallPlan` and plugin-managed nested plan schedulers are unsupported.

## 7. Closed IR patch algebra

A patch targets one declared base `PlanId` and revision. Supported edit vocabulary is: insert node, remove node, replace node, add edge, remove edge, bind slot, order slot, and compatible `ExtendFrameSchema`. These operate on IR and typed frame declarations only. Workflow plugins may lower domain patches into IR patches; Core never interprets domain patch vocabulary.

The plan owner controls entry, input/output contracts, whole-plan scope policy and the published stable slot contract. An ordinary patch cannot change them, introduce an unknown step form, or install a callback. A revised plan definition from the owner is required for owner-level metadata changes.

Slots declare cardinality, allowed operations, required outcomes, order semantics and authority constraints. Every field added by `ExtendFrameSchema` has a namespaced owner, version, type and initialization proof. Patches cannot remove or reinterpret existing fields without owner revision.

Composition is `Candidate = Compose(Base, SelectedPatches, ExplicitPolicy)`. The composer classifies conflicts and order constraints before applying edits, validates the complete candidate atomically and retains patch provenance. Stale targets, ambiguous effectful insertion order, conflicting exclusive replacement, incompatible outcomes and authority escalation fail. Explicit policy may choose compatible winners but cannot override schema or authority checks.

**Service wrapping remains a Layer, event observation remains a Listener, and provider choice remains a binding.** Patch operations do not duplicate those mechanisms.

## 8. Capability algebra and trust

Effective authority is the meet of root ceiling, caller grant, selected generation policy, provider/component limits, scope attenuation and adapter/guest limits. Each delegated capability is unforgeable and scoped to a call or root. A child cannot increase authority or transfer capabilities through frame data. Event publication, new-root triggers, guest callbacks and durable writes pass through the same authority checks.

In-process native plugins are *trusted process code*. The capability API does not isolate a malicious native library from host memory or syscalls. Guest interpreters restrict only capabilities they can enforce. Process-isolated plugins offer OS-backed containment where configured. Remote plugins require authenticated transport and enforcement at the remote boundary. These are different trust levels with the same logical plugin contract.

Audit grants at meaningful capability boundaries, denials, policy changes and delegation. Detailed per-invocation attenuation traces remain opt-in rather than unconditional hot-path logging.

Secrets are host-managed operation-scoped resources, ideally exposed through `credential.use`-style imports. Secret bytes never enter contribution descriptors, frames, inspection or generation hashes. Changing the selected generation cannot silently grant a different provider access to credentials. Host-pinned Environment bindings remain fixed across compatible generation selection.

## 9. Structured concurrency, native async and retirement

Every root pins one generation and owns a cancellation/task scope. Child tasks are structured descendants. Detached work requires an explicit host-owned lease and independent authority. A root cannot fully settle while a child may still run. A generation retires only after it stops admitting roots and all outstanding valid leases settle, or after an authorized isolation boundary safely terminates the work.

Cancellation is cooperative for trusted in-process native code and for guests without enforced preemption. Dropping a caller future or channel is not proof a plugin stopped. Timeouts and workload caps are opt-in policy, not arbitrary Core defaults. A noncooperative native call can delay retirement indefinitely; only isolated environments can promise stronger forced termination.

The native ABI is separately versioned, C-compatible and async-capable. It must support immediate or pending completion, one correlated settlement, cancellation correlation, wakeup/poll equivalence, and lease retention while callbacks or native code remain live. Exact C structs and callback versus poll encoding are implementation choices.

ABI invariants are fixed: no unwinding across the boundary, defined buffer ownership and lifetimes, no TLS-based authority, explicit callback reentrancy and threading policy, no host scheduler blocking, guarded late callbacks, and safe unload preconditions. Blocking providers run in a managed blocking executor or process boundary. Native loader is intrinsic; guest runtimes are ordinary native ABI provider plugins. A guest's own identity and authority never collapse into its adapter's identity.

## 10. Events, roots, and delivery

Root-scoped events carry their originating generation and provenance. Ambient unscoped events reach only the default generation. Listeners observe established facts; a Listener starting another root needs an explicit trigger grant and admission.

Transient events are best-effort, with no universal completion-order guarantee. Durable publications have stable event IDs and ordered journal commit positions *within a stream*, but default delivery is at-least-once and may reorder or duplicate after replay. An explicitly requested `ordered-per-stream` subscription must not dispatch a later position until the preceding one is settled per its published poison-message and acknowledgment policy. Providers reject unsupported ordering instead of silently downgrading. There is no cross-stream total order or exactly-once external side-effect guarantee.

## 11. Durable state and trial policy

Each namespace declares owner plugin, schema epoch, reader/writer compatibility, access mode and migration rules. Executable generation residency does not transfer durable ownership or grant write authority.

Trial modes are `read-only`, `isolated`, `shared`, or `migration-trial`. Shared state requires explicit authority and compatibility checks. Isolated trials require a separately authorized storage binding and cannot silently change an Environment pin. Destructive trials prefer isolated or read-only state where available.

Mutable writes must include expected revision or an equivalent conflict check and an authorized writer lease. Migrations acquire an exclusive or explicitly coordinated dual-read/dual-write transition lease. A monotonic fencing epoch invalidates writes from older leases, including writes by still-resident executable generations. Promotion checks resident reader/writer compatibility. Schema epochs plus fences are sufficient until an independent persistence-generation identity is justified.

## 12. Product and configuration semantics

Portable configuration may include, exclude, replace, override or lock selected providers and profile defaults. Resolve exclusions and substitutions before exact implementation dependency closure. No disabled default is silently reintroduced. Basic and Full are replaceable reference profiles, not kernel dependencies.

Nix installs artifacts and can generate the same portable config as CLI, files or another client. Nix cannot be the authority for provider choice, graph composition or permission grants.

## 13. Conformance and migration gates

1. A service-only plugin defines an interface, implementation and provide annotation, with generated descriptors and no manual registration or workflow dependency.
2. A service-only root runs with no plan or agent plugin installed.
3. A non-agent plan uses cycles, map fan-out, all join policies, compile-time subplan inclusion and typed frame extensions.
4. Input enumeration reversal yields the same frozen contributions, bindings, compiled IR and generation digest.
5. IR patch conflicts, stale references, owner-only metadata edits, invalid outcomes and authority escalation reject before activation with both contributors identified.
6. Native ABI conformance covers async settlement, ownership, thread/reentrancy requirements, cancellation and unload; a Lua guest calls a Rust import through a scoped adapter while Phenix.nvim remains a separate ACP client.
7. Generation trials, promotion and rollback preserve pinned roots, leases, event affinity, Environment constraints and durable fencing.
8. Security tests reject capability-bearing frame values, unauthorized data-reference resolution, stale durable writes and secret inheritance across provider replacement.
9. Migrate real Basic/Full product entrypoints through the IR after streaming, usage, tool, cancellation, progress and side-effect parity. Then remove legacy loop execution, static catalog, duplicate hooks and replaced plugin machinery.
10. The dependency-free Core fixture must never import AI-domain crates. Non-agent proofs and focused tests are required in addition to green CI.

The implementation order remains: #728 typed contribution foundation and #726 generic execution foundation; #729 IR patch composition and #730 kinds after #728; #731 real consumers once both are complete. The merged #734 is the native ABI target specification, not an implementation. Cleanup described by #733 follows successful consumer migration.

## 14. Explicitly deferred implementation details

Exact IR wire schema, hash algorithm, ABI table layout, polling versus callbacks, durable journal engine, and OS-specific sandbox strategy require implementation RFCs and platform tests. Those decisions cannot weaken the semantic guarantees above.

The kernel should remain small in concepts even when its implementation becomes substantial. A new kernel mechanism requires a demonstrated general requirement that cannot be expressed by the existing resolver, typed service invocation, execution plan, Layer, Listener, or plugin-defined contribution.
