# Responsibility boundaries — only one implementation

| Mechanism | Owner now | Superseded source in parent revision | Boundary |
|---|---|---|---|
| Nodes, typed ports, input readiness and buffers | `phenix-graph` | PR #1241 `phenix-graph/src/session.rs` | Other layers may provide actions, not schedule them |
| Trigger admission, cohorts, wave coloring and completion | `Engine` / `ExecutionSession` | PR #1241 `GraphSession`, `NativeGraphRun`, controllers | Frontends submit events; they never pump a second graph |
| DAG reachability, cycles and output publication | graph compiler/runtime | V1/V2 graph executors | No execution policy in `Kernel::resolve` |
| Modules, public boundaries and named endpoints | `GraphBuilder` / `ModuleCatalog` | V2 `Node`/`InputRef`/`OutputRef` descriptors | No parallel portable `Patch` topology |
| Patch conflict resolution, dependencies and splicing | `PatchPlanner` then `GraphAssembly` | `phenix-contract::graph::Patch`, old resolver compilation | Patches change the *uncompiled* graph, not runtime scheduling |
| Node replacement and topology mutation | graph authoring | all legacy metadata composition | Final compilation once per selected assembly |
| Function execution binding/placement | `Deployment`/`Executor` adapter | `NativeGraphRun` | Dispatch adapter must not decide graph readiness |
| Store mechanics | prototype `Store` | kernel `value_state` | Store keeps values; graph owns delivery/admission of changes |
| Session state, persistence | future plugin-owned implementation | Basic session/journal providers | Not a graph engine; no implicit scheduling |
| Host selection, grants, authorization | future kernel/native adapter | old `Root::call_graph`, `resolve` graph bindings | Never inferred from graph paths or patch contributor labels |
| Native ticket, physical cancellation/drain | future adapter into graph `ExecutionSession` | old `NativeGraphRun` and `pending` scheduler coupling | Reuse authority/ticket ownership; no parallel scheduler |
| Model/tool loop, retries, continuation | future graph nodes | Basic `advance`, `begin_model_advance` | Plugin actions do domain work only; edges represent control flow |
| Frontend prompt/cancel/stream handling | future frontend nodes/adapters | `phenix-basic-runtime::begin_submission` etc. | No frontend-specific execution loop |
| ELF graph metadata | future one portable lowering of prototype contribution | old `.phenix.graph` `Patch` decoder/writer | No second IR, no local TypeId across ABI |
| Generation identity | future kernel hashing final compiled topology+selection | old `generation::tuple(graph_identity)` | Graph supplies deterministic identity; kernel signs composition |
| Startup provider dependency ordering | kernel **when reconnected** | old `dag.rs` | Distinct from graph dataflow scheduling; do not confuse |

## Duplicates explicitly removed in the isolated branch

- Retired graph engines, schema-based `Patch`, scheduler and native runner.
- Former Basic/Advanced `graph_patch` functions, graph embedding build scripts,
  graph-specific test executables and stale conformance binaries (restorable from Git history).
- Nix/CI/product/frontend entry points tied to the superseded graph.
- The prototype's `EdgeTransport` compatibility trait, wrapper and deployment
  route are replaced with the existing `EdgeAdapter` API, with tests kept.

## API consolidation

The `GraphAuthor` facade is removed. `GraphBuilder` is the only typed graph
**declaration** entry point; `PatchBuilder` is the only first-class **patch
contribution** entry point. `PatchPlanner` owns dependency/conflict selection;
`GraphAssembly` owns transactional staging, not a second compiler.
`ModuleDefBuilder` remains the underlying module representation for specialized
ports/actions and conformance coverage. All paths compile through the same
`ModuleCatalog` and execute through one `Engine`. Only `Deployment::new` constructs
and configures the engine from public code; direct `Engine::new` and its `with_*`
configurators are internal. Original direct execution tests were migrated to
that configuration path, with explicit retention of their old payload policy.

`PatchBuilder` has exactly `connect` and `disconnect`, with no `_path` aliases;
the optional `patch!` macro emits those same calls. The old independent
`GraphAuthor` test scenarios were migrated to `GraphBuilder` with explicit
wire-value semantics, not skipped or deleted.

## Out of scope and deliberately unclaimed

Portable authoring ABI, native invocation/authority adapter, serializing graph
action/policy identities, scheduling independent calls during production integration,
persistent stores and distributed failure handling. Graph-only tests do not
establish these invariants.

## Iteration 9: output routing ownership

A graph node function returns `T` (implicit broadcast) or `Output<T>` (one
payload plus selected destinations). The compiled executor validates those
selections against actual outgoing connections and atomically prepares the
batch. `OutputSlot` describes a port's type and name, not an independently
configured routing policy. Patches replace node actions, not port-level
selectors. Graph compiler, patch planner and executor do not maintain
parallel selector representations. An empty `to_prefix` selects no edges.
