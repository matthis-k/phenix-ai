# Operation Interface Redesign: Implementation Record

## Status And Provenance

Baseline: `5469c55b27ff0e6e9072f9ca923f29a854f3bafd` (`5469`), branch
`impl/operation-interface-20261109`. This branch preserves the recovered
uncommitted implementation, including schema ownership, independent endpoint
modes and Store-derived publication provenance. The reference stack is PR #1255
(`2b6b545dd000b40c2c535cefe0c50b87b9ea5283`) followed by PR #1256 (`5469`).
The exact delivered SHA and remote check URLs belong in the delivery report and
draft PR; the baseline SHA is not implementation verification evidence.

Acceptance remains **incomplete until final committed-head verification**. The
earlier report's missing typed interface, single-output enforcement, API removal
and cross-kind substitution are implemented in source, not outstanding gaps.
Review findings have been addressed through the iterations recorded below.
Historical checks in adjacent reports apply only to their named revisions.

## Architecture And Terms

An **operation** is an authoring boundary with an input interface and exactly one
logical output. A **Node** implements it with one action; a **Module** implements
it with a public projection over a subgraph. These relationships are conceptual,
not runtime wrappers. `Operation` is a typed authoring handle, not an executor.
`Module` owns an ordinary `GraphBuilder` during authoring. The existing compiler
expands modules into physical nodes and edges; neither adds an execution layer.

An **input interface** declares visible ports independently of children. An
**input layout** describes one port's domain value and slot frontier, and, for a
physical Node, its value reconstruction/preparation. A **view** selects a port or
structural field; an **input target** is a checked deliverable leaf. A
**projection** maps a public leaf to one or more compatible physical destinations,
possibly through nested boundaries. It is not an edge, reconstruction step, join,
merge, buffer or readiness policy. Edges own dataflow. A Module output projection
selects one existing source; combining producers needs an explicit operation.

A physical `InputSlot` owns delivery/buffer metadata. A physical `Port` owns
readiness and argument preparation. A logical public port may span independently
scheduled children without a shared barrier. Parallel composition preserves their
readiness; sequential composition adds edges. Nested projections compose maps;
repeated instances have distinct physical addresses. Encapsulation preserves
physical dataflow under qualified renaming, not arbitrary behavioral equivalence.

The **Graph root** is a container for nodes, instances, triggers and public routes,
with an optional exposed result. It is not an operation. The shared raw
`ModuleDef` representation therefore has an optional output; typed `Module::finish`
requires one. A Node always declares one output at canonical slot 0, even if an
action emits nothing. Unit is a real output value, not an absent endpoint.

`InterfaceShape` is ID-free boundary metadata. `GraphContract` adds explicit
semantic identity and a required complete shape. Matching checks exact inputs and
output presence, name and identity; additional outputs are not allowed. Claims
remain explicit declaration-owned `GraphContract` values, not proof of behavior,
codec honesty, authorization or readiness. Graph is the compatibility authority;
Plugin and Kernel delegate rather than implementing another matcher.

## Current Authoring

```rust,ignore
let source = graph.node("source", |x: i64| x)?;
let sink = graph.node("sink", |x: i64| x + 1)?;
graph.connect(source.output(), sink.inlet())?;

let interface = InputInterface::single(Layout::<Args>::fields())?;
let mut module = Module::new("service", interface)?;
let child = module.node("child", |x: Args| x.left + x.right)?
    .input(Layout::fields())?;
module.expose(module.inlet().field(Args::SLOT_LEFT),
    child.inlet().field(Args::SLOT_LEFT).target()?)?;
module.expose(module.inlet().field(Args::SLOT_RIGHT),
    child.inlet().field(Args::SLOT_RIGHT).target()?)?;
let package = module.finish(child.output())?;
let service = graph.instantiate("service", &package)?;
```

`Module::new` returns `Result`, checking public cardinality and mode. Every public
leaf needs an explicit projection; undeclared exports and duplicate destinations
reject. Distinct compatible destinations permit fanout. Layout is not inferred by
concatenating children: in `F(x,y) = A(x,B(y))`, only `A.first` and `B.input` are
exposed, while the internal `B -> A.second` edge remains private and intact.

Only `SinglePort<T,L>` gets `inlet()`. `NoPorts` and custom multi-port owners use
`inlets().port(...)`. Built-in atomic layouts return `Input<T,Mode>`; structured
layouts return `InletView<T,Mode>`. Passing a view directly to `connect` fails to
compile. `.field(...).target()?` checks the actual frontier and identity
dynamically, not at compile time. Fields/deep/bounded-depth/custom layouts have
different frontiers. Port/field owner types reject unrelated Rust owners, but a
custom erased physical multi-port definition's root Rust type cannot prove its
per-port reconstruction or behavior. Manual descriptors and `PortInterface`
defaults still require actual declaration and dynamic leaf validation.

Layouts are frozen in each admitted operation's address book. A supported pending
Node layout edit creates a new declaration revision/addressing, preserving claims
and handler origin. An old view may still select its old frontier, but Graph
rejects its stale endpoint token on use. Packaged Module layouts have no child
retargeting shorthand. Final compilation validates physical routes.

The optional `InputFields` derive is the single owner of typed `SLOT_*` field
descriptors. `LocalPortLayout` and `PortLayout` own only layout expansion and
reconstruction. A type may derive both layouts with `InputFields` once, without
duplicate constants; descriptors also support generic structs without layout or
codec bounds. Raw field descriptors do not silently reinterpret layout-specific
renames or optional slot types. Manual descriptors remain checked lookup requests.

### Zero-Input Activation

`graph.node("source", || 42_i64)?` has `NoPorts`, not a hidden unit input.
It starts only through an explicit trigger:

```rust,ignore
let source = graph.node("source", || 42_i64)?;
let sink = graph.node("sink", |x: i64| x + 1)?;
graph.connect(source.output(), sink.inlet())?;
graph.trigger("start", [source.operation().start()])?;
// After deployment: engine.fire("start", vec![])?;
```

`Operation<NoPorts,...>::start()` and the `SourceStart` handle describe root
admission, not input delivery. Module starts resolve to output-reachable zero-input
child producers; disconnected sources remain dormant. All admitted producers use
the existing ready queue, activation limits, cohorts, cancellation, activity
accounting and output routing. There are no synthetic slots, scheduling ports or
Module executor. Raw source contexts report `source_port_id() == None`.
Sources may return plain payloads or ordinary `Output<T>` routing, including no
emission and destination selection. Manifests include sorted source-trigger paths.

`NodeDefinition` selects independent input/output modes. `LocalValue` connections
require the same Rust payload type. `EncodedValue` connections allow different
Rust types with equal complete schema/codec descriptors, then use the receiver's
checked decoder. Mixed modes require an explicit conversion Node. `Output::to`
and `Output::to_all` target local inputs, not implicit encoded conversions.

Discovery uses `graph.discover(name)` for direct public boundaries or
`ModuleHandle::operation()`. Contract-bound imports use `bound.operation()?`, then
the same typed `inlets().port(...).field(...).target()?` and singular
`output::<T,Mode>()?`. Bound views retain borrowed Graph authority and revalidate
the complete effective contract on leaf/output access, not a cached attestation.
Kernel host portable consumers, including the independently compiled fixture,
use this path; discovery/selection remains Kernel composition policy.

## Ownership Audit

Paths are relative to `packages/phenix-graphs/src/`. Most new types/traits live in
`authoring/operation.rs`, not `authoring.rs`. They enforce authoring/compiler
invariants, not new runtime state.

| Type Or Trait | Owner / Invariant | Evaluation And Limit |
|---|---|---|
| `LocalValue`, `EncodedValue` | Graph representation markers (`authoring.rs`) | Necessary Rust dispatch distinction; erased before runtime, no conversion authority |
| `ValueMode<T>` | Graph declared identity selection | Reuses `DeclaredValue`; markers cannot bypass actual checks |
| `SourceReturn` and source-directed return tuple marker | Graph zero-argument closure inference | Separates no ports from a unit-valued port without a runtime action kind |
| `Operation<I,O,IM,OM>` | Graph boundary addressing and lineage | Shared Node/Module handle, no execution state |
| `InletPort<I,T,L>` | Owner-typed static/owned port descriptor | Rejects unrelated Rust owners; names/types still checked dynamically |
| `SinglePort<T,L>`, `NoPorts`, `Ports<I>` | Graph cardinality/layout markers | Restrict shorthand and map custom definitions; not physical ports |
| `NodeInputs<A>` | Graph typed admission from definition layout | Validates physical cardinality/atomic shape; custom validation remains a seam |
| `PortInterface` | Graph cardinality and custom validation | Defaults cannot prove erased per-port reconstruction or semantics |
| `Inlets<I,M>`, `InletView<T,M>` | Graph selection over frozen addresses | View is not endpoint; explicit leaf check, not static frontier proof |
| `InletLayout<T,M>` | Graph atomic endpoint versus structural view selection | Reuses layouts; custom implementations do not certify behavior |
| `InputInterface<I,M>` | Graph independent public port/layout metadata | Slot validation plus checked Module construction; no runtime container |
| `PublicInlets<I,M>`, `PublicView<T,M>` | Module-owned public selectors | Not connection endpoints; projection checks owner/destination |
| `SourceStart` | Graph root-admission handle | Scope/revision-checked start of an operation with no public ports; not a value endpoint |
| `Module<I,M>` | Authoring owner of builder and interface | Checked constructor, complete finish; Deref permits root-path overlap below |
| `TypedModulePackage<I,O,IM,OM>` | Reusable compiler package with typed boundary | Instantiate and existing sealed Replacement; no runtime wrapper |
| `DiscoveredOperation` | Checked erased boundary discovery | Caller chooses generic owner/mode; actual identity checked at leaves/output |
| `BoundOperation`, `BoundInlets`, `BoundInletView` | Borrowed contract-restricted Graph authority | Full effective shape/claim rechecked on access |
| Private `Address`, `AddressBook` | Graph selection metadata | Scope/revision plus shared addresses; not another validator or graph |
| Existing `Input`, `OutputRef`, `NodeHandle` | Graph endpoint/configuration authority | Reused rather than duplicate named endpoints; dynamic stale-token checks |
| Existing `NodeDefinition`, `InputLayout`, local/decoded layout traits | Graph definition and value preparation | Independent representation modes and physical reconstruction; not a second operation hierarchy |
| Existing `SlotField`, `SlotPath`, `FieldSelector` | Owner-typed paths (`authoring.rs`) | No domain Clone bounds; existence/frontier checked dynamically |
| Existing `InputEndpoint`, `OutputEndpoint`, `ConnectEndpoints` | Existing sealed Graph dispatch | Singular typed/symbolic connection/exposure dispatch, no separate validators |

### Raw Compiler Surface

`ModuleInput`, `ModuleOutput`, `ModuleInputTarget`, `ModuleOutputSource` and
`ModuleDefBuilder` remain public **raw physical compiler representations/API**.
They describe routing keys, numeric/symbolic addresses, metadata and declarations
for existing lowering/manual fixtures. They are not canonical typed interfaces,
layouts or operation handles. Retaining them avoids a duplicate compiler model;
renaming them or adding wrappers would enforce no new invariant. Raw output
insertion can assert on a second output, unlike checked ordinary authoring. This
compiler construction surface is not the typed operation authoring API; duplicate
inference, named endpoint and export conveniences formerly attached to it have
been removed rather than aliased.

Root `graph.expose_input`, `expose_input_field` and `expose_output` remain useful
for triggers/container routes. Typed `Module::expose` plus `finish` is the explicit
interface path. Both coexist, including through Module's builder Deref. Finish
rejects exports outside its declared interface, but this dual path remains an API
consistency risk, not an obsolete verb claimed removed.

## One-Output Migration

`NodeTemplate`/physical Nodes have one `OutputSlot`; `ModuleDef` has an
`Option<ModuleOutput>` for the container distinction. The erased action/executor
completion seam still returns `Vec<Emission>`, but is not a multi-output escape:
`scheduler.rs::publish_via` rejects more than one emission, nonzero slots and wrong
identity before routing/publication. Zero emissions are legal. One payload can
select zero, one or many outgoing edges. `Output::to_all([], value)` publishes
with zero deliveries; `Option::None` and `Result::Err` are ordinary payload data.

| Previous Use | Current Migration / Source Evidence | Semantic Boundary |
|---|---|---|
| Same-payload multi-output fanout | Slot-0 edges; `output_wrapper`, `output_fanout_to_two_valid_consumers_is_shared_not_recomputed` in `src/tests.rs` | One shared result, not recomputed per recipient |
| Distinct tuple members | `iteration_nine::one_tuple_payload_can_be_broadcast_to_multiple_independent_consumers` uses explicit flag/magnitude projections | Separate invocations, not atomic multi-invocation publication |
| Conditional branches | One endpoint with edge selection; `output_semantics`, `output_wrapper`, `patch_syntax` | Routing changes recipients, not output count |
| Raw/external invalid completions | Direct admission tests for duplicate emissions, wrong slot/schema/mode and empty completion in `src/tests.rs` | Rejection publishes/delivers nothing from that completion |
| Fanout admission failure | `destination_occupancy_rejects_entire_fanout_publication`, routing atomicity tests | Validate batch before commit; action effects not rolled back |
| Portable diagnostics | Kernel host provider now has one Response output; extra-input negative shape case remains | No diagnostic output extension |
| Store-derived receipt | Scheduler `PublicationEvent` identifies exact source/cohort alongside terminal tap | No second routable receipt or retained source payload |

Atomicity concerns one completion's validation/routing/admission batch or an
explicit trigger admission, not separately executing projection Nodes, every
invocation in a wave, or arbitrary action side effects.

## Existing Replacement Path

`graph.replace(name, replacement)` accepts a compatible Node or Module regardless
of the old direct kind. Use an owned/borrowed `TypedModulePackage` or a
`NodeTemplate`; no second substitution verb exists. Same-kind replacement keeps
its validation path. Cross-kind replacement in `authoring/substitution.rs`
compares the complete public shape (names, frontier, local TypeIds/full encoded
identities), retargets incident edges/public projections using actual routing
keys, and compiles a validation candidate before commit. Failure preserves topology,
imports, origins, pending state and handle authority.

Old claims must be explicitly present with identical definitions; shape does not
grant membership. Root requests move to the replacement boundary; removed private
child requests fail closed. Parent requests remain final compiler obligations.
Success attributes the boundary to the replacer, preserves independent package
implementation origins and revokes old tokens even across round trips. Unrelated
layout handles stay attached to the original pending store.

Validation defers unrelated final contract requests without removing them from the
committed candidate. Requests inside the replacement's dependency closure and
requests on the replaced boundary or descendants remain checked. Existing invalid
topology, claims or compiler-limit violations can still block a cross-kind edit;
this is structural preflight, not an alternate compatibility engine.

Patch replacement uses precise `IncidentConnections(name)` footprints, not blanket
graph-wide connection/root writes. Disjoint replacements and rewires commute;
incident explicit rewires conflict. Lifted requests are authorized only when
derived from the replaced Module. Existing revision events record rebound units
and removed/added edges. Nested substitution belongs inside
the owning Module before packaging, not through imported private boundaries.
`operation_substitution` covers structured/internal/nested/repeated implementations,
local/encoded lowering, claims, requests, origins, rollback, revocation and patches.

## Historical Removal Ledger

Old spellings below are migration accounting, not current APIs. Measured with
exact-word `rg --glob '*.rs'` across `packages examples docs` on 2026-11-09,
including tests/probes/comments. Removed definitions and calls are both zero.
Retained symbol totals include all references, not just calls. Historical Markdown
may quote old APIs only as clearly historical material.

| Old API | Replacement | Definitions Remaining | Call Sites Remaining | Tests / Examples / Docs Migrated | Justification |
|---|---|---:|---:|---|---|
| `connect_wire` | `connect` mode dispatch | 0 | 0 | Graph, Kernel, portable fixtures/current guides | No shim |
| `wire_input` | Operation inlet selection, EncodedValue | 0 | 0 | Rust tests/probes/fixtures/current docs | Removed |
| `wire_output` | Singular output, EncodedValue | 0 | 0 | Rust tests/probes/fixtures/current docs | Removed |
| `local_input` | Operation inlet selection, LocalValue | 0 | 0 | Rust tests/probes/fixtures/current docs | Removed |
| `local_output` | Singular output, LocalValue | 0 | 0 | Rust tests/probes/fixtures/current docs | Removed |
| `output_named` | Singular output | 0 | 0 | Rust source/tests/current docs | Removed |
| `expose_input_slot` | Module projections / root field exposure | 0 | 0 | Rust tests/examples/fixtures/current docs | Removed |
| `NamedInput`, `NamedOutput` | Existing typed endpoints | 0 | 0 | Raw/manual tests | Duplicate structs removed |
| NamedNodeHandle endpoint methods | Operation discovery | 0 endpoint methods | 0 endpoint calls | Manual/registration tests | Handle remains registration identity only (5 total symbol refs) |
| `register_fn`, `register_flattened` | GraphBuilder node / raw templates | 0 | 0 | Entire searched Rust surface | Inference helpers removed |
| `connect_decl` | Existing connection APIs | 0 | 0 | Entire searched Rust surface | No parallel helper |
| `export_input`, `export_input_slot`, `export_output` | Projections / root exposure / explicit raw declarations | 0 | 0 | Entire searched Rust surface | No legacy Rust examples |
| `ModuleInput` | Retained raw compiler metadata | 1 | 9 total refs | Compiler/manual tests retained | Physical record, not canonical interface |
| `ModuleOutput` | Retained singular source metadata | 1 | 8 total refs | Compiler/manual tests retained | Optional only for raw container |
| `ModuleInputTarget` | Retained raw target address | 1 | 200 total refs | Compiler/manual/substitution tests | Physical lowering representation |
| `ModuleOutputSource` | Retained raw source address | 1 | 164 total refs | Compiler/manual/substitution tests | Physical lowering representation |
| `ModuleDefBuilder` | Retained raw builder | 1 | 145 total refs | Compiler/manual tests | Public raw path remains a canonical-surface difference |
| `InputEndpoint`, `OutputEndpoint`, `ConnectEndpoints` | Existing sealed dispatch | 1 each | 4 / 4 / 8 total refs | Typed/symbolic authoring | Reused, not separate validators |

The zero-removal gate holds for the superseded Rust spellings in this snapshot,
not for removal of the raw public surface or completion of final review. Recount
after ongoing implementation changes.

## Review Iterations

| Finding | Fix Or Remaining Qualification |
|---|---|
| Return routing conflated with representation | Independent NodeDefinition modes; conversion tests in node_definition_modes |
| Stale partial report | Reconciled operation.rs, scheduler, typed tests and substitution |
| Rust ownership mistaken for leaf/erased-layout proof | Explicit checked target; direct-view/owner compile-fail cases, dynamic limits documented |
| Constructor/cardinality/mode failures hidden | Module::new returns Result; admission and missing-projection tests |
| Replacement claims/origin/requests/unrelated handles | Candidate compile, explicit claims, revocation and substitution/patch regressions |
| Workspace portable-host schema error assertion wording mismatch | Main reports assertion fixed and host tests subsequently passed; full workspace rerun pending |
| Duplicate endpoint/helper paths | Removed Rust refs measured zero; raw compiler/root exposure paths explicitly flagged |
| Descriptor split / abstraction economy | InputFields owns descriptors once; layouts no longer generate duplicate keys |
| Atomic/cardinality panic paths | Atomic identity and named frontiers checked before handles; incomplete Module construction returns an error |
| Repeated layout expansion / retarget bypass | Frozen frontier expanded once; registration and retargeting share admission validation |
| Zero-argument closures were inert | Explicit source-trigger admission through the existing queue; source_operations executes result 43 with no ports |
| Source routing parity / incomplete manifests | Ordinary Output returns and deterministic source-trigger paths, with regressions |
| Broad replacement footprints | Exact incident-edge claims and constrained requirement lifting; disjoint replacement tests |
| Premature unrelated requirement checks | Filtered validation clone; committed final obligations remain intact |

Earlier provisional scores (authoring 6/10 to 6.5/10, architecture 7.5/10) rated a
smaller partial slice, not this implementation. Debt-cleanup scores concern its
earlier scope. A later independent reviewer and current numeric ratings are
forthcoming; no target 9.5/10 or acceptance score is claimed.

## Verification Gates

This documentation pass inspected source and ran symbol/whitespace audits, not
Cargo tests. Handoff reports Graph tests and doctests passed. A workspace run
failed in portable-host schema-error wording; main fixed the assertion and reports
the focused host suite passed. This is not a subsequent complete workspace pass.

| Gate | Current Evidence | Final Commit Evidence |
|---|---|---|
| Baseline/branch | Git inspected: 5469c55b, impl/operation-interface-20261109; dirty work preserved | Final implementation SHA pending |
| Typed/projection/substitution tests | Assertions inspected; Graph pass reported | Exact command/output pending |
| Graph doctests / compile-fail | Pass reported; owner/view negatives inspected | Exact final configuration pending |
| Workspace default tests | Host failure then focused host pass reported | Full rerun pending |
| Workspace and isolated Graph no-default | Historical runs do not certify redesign; feature unification is not isolated proof | Pending |
| Formatting / strict Clippy / examples / Nix | No new run by this documentation pass | Pending |
| Obsolete Rust API audit | Zero references as measured above | Final recount pending |
| Independent review / ratings | Iteration findings above; later reviewer forthcoming | Pending |
| Final-head CI / benchmarks / long-run evidence | No current final commit or final-head artifact supplied | Pending; no performance claim |

Main must fill exact final source, commands, outputs and review disposition before
acceptance. Residual risks are the raw compiler public path, dual exposure APIs,
custom erased-layout trust, dynamic leaf/frontier checks and pending common
descriptor derive. No new scheduler, native ABI guarantee, sandbox, action-effect
rollback or universal behavioral substitution is claimed.
