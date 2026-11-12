# Native Storage Graph

This adapter authors only native `NetworkBuilder` components and runs them through
the kernel's persistent `NetworkSession`. It has no scheduler, action adapter, or
ambient membership registry. Candidate preparation returns owned patches, exports,
and activation factories; write grants remain host-owned. See `examples/derived.rs`
for independently authorized readers and a writer with fixed composition policy.

## Execution

- `sources` creates one atomic same-mount snapshot group, without reading storage.
- `derive_from` transforms the group; derived joins support arities one through four
  and reject unrelated refresh roots and foreign authoring capabilities.
- Source, transform, join, and terminal authoring use native additions transactions.
  Errors and unwinding panics restore topology and preserve existing capabilities;
  handles escaping an aborted transaction are revoked.
- Each source exposes `<name>.refresh`, `<name>.snapshot`, and `<name>.errors`.
  Storage failures use the diagnostic output and do not poison the session. They
  become query errors, never `Result` values passed to ordinary derived consumers.
- Observation acquires every authorized subscription before its bootstrap event.
  Every notice causes a full snapshot. Overflow notices retain their native
  `through` sequence in `RefreshCause::Invalidated`, unlike the bootstrap's
  `RefreshCause::Resynchronize`. Polling rotates across subscriptions; native kernel
  admission retains bounded pending events and never executes process code.
- Demand uses `begin_request`, `inject_request`, `close_request`, bounded native
  execution, authenticated capture draining, settlement, and `release_request`.
  Sibling effects run normally. Older observation captures cannot satisfy demand.
  Both the source snapshot and selected terminal must publish exactly once.
- Binding checks unique opaque `DefinitionKey`s and the exact single refresh ingress
  destination. Prepared binding and final query admission validate the terminal,
  snapshot, and diagnostic output projections against their exact declarations.
- `DerivedRuntime` and `DerivedHost` install `ProcessExecutionGuard` on actual worker
  execution, retaining any application guard. Local, offloaded, same-runtime and
  cross-runtime nested queries fail before acquiring the session mutex.
- Queries return `QueryValue<T>`, which owns the captured native `Value`. Cloning
  retains its actual code leases. Borrowing uses `Deref`; no bare owned extraction
  or unsafe lifetime escape is provided. Explicitly cloning application data from
  a borrow is the caller's responsibility under the data type's lease contract.

The default query step budget is 65,536; `query_with_budget` overrides it. Native
queue/capture/request capacities remain controlled by host `SessionOptions`.

## Failure Semantics

`DerivedRuntime` and `DerivedHost` explicitly select
`RequestFailurePolicy::RestartAffectedProcess`. A tagged process error or panic
fails its demand request, disposes that scope's pending work, and reconstructs only
affected process instances. Unrelated process state is retained. The query checks
failed status before draining captures and preserves the native request diagnostic.
Budget exhaustion cancels only the demand scope, not the session. Subsequent demand
can succeed after either failure or cancellation.

Before releasing a request, the adapter snapshots its cancelled runner-job tickets
and disposal inventory. If any handed jobs were invalidated, the error is
`Error::RequestInterrupted { cause, cancelled_jobs, disposal }`. The runner must
still settle its owned jobs; invalidation cannot preempt code, and late completions
are rejected by the native session. The original error is retained in `cause`.

## Limitations

- Recovery restarts affected process-private state; it does not roll back storage
  commits or other already performed effects. Authoring transactions likewise do
  not roll back arbitrary callback external effects.
- Synchronous queries hold the native session lock. A dispatcher that requires
  independent session admission to deliver deferred completions cannot settle
  within that call; the query cancels its scope and returns invalidated job inventory.
- Untagged observation work has no demand scope to recover. Untagged process
  failures and a panic while spawning a replacement process can fail the session.
- Low-level `QuerySpec::bind_kernel` validates declarations but does not configure
  an arbitrary kernel's execution guard or failure policy. Use the supported
  `DerivedRuntime` or `DerivedHost` builders for these guarantees.

## Verification

```sh
cargo test -p phenix-storage-graph
cargo test -p phenix-storage-graph --no-default-features
cargo clippy -p phenix-storage-graph --all-targets -- -D warnings
cargo run -p phenix-storage-graph --example derived
```
