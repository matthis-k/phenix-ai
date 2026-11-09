# Kernel composition and graph contracts

The kernel composes plugin-supplied Graph units, resolves their advertised semantic contracts, and publishes complete generations. **Graph remains the sole compatibility and execution authority.** The generic `phenix-plugin` registry is Graph-independent.

## Graph composer authoring

A composition registers itself and its imports/offers **as one atomic bundle**, rather than registering stand-alone `GraphOffer` and `ContractRequirement` values with hand-authored producer/consumer links:

```rust,ignore
Contribution::new("model", |cx| {
    let node = cx.graph.node("service", |v: i64| v + 1)?;
    cx.graph.implements("service", &model_contract())?;
    Ok(())
})
.offer("model", "service", Key::new("spec", "model"))
.register(into)?;

Contribution::new("client", |cx| {
    let selected = cx.import("model")?;
    let bound = selected.bind(cx.graph)?;
    let input = bound.local_input::<i64>("run", "value")?;
    let _output = bound.local_output::<i64>("value")?;
    cx.graph.expose_input("request", input)?;
    Ok(())
})
.import("model", Key::new("spec", "model"))
.register(into)?;
```

For independent specifications, register `ContractDefinition { id, contract }` directly through `Registrar::add`. Non-Graph typed contributions also continue using the generic Registrar. Use `.after([Key::new("owner", "contribution")])` for explicit composition order and `.route(vec!["tick".to_owned()])` to declare producer routes. Arbitrary dependency/route vectors can be supplied directly. The `ComposeContext` still owns `graph`, `import`, `export`, and `activate` functionality; no Graph API or node scheduler was replaced.

Registration preflights every record in a temporary same-owner Registrar; any duplicate or foreign-owner error leaves the destination registrar unchanged. Composer identity is derived from the registered owner and the enclosing `Contribution::new` ID. Every offer/import has the normal `Key(owner, local_id)` identity for trusted host selection.

## Resolution and deployment

During `Kernel::build`, the kernel discovers compatible offers and resolves declared imports before composing. A unique eligible offer can be selected automatically; a missing or ambiguous offer fails closed. Trusted host `ImportPolicy` selections and allow/deny rules override automatic choice. The composer uses `cx.import("local_id")?.bind(cx.graph)?`; switching selection changes the actual imported endpoints and graph topology, not only metadata. Explicit dependencies plus selected-import dependencies produce deterministic composition order and cycle diagnostics.

Graph claims must be explicit. Final compiled units are checked against their advertised contract, portable/local type identities, declared path, and exact composing revision origin; merely matching structural shape cannot prove contract membership. Module public boundaries and nodes are both valid offered units. An offer on another composer's unit must fail; an unrelated plugin cannot forge its producer identity. Origin proves composition ancestry, **not native handler honesty or a permission grant**.

`KernelHost::prepare` builds the candidate graph, validates a restricted `Binding`, and starts producers without polling. `KernelHost::publish` commits staged plugin batches and switches the active generation coherently. Old revision snapshots and explicit engine handles remain pinned and valid after replacement, while retired producers cannot admit new events. Arbitrary external plugin effects cannot be rolled back by this process. Native dynamic-library ABI, sandboxing, and dishonest codec/handler detection are not provided.

## Verification

The targeted migration and its adversarial critique are recorded in `docs/plugin-composer-design.md` and `docs/plugin-composer-critique.md`. See `tests/composer_declarations.rs` for multi-offer host selection and failure-atomic registration, `tests/declarative_imports.rs` for policy, self-import and origin, `tests/host_portable_contracts.rs` for separately compiled consumers, and `tests/generation_retirement.rs` for revision and producer lifecycle.

This README describes the new branch API. It does not claim final-head verification; CI and Phase 4 evidence are required before acceptance.
