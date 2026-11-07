# Phenix graph — prototype runtime

This crate is the **only active implementation** on the isolated cutover
branch. It implements typed nodes, slots, port policies, modules, graph
compilation, patch planning, waves, coloring and an execution session.

- `src/authoring.rs`: graph construction, assembly, patch contribution.
- `src/authoring/patching.rs`: patch priority/conflict/splice ordering.
- `src/lib.rs`: typed graph semantics and one runtime.
- `src/deployment.rs`: external execution and pure edge adapter placement.
- `port-args-derive/`: optional derive/macro sugar.

`GraphAuthor` has been deleted. `GraphBuilder` is the graph-declaration API;
`PatchBuilder` is the contributor-facing edit API, both sharing the same compiler.
Typed handles and symbolic addresses share the exact same `.connect`,
`.disconnect`, `.expose_input`, `.expose_output`, and `.trigger` verbs.
`ModuleDefBuilder` remains the low-level representation used to construct modules. `EdgeTransport` was
removed; `EdgeAdapter` is the sole local edge-value preparation interface.

Run from workspace root: `cargo test -p phenix-graph --all-targets`.
Native host authorization, binary graph descriptors and the model/tool product
loop are deliberately not implemented here. No old runtime fallback exists.

Only `Deployment::new(compiled_graph)` can configure an `Engine` publicly;
caller-chosen `store`, `report_retention`, `executor`, `node_executor` and
`edge_adapter` share that one builder. `Engine::new` and its old chained
configuration methods are internal implementation details. `Engine::fire` and
`Engine::begin` are synchronous and externally driven interfaces to the *same*
wave scheduler, not separate engines.
