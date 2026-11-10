# Graph Interface Contracts

## Implementation Status

Current operation redesign snapshot: baseline `5469c55b`, branch
`impl/operation-interface-20261109`, prior uncommitted work preserved. The final
implementation commit is forthcoming and unknown. Typed operations, explicit
Module projections, single-output scheduler validation and cross-kind
`graph.replace` are present in source; final tests and independent review remain
pending. The [current implementation record](operation-interface-redesign.md)
owns the removal ledger, type/invariant audit and verification table.

**Historical report:** the commit-specific measurements, matrices and verification
below describe PR #1251 and its stated revisions, not the current operation
interface. The API snippets and ownership inventory below are historical and
must not be used as current authoring examples. For the current typed operation
API and its implementation status, see [Typed Operation Interfaces](grouped-authoring.md)
and [Operation Interface Redesign](operation-interface-redesign.md). For the
current e91-based resolver/host implementation,
API migrations and pending final verification, see
[Contract Composition Iteration](contract-composition-iteration.md) and
[Kernel](kernel.md). In particular, current Kernel discovers eligible providers,
exposes consumer-local resolved imports, owns candidate-local activation/exports,
and offers an optional prepare/bind/start/publish coordinator. Explicit Graph
connections and trusted callback cleanup remain necessary; no arbitrary external
effect rollback or automatic edge insertion is claimed.

This is the implementation report for [draft PR #1251](https://github.com/matthis-k/phenix-ai/pull/1251), not a
preimplementation proposal or a release acceptance certificate. C2, shape plus
explicit claim, is implemented in Graph and integrated with Plugin and Kernel.
C1, C3 and C4 remain alternative designs, not independently implemented or tested.
The report and implementation are reviewed together on the new contract branch.

Mission acceptance is **incomplete**, despite passing implementation checks. All
requested commands passed on implementation commit
`2a95b8519ff162caa68fee8b844df6ca42e1e515`, including the full Nix check and
[exact-head CI 42503290164](https://github.com/matthis-k/phenix-ai/actions/runs/42503290164).
At that revision Q04 remained PARTIAL because schema interning was unbounded.
R01 now uses reclaimable owned identities; see the current
[Schema contract and measurements](../packages/phenix-schema/README.md).
At that revision Kernel validated host-selected offers; it did not discover
providers or wire consumers automatically and had no plugin/kernel activation coordinator.
The weighted implementation assessment is **8.3/10**, not 9+ and not an override
of these outstanding gates.

At the PR #1251 snapshot described by this historical report, H07 required that
**inputs match exactly** and permitted output extensions. That policy has since
been superseded: current operation contracts require the complete single-output
interface to match. The earlier output-subset discussion below is historical
only.

## Evidence Index

Current references below point to the implementation under review. Test aliases
are used in the horizontal matrix and traceability tables; a reference to a test
means the actual test exists, not a proposed test name.

| Key | Current Source Or Test File | Responsibility |
|---|---|---|
| I | [interface.rs](../packages/phenix-graphs/src/interface.rs) | Shape, contract, sealed inspection, compiled bindings, final validation, identity unit tests and compile-fail doctests. |
| A | [authoring/interfaces.rs](../packages/phenix-graphs/src/authoring/interfaces.rs) | Effective inspection, authoring binding, claims, requests, replacement obligations and patch claim footprints. |
| B | [authoring.rs](../packages/phenix-graphs/src/authoring.rs) | Declaration tokens, clone/package identity, grouped exposure, portable connection and dependency identity refresh. |
| G | [Graph lib.rs](../packages/phenix-graphs/src/lib.rs) | Public exports, node/module declarations, expansion, compiled metadata, boundary validation and directed destination checks. |
| O | [provenance.rs](../packages/phenix-graphs/src/provenance.rs), [authoring/provenance.rs](../packages/phenix-graphs/src/authoring/provenance.rs) | Opaque `GraphOrigin`, final-unit origin lookup and scoped attribution. |
| P | [Plugin manager.rs](../packages/phenix-plugins/src/manager.rs), [registry.rs](../packages/phenix-plugins/src/registry.rs) | Snapshot/handle leases, per-loaded-generation origin, locator-only contribution API and final graph binding. |
| K | [Kernel lib.rs](../packages/phenix-kernel/src/lib.rs) | Definitions, offers, selected requirements, final checks and existing lifecycle. |
| S | [Schema lib.rs](../packages/phenix-schema/src/lib.rs) | Full descriptor equality, canonical framing and existing process-lifetime interning. |
| T1 | [canonical_interfaces.rs](../packages/phenix-graphs/tests/canonical_interfaces.rs) | 26 tests: structural/semantic distinction, exact inputs, authoring/compiled inspection, claims, binding, replacements, diagnostics and late layouts. |
| T2 | [clone_lineage.rs](../packages/phenix-graphs/tests/clone_lineage.rs) | 7 tests: inherited replay versus divergent declarations, package identity and directed output selection. |
| T3 | [compiled_bindings.rs](../packages/phenix-graphs/tests/compiled_bindings.rs) | 4 tests: graph-generation checks, nested projections and compiled root inspection/binding. |
| T4 | [composition_origin.rs](../packages/phenix-graphs/tests/composition_origin.rs) | 5 tests: allocation identity, attribution, replacement, packaging and scope restoration. |
| T5 | [contract_dependency_identity.rs](../packages/phenix-graphs/tests/contract_dependency_identity.rs) | 7 tests: transitive claim edits/import conflicts, alias refresh and replacement request retention. |
| T6 | [module_slots.rs](../packages/phenix-graphs/tests/module_slots.rs) | 8 tests: grouped public slots, real nested routing, literal names, escaped routing keys and replacement grouping. Requires `derive`. |
| T7 | [portable_wire.rs](../packages/phenix-graphs/tests/portable_wire.rs) | 15 tests: `independently_compiled_portable_crates_execute` builds separate provider/consumer rlibs and runs a linked harness; independent codecs, descriptor mismatch dimensions, legacy/local rejection, ordinary connect, stale tokens and rustc type probes. |
| T8 | [declaration_layout.rs](../packages/phenix-graphs/tests/declaration_layout.rs), [manual_api.rs](../packages/phenix-graphs/tests/manual_api.rs) | 9 layout tests and 6 manual tests, including new revision regression and updated direct literals. |
| T9 | [Plugin graph_contracts.rs](../packages/phenix-plugins/tests/graph_contracts.rs) | 3 tests: final node/module binding, missing claims/locators, reload, sibling sets and aborted candidates. |
| T10 | [Kernel graph_contracts.rs](../packages/phenix-kernel/tests/graph_contracts.rs) | 4 tests: three-plugin portable execution, generation leases, failed candidates, missing references and late composition. |
| T11 | [Kernel contract_offers.rs](../packages/phenix-kernel/tests/contract_offers.rs) | 2 passing tests: node/module offer matrix and foreign replacement origin matrix; node replacement now explicitly claims the contract. |
| T12 | [symbolic_pending.rs](../packages/phenix-graphs/tests/symbolic_pending.rs) | 2 passing tests: symbolic fields use effective pending local layouts; stale atomic/ambiguous paths reject and roll back before successful recovery. Requires `derive`. |
| T13 | [patch_lineage.rs](../packages/phenix-graphs/tests/patch_lineage.rs) | 2 passing tests: same-package and same-factory replacement still require declared writes; rollback preserves handles, revisions and origins. |
| T14 | [static_contracts.rs](../packages/phenix-graphs/tests/static_contracts.rs) | 5 passing tests: zero-input source, zero-output sink, empty-module opt-in, distinct readiness policies and independent resource authorization. |

### Historical Baseline

The implementation builds on derived baseline
`ee2abaa2f0b39e09ae32db5b16d6618b18fc388a`, descended from storage prerequisite
`44f66c1d155eedd7a1ca374df0f00e6127be65b7`. Historical source is pinned here:
[baseline tree](https://github.com/matthis-k/phenix-ai/tree/ee2abaa2f0b39e09ae32db5b16d6618b18fc388a),
[old InterfaceBounds](https://github.com/matthis-k/phenix-ai/blob/ee2abaa2f0b39e09ae32db5b16d6618b18fc388a/packages/phenix-graphs/src/interface.rs),
[old graph_bounds test](https://github.com/matthis-k/phenix-ai/blob/ee2abaa2f0b39e09ae32db5b16d6618b18fc388a/packages/phenix-plugins/tests/graph_bounds.rs).

The baseline workspace test run and
[baseline CI 42466577317](https://github.com/matthis-k/phenix-ai/actions/runs/42466577317)
passed before contracts were implemented. They establish ancestry and regression
context, not contract acceptance. Current verification is recorded below for
implementation commit `2a95b8519ff162caa68fee8b844df6ca42e1e515` in draft PR #1251.

## Ownership And Semantics

```text
C2 IMPLEMENTED
phenix-schema: Schema { name, revision, structure, codec }
                    | DeclaredValue / TypeIdentity at endpoints
phenix-graph: InterfaceShape (actual ID-free boundary)
             GraphContract { id, required: InterfaceShape }
                    | explicit GraphContract values in claims
             NodeTemplate / ModuleDef + ModuleInput { port, slot, routing name }
                    | GraphBuilder effective pending declarations
                    +--> AuthoringUnit : sealed GraphUnit --> UnitSnapshot
                    | compile, expand, validate all claims and requests
                    v
             CompiledGraph / NodeRef / ModuleRef / FindResult : GraphUnit
                    +--> BoundUnit --> BoundInput / BoundOutput (graph borrow)
                    +--> FindResult::origin() --> GraphOrigin allocation identity
phenix-plugin: Loaded { GraphOrigin, contributions, implementation, factory }
                    ^ Arc<Loaded> in snapshots and contribution handles
                    | bind(locator, contract, actual compiled graph)
phenix-kernel: selected snapshots --> compose under their origins --> one graph
              definitions + offers + requirements --> final Graph validation
              engine/actions die before retained snapshots/code leases
```

`Schema` describes a wire encoding, not a unit. `InterfaceShape` describes a whole
public boundary without a semantic ID. `GraphContract` adds explicit semantic
identity and its required shape. A claim is stored directly as a `GraphContract`
in `NodeTemplate::claims` or `ModuleDef::claims`; there is **no extra claim wrapper**.
`ModuleDef::contract_requests` stores `(String, GraphContract)` final obligations.
At the reported revision Kernel `ContractRequirement` referenced owner-qualified definition and offer keys;
it is not a Graph claim and is not a storage requirement or grant.

Actual shape is extracted from effective `ports`/`outputs`, or module public
inputs/outputs, not inferred from a cached one-input `NodeSignature`. Named
endpoints use Graph's name rules, not the stricter contract-ID grammar. Unnamed
endpoints retain separate numeric-address identity and never acquire invented
names. Empty shapes are valid. Local equality is `TypeId`, never diagnostic Rust
type names; Wire equality uses the declared wire identity. Neither permits a
Local/Wire coercion.

`InterfaceShape::check_shape(required, actual)` is expressed in Rust as
`required.check_shape(&actual)`. It requires exactly the same named input ports,
slots and identities and exactly the same unnamed input map. At this historical
revision, required outputs had to be present with equal identity and extra
outputs were allowed. Current `check_shape` instead requires matching singular
output presence/name/identity; there is no output-extension policy. `same_shape`
checks both directions. `GraphContract::check_shape` is structural only;
`check_implementation` additionally requires an explicit, exactly matching
definition. Every same-ID claim on that unit is checked, not just the first.

Claims are declaration-owned. Editing `one/private` changes the shared definition
used by `two/private`, not just one physical instance. A distinct implementation
requires a distinct definition. Nested edits refresh package identity for the
changed dependency and every ancestor alias, while unrelated dependencies remain
reusable. Repeating an existing identical claim is a no-op. Distinct builders'
packages are not deduplicated merely because their definition names match.

Final validation is in `ModuleCatalog::compile` after boundary expansion and
before flat-graph build/action-factory activation. It checks root, every expanded
module and node claim, plus requests retained in definitions. It rejects
conflicting same-ID definitions across claims and requests, even when no consumer
asked to bind the offending claim. Uninstantiated catalog definitions are not
described here as executed units. Kernel separately checks all published contract
definitions, including unused definitions, and all offers, including unused offers.

### Boundaries And Mutation

Module inputs have an explicit public `(port, slot)` pair independent of their
flat routing `name`. Plain exposure means `(public_name, "value")`. Dots remain
literal; they are not parsed into grouping. Nondefault slots and default names
inside the reserved `__slot_` namespace use length-framed escaped routing keys.
Those keys are implementation routing identity, not a public port-name grammar.
Grouping creates no runtime barrier or join. Compiler expansion checks unique
IDs, routing names and pairs, nonempty targets, duplicate targets, actual target
existence and consistent fanout identities. Outputs likewise resolve to actual
sources with matching identities.

Inspection borrows the exact builder, overlays pending layouts and returns
`InterfaceError` rather than trusting cached handles. Module inspection validates
the boundary through the existing catalog/expander. Owned `UnitSnapshot` values
are historical data, not transferable validation tokens. The compiled graph
itself implements `GraphUnit` for the root at `""`; `find("")` still returns `None`.
Root binding resolves its real metadata inputs/outputs separately. Root inspection
does not invent a single provider origin for a composed graph.

Replacement requires exact complete public shape; node replacement also preserves
the existing numeric port/slot/output signature requirements. The replacement
author must explicitly include each old claim with the same definition. Claims
are **not silently copied onto a new handler**. New claims are validated. Module
replacement must retain every old `(path, contract)` request, independent of
ordering; removing or changing one fails before mutation. Preserved requests
remain final compile obligations, even if the replacement's internals no longer
claim them. Successful replacement revokes declaration handles; failed replacement
leaves old handles, pending layout, imports and claims usable. Patch claim/request
writes participate in assembly write-footprint checks and rollback.

Cloning preserves scope and inherited declaration tokens for intentional
preflight/replay. It isolates the pending `Rc<RefCell<_>>` and assigns a fresh
package `definition_id`. New sibling declarations, replacements and layout
revisions receive distinct `declaration_id` tokens. This is not the old proposal
to reject every inherited handle merely because builder `Rc` identity differs.
Typed connect/disconnect/exposure, public-input trigger lookup, low-level named
handles and directed destination resolution check the relevant declaration token.
Consuming a builder invalidates configuration through weak pending access.

### Origins And Generations

The reported `GraphOrigin(Arc<()>)` compared allocation identity via `Arc::ptr_eq`.
Current origins retain allocation identity plus parent ancestry: Kernel creates a
distinct child per composer; plugin binding accepts exact revision ancestry while
Kernel offer checks require the exact producer child. Each staged
`Loaded` plugin generation receives a new token, including aborted candidates;
equal names or numeric revisions in sibling sets are insufficient. Snapshot and
contribution handle `graph_origin()` refer to this same token. The token alone
does not pin code; their `Arc<Loaded>` does.

`GraphBuilder::with_origin` attributes declarations created/replaced inside its
callback and restores the previous scope on return, error or unwind. Inspecting
or claiming an existing unit does not relabel it. Pending layout changes preserve
the handler origin. Successful replacement uses the replacing scope; unscoped
replacement loses the old origin. A node stores its composition origin directly;
module instance origins are looked up by declaration ID in the compiled origin
map. Module origin means the instantiator/replacer of the public boundary, not
ownership of every private handler imported in its definition.

`PluginSnapshot::bind` resolves the actual compiled locator, checks this exact
origin, then delegates semantic validation to Graph. Kernel composes each
contribution under its leased origin and checks every final offer's origin against
the offering generation. Neither contract-ID equality nor matching shapes can
launder a foreign unit. T4 and T9 pass; T11 explicitly claims the compatible
replacement contract and verifies final foreign-origin rejection for nodes
and modules.

Compiled `BoundInput::resolve(&CompiledGraph)` and `BoundOutput::resolve` compare
the supplied graph pointer to the retained graph borrow with `std::ptr::eq`.
Equal paths and numeric addresses in another compiled generation do not pass.
Cloning the owning `Arc<CompiledGraph>` still refers to the same graph and passes.
Projections retain their own `BoundUnit` when the original binding is dropped.
Explicitly extracted raw addresses have no provenance and are not validated handles.

### Portable Connections

`connect` on encoded endpoints requires equal `schema()` descriptors (name, revision,
structure and codec) and nonempty name, structure and codec. Both actual
endpoints must be canonical Wire endpoints matching that descriptor. It then
delegates to ordinary typed connection validation, preserving scope, declaration
revision, endpoint existence, identity and duplicate-edge checks before insertion.
The receiver decodes ordinary bytes; there is no adapter node, transport or ABI
cast. T7 executes independent types in both directions and observes native typed
wave failures for a malformed encoder. A descriptor does not prove codec honesty.

T7's `independently_compiled_portable_crates_execute` also compiles
[`fixtures/portable_provider.rs`](../packages/phenix-graphs/tests/fixtures/portable_provider.rs)
and [`fixtures/portable_consumer.rs`](../packages/phenix-graphs/tests/fixtures/portable_consumer.rs)
separately as `libportable_provider.rlib` and `libportable_consumer.rlib`, then
links and runs
[`fixtures/portable_execution.rs`](../packages/phenix-graphs/tests/fixtures/portable_execution.rs).
The harness asserts distinct Rust `TypeId`s but equal full
descriptors, connects the independently defined types with `connect`, and
executes six value vectors through the real deployment, checking both invocations
and the consumer's decoded publication. This is separate-compilation execution
evidence, not just two declarations within one test crate or a native ABI claim.

R01 removes `ContractValue::SCHEMA`, `schema_descriptor()` and nominal fallback.
Manual implementations must provide `schema()`; primitives also provide complete
descriptors. `wire_schema()` returns the descriptor's canonical `Arc<str>` identity.
Local typed connections require the same Rust endpoint type; encoded connections
allow different Rust types and validate complete descriptors through `connect`.

R01 replaces process-lifetime interning with reclaimable `Arc<str>` identities
throughout graph declarations, values, interfaces, diagnostics and compiled
metadata. There is no global strong cache or static-string compatibility API.
Clones share ownership; independently allocated equal descriptors compare by
content, including codec. Local `TypeId` identities remain a distinct variant.
Weak-reference regression tests cover repeated graph/deployment unloads, retained
reports, derived contracts, diagnostics and cheap clones. Earlier review tables
below describing nominal compatibility or unbounded interning are historical and
superseded by R01. No per-wave semantic contract matcher is added.
The existing directed destination lineage check does run when selections resolve.

## Historical API (PR #1251)

The following signatures were implemented at PR #1251, rather than its earlier
`graph.unit` / `UnitError` / bare-address sketches. This is a historical inventory,
not current operation API guidance. Supporting method bodies are omitted.

```rust
pub trait GraphUnit: sealed::Sealed {
    fn inspect(&self) -> Result<UnitSnapshot, InterfaceError>;
}
// GraphBuilder
pub fn inspect(&self, path: &str) -> Result<AuthoringUnit<'_>, InterfaceError>;
pub fn implements(&mut self, path: &str, contract: &GraphContract)
    -> Result<(), InterfaceError>;
pub fn implements_public(&mut self, contract: &GraphContract)
    -> Result<(), InterfaceError>;
pub fn require_contract(&mut self, path: &str, contract: &GraphContract)
    -> Result<(), InterfaceError>;
pub fn bind_contract(&self, path: &str, contract: &GraphContract)
    -> Result<BoundAuthoringUnit<'_>, InterfaceError>;
pub fn connect<A, B>(&mut self, from: A, to: B) -> Result<&mut Self, String>
where (A, B): ConnectEndpoints;
pub fn with_origin<R>(&mut self, origin: &GraphOrigin,
    compose: impl FnOnce(&mut Self) -> R) -> R;
// FindResult<'g>, NodeRef<'g>, ModuleRef<'g>: consuming receiver
pub fn bind_contract(self, contract: &GraphContract)
    -> Result<BoundUnit<'g>, InterfaceError>;
// CompiledGraph: borrowed root receiver
pub fn bind_contract(&self, contract: &GraphContract)
    -> Result<BoundUnit<'_>, InterfaceError>;
// BoundUnit<'g>
pub fn input(&self, port: &str, slot: &str) -> Result<BoundInput<'g>, InterfaceError>;
pub fn output(&self, name: &str) -> Result<BoundOutput<'g>, InterfaceError>;
// BoundInput / BoundOutput
pub fn resolve(&self, graph: &CompiledGraph) -> Result<&[InputAddr], InterfaceError>;
pub fn resolve(&self, graph: &CompiledGraph) -> Result<OutputAddr, InterfaceError>;
```

This is a signature inventory, not a standalone Rust module: several methods have
the same name on different owners. Both `AuthoringUnit` and `BoundAuthoringUnit`
are re-exported from `phenix_graph::authoring` and the `phenix_graph` crate root;
callers may name either borrowed view type directly or use type inference.

### Historical Node And Module Example (PR #1251)

This macro-free example uses the same calls as T1's
`shape_has_no_semantic_identity_and_membership_requires_claim`,
`root_claim_survives_packaging_and_nested_instances` and T3's compiled bindings.
Those executable tests are the compilation evidence; this Markdown block is not
itself a doctest. Ordinary closure ports really are `run.value` and output `value`.

```rust
use phenix_graph::{DeclaredValue, GraphBuilder, GraphContract, GraphUnit, InterfaceShape};

fn main() {
    let contract = GraphContract::new(
        "app/transform@1",
        InterfaceShape::new()
            .input_slot("run", "value", DeclaredValue::local::<i64>()).unwrap()
            .output("value", DeclaredValue::local::<i64>()).unwrap(),
    ).unwrap();

    let mut service = GraphBuilder::new("service-definition");
    let private = service.node("private", |x: i64| x + 1).unwrap();
    service.expose_input_slot("run", "value", private.inlet()).unwrap();
    service.expose_output("value", private.output()).unwrap();
    service.implements_public(&contract).unwrap();

    let mut graph = GraphBuilder::new("host");
    graph.node("worker", |x: i64| x + 1).unwrap();
    graph.implements("worker", &contract).unwrap();
    graph.instance("service", &service.module()).unwrap();
    for path in ["worker", "service"] {
        let view = graph.inspect(path).unwrap();
        let unit: &dyn GraphUnit = &view;
        contract.check_implementation(unit).unwrap();
    }
    let (from, to) = {
        let node = graph.bind_contract("worker", &contract).unwrap();
        let module = graph.bind_contract("service", &contract).unwrap();
        (node.local_output::<i64>("value").unwrap(),
         module.local_input::<i64>("run", "value").unwrap())
    };
    graph.connect(from, to).unwrap();
    let compiled = graph.compile().unwrap();
    let bound = compiled.find("service").unwrap().bind_contract(&contract).unwrap();
    let input = bound.input("run", "value").unwrap();
    assert_eq!(input.resolve(&compiled).unwrap().len(), 1);
    assert!(bound.output("debug").is_err());
}
```

At that historical revision portable types used `Node::wire`, `DeclaredValue::wire`, bound
`wire_input::<T>`/`wire_output::<T>` and `connect`. T1's
`authoring_wire_binding_limits_endpoints_and_connects_nodes_and_nested_modules`
provides concrete node and nested-module examples. T10 defines three independent
sets of request/response types and wires both edges without importing the
provider's implementation types. A plain `request` slot is not manufactured by
an ordinary closure; custom slots require a real layout or low-level declaration.

### Historical Public Ownership Inventory (PR #1251)

This inventory includes new public types, methods and declaration fields, plus
private state added to public owners where that state enforces a public invariant.
Existing unrelated graph/runtime APIs are not duplicated here.

| Surface And Actual Members | Owner / Reason | Invariant And Evidence |
|---|---|---|
| `InterfacePhase::{Definition, Authoring, Compiled}` | Graph enum for diagnostic stage, not execution state. | T1 `errors_report_contract_phase_path_endpoint_and_identities`; T3 resolve errors. |
| `InterfaceError { contract: Option<Box<str>>, path: Box<str>, phase, endpoint: Option<Box<str>>, expected: Option<TypeIdentity>, actual: Option<TypeIdentity>, message: Box<str> }`; `Display`, `Error` | Graph structured failure, enough context without executable authority. | T1 diagnostics; compile wraps formatted error in existing `CompileDiagnostic` at Index phase, not a new typed compiler-error variant. |
| `InterfaceShape::{new, node(NodeSignature), input_slot, output, inputs, outputs, check_shape, same_shape}` | Graph owned ID-free shape; private ordered named maps plus numeric unnamed maps. Builders consume `self` and return `Result<Self, InterfaceError>` for insertion. | Shared equality/matching; T1 H07 tests and I's unnamed/local identity tests. No action pointer or grant. |
| `GraphContract::{new, id, required, check_shape, check_implementation, check_definition}` | Graph private `id: String`, `required: InterfaceShape`; immutable through accessors. | `new` validates ID; checks take `&dyn GraphUnit` except exact definition check taking `&Self`; T1 structural versus semantic checks and conflicts. |
| `UnitProvenance { phase, definition: String, path: String, instance_path: Vec<String> }` | Graph diagnostic definition/instance context, separate from provider origin. | T1 nested snapshots and T3 root provenance. Not a permission or plugin generation token. |
| `UnitSnapshot { shape, claims: Vec<GraphContract>, provenance }`, `check_contract(&GraphContract) -> Result<(), InterfaceError>` | Graph owned historical inspection data. Public fields allow data construction, not sealed-unit attestation. | T1 absent/contradictory claims; final compiler rechecks rather than trusting snapshots. |
| `GraphUnit::inspect` | Graph sealed object-safe trait on `AuthoringUnit`, `CompiledGraph`, `FindResult`, `NodeRef`, `ModuleRef`. | T1 dynamic inspection, T3 compiled root; two compile-fail doctests reject external implementations and `NodeHandle` as unit. |
| `AuthoringUnit<'a> { graph: &'a GraphBuilder, path: String }`, consuming `bind_contract` | Graph borrowed effective view, no cached shape. | A overlays pending layouts; T1 late-layout revalidation and missing-path tests. |
| `BoundAuthoringUnit { unit, contract }`; `wire_input<T: ContractValue>`, `wire_output<T: ContractValue>`, `local_input<T: Send + Sync + 'static>`, `local_output<T: Send + Sync + 'static>` | Graph borrowed projection, re-exported at the crate root; encoded outputs are `OutputRef<T, EncodedValue>`, inputs `Input<T, EncodedValue>`; local endpoints default to `LocalValue`. All return `InterfaceError`. | Checks promised name/type then normal authoring accessibility; T1 wrong type, extra debug output, private nested access and revision revocation. |
| `GraphBuilder::{inspect, implements, implements_public, require_contract, bind_contract}` | Graph authoring; requirements store paths for final checking, not early certification. | T1 final requests and claims; T5 transitive identity; A claim footprint ledger. |
| `NodeTemplate::claims: Vec<GraphContract>`; private `composition_origin`, `declaration_id` | Graph declaration owns explicit promises and handler lineage; layout retarget preserves claims/origin, replacement requires new explicit claims. | T1 low-level claims and explicit replacement; T4 origins; T8 late layout. |
| `ModuleDef::{claims, contract_requests, declaration_ids: BTreeMap<String, u64>}`; corresponding private builder storage | Graph reusable declaration owns obligations and instance lineage; raw literals may use an empty ID map without typed handles. | T5 retained requests; T8 direct literal; T2 low-level handles. No plugin-owned actual shape. |
| `ModuleInput::{port: String, slot: String}` alongside existing `name`, ID, identity and targets | Graph actual public boundary, not a free-floating annotation. | T6 literal dots, duplicate pairs and collision routing; G fanout identity checks. |
| `GraphBuilder::expose_input_slot<E: InputEndpoint>(port: impl Into<String>, slot: impl Into<String>, to: E) -> Result<PublicInput, String>` | Graph exact public grouping without a wrapper node. | T6 independent slot waves and duplicates. Plain exposure still defaults to value. |
| `InputEndpoint::expose_slot(self, &mut GraphBuilder, String, String) -> Result<PublicInput, String>`; `expose` default delegates with `"value"` | Graph existing sealed endpoint trait extended for typed and symbolic grouping. | T6 both exposure paths; addresses.rs resolves actual accessible endpoints. |
| `ModuleHandle::{input_slot<T: ContractValue>, local_input_slot<T: Send + Sync + 'static>}` returning `Result<Input<T, EncodedValue>, String>` and `Result<Input<T>, String>` respectively | Graph typed boundary access with private `input_slots: BTreeMap<(String, String), String>` and declaration token. | T6 nested real routing; T2 stale module handles. Existing single-name access means value slot. |
| `ModulePortInfo::{port_name, slot_name}(&self) -> &str` | Graph compiled inspection metadata; existing `name()` remains routing key. | T6 resolved left/right targets; outputs report their output name and value slot. |
| `GraphBuilder::connect` | Graph connection verb for local, encoded and symbolic endpoints. Local Rust types must agree; encoded Rust types may differ when canonical descriptors agree. | T7 independent execution, descriptor mismatch dimensions, compile-time local/mixed-mode rejection and stale rejection. |
| `BoundUnit<'g> { graph: &'g CompiledGraph, path: String, contract: GraphContract }`, `Clone`, `input`, `output` | Graph compiled projection; private state ties lookup to one immutable graph and promised names. | T1 extension hiding; T3 named/nested/root bindings. |
| `BoundInput<'g> { unit, endpoint: String, targets: Vec<InputAddr> }`, `resolve`; `BoundOutput<'g> { unit, endpoint: String, source: OutputAddr }`, `resolve` | Graph endpoint projections retain binding borrow and diagnostic context rather than returning unowned addresses immediately. | T3 graph pointer checks, including numerically equal addresses in different graphs and same-Arc acceptance. |
| `bind_contract` on compiled root and compiled named views | Graph validation against actual immutable metadata; signatures above. | T3 root path is empty but not selected by `find`; T1 node/module parity. |
| `GraphOrigin`, `Default`, `Clone`, `PartialEq`, `Eq`; `GraphBuilder::with_origin`; `FindResult::origin() -> Option<&GraphOrigin>` | Graph opaque allocation identity supplied by trusted composition host. | T4 restoration, non-relabel, module versus node attribution; T9 rejects sibling/aborted origins. |
| Private `declaration_id` in `Input`, `OutputRef`, `NodeHandle`, `PublicInput`, `ModuleHandle`, `NamedNodeHandle`; physical node/instance metadata and directed `DestinationTarget` | Graph revision lineage is necessary because equal scopes/names/types are insufficient after clones/replacements. `NamedNodeHandle` retains registration identity only, without endpoint projection. | T2, T8 and T1 bound revision tests. `Output::to` now carries lineage to final destination resolution, not semantic matching. |
| Private GraphBuilder `public_input_ids`, fresh clone `definition_id`, dependency `identities`, isolated pending Rc; package/compiled `composition_origins`, builder `active_origin` | Graph preserves inherited replay but prevents divergent package deduplication and origin loss. | T2 clone preflight; T5 both import orders and ancestor refresh; T4 packaging. |
| `Contribution::graph_unit(&self) -> Option<&str>` default `None` | Plugin locator only; registry no longer stores implementation bounds. Existing `KIND` and `id` remain. | T9 missing locator/path and actual node/module binding. |
| `PluginSnapshot::graph_origin`, `ContributionHandle<C>::graph_origin` | Plugin exposes Loaded's private Graph origin; existing Arc-backed handles retain implementation/factory lifetime. | T9 same numeric revision across sets/candidates rejects; T10 unload/drop counters. |
| `PluginSnapshot::bind<C: Contribution>(&self, id: &str, requested: &GraphContract, graph: &CompiledGraph) -> Result<ContributionHandle<C>, Failure>` | Plugin validates actual graph then returns a code lease, not a graph-bound endpoint certificate. | T9 `MissingBinding`, `Denied` origin and `SchemaRejected` claim paths; caller must revalidate another graph. |
| `ContractDefinition { id: String, contract: GraphContract }` (`graph.contract`) | Kernel contribution references Graph's semantic definition, not another schema language. | T10 conflicting definitions including unused definitions rejected before composition. |
| `GraphOffer { id: String, unit: String, contract: Key }` (`graph.offer`) | Kernel contribution names a real unit and definition; `graph_unit()` returns its path. | T11 valid/missing node/module offers and compatible-claim foreign replacement rejection; K delegates final contract checks to Graph. |
| `ContractRequirement { id: String, contract: Key, offer: Key }` (`graph.requirement`) | Kernel explicit host selection; no discovery, wiring or authorization. | T10 absent selection, unrelated semantic ID, conflicting definition. Requirement is not proof a closure wired its selected path. |
| Kernel private `_snapshots: Vec<PluginSnapshot>` alongside existing composition `leases` | Kernel retains definition/offer/consumer code past engine/action teardown; producer-first drop remains. | T10 old/new behavior and drop counters; no atomic activation receipt or new scheduler. |

## Historical Kernel Policy Boundary

This section describes the original API. Current `GraphOffer` adds `producer`,
`ContractRequirement` uses `consumer` instead of `offer`, `ImportPolicy` controls
selection, and `ComposeContext` exposes local imports and candidate artifacts.
The current coordinator and trust limits are documented in [Kernel](kernel.md).

`Kernel::build(name, snapshots, executor)` collects typed definition/offer/
requirement contributions, resolves references, rejects conflicting definitions,
orders compose callbacks using `Contribution::requires`, composes into one builder
under each generation's origin, registers every offer and selected requirement
using `require_contract`, compiles once, checks final actual units/origins, checks
routes, then builds one deployment. Producer activation is still `Kernel::start`.

Host-selected offers are a deliberate policy boundary, but this is **not full
implementation of a requirement that Kernel itself resolves providers and wires
consumers**. The host selects the `Key`; the consumer closure must use the same
offer path and explicitly call `connect`. Changing only a requirement
does not change captured wiring. Native code can wire a different path; validation
does not prove otherwise. T10 is real A-definition/B-provider/C-consumer execution,
not evidence of automatic resolution or automatic edge insertion. This is an
additional mission limitation, not a redefinition of H19's scheduler/dependency
constraint. Explicit owner-qualified selection is supported host policy; no
automatic wiring or integrated activation transaction is claimed.

A host may prepare a plugin reload, build using the candidate snapshot, then
commit and start/publish the new kernel. Build failure leaves the old set/kernel
usable when the stage is dropped. Build success neither commits the stage nor
starts producers. Commit can reject a stale stage, and start can fail. No integrated
atomic activation coordinator makes these operations a single transaction. Old
graphs remain pinned and do not redirect to new providers after reload/unload.

## Candidate Comparison

At the historical PR #1251 revision only C2 was implemented. The alternatives
below are historical **proposals**, not current authoring examples, with
missing attachment/projection/adapter machinery explicitly identified. They use
ordinary closure `run.value` / output `value`, not fictitious default request ports.

### C1: Extended Bounds

```text
PROPOSED C1
Graph: InterfaceBounds { optional semantic id, named boundary }
       NodeTemplate / ModuleDef --> attached bounds list --> compiler validation
       common unit view --> ID-free inspection mode --> bound projection
Plugin: locator + Graph bounds expectation --> actual final-unit checker
Kernel: selected offers + leases --> same Graph compiler, explicit host wiring
```

Proposed node and module operations, not available in this tree:

```rust,ignore
let bounds = InterfaceBounds::new("app/transform@1")?
    .input_slot("run", "value", DeclaredValue::local::<i64>())?
    .output("value", DeclaredValue::local::<i64>())?;
graph.node("worker", |x: i64| x + 1)?;
graph.claim_bounds("worker", &bounds)?; // missing C1 attachment
let n = module.node("private", |x: i64| x + 1)?;
module.expose_input_slot("run", "value", n.inlet())?;
module.expose_output("value", n.output())?;
module.claim_public_bounds(&bounds)?; // missing reusable root attachment
graph.instance("service", &module.module())?;
let bound = graph.bind_bounds("service", &bounds)?; // missing projection
```

This would minimize nominal migration from the historical bounds API but needs
an ID-free mode, multiple declaration claims, effective extraction, final checks,
origins and restricted binding. Optional/synthetic IDs conflate actual structure
with an explicit semantic promise. C1 is feasible, not verified; splitting that
representation leads toward C2. `InterfaceBounds` is removed, not a live C1 prototype.

### C2: Shape Plus Claim

The ownership diagram, actual signatures and macro-free example above are C2's
node and module API. Its additional types separate genuinely different roles:
wire descriptor, actual shape, semantic definition, historical snapshot,
authoritative borrowed unit, restricted projection and composition origin.
No plugin implementation descriptor or runtime wrapper node is introduced.
The historical implementation checks and exact-head CI passed. Its remaining limitations were manual
host-selected wiring, explicit activation sequencing, API complexity and unbounded
schema interning, not missing origin/generation checks or regression verification.

### C3: Per-Contract Trait

```text
PROPOSED C3
Graph: authoritative declarations --> erased shape/claim compiler metadata
       sealed generated/handwritten TransformV1 adapter (node or module)
       typed endpoint methods --> normal Graph connections
Plugin: dynamic contract registry --> checked erased adapter selection
Kernel: selected offer + existing leases --> same compiler and engine
```

Proposed adapter surface and usage, not available in this tree:

```rust,ignore
trait TransformV1: private::Sealed {
    fn input(&self) -> Result<Input<i64>, InterfaceError>; // actual run.value
    fn output(&self) -> Result<OutputRef<i64>, InterfaceError>; // actual value
}
graph.node("worker", |x: i64| x + 1)?;
graph.claim_transform_v1("worker")?; // missing generated claim API
let n = module.node("private", |x: i64| x + 1)?;
module.expose_input_slot("run", "value", n.inlet())?;
module.expose_output("value", n.output())?;
module.claim_public_transform_v1()?;
graph.instance("service", &module.module())?;
let (from, to) = {
    let node = graph.transform_v1("worker")?;
    let service = graph.transform_v1("service")?;
    (node.output()?, service.input()?)
};
graph.connect(from, to)?;
```

Typed spelling and a closed Rust catalog could benefit. Dynamic IDs, independent
wire types and heterogeneous lookup still require erased metadata and final
validation; arbitrary user trait implementations cannot attest actual membership.
No generator, adapters, compile-fail spoof tests or registry parity tests exist
for this candidate. Optional adapters on top of C2 remain feasible future work,
not necessary infrastructure added now.

### C4: Structural Only

```text
PROPOSED C4
Graph: NodeTemplate / ModuleDef --> actual InterfaceShape --> structural checker
       shape-bound node/module projection --> normal connections
Plugin: locator + required shape (no semantic claim or ID)
Kernel: selected structural offer + leases --> same Graph compiler and engine
```

Proposed projection API, not available as a C4 implementation:

```rust,ignore
let required = InterfaceShape::new()
    .input_slot("run", "value", DeclaredValue::local::<i64>())?
    .output("value", DeclaredValue::local::<i64>())?;
graph.node("worker", |x: i64| x + 1)?;
graph.inspect("worker")?.check_shape(&required)?; // proposed convenience
let n = module.node("private", |x: i64| x + 1)?;
module.expose_input_slot("run", "value", n.inlet())?;
module.expose_output("value", n.output())?;
graph.instance("service", &module.module())?;
let bound = graph.bind_shape("service", &required)?; // missing C4 projection
```

C2's structural checker is useful implementation evidence for feasibility, not
an independent C4 result. C4 cannot distinguish unrelated operations with equal
shapes and deliberately has no explicit semantic membership, inherited claims or
same-ID conflicts. Those are FAILs for the semantic task, not cosmetic deductions.

## Horizontal Gates

This entire matrix is historical PR #1251 evidence. In particular H08's output
extension policy is superseded by exact single-output matching. Its PASS labels
and later weighted scores are not current redesign verification or ratings.

The matrix preserves the requested H01-H23 and Q01-Q05 criteria. PASS means
implemented with the cited source/tests and exact-commit verification below,
**not** complete mission acceptance. PARTIAL names a missing implementation,
check or quality constraint; FAIL means the candidate omits the requirement.
C1/C3/C4 remain unimplemented, and reusable C2 infrastructure is not independent
verification of those designs.

| Gate / Requirement | C1 Extended Bounds | C2 Shape + Claim | C3 Per-Contract Trait | C4 Structural Only |
|---|---|---|---|---|
| H01 Node/exported subgraph same canonical API | PARTIAL: common bounds-based node/module view and projection missing. | PASS: I/A sealed `GraphUnit` and binding on nodes, modules and compiled root; T1 dynamic views, T3 root. | PARTIAL: both typed adapters plus common erased API missing. | PARTIAL: shared structural unit view feasible, independent candidate not built. |
| H02 Actual shape authoritative no forged copy | PARTIAL: must extract effective declarations instead of trusting copied bounds. | PASS: A pending overlay, G physical/public metadata, I sealed views; T1 late-layout/low-level negatives, compile-fail doctests, T12 symbolic pending checks. | PARTIAL: seal adapters and resolve actual declarations; descriptor/adapter parity unimplemented. | PARTIAL: actual extractor and sealed structural view missing as a candidate. |
| H03 Named input ports individual slots survive exports flatten inspection | PARTIAL: explicit module port/slot mapping and expansion missing. | PASS: G/B `ModuleInput` port/slot metadata and routing keys; T6 nested calculators, literal dots/collisions, resolved physical slots; T1 compiled shape parity. | PARTIAL: generated module export/flatten mapping missing. | PARTIAL: named grouping can be structural, but candidate export/inspection integration missing. |
| H04 One Graph matcher shape claims plugin compile | PARTIAL: one bounds matcher across inspection/claims/compiler/plugin not implemented. | PASS: I `InterfaceShape::check_shape`, `UnitSnapshot::check_contract`, `validate_metadata`; A/P/K delegate; T1/T9/T10 structural and semantic positives/negatives. | PARTIAL: erased Graph matcher needed behind generated methods, no implementation. | PARTIAL: single structural matcher feasible; semantic claim integration absent by design. |
| H05 Shape differs semantic equal shape no claim | PARTIAL: add ID-free bounds mode and explicit declaration opt-in. | PASS: I separates shape/contract; T1 equal-shape unclaimed/unrelated-ID tests, T3 unclaimed root, T14 empty-module opt-in. | PARTIAL: explicit checked adapter claim rather than arbitrary Rust trait implementation missing. | FAIL: structural-only semantics cannot distinguish equal shape without opt-in. |
| H06 Same ID conflicting def rejects before composition/publication | PARTIAL: cross-definition registry and final validation missing. | PASS: K rejects registered conflicts before compose; I rejects graph claim/request conflicts at final compile before deployment/publication, not globally at every authoring call; T1 cross-module/request conflicts, T10 definition mismatch. | PARTIAL: dynamic erased definition conflict registry missing. | FAIL: no semantic IDs or definition conflicts in candidate. |
| H07 Exact inputs | PARTIAL: implement exact named/unnamed input comparison and regressions. | PASS: I `check_shape`; T1 three `h07_*` tests reject extra/missing named ports/slots on actual nodes/modules; I unnamed H07 map/identity test. | PARTIAL: adapters need exact erased input validation, tests absent. | PARTIAL: exact structural comparison feasible, candidate tests absent. |
| H08 Required outputs extra allowed | PARTIAL: separate satisfaction from exact bounds-definition/replacement equality. | PASS: I output inclusion versus `same_shape`; T1 output-subset/debug-extension test and bound extension hiding. | PARTIAL: generated promised outputs plus erased extension matching missing. | PARTIAL: output inclusion feasible, independent structural matcher unverified. |
| H09 No local TypeId wire coercion | PARTIAL: preserve identity distinctions in new bounds extraction/binding, tests missing. | PASS: I identity checks, A typed projections, B connector; I local/wire tests, T1 distinct local types/wrong binding, T7 actual local endpoint rejection. | PARTIAL: typed/erased adapter identity enforcement missing. | PARTIAL: structural identity distinction feasible but not candidate-tested. |
| H10 Explicit safe portable unrelated Rust types execute | PARTIAL: descriptor-aware cross-type connector missing. | PASS: B `connect` on encoded endpoints; T7 `independently_compiled_portable_crates_execute` builds provider/consumer rlibs and executes the linked harness; bidirectional codecs/descriptor negatives/native decode failure and T10 A/B/C execution add coverage. | PARTIAL: descriptor-based cross-type adapter/connector missing. | PARTIAL: descriptor connector feasible, candidate execution unimplemented. |
| H11 Nominal legacy no portable certification | PARTIAL: strict descriptor path separate from nominal legacy missing. | PASS: B requires explicit `Some` descriptors/canonical Wire endpoints; T7 equal-SCHEMA legacy/overridden-identity rejection; ordinary same-type baseline retained. | PARTIAL: nominal-versus-structural adapter admission missing. | PARTIAL: legacy rejection possible structurally, candidate tests missing. |
| H12 Bound only promised endpoints | PARTIAL: restricted bounds projection missing. | PASS: A/I promised-name/type projections; T1 debug/private/wrong-type negatives and authoring connections, T3 restricted root/nested compiled projections. | PARTIAL: sealed promised-only adapters not implemented. | PARTIAL: shape-bound projection possible but missing. |
| H13 Replacements protect all edges exports used extra | PARTIAL: exact complete public replacement plus claim/request retention missing. | PASS: B compares complete node numeric/name/identity signatures and module public maps, A exact shape/obligations; T1 explicit replacement and rollback, T5 requests, T6 grouping; existing assembly replacement tests preserve external edges/exports. Output extensions are not ignored by replacement. | PARTIAL: generated replacement must protect full actual boundary, not only adapter methods; missing tests. | PARTIAL: exact structural replacement feasible, candidate integration/regressions missing. |
| H14 Stale snapshots crossclone no false validation | PARTIAL: effective revalidation, declaration tokens and package lineage missing. | PASS: snapshots are data, not `GraphUnit`; B/G revision tokens; T1 late-layout recheck, T2 divergent/inherited clones, T5 both dependency import orders, T8 layout revision, T12 stale symbolic rollback. | PARTIAL: adapters need declaration lineage and effective revalidation, absent. | PARTIAL: structural receipts need lineage/revalidation, absent. |
| H15 Offer no duplicate/forge compiled claims | PARTIAL: replace copied offer authority with actual-unit claim validation and provenance. | PASS: P registry stores locator, not actual claims; K offers reference Graph definitions; I checks actual compiled claims, O origin prevents foreign-path laundering; T9 unclaimed/dishonest definitions, T11 corrected foreign node/module replacement matrix. | PARTIAL: offers must resolve checked erased actual claims, no adapter implementation. | FAIL: structural-only offers cannot require actual explicit semantic claims. |
| H16 Staged final candidate validation failure old preserved | PARTIAL: candidate final-unit validation and retained old generation tests missing. | PASS: P staging and K final compile/check before build/start; T10 failed candidate drops without commit/activation and old graph executes; T12/T13 rejected patches preserve graph/revisions/handles. No atomic commit/start coordinator is implied. | PARTIAL: candidate adapter validation/rollback lifecycle unimplemented. | PARTIAL: structural candidate validation with preserved leases feasible, tests missing. |
| H17 Bound handles/plugin leases generation safe | PARTIAL: graph-bound projections and exact-origin leased binding missing. | PASS: I graph-pointer `resolve`, P Arc-backed Loaded/origin leases, K retained snapshots; T3 equal-address foreign graph rejection/same-Arc acceptance, T9 sibling/aborted candidates, T10 old/new reload/unload/drop behavior. | PARTIAL: adapter generation and executable lease coupling missing. | PARTIAL: structural binding still requires graph-generation and code leases, unimplemented. |
| H18 No auth/semantic proof/scheduling from claims | PARTIAL: must document/test bounds as declarations, not grants/behavior/readiness. | PASS: I metadata has no grants/policy; T14 equal claims with AnyOne/AllSameCohort/custom policy differ in invocation count and never promise emission; read denied without separate grant; T7 malformed codec still fails. Semantic opt-in is not proof of behavior. | PARTIAL: trait membership must not imply behavior, readiness or grants; candidate tests missing. | PARTIAL: structural equality implies neither behavior nor authority; candidate policy/auth tests missing. |
| H19 No scheduler/storage dep/blanket ABI/crate explosion | PARTIAL: feasible on existing compiler/engine, candidate dependency/diff audit missing. | PASS: G/B/K reuse compiler/engine and existing crates; Graph has no storage dependency, S shared descriptor reused; T10 real engine execution and exact-commit workspace regressions pass. Native Rust registration remains in-process, not blanket ABI compatibility. | PARTIAL: adapter generation could stay authoring-only, implementation/dependency audit missing. | PARTIAL: structural tooling could reuse engine without storage, candidate audit missing. |
| H20 Macrofree lowlevel viable | PARTIAL: macro-free bounds attachment/extraction prototype missing. | PASS: T1 manual shapes/claims and low-level templates, T8 direct module literal/manual execution, T14 source/sink; exact-commit no-default workspace tests pass. | PARTIAL: handwritten adapters possible, macro-free dynamic catalog prototype missing. | PARTIAL: macro-free structural surface feasible, candidate prototype missing. |
| H21 Full Derived V1 preserved | PARTIAL: candidate-specific full derived regression run missing. | PASS: exact-commit default/no-default workspace runs, derived suites and example exit 0; existing engine-membership/freshness/cohort/redeploy/authority checks remain authoritative; T2 preserves deliberate preflight replay. See protected-invariant table. | PARTIAL: adapter generation/lifecycle changes need full Derived V1 regressions. | PARTIAL: structural integration needs full Derived V1 regressions, absent. |
| H22 Every protected invariant negative + multiplugin | PARTIAL: bounds-specific positive/negative and multiplugin matrix absent. | PASS: traceability below maps T1-T14 plus existing Graph/Kernel/storage/Derived negative/recovery suites; exact-commit workspace runs pass, T10 real three-plugin execution, T11 foreign offers, T14 source/sink/readiness/auth. This is executable coverage, not exhaustive behavioral proof. | PARTIAL: spoofed adapters/version registry/multiplugin and protected negatives missing. | FAIL: structural negatives feasible but required semantic membership/conflict negatives cannot be satisfied by this design. |
| H23 Exact new-head fmt workspace nodefault clippy Nix CI | PARTIAL: no implemented candidate/head or checks. | PASS: all ten Cargo commands and full Nix check exit 0 on `2a95b8519ff162caa68fee8b844df6ca42e1e515`; [CI 42503290164](https://github.com/matthis-k/phenix-ai/actions/runs/42503290164) completed/success on that same SHA. Exact commands and check inventory are below. | PARTIAL: no implemented candidate/head or checks. | PARTIAL: no implemented candidate/head or checks. |
| Q01 Smallest API stateful abstraction | PARTIAL: fewer names, but ID-free bounds mode and restricted projections still undesigned. | PARTIAL: I/A/B/O split real ownership roles and reuse Graph; T1/T3/T14 validate useful stateful projections. No minimality proof; revision/origin tokens and maps remain real API/maintenance costs. | PARTIAL: adapter/codegen/erased registry state exceeds plain traits; prototype/cost evidence missing. | PARTIAL: smallest structural surface feasible, but omits required semantics and has no candidate prototype. |
| Q02 Diagnostics path stage contract endpoint expected actual | PARTIAL: contextual structured bounds errors missing. | PASS: I `InterfaceError` fields; T1 asserts phase/path/contract/endpoint/identities, T3 generation context. Optional fields are populated when applicable; existing compiler/plugin/kernel wrappers retain formatted context, not every legacy error supplies identities. | PARTIAL: adapters need both runtime path and expected/actual identity diagnostics, absent. | PARTIAL: structural diagnostic context feasible, candidate errors/tests absent. |
| Q03 Migration bounds ContractValue graph_bounds | PARTIAL: bounds attachment migration missing. | PASS: G/B/P remove InterfaceBounds/copied graph_bounds with no shim, T8 literal migration, T9 actual-unit replacement test; required SCHEMA/ordinary same-type legacy retained while portable path rejects nominal certification. | PARTIAL: every semantic contract needs adapters and erased registry migration. | FAIL: removing semantic identity loses required old membership meaning, not an equivalent migration. |
| Q04 No per-wave matching bounded schema lifecycle | PARTIAL: authoring-only comparison feasible; bounded schema-lifecycle policy missing. | PARTIAL: I/B match during authoring/compile/bind, no per-wave matcher; new connector uses `canonical`, not `intern`. S still leaks distinct canonical identities for process lifetime; finite stable reload descriptors are expected, not enforced. | PARTIAL: avoid runtime adapter matching and bound schema registry; both unverified. | PARTIAL: structural matching can be compile-time, but bounded schema-lifecycle implementation missing. |
| Q05 Portable schema for future ABI no Rust trait objects claims | PARTIAL: bounds endpoints could reuse S portable descriptors; no candidate boundary design/verification. | PASS: S owned name/revision/structure/codec and canonical framing, B explicit codec admission, T7 independent Rust codecs; claims are GraphContract data, not exported Rust trait objects. Future ABI adapter is not implemented or guaranteed. | PARTIAL: per-contract Rust traits alone are not portable; erased schema/data boundary and explicit ABI adapter needed. | PARTIAL: portable shape descriptors feasible, but candidate schema/ABI boundary unimplemented. |

Automatic Kernel provider resolution/wiring and atomic plugin/kernel activation
remain additional mission limitations described above. They are not substituted
for H19 or H20, and do not become implemented merely because explicit host policy
is supported.

## Invariant Traceability

The following is positive/negative evidence, not a claim that a passing happy
path covers all mutation paths. File aliases resolve in the Evidence Index.

| Invariant | Positive Evidence | Negative / Recovery Evidence |
|---|---|---|
| Actual shape is not semantic membership | T1 `graph_contract_structural_and_semantic_checks_are_distinct_on_real_units` checks authoring and compiled views. | T1 unclaimed and unrelated-ID rejection; contradictory claims reject in both orders; T3 unclaimed compiled root rejects binding. |
| Exact inputs, output extensions | T1 `exact_definition_and_output_subset_matching_are_distinct`; bound extra output remains visible only unbound. | T1 three H07 tests reject named extra/missing ports/slots for real nodes/modules and low-level final claims; I unnamed H07 test rejects extra/wrong numeric inputs. |
| Complete named/unnamed identity | T1 multiport extraction and endpoint-name Graph grammar; I diagnostic-name-independent local identity. | I unnamed endpoints cannot satisfy names; local/wire do not coerce; T1 distinct local Rust types reject. |
| Pending layout cannot retain stale certification | T1 effective layout reports new slots and retained claims; T12 symbolic field connections/exposure execute pending local layout. | T1 late invalidated claim and invalid public boundary fail final compilation; T8 consumed/configuration revisions reject; T12 stale atomic/ambiguous symbolic paths roll back before successful recovery. |
| Public grouping is real routing metadata | T6 nested calculators route two actual slots in one wave; literal grouped exposures execute independently. | T6 duplicate pairs, private lookup and changed grouping replacement reject; escaped-key tests run both declaration orders. G validates fanout targets/identity; dedicated exhaustive fanout corruption matrix was not added. |
| Root and instance boundaries | T1 root claim survives packaging/nesting; T3 compiled root maps actual physical endpoints. | T3 `find("")` is None, unpromised root outputs reject, foreign graph generation rejects; T1 invalid direct root claim fails catalog compilation. |
| All final claims/requests are checked | T1 equal repeated definitions compile; T5 preserved requests compile with valid internal claims. | T1 low-level bad claims/conflicting definitions/missing requests fail; invalid claim factory count stays zero; T5 replacement preserving request but losing internal membership fails final compile. |
| Explicit replacement promise | T1 `same_shape_node_replacement_requires_author_claim_without_mutating_on_failure` executes old +0 behavior then explicit claimed +1 replacement. | Same shape without claim rejects and leaves handles usable; T1 module claim removal rejects unchanged; T5 removes/changes request path/definition rejects before import/revocation. |
| Patch claim and replacement writes are transactional | A `claim_changes` accounts for root/node/transitive claims and root requests; B assembly diff observes declaration ID changes even for the same package/factory. | T1 undeclared claim write rolls back; T13 same-package/same-factory replacements require declared writes, roll back earlier edits and preserve handles/origin on failure; declared writes revoke old handles. Existing patch suites pass in exact-commit workspace runs; no exhaustive nested-ledger fuzzing claimed. |
| Dependency edits cannot hide behind wrappers | T5 `claim_edits_refresh_every_ancestor_alias_but_leave_unrelated_dependencies_reusable`; repeated identical claim preserves reuse. | T5 nested node/module claim conflicts reject in both import orders; cloned unique wrappers cannot hide divergent dependencies; failed import leaves no second instance. |
| Clones distinguish lineage without banning replay | T2 inherited endpoints connect in both preflight and original; T8 inherited old revision still works on untouched clone. | T2 sibling same-name handles/public inputs and replacement endpoints reject; directed destination runtime checks reject divergent nodes/modules; T8 revised endpoint cannot bind unchanged clone. |
| Origin tracks actual ownership boundary | T4 created nodes, low-level registration, nested instantiation and replacement record expected opaque origins. | T4 no relabel by inspect scope, unscoped replacement clears origin, failure preserves origin, nested error/panic restores scope; T9 equal numeric revisions/aborted/sibling origins reject; corrected T11 compatible-claim foreign nodes/modules/replacements fail origin checks. |
| Compiled endpoints cannot cross generations | T3 same Arc and endpoints retained after binding drop resolve. | T3 equal paths/addresses across separately compiled graphs reject for node, nested module and root, with compiled path/contract/endpoint context. |
| Portable encoding is explicit | T7 separately compiled provider/consumer rlibs execute through the linked `portable_execution` harness; independent codecs also roundtrip vectors in both directions; T10 independent A/B/C types execute. | T7 changes name/revision/structure/codec/optionality independently; legacy None, local endpoints, overridden nominal identity, stale tokens reject; malformed bytes cause native `InvalidArgs`, later valid wave succeeds. |
| Plugin bind uses final actual unit | T9 node/module bind requires matching origin and actual claim. | Missing locator/path, dishonest definition and unclaimed shape reject; old snapshot still binds old graph after unload, newer/sibling/candidate snapshot does not. |
| Kernel checks final selection before activation | T10 real three-plugin execution; T11 valid node/module matrix passes. | T10 missing definitions/paths/selections, unrelated IDs, conflict, failed candidate and late builder replacement reject with activation counters unchanged. Does not prove automatic wiring or activation atomicity. |
| Diagnostics preserve useful context | T1 asserts phase/path/contract/endpoint/expected/actual; T3 asserts generation error context. | Compile errors are formatted into existing diagnostics; plugin errors into `Failure`; kernel uses `String`. Not every legacy boundary error has optional endpoint/identity fields populated. |
| Empty/source/sink boundaries are real interfaces | T14 zero-input source output is addressable; zero-output sink input is bindable and executes with zero publications. | T14 empty actual module with equal shape rejects authoring/compiled membership and binding until explicitly claimed. Zero endpoint count does not fabricate membership or emission. |
| Claims are not scheduling or authorization | T14 identical shapes/claims bind under AnyOne, AllSameCohort and custom SelectRight policies; explicit resource grant permits a read. | Same left-only input produces different invocation counts by policy; declared output emits nothing; matching reader claim still receives `read of counter not granted` without separate resource grant. |

### H22 Protected Invariants

Existing protected behavior is not replaced by contracts. H22 is PASS for the
positive/negative regression coverage below, supported by the default/no-default
workspace runs on implementation commit `2a95b8519ff162caa68fee8b844df6ca42e1e515`.
H21 uses the existing full Derived V1 suites, not a new contract-only approximation.
The exact-commit command results and matching CI evidence also satisfy H23.
Coverage means concrete executable regressions, not an exhaustive proof of every
possible native handler, codec, malformed declaration or concurrent host sequence.

| Protected Area | Verified Evidence And Limits |
|---|---|
| Graph identity, layouts, module routing, replacement, clone replay and codec failures | T1-T8 plus I unit/doctests supply positive/negative boundaries; T11-T14 cover origins, stale symbolic rollback, same-package/factory patch writes, source/sink and policy/auth distinctions. Existing conformance/output/patch suites pass in exact-commit workspace verification. |
| Single engine/executor, waves/cohorts, publication retention and module output timing | G/B reuse the engine but add directed destination lineage checks, so this is not zero runtime change. Existing Graph output/cohort/fanout negatives and recovery suites pass in exact-commit workspace verification; T2 divergent directed targets reject, T14 static claims do not alter readiness/emission. |
| Kernel dependencies, route preflight, rollback/retry and drop order | [kernel.rs](../packages/phenix-kernel/tests/kernel.rs), notably `compile_failure_never_starts_and_startup_rolls_back` and `routes_are_preflighted_without_activation`, passes in exact-commit workspace verification. T10 failed candidates preserve old behavior/code lifetime; full lifecycle suite remains authoritative for activation retry/drop. |
| Storage schema admission, grants, session/dataset authority and no unauthorized effects | [scoped_storage.rs](../packages/phenix-kernel/tests/scoped_storage.rs), [storage.rs](../packages/phenix-kernel/tests/storage.rs), [shared_schema.rs](../packages/phenix-storage-graph/tests/shared_schema.rs) and derived_plugins grant tests pass in exact-commit workspace verification. T14 directly tests claim-without-resource-grant denial versus separately granted success. ContractRequirement grants nothing. |
| Atomic storage batches, revisions, subscriptions, bounded tails/overflow and wake/teardown | Kernel storage/scoped_storage, [binding.rs](../packages/phenix-storage-graph/tests/binding.rs) and storage suites pass in exact-commit workspace verification, including mismatch/no-effect and overflow/recovery negatives. No storage source, format or observation protocol change is attributed to contracts. |
| Derived exact-engine/physical endpoint membership, query freshness and cohort/count validation | [derived.rs](../packages/phenix-storage-graph/tests/derived.rs) passes in exact-commit workspace verification: `binding_uses_final_engine_not_temporary_or_discarded_deployments`, `failed_and_nested_builds_have_no_ambient_membership_state`, `shared_actions_in_two_module_instances_are_ambiguous`, `same_wave_payloads_republished_at_other_compatible_endpoints_are_rejected`, freshness/count/cohort negatives. GraphOrigin does not replace those runtime checks. |
| Derived preflight replay, deliberate redeployment, recursion/dispatch guards and backend lifetime | Same passing derived suite includes `an_authored_module_can_be_deliberately_redeployed_and_each_query_pins_its_runtime`, guard/error/panic tests and `backend_lives_until_runtime_and_all_saved_specs_are_dropped_in_either_order`; the derived example also exits 0 on the implementation commit. T2 verifies inherited declaration replay remains valid while divergent clones reject. |
| Native handler/codec trust and authorization | T7 malformed codecs still fail natively; T14 identical contracts do not promise readiness, emission or access. No claim proves behavior, sandboxing or ABI compatibility. These explicit negatives plus T10 three-plugin execution support H18/H22 without promoting opt-in into behavioral proof. |

## Migration And File Inventory

`InterfaceBounds` and plugin `graph_bounds` integration are **removed**, with no
compatibility shim. Construct `GraphContract::new(id, InterfaceShape)`, attach an
explicit declaration claim, compile, then bind the actual unit. Old matching
metadata must not fabricate membership. Plugin registry entries no longer retain
a second purported implementation shape. `Contribution::graph_unit` is a locator.

Direct `ModuleDef` literals must supply `claims`, `contract_requests` and
`declaration_ids`; direct `ModuleInput` literals supply `port` and `slot` separately
from `name`. T8's literal fixture demonstrates this source migration. Plain module
input APIs keep the historical value-slot convention, including the existing
empty-slot authoring lookup convention. Legacy `ContractValue::SCHEMA` and ordinary
same-type composition are retained; only the explicit portable connector demands
structural descriptors. There is no storage schema/version or grant migration.

| Files In The Implementation | Owner And Change |
|---|---|
| Graph `src/interface.rs` | Replaces bounds with shape/contract inspection and validation; adds structured diagnostics and graph-bound projections. |
| Graph `src/authoring/interfaces.rs` | New effective authoring inspection, semantic binding, claim/request storage edits, replacement obligations and ledger changes. |
| Graph `src/provenance.rs`, `src/authoring/provenance.rs` | New Graph-owned opaque origin and scoped composition attribution. |
| Graph `src/authoring.rs`, `src/authoring/addresses.rs`, `src/lib.rs`, `src/output.rs` | Integrate grouping/routing, declaration/package lineage, portable connect, final validation, compiled root metadata and directed selection tokens. |
| Graph new integration suites T1-T7 and T12-T14; modified T8 layout/manual fixtures | Actual executable coverage, including pending symbolic layouts, same-factory/package patch lineage, static source/sink/readiness/auth invariants and explicit low-level literal migration; not the old planned `interface_contracts.rs`. |
| Graph `tests/fixtures/portable_provider.rs`, `portable_consumer.rs`, `portable_execution.rs` | Two independently compiled rlibs with distinct Rust types and equal portable descriptors, plus a linked real-engine execution harness invoked by T7. No new production crate or runtime type. |
| Plugin `src/manager.rs`, `src/registry.rs`; deleted `tests/graph_bounds.rs`, new T9 | Replace copied bounds authority with actual-unit locator binding, opaque origin checks and exact-generation leases. |
| Kernel `src/lib.rs`; new T10/T11 | Generic graph definition/offer/requirement contributions, host-selected final validation, origin-scoped composition and retained snapshots. |
| Plugin [README](../packages/phenix-plugins/README.md) | Plugin-owned lease/locator/origin binding instructions, not an alternative Graph schema authority. |
| Kernel [README](../packages/phenix-kernel/README.md) | Kernel-owned composition/selection/lifecycle policy, explicitly no auto wiring or activation coordinator. |
| This report | Cross-owner implementation comparison, evidence and acceptance limitations. It is not an additional runtime/specification owner. |
| `phenix-schema`, storage and storage-graph sources | Contract implementation does not change these sources; descriptor interning and storage/derived authority remain their existing owners. |

## Scoring

Scores use exactly the requested dimensions and weights. They are an engineering
assessment, not a hard-gate completion percentage. C2 scores describe inspected
implementation and exact-commit command/CI verification.
Comparative C1/C3/C4 scores are **feasibility estimates only**, not verified
implementations; uncertainty about their missing code/tests informs each estimate
rather than adding an unrequested verification dimension.

| Dimension | Weight | C1 Feasible | C2 Implemented | C3 Feasible | C4 Feasible | C2 Evidence / Deduction |
|---|---:|---:|---:|---:|---:|---|
| Correctness/invariants | 30% | 6 | 9 | 6 | 2 | I/A/G final checks, exact inputs/output inclusion, T1-T14 negative/recovery evidence and exact-commit workspace/Derived/CI passes. Native behavior and correct host-selected wiring still require independent policy/testing; claims do not prove them. |
| Domain API ergonomics | 20% | 6 | 8 | 7 | 7 | Uniform macro-free node/module/root inspection/binding with named public borrowed types; T1/T3/T8/T14. Explicit portable opt-in, separate authoring/compiled projections and manual host-selected wiring remain usability costs. |
| Responsibilities/dependencies | 20% | 5 | 8 | 5 | 7 | One Graph matcher, Schema owns wire descriptors, Plugin owns leases and Kernel owns selected composition; P/K delegate, no scheduler/storage dependency/crate expansion. No atomic activation coordinator; Q04 lifecycle cost remains. |
| Evolution/replacement | 15% | 6 | 9 | 6 | 6 | Full actual signature and explicit replacement claims/requests, origins, dependency refresh and revision checks; T1-T5/T8/T11/T13 cover rollback/reload/foreign generations. Conservative exact replacement intentionally limits evolution. |
| Migration/complexity | 15% | 7 | 7 | 4 | 8 | No bounds shim/duplicate actual-shape authority, T8 literals/T9 graph_bounds migration, legacy same-type baseline preserved. Source-breaking fields, token/maps and unchanged schema interning add cost. |

Weighted calculations (weights total 100):

- C1 feasibility: `(30*6 + 20*6 + 20*5 + 15*6 + 15*7) / 100 = 5.95/10`.
- C2 implementation: `(30*9 + 20*8 + 20*8 + 15*9 + 15*7) / 100 = 8.30/10`.
- C3 feasibility: `(30*6 + 20*7 + 20*5 + 15*6 + 15*4) / 100 = 5.70/10`.
- C4 feasibility: `(30*2 + 20*7 + 20*7 + 15*6 + 15*8) / 100 = 5.50/10`.

C2 is the chosen implementation because actual structure, explicit semantics,
provider origin and code lifetime now have distinct owners without another
executor or plugin shape registry. This is not an apples-to-apples performance
benchmark of four implementations. C1 could still be cheaper for existing bounds
consumers, C3 stronger for a closed typed catalog, and C4 adequate for structural
tooling alone. None can convert missing hard requirements into acceptance through
a weighted average. C2 is not complete; C4 fails the semantic task by definition.

## Verification

The implementation is committed as
`2a95b8519ff162caa68fee8b844df6ca42e1e515`. The exact commands below were run
sequentially under `nix develop --command`, with the same HEAD checked before and
after. No Cargo package/test-target adjustments were needed.

### Current Evidence

| Command (Cargo Commands Prefixed With `nix develop --command`) | Exit / Result |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo test --workspace --locked` | 0; 665 passed, 0 failed, 1 existing ignored introductory doctest |
| `cargo test --workspace --no-default-features --locked` | 0; 665 passed, 0 failed, 1 existing ignored doctest |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 |
| `cargo test --locked -p phenix-graph` | 0; 495 passed including doctests |
| `cargo test --locked -p phenix-plugin` | 0; 16 passed |
| `cargo test --locked -p phenix-kernel` | 0; 23 passed |
| `cargo test --locked -p phenix-storage` | 0; 66 passed |
| `cargo test --locked -p phenix-storage-graph --test derived --test derived_plugins` | 0; 29 + 5 passed |
| `cargo run --locked -p phenix-storage-graph --example derived` | 0; sum=9, diff=-1, both=-9, twice=18; independent subscriptions, project isolation, persistence, denied grants verified |
| `nix flake check --no-update-lock-file --print-build-logs` | 0; full check inventory passed |

Ninety tests in the new integration suites pass. T7's
`independently_compiled_portable_crates_execute` separately builds
`portable_provider.rs` and `portable_consumer.rs` as rlibs, links
`portable_execution.rs` against both, and runs it. The fixture types differ in
Rust identity and SCHEMA constants but share an explicit portable record
descriptor; checked bytes and separate decoders drive actual graph execution,
not native layout assumptions.

[CI run 42503290164](https://github.com/matthis-k/phenix-ai/actions/runs/42503290164)
completed successfully on that exact implementation SHA. Inventory, shared build,
all three shards and aggregate evidence checks passed. The Nix inventory includes
`derived`, `no-default`, `storage`, `kernel`, `integration`, `graph`, `format`,
`clippy`, `workflow` and `kernel-smoke`. Existing graph and integration outputs
already run the new suites, so no redundant new check was introduced. This is
new contract CI, distinct from V1 run 42466577317.

The current change is tracked in
[draft PR #1251](https://github.com/matthis-k/phenix-ai/pull/1251). Local Cargo
logs for the implementation run are retained under
`/tmp/opencode/verify-2a95b851-*.log`.

### Superseded Failure

An earlier focused `contract_offers` run exited 101 because the foreign-node
replacement fixture omitted its explicit contract claim: it failed at replacement
validation before reaching the expected origin mismatch. The fixture now attaches
the compatible claim, and T11's complete node/module foreign-origin matrix passes
in the exact-commit verification above. This was a fixture correction, not a
relaxation of replacement or origin checks.

H23 PASS is supported by exact committed implementation-head commands and hosted
CI above. H21/H22 PASS is based on concrete negative/recovery and multiplugin
coverage plus those workspace runs. Q01/Q04 remain PARTIAL for API complexity and
unbounded process-lifetime schema interning respectively. Automatic Kernel wiring
and atomic activation remain explicit mission limitations, so neither passing
regression suites nor the 8.3 score is a claim of full mission completion.
