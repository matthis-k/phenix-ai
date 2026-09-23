---
status: planned
source: kernel-runtime-mechanism-audit-2026-09-23
base_sha: b7334e59159fcd4eeedb802d953a801138fbf43c
---

# Runtime trace sink and bounded provenance

## Goal

Separate kernel diagnostics from semantic plugin events.

Runtime tracing currently serializes `RuntimeTraceEvent` into an `EventEnvelope` and admits it through `EventBus`. Event admission creates an OS thread for every accepted delivery. Service provenance is also appended to an unbounded `Vec<ServiceInvocationProvenance>`.

The kernel needs diagnostics, but diagnostics do not need plugin event scheduling, event causality, listener dependency ordering, or an unbounded history.

This PR gives runtime diagnostics one direct typed sink and keeps `EventBus` for semantic events.

## Current path

```text
runtime policy/service/data stage
  -> RuntimeTraceEvent
  -> serde_json::to_vec
  -> EventEnvelope(kernel.runtime.trace)
  -> EventBus::admit_in_generation
  -> one delivery thread
  -> optional listener threads

service invocation
  -> ServiceInvocationProvenance
  -> Mutex<Vec<_>>
  -> clone whole Vec from Kernel::service_invocation_provenance
```

`phenix-plugin-debug` currently subscribes to `kernel.runtime.trace` to persist diagnostic logs, and `phenix-plugin-step-runner` emits policy stages through the same EventBus route. Both are first-party diagnostic consumers/producers rather than semantic event traffic. This PR migrates Step Runner to direct sink recording and installs the Debug logger as a `RuntimeTraceSink` when the debug plugin is part of the harness.

## Target ownership

Add one runtime-owned diagnostic interface:

```rust
pub trait RuntimeTraceSink: Send + Sync {
    fn record(&self, event: RuntimeTraceEvent);
}

pub struct RuntimeTraceBuffer {
    capacity: NonZeroUsize,
    events: Mutex<VecDeque<RuntimeTraceEvent>>,
}
```

The default kernel installs a bounded in-memory sink. Tests may inject a recording sink. Product logging may later install a structured-log adapter without changing dispatch.

Keep `RuntimeTraceEvent` as the typed diagnostic record. Move it out of the semantic event implementation if that makes the ownership clear.

## Provenance

Keep `ServiceInvocationProvenance` as the detailed service-call record because provider selection and authority evidence are useful for inspection and regression tests.

Store it in a bounded runtime-owned buffer:

```rust
pub struct ProvenanceBuffer {
    capacity: NonZeroUsize,
    entries: Mutex<VecDeque<ServiceInvocationProvenance>>,
}
```

Use a named default capacity constant. Set the initial default to 256 entries. The constructor must also accept an explicit `NonZeroUsize` for tests and products that need another bound.

Insertion evicts the oldest record before pushing a new record when the buffer is full.

`Kernel::service_invocation_provenance()` keeps its current snapshot semantics, but returns only the retained bounded history.

## Runtime call flow

Replace the helpers that build an `EventEnvelope` with direct typed recording:

```text
emit policy stage
  -> trace_sink.record(RuntimeTraceEvent::PolicyStage)

finish service invocation
  -> provenance.record(completed)
  -> trace_sink.record(RuntimeTraceEvent::ServiceInvocation)

trace data mutation
  -> trace_sink.record(RuntimeTraceEvent::DataMutation)
```

Do not serialize and immediately deserialize runtime trace metadata inside core.

## EventBus boundary

Keep all behavior from #495 for semantic plugin events:

- bounded admission
- asynchronous delivery
- dependency ordering
- causal re-entry protection
- generation capture
- typed delivery status
- subscription replacement semantics

This PR does not redesign `EventBus` scheduling.

Delete the `kernel.runtime.trace` EventBus route after all runtime trace writers use `RuntimeTraceSink`. Do not keep a compatibility publisher beside the sink.

If an external debug adapter needs trace records, it consumes the trace sink or a later logger adapter. It does not subscribe through plugin event semantics.

## Files and mechanical changes

Primary files:

- `rust/crates/phenix-core/src/runtime/dispatch.rs`
- `rust/crates/phenix-core/src/runtime/host.rs`
- `rust/crates/phenix-core/src/runtime.rs`
- `rust/crates/phenix-core/src/runtime/kernel.rs`
- `rust/crates/phenix-core/src/events.rs`
- `rust/crates/phenix-plugin-step-runner/src/runner.rs`
- `rust/crates/phenix-plugin-debug/src/component.rs`
- `rust/crates/phenix-plugin-debug/src/implementation.rs`
- `rust/crates/phenix-plugin-catalog/src/lib.rs`
- `rust/crates/phenix-harness/src/lib.rs`

Suggested new module:

- `rust/crates/phenix-core/src/runtime/trace.rs`

Mechanical sequence:

1. Add `RuntimeTraceSink`, `RuntimeTraceBuffer`, and `ProvenanceBuffer`.
2. Put both handles in `Kernel` and the internal invocation view.
3. Replace `emit_policy_stage`, service trace emission, and data mutation trace emission with typed sink writes.
4. Replace `Mutex<Vec<ServiceInvocationProvenance>>` with `ProvenanceBuffer`.
5. Keep the public provenance snapshot method and existing provenance fields.
6. Remove runtime trace serialization and `EventBus::admit_in_generation` calls from runtime tracing.
7. Route Step Runner policy stages through `KernelAccess::record_runtime_trace` instead of `dispatch_event`.
8. Replace the Debug plugin's runtime-trace `ComponentListener` with a direct `RuntimeTraceSink`; the harness installs that sink whenever `phenix.debug` is selected.
9. Remove `RUNTIME_TRACE_EVENT`, `RUNTIME_TRACE_EVENT_VERSION`, and `runtime_trace_event_type`.
10. Remove imports and tests that only existed for the EventBus trace route.

## Invariants to preserve

- Trace records contain metadata only. Request bodies, model content, tool payloads, persistence values, credentials, and secrets stay out.
- Trace recording cannot fail a service call or durable mutation.
- Provider provenance remains complete for retained calls.
- Trace writes do not create plugin tasks, event causality, or listener delivery work.
- Semantic EventBus behavior is unchanged.
- Bounded buffers have deterministic oldest-first eviction.
- A poisoned or failed diagnostic sink cannot become a new runtime failure mode. Custom sinks must have an infallible `record` contract.

## Tests

Add focused tests for:

- trace recording does not call EventBus admission
- one service invocation produces one service trace record
- policy and data mutation stages record typed events
- capacity 2 retains the newest two records after three writes
- provenance capacity 2 retains the newest two invocations
- `Kernel::service_invocation_provenance()` returns retained order
- semantic EventBus delivery still uses #495 behavior
- Step Runner policy diagnostics reach the direct sink without EventBus serialization
- the Debug component no longer declares a runtime-trace listener while Debug logging remains installed through the sink path
- runtime trace records do not include request or response bytes

Update existing service-layer and provider-fallback regressions to read provenance from the bounded buffer through the existing kernel accessor.

## Deletion target

At completion the runtime must no longer contain both of these diagnostic paths:

```text
RuntimeTraceEvent -> EventBus
ServiceInvocationProvenance -> unbounded Vec
```

The canonical paths are:

```text
RuntimeTraceEvent -> RuntimeTraceSink
ServiceInvocationProvenance -> ProvenanceBuffer
```

## Non-goals

- no EventBus executor rewrite
- no OpenTelemetry exporter in this PR
- no logging format redesign
- no service dispatch redesign
- no provider selection change
- no change to plugin-visible semantic events

## Acceptance criteria

- [ ] Runtime diagnostics never enter `EventBus`.
- [ ] Runtime trace emission performs no JSON serialization in the core hot path.
- [ ] Provenance storage has an explicit non-zero bound and deterministic eviction.
- [ ] Existing provider selection and authority provenance fields remain available.
- [ ] EventBus semantic event tests keep their current behavior.
- [ ] No compatibility publisher, listener, event constant, or Step Runner emitter keeps `kernel.runtime.trace` alive beside the new sink.
- [ ] Source, Rust, Clippy, Product, Integration, Docs, and Maintenance checks pass at exact head.

## Follow-up relation

This is the first slice from the kernel mechanism audit. Later runtime-generation and call-scope PRs should carry the trace and provenance handles as runtime services rather than rebuilding EventBus coupling.
