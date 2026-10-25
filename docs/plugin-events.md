# Plugin events in Phenix

Kernel events are minimal, and **plugins own application semantics**. A
plugin can declare a typed event, emit it through the current graph root,
and subscribe to events with existing M1 graph triggers. No second event bus,
scheduler, persistent event store, or direct cross-plugin implementation edge
is required.

## Choose the correct mechanism

| Intent | Mechanism | Example |
|---|---|---|
| Notify unknown zero-or-more consumers of a semantic occurrence | Plugin-owned typed event | `phenix.providers.ready` |
| Run several independently contributed implementations of an extension phase | Plugin-owned event + typed registration contracts | `phenix.providers.populate` |
| Ask an accountable callee to perform work and return a result | Contract | `ProviderRegistry::register` |
| React to the latest authoritative plugin-owned value | Watched property trigger | `state:phenix.basic.agent/pending_model_request` |
| Order one step after another, including asynchronous dependencies | Graph `after` edge or event settlement | `settled:phenix.providers.populate` |
| Stream high-frequency data | Transport/observer stream | Model tokens |

Do **not** promote every function call or storage write to an event. Subscribers
must never gain new authority merely from knowing an event name.

## Event namespaces and ownership

Kernel reserves `phenix.kernel.*`: `begin`, `plugin-loaded`,
`plugin-unloaded`, `plugin-reloaded`, and `shutdown`. These are
generation-scoped trusted entry sources, not user actions. The host has to
admit these at the corresponding lifecycle transition; the plugin cannot emit
them using `Host::emit_event`. Actual hot reload/unload requires real loader
transactions before those events may be emitted.

All other semantic events are declared by a plugin. For example,
`phenix-plugin-providers` declares `phenix.providers.populate` and
`phenix.providers.ready`; `phenix-plugin-provider-openai` is only a
subscriber. The kernel authenticates each emission against the declaring
plugin's live call scope and rejects forged or schema-invalid events.

Events are **transient** and **generation-local**, not durable state or a
replayable message queue. A late subscriber queries the owning plugin's
current state using a contract. Incompatible event schema changes need an
explicitly versioned event identity rather than silent mutation.

## Graph execution and completion

An event emitted from a plugin is queued on the active graph root. It does
not start a nested root or execute another plugin recursively. Delivery is
FIFO between event waves; within each wave the existing graph dependency DAG
determines handler order. Pending tickets must settle before a wave completes.

A declaring owner can subscribe to `settled:<event-id>` using a standard
M1 graph node. That is a **synthetic owner-only completion trigger**, not a
separate event or public action. Its payload is an empty struct because
the owner has its own authoritative Rust state. The kernel admits the hook
only after that event's subscribers finish with no domain failure,
infrastructure failure or cancellation. When no subscribers exist, the
owner's settlement hook must still run.

Example, schematically (annotations are proposed optional SDK sugar):

```rust
// phenix-plugin-providers
#[phenix::on("phenix.kernel.begin")]
fn begin(state: &mut State, host: &Host<'_>) -> Result<()> {
    state.registry.begin()?;
    host.emit_event(&ProvidersPopulate { scope: "user".into() })?;
    Ok(())
}

#[phenix::on_settled(ProvidersPopulate)]
fn publish(state: &mut State) -> Result<()> {
    state.selected = Some(state.registry.publish()?.providers);
    Ok(())
}

// phenix-plugin-provider-openai
#[phenix::on(ProvidersPopulate)]
fn register(providers: Providers<'_>) -> Result<()> {
    providers.register(OPENAI)
}
```

The *implementation* uses `StaticGraphNode` and generated
`#[provide(...)]` handlers; new author-side annotations must lower to these
existing graph and contract mechanisms, not introduce a parallel registry.

## Responsibility and migration

- **Kernel/SDK:** a few intrinsic lifecycle sources, generic typed
  `Host::emit_event`, static event ownership, validation, root admission,
  DAG scheduling, bounded event queue, settlement, cancellation, generation
  identity, introspection. The kernel knows nothing about provider catalogs.
- **Providers manager:** provider definitions' shared protocol/auth/discovery
  strategies, private registry staging/validation, `onBegin` population,
  `onSettled` publication, and `providers.ready`.
- **OpenAI declarations:** plain `Provider` data and one graph-triggered
  registration handler. No auth loop, catalog or native state duplication.
- **Basic Agent:** already graph-driven by `phenix.basic.prompt` and typed
  staged-state triggers; retain directed model/tool/session contracts.
- **Advanced preparation:** already driven by watched preparation properties;
  keep it reactive rather than creating redundant event notifications.
- **Sessions/Storage:** retain transactional, revision-checked contracts;
  introduce semantic `session.opened` or `configuration.reloaded` only
  when independent consumers need those transitions.
- **Frontends:** emit/admit input through existing graph actions; observe output
  through existing contracts/observers. Avoid coupling the kernel to Neovim
  or another UI.

## Acceptance gates

1. Owner-only event emission; reserved kernel sources cannot be spoofed.
2. Payload validation before enqueue, bounded emission and cyclic-wave budget.
3. Multiple subscriber plugins register independently and deterministically.
4. No publication before all subscribers (including pending tickets) finish.
5. Failure/cancellation never admits owner settlement or exposes partial state.
6. A zero-subscriber event still settles.
7. Event triggers, contract authority, and graph `after` dependencies remain
   separate; reload does not duplicate handlers.
8. Native product proves `kernel.begin → providers.populate →
   provider registration → settled → providers.ready`.
9. Production host emits intrinsic begin/shutdown at genuine lifecycle points;
   ABI and authorization conformance must remain green.

**Migration boundary:** legacy pull-based provider registrations and old
provider-specific auth/catalog hosts remain temporary compatibility surfaces.
Remove them only after their production consumers are moved to the shared
manager and acceptance tests pass. Do not keep two authoritative registries.
