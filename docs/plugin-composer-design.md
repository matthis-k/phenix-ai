# Plugin composer-owned declarations — Phase 1, revision 5

**Base:** `5469c55b27ff0e6e9072f9ca923f29a854f3bafd` (#1256). **Scope:** Graph-specific plugin authoring, not the plugin lifecycle, Graph contract checker, or scheduler.

## Phase 0: no-new-abstraction baseline

Existing `Registrar::add(Contribution)`, `GraphOffer`, `ContractRequirement`, and `KernelHost` already support correct composition, but providers and consumers must separately register a composer and correlated metadata with hand-written owner-qualified keys. The correlation is authoring duplication and permits mistakes. No runtime abstraction is missing; retain `Registrar`, typed plugin values, snapshot/code leases, graph interfaces, and generation preparation/publication.

Analogous consumers inspected: `kernel/examples/basic.rs`, `tests/declarative_imports.rs`, `tests/host_portable_contracts.rs`, `tests/contract_offers.rs`, `storage-graph/tests/derived_plugins.rs`, and `plugins/tests/cross_crate_api.rs`. Errors use `Failure`/`CallbackError`, identity is owner+kind+id, and Graph is the sole compatibility and provenance authority.

## Consumer examples (target API)

Trivial:

```rust
Contribution::new("main", |cx| {
    let node = cx.graph.node("increment", |n: i64| n + 1)?;
    let input = cx.graph.expose_input("request", node.inlet())?;
    cx.graph.trigger("run", [input])?;
    Ok(())
}).register(registrar)?;
```

Representative provider and consumer (independently authored contract):

```rust
into.add(ContractDefinition { id: "model".into(), contract: model_contract() })?;
Contribution::new("serve", |cx| {
    cx.graph.instance("service", &service_module)?;
    Ok(())
})
.offer("model", "service", Key::new("spec", "model"))
.register(into)?;

Contribution::new("client", |cx| {
    let selected = cx.import("model")?;
    let bound = selected.bind(cx.graph)?;
    let _input = bound.wire_input::<Request>("run", "value")?;
    Ok(())
})
.import("model", Key::new("spec", "model"))
.register(into)?;
```

Falsifying multi-composer case: `serve` can own two separately named offers and `client` can import one selected by the host. Both offers derive producer identity `Key(plugin, "serve")`, the import derives consumer `Key(plugin, "client")`; a foreign or sibling composer cannot claim either unit. See `tests/composer_declarations.rs` and `tests/declarative_imports.rs`.

## Concepts, representation, responsibility, timing

| Concept | Code-level existence | Owner | Timing |
| --- | --- | --- | --- |
| Plugin identity/revision/lease | existing catalog, snapshot and handle | phenix-plugin | load / reload |
| Graph composer | `Contribution` authoring value, private registered payload | Kernel | registration, then build |
| Offer/import | private typed registrar records under composer identity | Kernel, host selects | register, resolve before compose |
| Contract meaning and actual unit | existing `GraphContract`, compiled unit, origin | Graph | author / compile |
| Producer and generation | existing `Producer`, `KernelHost`, `Generation` | Kernel / host | prepare, publish, runtime |

`Contribution::new/after/route/offer/import/register` is a single authoring path, not a second runtime dispatcher. `after(impl IntoIterator<Item=Key>)` and `route(impl IntoIterator<Item=String>)` accept dynamic metadata vectors as found in real source consumers; one-element arrays support simple calls. Avoid a new public builder, registry façade or Graph dependency in `phenix-plugin`.

`register` first creates a temporary same-owner Registrar, adds the private composer and its offers/imports, and atomically merges it into the destination using `Registrar::merge(Contributions)`. Preflight duplicate/foreign names before insertion, even if the author catches a registration error. Producer/consumer owner links are derived at registration. Import eligibility and dependency order resolve before compose; final unit claims and exact origin are checked by Graph/Kernel before producer start. Candidate publication/retirement is unchanged.

## Alternatives, subtraction and decision

| Alternative | Strength | Weakness |
| --- | --- | --- |
| Add nothing | zero migration, proven lifecycle | duplicated linkage and empty boilerplate |
| Constructor-only helpers | smaller diff | retains independently authored, forgeable correlation keys |
| Composer-owned registration (chosen) | local ownership, failure-atomic admission, simpler common code | source-breaking migration across Graph callers |

Delete public `GraphOffer`/`ContractRequirement` authoring, direct `Registrar::add(kernel::Contribution)`, manual producer/consumer keys, ordinary `Box::new` closures and empty metadata fields. Keep private semantic offer/import records for existing resolution. Retain ordinary `Registrar::add` for independent non-Graph contributions and standalone definitions. There is no compatibility path.

## Changes discovered during implementation

- Revision 4: dynamically constructed dependency and trigger vectors are real primary consumers; make `.after` and `.route` accept iterable collections, not just singleton values.
- Revision 5: use `Item=String` for `.route` to retain inference for existing `vec!["tick".into()]` call sites; `Contribution::new` accepts bare names without `.into()` to avoid generic inference ambiguity.
- The old `GraphOffer` typed `snapshot.bind::<GraphOffer>` is removed with the old public metadata type. Generic typed plugin contributions with `graph_unit` remain bindable; dedicated tests cover that independent capability. No new public resolver or proxy is introduced.

**Limits:** Native code and codecs are trusted; there is no ABI, sandbox, external-effect rollback, or preemption claim. A source API migration is accepted by protocol default only if all in-tree consumers are migrated and final verification passes. No human signoff is implied.
