# Debt Cleanup Evidence And Disposition

Date: 2026-11-09. Base: `2b6b545dd000b40c2c535cefe0c50b87b9ea5283`.
Branch: `impl/debt-cleanup-stacked-20261109`. PR: pending.

## Current Redesign Context

The base/branch above identifies the earlier debt-cleanup task, not the current
operation redesign branch. Current baseline is
`5469c55b27ff0e6e9072f9ca923f29a854f3bafd`, branch
`impl/operation-interface-20261109`, with prior uncommitted work preserved. The
final implementation commit is forthcoming and unknown. The
[operation implementation record](operation-interface-redesign.md) owns the
current architecture/type audit, complete API-removal ledger and final gates.

Typed operation views, independent public Module layouts/projections, singular
Node/Module output enforcement, scheduler zero-or-one emission admission and
cross-kind edits through existing `graph.replace` are present in source. Old
representation-specific endpoint methods, grouped exposure spelling, duplicate
named endpoint structs, NamedNodeHandle endpoint methods and raw inference/export
helpers have zero obsolete Rust references in the measured packages/examples/docs
surface. Raw `ModuleInput`, `ModuleOutput`, `ModuleInputTarget`,
`ModuleOutputSource` and `ModuleDefBuilder` remain physical compiler API, not new
canonical wrappers. Root exposure and typed Module exposure remain dual paths.

R12/R14's intermediate field helper examples below are superseded for ordinary
operation authoring by `inlets().port(...).field(...).target()?` and one `output()`.
`Module::new` now returns Result. Owner typing rejects unrelated descriptors but
does not prove custom erased per-port reconstruction or static frontier validity.
Current LocalPortLayout derive emits SLOT descriptors; encoded descriptors are
manual pending main's common derive. No runtime port/module/operation wrapper,
readiness barrier or atomicity across separate projection invocations is claimed.

Handoff reports Graph tests/doctests passed. Workspace testing encountered a host
portable schema-error wording assertion; main fixed it and reports the host suite
passed. Final workspace/feature/tooling checks, exact commit evidence and the
later independent operation reviewer/ratings remain pending. Earlier measurements,
review findings and numeric debt-cleanup assessments below retain their original
scope; they are not current redesign acceptance or a fresh independent rating.
R08/R10/R15 remain qualified as below. This documentation pass runs no Cargo tests.

## Debt-Cleanup Scope

This shared-repository final report inventories the current implementation, tests
and evidence. The implementation includes tracked and untracked worktree files;
its disposition remains provisional until the combined implementation is committed
and final-head verification is attached. It is not a final-head acceptance certificate.

The baseline `cargo test --workspace --locked` was actually executed and passed.
The iteration measurements below are retained with their workload qualifications;
neither the baseline pass nor those measurements certify the final combined tree.
Test references identify regression assertions, not a final-head pass. R15 remains
open pending an identified final implementation commit and its command outputs.

## Evidence Inventory

R01-R15 below follow the requested review mapping, not a new topic numbering. "Corrected"
means a correction and corresponding assertions are present in the source read;
it does not mean independently verified at the final head. Paths are relative to
the repository root. Test files are existing base coverage or added worktree
coverage as indicated; additions to an existing file are called out explicitly.

### R01: Reclaimable Schema Identity

- **Owner and invariant:** `phenix-schema` owns canonical descriptor encoding;
  Graph metadata, values, interfaces and diagnostics own their identity lifetimes.
  Complete descriptor content, never pointer identity, is wire authority. Local
  Rust `TypeId` remains distinct from wire identity.
- **Failure and smallest correction:** process-lifetime interned strings retained
  distinct unloaded schemas. `Schema::identity() -> Arc<str>` now allocates an
  owned identity with no global interner/strong cache. Consumers share clones and
  release their strong ownership on drop; independent equal descriptors need not
  share an allocation. `ContractValue::schema()` replaces nominal fallback.
- **Evidence:** `packages/phenix-schema/src/lib.rs` contains the new repeated-unload,
  content-authority and ignored retention tests. New
  `packages/phenix-graphs/src/schema_ownership_tests.rs` checks graph/action/value/
  report ownership, diagnostic clones and independent derived values. Existing
  `packages/phenix-storage-graph/tests/shared_schema.rs` exercises shared descriptors.
- **Disposition:** corrected in source. A retained report or snapshot legitimately
  retains an identity. Weak-reference ownership evidence is not RSS evidence;
  canonical string construction still costs allocations. See measurements.

### R02: Generation Publication Lock Scope

- **Owner and invariant:** Kernel owns producer serialization/retirement; immutable
  generation readers pin the engine, artifacts and leases independently. Old
  producers must never admit notices after successful retirement.
- **Failure and smallest correction:** a generation-wide mutex serialized explicit
  waves/read callbacks with publication. `Generation` now owns its `Kernel`
  directly, while Kernel locks only producer state. Publication waits for an
  in-progress producer drain, retires/takes its producers, and swaps the active
  generation without acquiring a lock held by explicit waves or read callbacks.
- **Evidence:** `packages/phenix-kernel/src/host.rs`, `src/lib.rs`; existing
  `tests/generations.rs` adds
  `publication_does_not_wait_for_old_explicit_wave_or_callback`. Existing
  `tests/generation_retirement.rs` checks a blocked poll completes its entire
  drain before retirement, then rejects all old drains and checks destruction.
- **Disposition:** corrected lock scope, supported by barrier/channel assertions.
  These are ordering/liveness tests, not latency measurements. Slow producer
  polling, producer-fired waves and destructors can still delay publication.

### R03: Blocking Driver Liveness And Terminal Errors

- **Owner and invariant:** Kernel owns blocking-driver wake/shutdown coordination
  and producer retirement. Idle or empty drivers must wait without hot spinning;
  permanent poll failures must not become an endless stream of error events.
- **Failure and smallest correction:** `run_until` uses the wake generation and
  condition variable to wait only while no wake or sticky shutdown has occurred.
  `Producer::poll` errors are terminal: `drain` reports the cause once, detaches the
  failed producer, repairs the round-robin cursor and drops detached producers
  outside the state lock. Transient unavailability must return `Ok(None)` and wake
  when ready. Driver setup rejects `NotStarted` and keyed `UnsupportedWake`.
- **Evidence:** `packages/phenix-kernel/tests/kernel.rs` includes
  `empty_driver_blocks_until_cross_thread_shutdown`,
  `permanent_poll_failure_is_reported_once_and_driver_remains_shutdown_capable`
  and shutdown-during-poll/unsupported-wake coverage. Unit tests in
  `packages/phenix-kernel/src/lib.rs` check terminal destructor reentry after
  detachment and round-robin/idle poll budgets; `tests/callback_errors.rs` checks
  typed driver setup errors and preserved poll causes.
- **Disposition:** corrected identified idle/error hot loops. Shutdown is checked
  between work, not a means of interrupting a running poll, wave or callback.

### R04: Finite Wave Execution Bound

- **Owner and invariant:** Graph's execution session owns per-wave dispatch limits,
  completion tickets, cancellation and structured reports. Kernel forwards trusted
  deployment configuration rather than imposing a separate scheduling policy.
- **Failure and smallest correction:** a productive cycle could keep synchronous
  `fire` running indefinitely. `DEFAULT_MAX_ACTIVATIONS` is **1024**;
  `Deployment::max_activations` and `Engine::with_max_activations` override it.
  `WaveError::BudgetExhausted` reports limit/count; exhaustion invalidates outstanding
  work when another dispatch is required. Cancellation suppresses late results.
- **Evidence:** new `packages/phenix-graphs/tests/execution_budget.rs` covers
  productive cycles, synchronous reports, exact-limit DAG success, independent
  waves, zero budget, invalidated tickets and cancellation during dispatch. New
  `tests/scheduler_lifecycle.rs` covers outstanding completions at the exact limit
  and cancellation precedence. New `packages/phenix-kernel/tests/execution_options.rs`
  and existing storage-graph `tests/host_derived.rs` exercise overrides through
  `BuildOptions { imports, execution: ExecutionLimits { max_activations: ... } }`,
  KernelHost and DerivedHost.
- **Disposition:** corrected unbounded activation count, not callback preemption,
  a wall-clock timeout, memory bound or rollback. Large valid DAGs may require a
  host override. Side effects of an already executing action remain observable.

### R05: Derived Guard Construction And Preparation Capabilities

- **Owner and invariant:** KernelHost owns candidate preparation and publication;
  binding must validate exact candidate exports/deployment identities before
  producer startup without receiving ordinary admission access.
- **Failure and smallest correction:** a binding callback taking `&Kernel` could
  call `engine()` and execute an unpublished candidate. `prepare` now passes
  `&host::Binding`, exposing typed exports, action node IDs and trigger targets,
   but no engine, Kernel, query handle, fire or drain API. Candidate remains opaque.
   Build options carry only Graph-owned `ExecutionLimits`; Kernel retains the
   validated Deployment, with no callback that can build/fire or substitute it.
- **Evidence:** `packages/phenix-kernel/src/host.rs` includes compile-fail examples
   for engine/fire access. `BuildOptions` compile-fail examples also reject building
   execution limits, substituting a Deployment, and the removed deployment hook.
   Existing `tests/generations.rs` checks failed binding
  leaves the active generation unchanged; new `tests/callback_errors.rs` checks
  the binding cause. Existing
  `packages/phenix-storage-graph/tests/host_derived.rs` validates query specs through
  `Binding` before startup. `QuerySpec::validate_binding` is in `src/derived/query.rs`.
- **Derived guard invariant:** storage-graph owns host-only query dispatch policy;
  Kernel remains storage-independent and Graph remains the only scheduler.
  Nested queries must reject on dispatch and delegated execution threads.
- **Failure and smallest correction:** low-level host integration could omit the
  derived executor guard. New `DerivedHost::prepare` always installs
  `DerivedRuntime::guard_executor`; `DerivedGeneration::query` binds the exact
   published engine and preserves the Kernel start guard.
   Data-only build options expose no per-node executor override; a compile-fail
   example on `DerivedHost` checks that this guard bypass is unavailable.
- **Evidence:** new `packages/phenix-storage-graph/src/derived/host.rs`; existing
  `tests/host_derived.rs` now covers cross-runtime rejection for local/worker
  dispatch and old/new generations. Existing `tests/derived.rs` covers recursive
  query denial, dispatch/node panic and error restoration, and one delegation.
- **Final independent review:** caught an unsafe deployment customization hook
  that could build/fire or substitute the validated candidate, and a per-node
  executor override that could bypass the derived guard. Both were removed.
  `BuildOptions { imports, execution: ExecutionLimits }` is data-only and
  nonbuildable, with no deployment hook or executor override. Kernel constructs
  and retains the validated Deployment; DerivedHost wraps the supplied executor.
- **Disposition:** corrected supported capability boundary and adapter, not a
  sandbox. Explicit low-level `bind_engine`
  and arbitrary unguarded engines remain escape hatches; detached application
  threads are not globally sandboxed. No second runtime/scheduler was added.

### R06: Full Import Policy Validation

- **Owner and invariant:** Kernel validates the trusted host's entire ImportPolicy
  against the candidate inventory before composition. Stale or misspelled keys in
  any policy table must fail closed, including entries that selection would not use.
- **Failure and smallest correction:** checking only chosen selections could ignore
  invalid allow/deny entries. `Kernel::build_with_options` validates every
  requirement and offer key in `selections`, `allow` and `deny` before composing.
  Eligibility still requires compatible definitions, inclusion in an optional
  allow set and absence from deny; denial wins. Explicit ineligible selections,
  missing candidates and ambiguous candidates reject rather than silently falling
  back. Selected providers become consumer dependencies.
- **Evidence:** `packages/phenix-kernel/src/lib.rs` contains the full-table preflight
  and selection logic. `tests/declarative_imports.rs::every_policy_table_rejects_unknown_requirement_and_offer_keys`
  checks both unknown-key positions in all three tables and zero compositions;
  the same file covers allow/deny precedence, ineligible selections, ambiguity,
  missing providers, dependency ordering and reload selection.
- **Disposition:** corrected full policy preflight. Policy controls declared import
  resolution, not arbitrary native composition code or external effects.

### R07: Candidate Effects And Additional Storage Findings

- **Owner and invariant:** KernelHost owns candidate preparation, publication and
  candidate-owned resource cleanup. Failed preparation must leave the active
  generation unchanged, but arbitrary application effects are not transactional.
- **Correction and evidence:** the R05 nonadmitting `Binding` and data-only options
  remove supported prepublication admission capabilities. Existing
  `packages/phenix-kernel/tests/generations.rs` covers failed binding and unchanged
  active state; `tests/callback_errors.rs` preserves the binding cause. Dropping a
  failed candidate releases owned artifacts/producers/leases, but cannot undo
  effects already performed by constructors, composition, binding or factories.
- **Disposition:** candidate isolation is not a sandbox or universal rollback.
  Exported application values and arbitrary native callbacks remain trusted.

#### Additional Findings: Storage Callbacks, Locks And Preflight Effects

- **Owner and invariant:** Storage owns registry/mount/watch locks and notification
  publication; host-selected backend factories must not run for invalid or already
  reserved mounts. User waker behavior must not run under storage mutation locks.
- **Failure and smallest correction:** waker wake/drop callbacks could reenter
  while locks/outer reentrancy guards were held; backend opening could create or
  migrate files before duplicate-name rejection. `State::emit` collects owned
  `Arc<Waker>` references; wake and replaced-waker destruction occur after locks
  and the outermost active operation end. `mount_with` reserves a valid unused
  name before opening without registry locks; RAII releases reservations on failure
  or unwind. Storage-plugin rechecks SQLite path aliases at activation.
- **Evidence:** added unit tests in `packages/phenix-storage/src/lib.rs` cover wake,
  replacement destruction, nested error/unwind and teardown. Existing
  `tests/storage.rs` adds factory contention/retry and rejected-file-effect tests.
  Existing `packages/phenix-storage-plugin/src/lib.rs` adds
  `rejected_offer_mounts_do_not_invoke_factory` and
  `activation_rechecks_aliases_introduced_after_prepare` (Unix).
- **Disposition:** corrected identified lock/preflight paths. Factory effects after
  valid reservation are still external effects, not reversible transactions.
  Existing files are not deleted on failed activation. Rechecking paths does not
  eliminate concurrent filesystem replacement/TOCTOU or establish a sandbox.

### R08: Live Inspection And Contract Catalog Cost

- **Inspection owner and invariant:** Graph authoring owns effective pending
  declarations; checks and projections must observe current retained-handle edits.
- **Inspection correction:** `with_inspected` resolves current declarations,
  borrows claims and builds a shape without cloning the root declaration or
  materializing unnecessary snapshots. Sealed `GraphUnit::check_interface` permits
  direct checks; public `inspect()` still returns an owned `UnitSnapshot`.
  Root/module boundary validation is retained, not replaced by a stale cache.
- **Inspection evidence:** `packages/phenix-graphs/src/authoring/interfaces.rs` and
  `src/interface.rs`; new `tests/inspection_live.rs` covers retained views,
  projections, shape restoration and invalid pending root routes. Existing
  `src/inspection_tests.rs` and `tests/canonical_interfaces.rs` retain coverage.
  `tests/inspection_cost.rs` measures root, snapshot, bind, claim, repeated checks,
  projections and catalog checks. All these paths are under `packages/phenix-graphs/`.
- **Inspection limitation:** fewer copies do not make inspection constant-time.
  Root/module checks still build validation catalogs and expand boundaries; root
  typed endpoint projection remains unsupported in the repeated-cost harness.

- **Owner and invariant:** Graph validates complete contract definitions, claims
  and requests, including unused declarations; Kernel must not weaken this check
  to only selected offers.
- **Failure and smallest correction:** catalog consistency copied contract IDs and
  definitions during checking. `CompiledGraph::check_contract_catalog` now builds
  a borrowed `BTreeMap<&str, &GraphContract>` and compares authoritative definitions.
- **Evidence:** `packages/phenix-graphs/src/interface.rs`; existing
  `packages/phenix-kernel/tests/contract_catalog_consistency.rs` exercises catalog
  disagreement, while Graph `tests/inspection_cost.rs::repeated_inspection` times
  catalog checks alongside repeated inspection/projection.
- **Disposition: partial.** Per-check owned copies were removed, but the whole
  compiled catalog walk/map rebuild persists. Root inspection also still constructs
  a validation catalog. There is no revision-indexed incremental catalog cache and
  no measured catalog-specific improvement supplied here. A cache would require
  an explicit invalidation/ownership design, not skipped validation.

### R09: Builder Clone/Replay And Incremental Registration

- **Owner and invariant:** Graph owns declaration IDs, registration indexes and
  rollback of append-only additions. Existing handles and pending-map identity
  must survive failed unrelated additions; escaped rolled-back handles must not
  become valid when names/addresses are reused.
- **Failure and smallest correction:** whole-builder clones copied growing prior
  state and risked disconnecting configuration ownership; replaying an authoring
  closure could repeat external effects. `GraphBuilder::additions` uses a restricted
  `GraphAdditions` view and length/counter checkpoints, truncating only additions
  on error or unwind. Name/ID indexes and the next-ID cursor remove repeated
  registration scans. Derived source/query authoring uses this transaction without
  cloning prior state or replaying the closure.
- **Evidence:** new `packages/phenix-graphs/src/authoring/additions.rs` contains
  nested rollback, panic, ID exhaustion, escaped-handle and interior-configuration
  tests. Existing `packages/phenix-storage-graph/tests/derived.rs` adds late-failure
  and handle-preservation assertions plus ignored
  `builder_scaling_repeated_derived_additions`. New
  `packages/phenix-graphs/tests/authoring_registration.rs` isolates registration
  timing. Implementation also resides in Graph `src/authoring.rs`, `src/lib.rs`
  and storage-graph `src/derived/source.rs`, `query.rs`.
- **Disposition:** corrected append-only authoring, not a general edit transaction.
  Separately held configuration handles can still edit existing declarations;
  those edits and external callback effects are explicitly not rolled back.
  Connection/edge validation can still scan growing collections.

### R10: Module Boundaries

- **Owner and invariant:** Graph owns scheduling/authoring; the optional proc macro
  owns generated record codecs. Extraction must preserve public paths and runtime
  behavior instead of inventing interchangeable parallel implementations.
- **Failure and smallest correction:** unrelated scheduler, codec-generation and
  transaction responsibilities were embedded in large modules. They now have
  focused implementation files: `packages/phenix-graphs/src/scheduler.rs`,
  `packages/phenix-graphs/port-args-derive/src/contract_value.rs`, and
  `packages/phenix-graphs/src/authoring/additions.rs`. Root reexports retain the
  scheduler's ordinary public API. Portable conformance helpers also live in
  `packages/phenix-graphs/src/testing/portable.rs`.
- **Evidence:** existing Graph `src/tests.rs`, `tests/assembly_cancel.rs`,
  `tests/manual_api.rs`, new `tests/scheduler_lifecycle.rs` and `tests/portable_codec.rs`,
  and transaction-local unit tests cover the moved responsibilities.
- **Disposition: partial.** Scheduler, derive codec and transaction extraction is
  present. Graph `src/lib.rs`, Kernel `src/lib.rs`, Storage `src/lib.rs` and
  storage-plugin `src/lib.rs` remain large mixed-responsibility modules. The
  scheduler itself remains substantial. File movement is not evidence that all
  coupling or maintainability debt is removed.

### R11: Preserve Callback Causes And Identity

- **Owner and invariant:** plugin/kernel phase envelopes own callback causes and
  identify the plugin/contribution or producer where applicable. Callers must be
  able to inspect the original concrete error, not only formatted text.
- **Failure and smallest correction:** String callback boundaries and the host's
  string-only start wrapper erased causes. Read definition:
  `packages/phenix-plugins/src/error.rs:5` defines
  `CallbackError = Box<dyn std::error::Error + Send + Sync + 'static>`.
  Compose, activation, poll and binding use this owned error. `ResolutionError`,
  `StartError`, `ProducerError` and `LifecycleError` expose sources. Plugin
  `Failure::with_source`/`CallbackContext` retain construct/contribute/bind context;
  driver setup distinguishes `NotStarted` from keyed `UnsupportedWake`.
- **Evidence:** new `packages/phenix-kernel/tests/callback_errors.rs` downcasts the
  exact cause and checks its `Arc` identity for compose/start/poll/bind. Added
  `packages/phenix-plugins/src/manager.rs` tests inspect nested Failure/io sources
  and phase identity. Existing plugin `tests/graph_contracts.rs` covers binding;
  manager also rejects replacement of the registrar owner before accepting inventory.
- **Disposition:** corrected callback boundaries, not universal typed diagnostics.
  Metadata/admission/wave paths still include strings. Callers that stringify an
  error before boxing it have already lost the type; this API cannot recover it.
  `Failure` is no longer a blanket cloneable string record.

### R12: Exact Grouped Endpoint Identity

- **Owner and invariant:** Graph owns `(port, slot)` endpoint identity, declaration
  lineage and local/wire checks. Nested field paths are complete slot names, not
  a new punctuation grammar or an implicit readiness barrier.
- **Failure and smallest correction:** grouped consumers repeated raw names and
   symbolic dotted shorthand could not unambiguously split arbitrary port/slot
   pairs. The intermediate cleanup used `expose_input_field`, module/bound field projections and
  `public_input_slot` delegate to exact existing slot lookup. `public_input(name)`
  continues to mean the literal `(name, "value")` pair.
- **Evidence:** new `packages/phenix-graphs/tests/grouped_authoring.rs` and
  `tests/grouped_wire_authoring.rs` cover nested exports, independent execution,
  subtraction, duplicates, malformed/nonexistent paths and type/mode rejection.
  Existing `tests/clone_lineage.rs` and `tests/layout_handle_identity.rs` retain
  stale/divergent declaration coverage. See `docs/grouped-authoring.md`.
- **Disposition:** corrected typed/exact-slot path. Symbolic dotted shorthand is
  intentionally not extended. Grouping alone adds no join/readiness policy, and
   manually constructed selectors still require runtime declaration validation.

Current operation addressing supersedes the intermediate module/bound helpers:
use owner-typed views and explicit checked `target()`. Root `expose_input_field`
remains alongside typed `Module::expose`; the overlap is a residual API risk.

### R13: Portable Codec Evidence And Generator Hygiene

- **Owner and invariant:** codec authors own byte correctness; shared schema
  agreement is not proof that arbitrary codec implementations honor it. The
  derive must reject malformed framing without domain member names shadowing its
  internal decoder state.
- **Failure and smallest correction:** positive round trips alone cannot establish
  portable byte agreement. `testing::portable::{GoldenVector, check}` compares full
  descriptors, canonical wire identity, exact encoding, decoding and malformed
  rejection. Generated decoders put field identifiers only in struct members,
  check offset/length arithmetic, enforce option tags and reject trailing bytes.
- **Evidence:** new `packages/phenix-graphs/tests/portable_codec.rs` probes descriptor
  fields, dishonest identity/encode/decode implementations, decoder-local names and
  raw identifiers. Existing `tests/fixtures/portable_execution.rs` adds independent
  manual/derived record golden vectors and truncation/tag/UTF-8/overflow/trailing
  failures; `tests/portable_wire.rs` drives the separately packaged fixtures.
- **Disposition:** corrected concrete generator/evidence gaps. Helpers test supplied
  vectors, not arbitrary codec honesty, exhaustive fuzzing or native ABI safety.
  Derive-only cases in the current portable-codec file are feature-gated; their
  successful execution without default features is not asserted here.

### R14: Provider/Consumer Authoring Ergonomics

- **Owner and invariant:** existing GraphBuilder and owned `Input<T>`/`OutputRef<T>`
  handles remain the authoring model. Contract binding may project only promised
  endpoints, and metadata cloning must not require cloning domain values.
- **Failure and smallest correction:** provider/consumer examples needed repeated
  raw names and unnecessary tuple scopes to end a binding borrow. Typed
  `SlotField::then`/`SlotPath::then` selectors and direct `bind_contract` projections
  now work with ordinary borrow ending after last use. Manual descriptor Copy/Clone
  implementations avoid phantom owner/leaf Clone bounds.
- **Evidence:** new `packages/phenix-graphs/examples/grouped_contract.rs` is a
  macro-free provider/consumer example; new `tests/grouped_wire_authoring.rs`
  asserts descriptors work for noncloneable domain types. New grouped tests and
  `docs/grouped-authoring.md` show both local and wire patterns.
- **Disposition:** improved without another ownership facade, adapter registry or
  compatibility builder. Typed selectors do not certify field existence. Local
  owned-field execution still depends on payload-retention policy; the local
  regression uses `ReportRetention::TerminalOutputs`.

### R15: Final-Head And Long-Run Evidence

- **Owner and invariant:** implementation/release reviewers own reproducible
  command results tied to a specific source revision and feature configuration.
  Performance evidence must identify what is measured and what is not.
- **Failure and smallest correction:** mixed iteration results and ignored timing
  harnesses cannot certify final-tree behavior or long-run retention. Preserve
  reported results separately and rerun acceptance on the final combined head.
- **Evidence:** ignored schema/inspection/registration/derived-builder harnesses
  listed above; existing `packages/phenix-graphs/benches/ownership.rs` runs 1,000
  waves per retention strategy and counts payload clones, and `benches/layout.rs`
  runs 20,000 fires for its execution comparisons. These finite microbenchmarks
  are not a prolonged reload/publication/queue stress workload.
- **Disposition: partial.** Benchmark scaffolding and focused ownership assertions
  exist; no exact-head long-run benchmark result is available. No allocator/RSS,
  concurrency-tail-latency, sustained reload throughput or universal no-regression
  claim is made. No final-head test pass is claimed.

## Public API Delta

These before/after references come from the base-to-worktree diffs and current
source, not suggested compatibility shims. This cleanup changes source APIs; it
does not retain a nominal legacy fallback.

| Area | Before | Current | Source reference |
|---|---|---|---|
| Wire descriptor | `ContractValue::SCHEMA`, optional `schema_descriptor()`, nominal fallback | Required `ContractValue::schema() -> Schema`; `wire_schema()` uses full identity | `packages/phenix-graphs/src/lib.rs`; schema README |
| Identity ownership | Interned/static string identity, Copy metadata | `Schema::identity() -> Arc<str>`; owned/cloned metadata, borrowed accessors | `packages/phenix-schema/src/lib.rs`; Graph `src/authoring.rs` |
| Kernel configuration | `build_with_policy(..., &ImportPolicy)` | `build_with_options(..., BuildOptions { imports, execution: ExecutionLimits })`; data-only, nonbuildable, no deployment hook; default `build` retained | `packages/phenix-kernel/src/lib.rs` |
| Host preparation | `prepare` / `prepare_with`, binding callback received `&Kernel` | One `prepare(..., BuildOptions, ..., FnOnce(&Binding) -> Result<(), CallbackError>)` | `packages/phenix-kernel/src/host.rs` |
| Derived host integration | Low-level host callback/engine binding, caller must arrange guard | `DerivedHost::prepare` always guards; `DerivedGeneration::query`, `QuerySpec::validate_binding` | storage-graph `src/derived/host.rs`, `query.rs` |
| Callback failures | String causes; host start formatted to String | Owned `CallbackError`, source-preserving phase envelopes; `LifecycleError::Start(StartError)` | plugin `src/error.rs`; kernel `src/lib.rs`, `host.rs` |
| Wave limits | No finite productive-cycle dispatch cutoff | Default 1024; deployment/engine override; structured exhaustion report | Graph `src/deployment.rs`, `src/scheduler.rs` |
| Incremental authoring | Derived operations cloned growing builder state | Restricted `GraphBuilder::additions` / `GraphAdditions` | Graph `src/authoring/additions.rs`; storage-graph derived source/query |
| Grouped authoring | Historical raw input-slot names, representation-specific field helpers and scoped binding idioms | Operation `inlets().port(...).field(...).target()?`, singular output; explicit Module interface/expose/finish; root exposure retained | Graph `src/authoring/operation.rs`, `authoring/interfaces.rs`, `authoring/addresses.rs` |
| Backend construction | `mount_boxed` validates only after caller created backend | `mount_with` reserves before factory; `mount_sqlite`/`mount_offer` use it | storage `src/lib.rs`; storage-plugin `src/lib.rs` |

Primitive and record payload bytes are intended to remain unchanged, but full
wire identities replace name-only envelopes. Persisted name-only envelopes need
explicit migration; no implicit compatibility or native shared-library ABI is
promised. The portable fixtures remain in-process Rust packaging evidence.

## Abstraction Audit

| Abstraction/change | Why it exists | Boundary and remaining cost |
|---|---|---|
| `Arc<str>` identity | Reclaimable metadata shared by actual consumers | No cache owner; equal independent identities can allocate separately |
| `with_inspected` / sealed direct check | Avoid snapshot/claim copies while preserving live validation | Internal path, not a second matcher or invalidation cache |
| `GraphAdditions` | Bound rollback to append-only state without cloning prior declarations | No replace/remove/configuration rollback; existing external effects remain |
| Registration name/ID indexes | Avoid growing repeated declaration scans | Index maintenance is part of rollback/clone/replacement invariants |
| Producer-state mutex | Serialize only producer admissions and retirement | Drain/poll/wave duration can still delay retirement |
| `Binding` | Preparation-specific nonadmitting inspection | Exported arbitrary values are trusted, not capability-safe by construction |
| `BuildOptions` | Carry import decisions and Graph-owned `ExecutionLimits` | Data-only limits; no deployment ownership, admission, graph substitution or per-node executor override |
| `DerivedHost` / `DerivedGeneration` | Make the store-owned dispatch guard mandatory on the supported host path | Thin adapter, not a new scheduler; low-level engine access still exists |
| `CallbackError` and phase envelopes | Preserve owned sources and lifecycle identity | Dynamic Rust error object, not cross-library ABI or typed wave reconstruction |
| `mount_with` / reservation guard | Reject invalid/competing names before external opening effects | Does not undo a valid factory's external effects |
| Deferred waker actions | Separate application callbacks/destruction from locks and nested mutation guards | Thread-local deferral, not a new task executor or wait-free API |
| Typed field helpers | Reuse existing exact endpoint lookup without repeated names | No alternate endpoint grammar, ownership layer or readiness semantics |
| Portable conformance helper | Reuse descriptor/byte/vector assertions across manual and derived codecs | Test support, not runtime codec certification |

No new general cache, universal backend/query AST, scheduler, native ABI adapter,
or backward-compatibility layer is evidenced by these corrections. Extraction
alone does not justify a quality score increase unless it clarifies an actual
ownership/validation responsibility.

## Reported Measurements

Numbers are supplied/reported iteration results, not rerun here and not tied to
an identified final implementation commit. Do not combine them into a claim of
whole-system speedup.

| Workload | Before | After | Interpretation |
|---|---:|---:|---|
| Schema, 10,000 distinct descriptors: retained canonical string bytes | 698,890 | 0 | Weak ownership probe after strong consumers drop, not RSS/free-list accounting |
| Schema final incremental batch time | 16.18 ms | 5.98 ms | Rounded single-release-run figures; last batch adds 9,000 descriptors, totals are cumulative |
| Inspection, named 1,000 nodes: root | 4,808 us/op | 2,782 us/op | Root operation, not constant-time lookup |
| Inspection, named 1,000 nodes: bind | 4,917 us/op | 2,774 us/op | Full validation retained |
| Inspection, nested 1,000 nodes: root | 4,686 us/op | 3,837 us/op | Expansion/validation cost remains |
| Repeated derived builder, 1,000 total nodes | 42.45 ms | 3.50 ms | Incremental authoring, not compilation/execution or isolated rollback |
| Publication | No timing supplied | No timing supplied | Barriers establish ordering/independence only |

`packages/phenix-schema/README.md` records the more precise schema figures
16.184144 ms and 5.977940 ms and smaller descriptor counts. It explicitly describes
single runs and the extra Weak/drop checks in the after harness.
`tests/inspection_cost.rs` uses a warm-up and median of 11 batches of 16 operations,
excluding graph construction/cloning from the timed operation. The derived-builder
harness uses 21 samples and excludes fixture setup, compile, execution and graph
destruction. The separate registration harness has a different workload and must
not be presented as the 42.45-to-3.50 ms result.

No supplied figure measures catalog-only gains, callback boxing overhead, waker
latency, publication latency, large-payload cloning, steady-state memory, or the
cost of the activation-budget check. Timing assertions are intentionally absent
from the ignored diagnostic harnesses. The 1024 default is a policy limit, not a
performance measurement.

## Review Log

This log records review findings and their implementation dispositions. The final
independent review caught the deployment-hook and guard-bypass defects below;
their removal is reflected in the current source and compile-fail assertions.
Final-head execution remains a separate pending gate.

| Iteration/topic | Finding | Visible correction and remaining qualification |
|---|---|---|
| Ownership pass | Unloaded schemas could retain process-lifetime strings | R01 uses consumer-owned Arc identities; Weak tests cover reports/diagnostics as legitimate owners; no RSS claim |
| Inspection pass | Copy reduction must not make retained bindings stale | R08 re-resolves pending layouts on each check/projection; restoration does not revive old declaration handles |
| Builder pass | Whole-builder snapshots/replayed edits undermine scaling and handle ownership | R09 append-only checkpoint rollback, indexed registration, nested/panic/escaped-handle assertions; external/configuration effects excluded |
| Lifecycle pass | Generation-wide locking blocks publication behind explicit work | R02 producer-only locking plus barriers for blocked explicit work and blocked producer drain; no latency claim |
| Blocking-driver pass | Empty drivers and permanent poll failures can hot spin | R03 condition-variable idle wait, sticky shutdown and one terminal error followed by producer detachment/drop outside the lock |
| Preparation pass | An unpublished candidate's Kernel exposes engine admission | R05 Binding plus compile-fail examples; exported values can still have external effects, which are not rolled back |
| Derived integration pass | Optional guarding can be omitted on worker/host paths | R05 mandatory guard in supported DerivedHost; local and worker tests; arbitrary low-level engines remain outside that guarantee |
| Final independent review | Deployment customization hook could build/fire or substitute the candidate; per-node executor override could bypass the derived guard | R05 removed both paths; BuildOptions contains only imports and nonbuildable ExecutionLimits, with no deployment hook; compile-fail assertions cover admission, substitution and guard override |
| Policy pass | Unknown keys in unused allow/deny entries could escape selection checks | R06 validates every requirement/offer key in selections, allow and deny before composition; denial and compatibility checks remain authoritative |
| Candidate-effects pass | Failed preparation cannot undo arbitrary callback effects | R07 keeps active generation unchanged and drops candidate-owned resources; external effects remain explicitly outside rollback |
| Scheduler pass | Productive cycles need a bound without failing exact-limit completions | R04 finite per-wave budget, cancellation precedence, zero budget and outstanding-completion tests; no callback interruption |
| Diagnostics/authority pass | Formatting loses callback causes; registrar ownership can be replaced | R11 owned sources and phase/key assertions; staging rejects replaced owner; string wave diagnostics remain |
| Storage callback/effects pass | Wake/destructor reentry and premature file/factory effects | R07 additional findings: wake/drop deferral including nested unwind/teardown, name reservation and activation alias recheck; no filesystem race immunity or external rollback |
| Portable/grouped pass | Round trips, raw strings and phantom Clone bounds are insufficient | R12-R14 independent golden/malformed vectors, decoder hygiene, exact typed selectors and manual descriptor traits; not exhaustive codec certification |
| Documentation reconciliation | Earlier `docs/grouped-authoring.md` records a no-default suite blocker from ungated portable derives | Current `tests/portable_codec.rs` gates derive-only imports/types/tests; historical failure is not proof of a current failure or a successful rerun |
| Disposition pass | Catalog walks, large modules and final-head long-run evidence are unfinished | R08/R10/R15 remain partial; no all-debt-closed or >=9 quality claim |

## Independent Quality Assessment

These are conservative, independent source-review judgments on a 0-10 scale,
not averages copied from earlier reports, benchmark scores or acceptance gates.
"Before" refers to the base/diff evidence; "after" is provisional for the read
worktree, discounted for lack of final-head execution. No category is awarded 9
or higher and no overall >=9 claim is made.

| Dimension | Before | After | Justification and unresolved debt |
|---|---:|---:|---|
| Ownership/reclamation | 5 | 8 | Strong ownership is explicit and tested with Weak; no allocator/RSS or long-run final-head evidence |
| Authoring correctness/ergonomics | 6 | 8 | Restricted rollback, exact typed fields and lineage tests; interior configuration/external effects excluded, root projection limited |
| Inspection/catalog scalability | 5 | 7 | Reported inspection gains and fewer copies; catalog rebuild/expansion and some linear lookups remain (R08) |
| Incremental builder scalability | 4 | 7 | Reported authoring improvement and registration indexes; edge validation and limited workload coverage remain |
| Lifecycle/concurrency | 6 | 8 | Smaller lock domain, opaque candidates and explicit retirement; trusted blocking polls/destructors and no latency evidence |
| Execution safety | 5 | 8 | Productive cycles have a structured limit with boundary tests; not time/memory preemption or side-effect rollback |
| Errors/diagnostics | 5 | 7 | Callback sources and lifecycle keys preserved; metadata/admission/node/wave strings still lose type information |
| Modularity/abstraction economy | 5 | 6 | Three meaningful extractions and narrow adapters; substantial monoliths remain (R10) |
| Portable contract evidence | 6 | 8 | Independent exact bytes and malformed vectors complement structural checks; manual codecs trusted, no ABI/fuzz proof |
| Verification/reproducibility | 6 | 6 | Executed passing workspace baseline and inspectable harnesses; combined final-head matrix and long-run result absent (R15) |

## Remaining Gates

1. Identify the final combined implementation head; attach actual outputs for
   `cargo test --workspace --locked`, workspace all-target checks, formatting and
   Clippy with the repository toolchain. This report supplies no such outputs.
2. Verify isolated Graph no-default-feature library/integration cases, the portable
   fixtures and grouped example with/without default features. Workspace feature
   unification is not proof that an isolated no-default configuration works.
3. Repeat ignored release measurements on controlled before/after revisions and
   retain workload/toolchain/sample metadata. Add sustained reload/publication/
   producer/retention evidence before declaring R15 complete; finite microbenchmarks
   do not substitute for a long-run result.
4. Keep R08 and R10 open unless separately scoped work eliminates whole-catalog
   rebuild cost and establishes clearer boundaries in the remaining monoliths.
   Do not add speculative cache/abstraction frameworks just to close an inventory.
5. Preserve the explicit non-guarantees: no arbitrary external-effect rollback,
   callback preemption, durable delivery, native ABI safety, sandbox, filesystem
   race immunity or measured publication latency.

The current disposition is targeted corrections with concrete source-level
regression evidence, three explicit partial items, and final-head verification
pending. It is not a claim that the repository is debt-free.
