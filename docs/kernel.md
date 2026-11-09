# Kernel

`phenix-kernel` composes Rust-only, in-process plugins into one graph using the
existing `PluginCatalog`, `PluginSnapshot`, `ContributionHandle`, Graph compiler
and Engine. It has no storage dependency and introduces no second scheduler.
See [Contract Composition Iteration](contract-composition-iteration.md) for the
current evidence ledger, ownership/mutation audit and pending verification gates.

```text
catalog / plugin stages -> candidate snapshots -> resolve imports/dependencies
    -> compose contexts + candidate artifacts -> compile/check actual graph
    -> bind host adapters -> start producers -> publish generation
    -> declared producer notice -> existing Engine -> separate wave
```

## ABI Boundary

Factories, `Any`, Rust trait objects and callbacks are statically linked,
in-process contracts, not shared-library ABI interfaces. Schema agreement does
not make Rust vtables ABI-safe or codecs honest. There is no native loader,
dynamic-library unload or sandbox. Plugin reload stages in-process revisions;
leases retain old code/implementation ownership until their graph users die.

## Run

```sh
cargo run -p phenix-kernel --example basic
cargo run -p phenix-kernel --example storage
cargo test -p phenix-kernel
```

`examples/basic.rs` composes a typed increment node and fires an ordinary wave.
`examples/storage.rs` uses memory and in-memory SQLite, with change notices routed
into separate observer waves. Subscriptions are acquired on start, not composition.
These are runnable entry points, not a claim that these commands were rerun here.

## Composition API

- Register `Contribution { id, requires, routes, compose }` using `Registrar::add`.
  `compose` receives `&mut ComposeContext`, with the ordinary builder at `context.graph`.
- Dependencies use owner-qualified `Key::new(owner, id)`. All contributions compose
  in topological order, with lexical owner/id tie-breaking. Duplicates, missing
  dependencies, cycles and duplicate snapshot owners fail before composition.
- Register `ContractDefinition { id, contract }`, `GraphOffer { id, producer,
  unit, contract }`, and `ContractRequirement { id, consumer, contract }` as ordinary
  typed contributions. Producer/consumer keys must belong to the registering plugin.
- `Kernel::build_with_policy` applies trusted `ImportPolicy` selections and optional
  allow/deny pairs. Denial wins. A unique eligible compatible offer is selected
  automatically; missing, ambiguous or invalid selections fail closed.
- Selected providers become dependencies of their consumers. A consumer calls
  `context.import(local_requirement_id)?`, then `ResolvedImport::bind(graph)` to
  project actual claimed endpoints. Connect them explicitly with Graph APIs.
  Consumers need not capture provider paths; automatic edge insertion is not provided.
- `context.activate(factory)` registers one candidate-local producer factory.
  There is no `Contribution.activate` field. The zero-argument factory captures
  the exact composed state and returns an owned `Box<dyn Producer>` after compile.
- `context.export(value)` registers one typed candidate-owned export. Bundle
  multiple values in a struct; retrieve using `Kernel::export::<T>(&key)`.
  Missing keys/wrong types return `None`. Shared snapshot slots are not candidate exports.
- Build validates all offers, including unused offers, all registered contract
  definitions against compiled claims/requests, exact contribution origins, and
  declared routes before constructing one Deployment with the supplied Executor.
  `Kernel::build` uses default policy and converts errors to strings;
  `build_with_policy` returns `ResolutionError`.

Graph owns actual shape and semantic matching. Kernel does not copy purported
implementation shapes into offers. Per-composer origin children distinguish
contributions within the exact loaded plugin revision; PluginSnapshot binding
checks revision ancestry, while Kernel offers check exact producer identity.
Native composers retain low-level graph access: selected imports are not a sandbox
or proof that every arbitrary closure wires the selected provider.

## Producer Lifecycle

`Kernel::build` does not activate producers. `Kernel::start` activates registered
factories in dependency order; a second successful start is rejected, even with
no producers. Failure drops already-created producers and leaves the same composed
artifacts available for retry. Factories must clean up partial acquisitions;
producers must stop/release resources on drop. Producers die before graph/actions,
artifacts and contribution/snapshot code leases; shutdown is not reverse dependency order.

`Producer::poll` must not wait for events. It returns at most one `Notice`, idle,
or an error. Routes are preflighted before activation and notices to undeclared
routes reject. `drain(max_polls)` polls round-robin across calls; idle polls count
toward the bound. Each notice fires a separate Engine wave. `ProducerError`
distinguishes poll, undeclared-route and admission errors; invocation failures
remain inside ordinary WaveReports. Errors do not stop remaining bounded polls.

`fire` rejects before successful start; `drain` produces no events before start.
`engine()` deliberately bypasses that startup guard. The optional blocking
`run_until(&WakeHandle, on_event)` requires wake registration from every producer;
it waits when idle and checks sticky shutdown between waves. Registration must
synchronize with publication. There is no background thread or new async runtime.
Poll count bounds admissions, not callback duration. Shutdown does not interrupt
running polls, waves or callbacks; queue/overflow policy belongs to producers.

## Host Publication

`host::KernelHost` owns the mutable PluginSet and active generation pointer.
`plugins()` permits staging through shared access, not committing the host's set.
`prepare`/`prepare_with` validates the entire staged batch and exact post-commit
snapshot inventory, builds a Kernel, optionally binds host adapters, then starts
producers without polling them. `Candidate` is opaque and bound to host identity
and expected epoch; it exposes no engine or generation admission handle.

The trusted binding hook receives `&Kernel` before producer startup. It may bind
generation-owned exports and query/store adapters, but must not fire waves or
leak admission handles. Kernel remains independent of Storage. For derived queries,
use `QuerySpec::bind_engine(kernel.engine())`; prepare with
`DerivedRuntime::guard_executor(executor)` to preserve host-only nested-query checks.
That wrapper is explicitly opt-in for KernelHost.

`publish` rejects stale/foreign candidates, locks the old generation against
in-flight scoped work, commits the validated plugin batch, marks old producers
retired, and replaces the active pointer. A retained `Arc<Generation>` pins its
old engine, exports and leases; explicit old waves/queries remain valid, but
retired `drain` calls return empty. Host drop also retires active producers.
Producer destructors complete before old leases are released.

`Generation::with_kernel` gives scoped read-only access to engine and exports;
`with_engine` is the engine-only convenience. Access is mutex-serialized with
publication. Do not reenter the same generation from a callback. A query adapter
may bind/query within `with_kernel` after publication; low-level `bind_engine`
bypasses Kernel's startup guard and must not execute in the preparation hook.

## Limits

Build/bind/start failure leaves the active inventory/generation unchanged, and
dropping abandoned candidates releases owned resources. Preparation can acquire
external resources before publication. This is **not guaranteed transactional
rollback of arbitrary external callback effects**, nor panic/process-failure
atomicity. Native callbacks, partial-acquisition cleanup and destructors are trusted.

There is no automatic retry, durable delivery, distributed transaction, native
ABI guarantee or live permission revocation. Claims establish declared semantic
membership, not behavioral correctness, scheduling or authorization. Resolution
and lifecycle errors have typed stage envelopes but still contain string errors
on several paths. Schema interning still retains distinct identities for process
lifetime. The graph deployment defaults to its existing in-memory resource store;
storage authorization and backend lifetimes remain external adapter responsibilities.
