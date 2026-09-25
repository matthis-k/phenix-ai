# Lifecycle hooks

status: legacy compatibility
coverage:
  - rust/crates/phenix-plugin-hooks/src/implementation.rs
  - spec/kernel-hooks.md

## Current state

`phenix-plugin-hooks` implements the old configurable dispatcher. It remains selectable for compatibility but is not installed by the default suite.

New hook behavior must use the kernel-owned mechanisms in `kernel-hooks.md`:

```text
operation interception -> Service Layer
completed fact         -> Event + Listener
```

Do not add new lifecycle wiring to `phenix.hooks@1`.

## Migration

| Legacy concept | Canonical mechanism |
| --- | --- |
| `CallableStart` with veto/transform | Layer on the callable service |
| `CallableCompleted` observation | Event + Listener |
| execution start policy | Layer on the execution service |
| execution completion observation | Event + Listener |
| context load policy | Layer on the context service |
| metadata/logging | Event Listener or around-call Layer |
| dependency ordering | Layer policy or Listener dependency DAG |
| failure policy | Layer result/error or Event failure policy |

Handler configuration belongs to the handler Plugin. Runtime ordering, authority, call scope, continuations, delivery, tracing, and provenance belong to the kernel.

## Removal condition

Delete `phenix-plugin-hooks` after all first-party and supported external users have migrated to Layers or Events.
