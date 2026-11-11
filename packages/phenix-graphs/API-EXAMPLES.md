# Phenix FBP

Components expose named typed streams. Primitive and composite definitions use
the same `ComponentRef`; compilation flattens modules, and a persistent session
owns bounded FIFO connections and one mutable process per physical instance.

## Closure Components

The closure conveniences use the same cooperative process and publication path
as custom processes. A sink has no dummy output. Sources start explicitly.

```rust
use phenix_graph::{ComponentDef, NetworkBuilder};
use std::{num::NonZeroUsize, sync::{Arc, Mutex}};
let capacity = NonZeroUsize::new(1).unwrap();
let observed = Arc::new(Mutex::new(Vec::new()));
let sink_values = observed.clone();
let mut network = NetworkBuilder::new();
let source = network.add("source", Arc::new(ComponentDef::source(|| 1_i64..=3)))?;
let map = network.add("double", Arc::new(ComponentDef::map(|n: i64| n * 2)))?;
let sink = network.add("sink", Arc::new(ComponentDef::sink(move |n: &i64| {
    sink_values.lock().unwrap().push(*n);
})))?;
network.connect(source.output::<i64>("output")?, map.input::<i64>("input")?, capacity)?;
network.connect(map.output::<i64>("output")?, sink.input::<i64>("input")?, capacity)?;
network.start(&source)?;
let mut session = network.build()?.compile()?.start();
let report = session.run_until_complete(128);
assert_eq!(report.state, phenix_graph::runtime::SessionState::Completed);
assert_eq!(*observed.lock().unwrap(), [2, 4, 6]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Interchangeable Bodies

Replacement checks the whole public interface and semantic obligations. Adjacent
symbolic connections do not change, but old authoring handles are revoked.

```rust
use phenix_graph::{ComponentDef, NetworkBuilder};
use std::{num::NonZeroUsize, sync::Arc};
let primitive = Arc::new(ComponentDef::map(|n: i64| n + 1));
let mut body = NetworkBuilder::new();
let child = body.add("child", primitive.clone())?;
body.expose_input("input", child.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
body.expose_output("output", child.output::<i64>("output")?)?;
let composite = Arc::new(ComponentDef::composite(body.build()?));
let mut network = NetworkBuilder::new();
let a = network.add("a", primitive.clone())?;
let b = network.module("b", match composite.body() {
    phenix_graph::Body::Composite(body) => (**body).clone(),
    _ => unreachable!(),
})?;
network.connect(a.output::<i64>("output")?, b.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
let a = network.replace(&a, composite)?;
let _b = network.replace(&b, primitive)?;
network.expose_input("request", a.input::<i64>("input")?, NonZeroUsize::new(1).unwrap())?;
let compiled = network.build()?.compile()?;
assert_eq!(compiled.processes().len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Streams And Ownership

`inject` returns the original value on `Full`, type mismatch or closure. A retained
emission waits for all selected destinations without rerunning its action.
`run_until_idle(budget)` yields at quiescence, delayed work or backpressure; it
does not imply completion. Close public inputs and drain captures to complete.
Feedback is legal, but full queue cycles can deadlock and are diagnostic.

Custom `Process::step` implementations return one `Step::Emit` at a time.
`Step::Complete` settles an activation; `Step::Finished` terminates the process.
`WaitInput`, `WaitInputs` and `WaitWake` preserve its continuation. Typed `Ports`
keys and input-record reconstruction are optional; manual interfaces use the
same representation and do not require derive support.

Local values use `TypeId`; encoded values use the complete versioned schema and
codec identity. Structural compatibility never establishes a semantic `Contract`
claim. Values carrying plugin/resource leases reject bare owned extraction;
`borrow`, `leased_shared` and `decoded` preserve the ownership boundary.

## Executable Examples

- `cargo run -p phenix-graph --example basic_fbp --locked`: source, map, sink.
- `cargo run -p phenix-graph --example streaming_fbp --locked`: capacity-one
  split, stateful join, finite feedback, repeated injection and shutdown.
- `cargo run -p phenix-graph --example plugin_composition --locked`: owned edits,
  Node/Module replacement, dependency-ordered splices, conflicts and revocation.
- `cargo run -p phenix-graph --example owned_dispatch --locked`: bounded worker
  handoff, delayed completion and publication gating.

Kernel owns producer lifecycle and selection policy. StorageGraph observes
authorized subscriptions and uses native request scopes; neither owns another
scheduler. Native behavior and migration coverage are recorded in
`docs/fbp-regression-ledger.md` at the workspace root.
