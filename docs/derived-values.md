# Derived Values

Derived values are graph declarations over authorized Storage snapshots, not another reactive runtime. Storage owns persisted values, codecs and access; Graph owns execution; plugin contributions own declarations; Kernel owns producer lifecycle and waves. This guide describes the current V1 API. [Design rationale](derived-values-design.md) and [review/verification ledger](derived-values-review.md) are separate.

## Complete Author Example

This Rust example defines its domain codec, resource requests, contribution factory, plugin declarations and host demand function. The factory accepts an already host-provisioned `Arc<Storage>`: it neither grants access nor opens a backend. Its empty `main` intentionally does not provision a host. The Markdown example is not a doctest; the canonical executable and verification boundary are linked below.

```rust
use std::sync::{Arc, OnceLock};
use phenix_kernel::{Contribution, Key};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use phenix_storage::{Codec, Operation, Schema};
use phenix_storage::scoped::{CellSpec, ResourceRequest, Storage, Stored};
use phenix_storage_graph::derived::{DerivedRuntime, GraphDeriveExt, QuerySpec};
use phenix_storage_plugin::ResourceOffer;

#[derive(Clone)]
pub struct Amount(pub i64);
#[derive(Default)]
pub struct AmountCodec;
impl Stored for Amount { type Format = AmountCodec; }
impl Codec for AmountCodec {
    type Value = Amount;
    fn schema(&self) -> Schema {
        Schema {
            name: "demo.amount".into(), revision: 1,
            structure: "i64".into(), codec: "big-endian".into(),
        }
    }
    fn encode(&self, v: &Amount) -> phenix_storage::Result<Vec<u8>> {
        Ok(v.0.to_be_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<Amount> {
        let bytes = bytes.try_into().map_err(|_| {
            phenix_storage::Error::Codec("expected eight bytes".into())
        })?;
        Ok(Amount(i64::from_be_bytes(bytes)))
    }
}
pub const A: CellSpec<Amount> = CellSpec::project_state("demo.values/a");
pub const B: CellSpec<Amount> = CellSpec::project_state("demo.values/b");

#[derive(Clone)]
pub struct Queries {
    pub query: QuerySpec<i64>,
}
fn text(e: impl std::fmt::Display) -> String { e.to_string() }

pub fn make_contribution(storage: Arc<Storage>) -> Contribution {
    Contribution {
        id: "amounts".into(), requires: vec![],
        routes: vec!["amounts.values.refresh".into()],
        compose: Box::new(move |context| {
            let graph = &mut *context.graph;
            let (a, b) = storage.bind((A.read(), B.read())).map_err(text)?;
            let group = graph.sources("amounts.values", (&a, &b)).map_err(text)?;
            let sum = graph.derive_from("amounts.sum", &group,
                |a: Amount, b: Amount| a.0 + b.0).map_err(text)?;
            let diff = graph.derive_from("amounts.diff", &group,
                |a: Amount, b: Amount| a.0 - b.0).map_err(text)?;
            let both = graph.derive("amounts.both", (&sum, &diff),
                |sum, diff| sum * diff).map_err(text)?;
            let twice = graph.derive("amounts.twice", (&sum,),
                |sum| sum * 2).map_err(text)?;
            let sink = graph.node("amounts.consumer", |v: i64| println!("{v}"))
                .map_err(text)?;
            graph.connect(twice.output(), sink.inlet())?;
            let query = graph.query_spec("amounts.query", &both).map_err(text)?;
            let observation = group.observation(16);
            context.export(Queries { query })?;
            context.activate(move || observation.activate().map_err(text))
        }),
    }
}

pub struct AmountPlugin {
    pub storage: Arc<OnceLock<Arc<Storage>>>,
}
impl PluginInstance for AmountPlugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        for (id, spec) in [("a", A), ("b", B)] {
            registrar.add(ResourceOffer::new(id, ResourceRequest::new(
                spec.resource_spec(), [Operation::Read, Operation::Subscribe],
            )))?;
        }
        let storage = self.storage.clone();
        // Declarations precede host provisioning; resolve the slot during compose.
        registrar.add(Contribution {
            id: "amounts".into(), requires: vec![],
            routes: vec!["amounts.values.refresh".into()],
            compose: Box::new(move |context| {
                let contribution = make_contribution(
                    storage.get().ok_or("storage not provisioned")?.clone(),
                );
                (contribution.compose)(context)
            }),
        })
    }
}

// Host calls this after DerivedRuntime::build and successful start().
pub fn demand(runtime: &DerivedRuntime) -> Result<i64, String> {
    let queries = runtime.kernel().export::<Queries>(&Key::new("amounts", "amounts"))
        .ok_or("missing candidate Queries export")?;
    runtime.bind_query(&queries.query)
        .map_err(text)?.query().map_err(text)
}
fn main() {}
```

`A=3, B=5` gives sum `8`, diff `-2`, both `-16`, twice `16`. The consumer runs for observations AND demand: querying `both` refreshes its whole root, including `twice` and the consumer. Register this plugin under catalog owner `amounts` for the export key shown. Activation and query exports belong to each composed candidate, not shared snapshot slots; the storage provisioning slot is separate host-owned setup state.

## Host Planning

Run the complete provisioning, independent writer/two-reader plugin, SQLite persistence and project-isolation example:

```bash
cargo run -p phenix-storage-graph --example derived
```

The runnable [examples/derived.rs](../packages/phenix-storage-graph/examples/derived.rs) is the canonical full host setup; see `offers`, `reader::Plugin::contribute`, `host_policy`, `build_with_policy`, `Demo::query_all` and `run`.

1. Load plugins once and retain their exact snapshots. `ResourceOffer::new` requests logical resources and operations; it is not a grant.
2. Resolve `CellSpec::resource_spec().resolve(&identity)`. The trusted host fixes `StoragePolicy` with `BackendPlacement`, `ResourceBinding` and per-plugin `plugin_access`. Put A/B datasets on one physical mount for an atomic group. The example grants the writer only `Write`, readers only `Read`/`Subscribe`.
3. Call `ResourceRequirements::collect(snapshots.clone(), &identity)`, then `PreparedStorage::prepare(requirements, identity, policy)?.activate()?`. Inject `active.views` into plugin-owned slots before graph composition. Host decisions must not be inferred from requests.
4. Call `DerivedRuntime::build(name, snapshots, Arc::new(LocalExecutor))`, then `start()`. Kernel compiles and preflights routes before activating observations. `drain(max_polls)` or `run_until(&wake, on_event)` drives existing producers. Start does not itself execute the bootstrap wave.
5. Retrieve the candidate's exported `QuerySpec` via `runtime.kernel().export`, then use `runtime.bind_query(&spec)?.query()?`. Binding does no payload IO and fires no waves; querying before start through DerivedRuntime fails. Each call fires one ordinary demand wave.

For host publication, use `DerivedHost`. Its `prepare` always guards the supplied
executor, including worker threads; there is no opt-in wrapper to forget. The
preparation callback receives a nonadmitting `Binding`: retrieve typed exports
and call `QuerySpec::validate_binding(binding)` before producers start. After
publication, use `DerivedGeneration::query(&spec)` against the exact guarded
deployment. Retained generations remain queryable after replacement. Explicit
low-level `bind_engine` bypasses Kernel's start guard and cannot ensure host-only
dispatch for arbitrary engines. Callback/export external effects are not rolled back.
See [Kernel](kernel.md#host-publication) and the source-reviewed
[host adapter tests](../packages/phenix-storage-graph/tests/host_derived.rs).

The compact example and runnable example deliberately use `Amount`, not `CellSpec<i64>` with a fictitious primitive codec. Computed local `i64` outputs need no storage codec.

## Source Groups

`graph.derive("sum", (&a, &b), closure)` creates an implicit source group and route `sum.sources.refresh`. `graph.sources("values", (&a, &b))` creates an explicit tuple-valued group with route `values.refresh`; `derive_from` unpacks that tuple for each sibling computation. Derived-only tuples use normal same-cohort joins and inherit the common root. Chains and diamonds remain ordinary graph wiring, and `output()` connects to arbitrary typed consumers.

Two separate calls to derive directly from stored handles create genuinely independent refresh roots, even if the leaves are identical. Combining their outputs fails with `MultipleRefreshRoots`; the implementation does not merge roots or try to join unrelated waves. To combine sum and diff, declare the explicit group first as above. Mixing stored and derived handles in one tuple is unsupported; include all leaves in one explicit group and rebuild dependent declarations from it.

Values and tuple arities 1-4 use `Clone + Send + Sync + 'static`; stored formats in graph inputs also require `Send + Sync`. Closures accessing fields can require explicit parameter annotations. `ReadCell::clone` shares a codec and pinned authority without requiring `T: Clone`, but transporting its decoded value through this graph API does require `T: Clone`. Large values may use suitable shared domain representations; do not assume zero-copy computation or query return.

## Ownership And Authority

| Type | Ownership and invariant |
|---|---|
| `CellSpec<T>`, `CellRead<T>` | Logical declaration and read request, not a grant, backend or scheduler. Singleton key is `value`. |
| `ReadCell<T>`, `OptionalReadCell<T>` | Resolved `ReadSource` and shared domain codec. Optional changes absence policy only. |
| `ReadSource`, `SnapshotReader` | Original session authority and registry/address/key/schema identity; every source is authorized separately, never by unioning grants. |
| `SnapshotEntry` | Ordered optional revisioned bytes plus source identity; decoding with the wrong cell fails. Decoding occurs outside backend locks. |
| `DerivedRef<T>`, internal root | Typed authoring output and shared refresh lineage/source action, no computed cache or runtime borrow. |
| `Observation` | Deferred subscription request; activated producer owns subscriptions, bounded tails and bootstrap. |
| `QuerySpec<T>` | Explicit terminal tap action plus value/root metadata; no runtime reference. |
| `DerivedRuntime`, `QueryHandle<'a, T>` | Runtime owns Kernel and guards dispatch; query handle borrows the exact engine, optionally retaining the Kernel start guard, and records physical source/tap IDs. `QuerySpec::bind_engine` supports scoped host adapters without a storage dependency in Kernel. |

Authority is fixed per binding/session. Binding validates read permission/schema without fetching values; observation separately requires `Subscribe`. Reads recheck the immutable authority and contract, but this is not live grant revocation. Host policy changes require new views/bindings. Resource/schema agreement does not make different Rust local endpoint types interchangeable. Native in-process plugins are not sandboxed by these grants.

## Queries And Limits

`query_spec` adds a terminal tap because ordinary compute output payloads can disappear under terminal-only retention when they fan out. `bind_query` requires exactly one source action and one tap action in the final engine via `Engine::node_ids_for_action`. Shared stateless `with_dyn_action` allocations permit deliberate compatible deployment reuse. Fresh incompatible recomposition rejects an old spec; multiple physical instances of the same actions are ambiguous and rejected with `WrongGeneration`.

Binding also inspects `Engine::trigger_targets`: the named refresh route must
accept exactly one payload and target only the bound source's port 0, slot 0.
Missing or redirected routes fail with `InvalidRefreshRoute` before execution.
When instantiating an authored module, explicitly forward its refresh input to a
parent trigger with the original route name. Reusing actions identifies deployed
endpoints, not a fingerprint of every intervening computation or user edit.

Extraction checks successful wave status, exact deployed source/tap node and slot IDs, one retained terminal value, one root receipt, event counts and matching current wave/cohort. It rejects replayed payloads, wrong cohorts and compatible-looking publications from another endpoint. It never falls back to an older value.

Queries are host-only. `DerivedRuntime` wraps the supplied executor with a panic-safe guard on dispatch and execution threads; nested queries to the same or another runtime return `NestedQuery` before firing. This wrapper is not a scheduler: demand calls `Kernel::fire`, observations use normal producers. Use graph edges within node logic, not recursive queries. The guard applies to dispatch through this wrapper, not arbitrary unrelated engines or detached application threads.

- Graph sources only use `AtomicWithinMount`: one registry/mount, a backend with atomic batch support. Storage's low-level `ReadConsistency::BestEffort` exists but is not exposed as a derived-group mode. Atomic snapshot reads do not turn separate writes into one transaction.
- Required absence fails as `source[MissingInput]`. `a.optional()` supplies `Option<T>`; codec/read/denial errors remain failures, not absence. No absence-history token exists for safe memo reuse.
- Host construction/activation preserves typed Storage errors. Node failures cross the current string diagnostic boundary as `source[Storage]` or `source[MissingInput]`; queries expose `Error::Wave`, not a typed Storage error recovered from a report.
- Subscribe-before-bootstrap covers queued registry-local invalidations with overflow rescans, but supplies no durable cursor, cross-process feed, coherent startup watermark, latest-at-return or exactly-once-per-notice/transaction guarantee. A concurrent commit after a snapshot can precede query return. Resync explicitly if stronger application recovery is needed.
- Immutable derived handles are built after their inputs, making cycles unconstructible through these declarations. There is no independent cycle detector; general Graph loops are unchanged.
- No memoization, materialization, equality suppression, cached `latest()` or dynamic dependency tracking is implemented. Repeated queries compute again and can repeat sibling/downstream side effects.
