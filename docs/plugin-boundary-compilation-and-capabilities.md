# Phenix — Rust Plugin Capabilities and Compilation Boundary

**Status:** concrete design for the plugin/graph/kernel integration; not a claim that native graph registration is implemented.  
**Basis:** `phenix-plugin` PRs #1242–#1245 and the standalone Graph Iteration 12 bridge.  
**Goal:** preserve one simple Rust authoring API while separating Rust compilation, graph validation, ABI dispatch, lifecycle, and authority.

## Decision

A plugin is an independently selected **source of executable Rust implementations and typed contributions**. A kernel instance selects plugins and collects contributions. A graph-specific adapter translates graph contributions into the **existing** `PatchPlanner` and compiles a generation. The generic plugin manager does not depend on the graph system.

**A plugin contributes definitions and behavior; it never directly mutates the active graph or kernel.**

## 1. Three different compile/validation stages

| Stage | Input | Invariant | Cannot prove |
|---|---|---|---|
| Plugin Rust compilation | Rust source + SDK | `Fn(A) -> O` matches declared Rust arguments and outputs; authoring methods type-check; private state compiles | Types of imports supplied by another independently built plugin; graph-wide topology; privileges |
| Host graph compilation | Selected, authenticated contributions + portable interfaces + graph patches | Matching port/slot identities; valid module replacement; authorized patch targets; absence of unresolved conflicts; valid graph topology | Rust function bodies are correct; plugin won't panic or act maliciously |
| Native call validation | Bytes / opaque handles over negotiated ABI | Correct ABI/version; payload decodes against declared schema; responses match the agreed output; plugin code remains loaded; kernel resource authority | Host process sandboxing or arbitrary native-code safety |

**The Rust compiler does not type-check an independently loaded `.so` against the host graph.** It only checks each separately compiled artifact against the declarations available while building that artifact. Binding different artifacts is graph-compile-time work (at application startup/recomposition), not Rust compile time.

### Two linking models

| Model | Node code crosses as | Types cross as | Viable now |
|---|---|---|---|
| Statically linked plugins in one Rust program | Real `Fn`, `NodeAction` or `dyn Trait` under the same Rust ABI | `TypeId` and compiled Rust generic/trait checks | Yes: `PluginCatalog` + `GraphPatch` bridge |
| Independently compiled native `.so` plugins | Opaque plugin instance/action handle + C-compatible ABI vtable | Canonical portable schema/interface descriptors + serialized values | M1 operations yes; graph-node integration **not yet** |

Do not use Rust `Any`, `TypeId`, `Box<dyn Trait>`, raw `Arc`, or `PatchBuilder` across the native ABI. Local graph values (`Value::local`) must not be sent across it. Rust-only is a **language policy**, not an ABI guarantee. A future Lua interpreter can be an implementation detail of a Rust plugin.

## 2. Actual supported extension surface

### Plugin manager — universal, small API

- Register/load a typed Rust factory; instantiate with `PluginKey<Config>`.
- Contribute **typed objects** into a host-scoped registrar.
- Query a selected plugin revision and its registered contributions.
- Prepare load/reload/unload; commit only after dependent kernel composition/graph compilation succeeded.
- Retain exact plugin revisions through leases, including after logical unload.
- Report identity, dependencies, metadata, lifecycle errors (not domain behavior).

The plugin manager **does not** compile a graph, schedule work, grant capabilities, store all application state, or identify tool/model/frontends by special type.

### What a selected plugin can contribute

| Capability | Minimal implementation | Owner of validation/use | Current status |
|---|---|---|---|
| **Node action** | Action factory (typed Rust closure locally; native ABI action handle across `.so`) | Graph execution; host ABI adapter | In-process working; native graph bridge missing |
| **Graph module/subgraph** | Definition with public typed ports/slots and internal nodes | Graph compiler | Graph prototype working |
| **Graph patch** | Symbolic structural edit: add/replace/connect/splice/route | One graph `PatchPlanner` + composition authority | In-process bridge working; native lowering missing |
| **Trigger ingress** | Host-registered trigger source with declared event schema and ingress policy | Kernel admission + graph | Existing frontend concepts; generic adapter pending |
| **Resource/service provider** | Instance-owned resource factory, permission-scoped façade and explicit lifetime | Resource subsystem + kernel authority | Existing domain storage/credentials; generic registration pending |
| **Domain adapter** | Model, tool, frontend, credential, storage, journal, observer, Lua VM, etc. implemented as nodes/resources/graph composition | Respective plugin implementation; graph & kernel enforce boundaries | Many existing domain plugins use M1 operations |
| **Metadata/configuration** | Identity, dependency/interface requests, configuration and fingerprints | Plugin package verifier + kernel selection | M1 descriptors exist; generic instance metadata pending |

There should be **no universal plugin operation registry** for each domain. Tool/model/auth/etc. remain domain-level constructs implemented using ordinary executable nodes and resources.

### Runtime activity a plugin can perform

- Execute its **own** node behavior when the graph invokes it.
- Read/write its **own private Rust state**.
- Access host-exposed resources and services, subject to explicit selection and effective kernel authority.
- Produce typed values, route over existing edges if the node's output policy permits, and return errors or pending results.
- Emit events through declared ingress/trigger mechanisms, not by silently creating graph edges or waves.
- Request intentional, revalidated persistent state migration through resources, not by mutating another generation's private state.

It must **not** change graph structure mid-wave, bypass graph scheduling to execute other nodes, claim authority from contribution IDs, or unload code still referenced by live actions.

**Security caveat:** native `.so` code in the host process can still use OS APIs and act outside these framework rules. API-level restrictions alone are *not* isolation. Only load trusted native plugins, or isolate untrusted ones in another process.

## 3. User-facing API styles

### In-process Rust plugin: no ABI adaptation needed

```rust
impl PluginInstance for ArithmeticPlugin {
    fn contribute(&self, reg: &mut Registrar) -> Result<(), Failure> {
        reg.add(GraphPatch::new("arithmetic", || {
            let mut p = PatchBuilder::new("arithmetic");
            p.node("add", |a: AddArgs| a.left + a.right);
            p
        }))
    }
}

let mut catalog = PluginCatalog::new();
let key = catalog.register("arithmetic", |cfg: Config| Ok(ArithmeticPlugin::new(cfg)))?;
let mut plugins = catalog.instance();
let snapshot = plugins.load(&key, config)?;
let generation = GraphGeneration::compile("agent", [snapshot])?;
```

**Prototype-level illustration:** use the exact validated graph builder signatures when integrating. `PluginCatalog`, `PluginSet`, and `GraphGeneration` are separate crates/components. `GraphPatch::new` is graph-adapter code, not part of generic `phenix-plugin`.

### Independently compiled native Rust plugin: authoring façade, ABI lowering

Plugin author writes Rust, using the same semantic concepts:

```rust
// Illustrative API, NOT current M1 native functionality.
fn define(graph: &mut PluginGraph<'_>) -> Result<(), BuildError> {
    let add = graph.node("add", |a: AddArgs| a.left + a.right)?;
    graph.export("add", add)?;
    Ok(())
}
```

`PluginGraph` **is plugin-local SDK code**, not a `&mut phenix_graph::GraphBuilder` pointer passed over FFI. It records structural declarations and maps authored Rust handlers to opaque action IDs. An ABI adapter publishes:

```
NativePluginDescriptor {
    plugin_id, artifact_digest, abi_revision,
    contributions: [
        GraphContribution {
            patch_id,
            writes, after, priority,
            edits: [Node { name: "add", inputs: [schema(AddArgs)],
                            outputs: [schema(i64)], action: ActionId(7) }, ...]
        }
    ]
}

NativeActionHandle = (loaded_image_lease, instance_id, action_id)
```

The host validates canonical descriptors and lowers edits into the **existing** `PatchPlanner`. It constructs normal `NodeAction` adapters around ABI call handles. The graph never needs a special plugin node type. Only the adapter knows that the action originated in a `.so`.

**Important:** arbitrary `FnOnce(&mut GraphBuilder)` patch closures cannot be serialized. Native structural contributions must be lowerable to an ABI-safe declaration/edit vocabulary, or mediated by an ABI-safe command stream. This is an actual additional implementation requirement; it is not solved by the current in-process `GraphPatch` closure.

## 4. Type identities and guarantees

1. **Local typed edges:** `TypeId` checks are meaningful only in the shared Rust linking domain. Keep type erasure inside the graph after validation.
2. **Native typed edges:** export an unambiguous **canonical schema fingerprint**, interface version and encoding revision. Do not accept only an arbitrary claimed type name/string as proof of compatibility.
3. **Cross-plugin wiring:** typed local endpoints can catch mismatches when the author has both endpoint types at Rust compilation time. Otherwise the graph compiler validates exported/imported port signatures at composition time.
4. **Nodes and modules:** replacement must preserve public port interfaces; internal implementation is free to change.
5. **Code ownership:** a compiled generation and its action handles pin exact plugin instance + library image, not just the plugin's logical ID.
6. **Authority:** schema and identity matching do not grant access; kernel selection and resource checks are separate.
7. **Runtime defenses:** validate inbound/outbound native payloads even after schema admission; fail the offending node/wave without corrupting another generation.

There is no realistic way to get Rust *cross-binary* compile-time type checking between arbitrary independently compiled plugins without rebuilding a common typed Rust host or replacing the native ABI with something equivalently restrictive. The accurate guarantee is **compile-time per artifact + composition-time cross artifact + runtime ABI validation**.

## 5. Startup, reload and generation transaction

```
Host startup
  -> verify package metadata and binary digest (before loading native code)
  -> select plugin factories for InstanceSpec
  -> construct per-instance plugin implementations
  -> collect typed/portable contributions with authenticated provenance
  -> authorize graph patch targets + resource requests
  -> lower native contributions to host graph patch objects
  -> ONE PatchPlanner -> compile/validate graph
  -> allocate generation with leased code/resource bindings
  -> activate generation for new triggers

Reload:
  -> prepare replacement plugin revision (not visible)
  -> collect proposed contributions
  -> validate + compile replacement graph
  -> atomically publish new generation/selection
  -> allow old executions to drain on old leased generation
  -> unload library only when final lease is released
```

The existing prototype only stages **one plugin revision** and commits it to the `PluginSet` before switching the active generation. For a production multi-plugin transaction, the kernel must stage a **batch** and publish its complete membership/generation atomically. Neither one-plugin commit nor the graph compiler should become a second execution scheduler.

## 6. Minimal crate/dependency boundary

```
phenix-contract     portable schemas, native ABI, capability vocabulary
      ^
phenix-plugin       generic factories, instances, contribution registry,
      ^             native loader and leases (graph-independent)
      |
phenix-graph        graph builder/compiler/executor (plugin-independent)
      ^
phenix-graph-bridge  consumes both; maps contributions into graph PatchPlanner
      ^
phenix-kernel       instance selection, trusted authority, generation transaction
```

This diagram is conceptual: `phenix-plugin` and `phenix-graph` are **siblings**, neither depends on the other. The bridge depends on both; kernel depends on the bridge and underlying subsystems. The SDK may re-export graph authoring convenience, but does not become the source of graph validation logic.

## 7. Required tests before claiming completion

**Rust static path**
- Typed `PluginKey<C>` rejects wrong configuration at compile time (compile-fail test).
- Typed `graph.connect::<T>` rejects mismatched endpoint types when both types are statically known (compile-fail test).
- Different plugin crates contribute local Rust node actions; their graph compiles and executes.

**Native boundary path**
- Two independently built Rust plugins advertise incompatible schema revisions; composition rejects their edge.
- Bogus schema fingerprint, mismatched serializer, malformed bytes, missing action ID and ABI revision reject deterministically.
- Native plugin graph patch is lowered into the existing planner; conflict and declared-write checks still apply.
- Plugin A/B both compiled independently; replacement that changes public schema fails before activation.
- Plugin-private library code persists through unload while an old generation references an action.
- Forged action or resource authority is denied; process isolation requirement for hostile native plugins documented.

**Kernel generation path**
- Staged reload fails graph compilation: active plugin and graph unchanged.
- Two kernel instances from one plugin catalog remain isolated, including their stage tokens.
- Successful multiple-plugin update changes new triggers only; old waves finish against their original generation.
- Failed multi-plugin update cannot partially mutate plugin membership, graph, resource grants, or active generation.

## 8. Deliberate exclusions for this milestone

- No Lua/JavaScript/Python plugin protocol; a Rust plugin may host those internally.
- No global service locator or mandatory universal plugin supertrait.
- No new patch semantics, graph scheduler, or graph plugin node kind.
- No automatic private-state migration or remote/external execution.
- No passing Rust trait objects, closures, `Any`, `TypeId`, or `Arc<T>` across native ABI.

**Recommended next implementation:** create a tiny `NativeGraphDescriptor` and `AbiAction` adapter against the existing M1 binary loader and the Iteration 12 planner. Include one native plugin with typed `i64 -> i64`, one incompatible `String` consumer, and conformance tests for compile-time-local vs composition-time-cross-plugin validation. Keep the adapter isolated from both core crates.