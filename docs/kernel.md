# Kernel

`phenix-kernel` composes Rust-only, in-process plugins into one graph. It uses
the existing `PluginCatalog`, `PluginSnapshot`, and `ContributionHandle` loader
boundary, not another plugin loader or graph scheduler. It has no storage dependency.

```text
host configuration -> PluginCatalog / PluginSet -> leased PluginSnapshots
                                                   |
                         resolve dependencies -> compose GraphBuilder
                                                   |
                              graph compiler -> Deployment / Engine
                                                   |
                         start producers -> public trigger -> new wave
                                                   |
                       cancel/drop producers -> nodes -> contribution leases
```

## ABI Safety

The existing loader accepts statically linked Rust factories. Its `Any` and Rust
trait-object contracts are deliberately in-process only, never shared-library
interfaces. There are no plugin artifacts to scan, ABI versions to validate, or
native library handles in this branch. Explicit selection is through catalog keys
and snapshots; existing cross-crate loader tests exercise that boundary. Neither
`libloading` alone nor a matching schema name would make Rust vtables ABI-safe.
No native loader, unload or hot reload is added here.

## Abstraction Inventory

The following covers every added public kernel abstraction. All are runtime
contracts with compile-time Rust signature checking, not domain policy.

| API | Owner/lifetime | Invariant | Why existing API is insufficient | Simplification | Replace/bypass |
|---|---|---|---|---|---|
| `Key` | contribution metadata | qualified dependency identity | graph IDs identify topology, not plugin contributions | explicit dependencies | use GraphBuilder directly |
| `Contribution`, `Compose` | existing leased plugin inventory | graph declarations before activation | registrar stores arbitrary kinds but does not assemble a graph | one ordinary GraphBuilder callback | another host may consume snapshots |
| `Activate` | leased contribution | I/O acquisition after compilation | graph actions do not own external producers | separate startup factory | omit for graph-only plugins |
| `Producer` | started kernel | drop releases external resources | executor owns waves, not sources | source-local polling and cancellation | host may fire triggers directly |
| `Notice` | one producer poll | one admission payload per event | Value has no external route | pairs existing values with trigger name | use Engine ingress directly |
| `ProducerEvent` | drain caller | preserve poll/admission and invocation failures | WaveReport cannot represent a failed poll | explicit per-source outcome | caller may drive producers itself |
| `Kernel` | trusted host, one generation | leases outlive graph and sources | foundations intentionally do not compose themselves | load/compose/start/execute/drop glue | use catalog, builder and Deployment directly |

## Run

From the repository root:

```sh
cargo run --manifest-path packages/phenix-kernel/Cargo.toml --example basic
cargo run --manifest-path packages/phenix-kernel/Cargo.toml --example storage
cargo test --manifest-path packages/phenix-kernel/Cargo.toml
```

The complete runnable example is `packages/phenix-kernel/examples/basic.rs`.
It registers an `Increment` plugin in the catalog, loads a snapshot, composes a
typed `GraphBuilder` node, compiles it, explicitly starts the kernel, and admits
`41_i64` through `run`.
The example checks that the wave succeeds. Graph values use the existing local
typed value API, not a kernel-specific graph DSL.

The `storage` example runs both memory and in-memory SQLite backends. It writes
and reads a counter through a graph module, then drains storage change notices
into separate observer waves. Subscriptions are acquired only by `start`.

## Integration API

- Register `phenix_kernel::Contribution` through `Registrar::add`.
- Set `id`, `requires: Vec<Key>`, `compose`, and optional `activate` factory.
- Use `Key::new(plugin_owner, contribution_id)` for dependencies. All selected
  contributions are composed, with lexical owner/id tie-breaking among ready
  dependencies. Duplicate contributions/dependencies, missing dependencies and
  cycles fail before composition.
- `compose` receives the single existing `GraphBuilder`. Use its typed handles,
  named endpoint lookups, modules and patching APIs for cross-plugin composition.
- `Kernel::build(name, snapshots, executor)` compiles once and builds one
   `Deployment` with the supplied graph `Executor`, leaving no active producers.
- `Kernel::start(&mut self)` explicitly activates producers in dependency order.
  Repeated successful startup is rejected, even for a producer-free kernel.
- `activate` is a zero-argument factory returning an owned `Box<dyn Producer>`.
   It receives no engine; trusted captured callbacks must not admit waves or perform
   setup I/O during composition. This is a lifecycle contract, not a sandbox.
- `Producer::poll` must not wait for events and returns at most one `Notice`, idle,
  or an error. A notice contains an existing public trigger name and values.
- `Kernel::drain(max_polls)` explicitly polls round-robin, preserving the cursor
  across calls. Idle polls count toward the bound. Each notice calls
  `Engine::fire`, creating a new wave. Poll/admission errors are returned in
  `ProducerEvent`; invocation failures remain ordinary `WaveReport` failures.
  Neither stops the remaining bounded polls.
- `Kernel::fire` rejects admissions until startup succeeds, then forwards them to
  `Engine::fire`. Before startup, `drain` returns no events.
  `engine()` exposes low-level engine/session APIs and bypasses this lifecycle guard.

## Lifecycle

Contribution handles pin the exact plugin revision even after loader unload.
Composition and compilation failures never activate producers. If activation
fails, a local activation vector drops already activated producers by RAII before
returning the error. The kernel stays unstarted with no producers and can retry
`start`; its engine and leases remain alive. Factories must clean up
their own partially acquired resources on error. Producers must stop and release
their resources on drop. `Kernel::drop` clears producers before nodes are dropped
with the engine, retaining contribution leases until both are destroyed, including
startup rollback; shutdown is not reverse dependency order.

## Limits

There is no background loop, async runtime, dynamic-library ABI, automatic reload,
retry, delivery durability, or transactional rollback of callback side effects.
The poll count bounds admissions, not callback duration or wave execution time.
Producer queues and overflow policies belong to producer implementations.
Producer trigger routes are not declared by contributions; notice trigger names
are therefore validated by the existing engine on admission, not before activation.
Panicking plugin code is not isolated. The initial deployment uses the graph's
 default in-memory graph resource store, not a database. The storage example passes
 scoped sessions directly to plugin-owned node closures without executor coupling.
