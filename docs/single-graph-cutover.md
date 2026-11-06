# Single graph cutover (in progress)

This branch is the destructive migration workspace for one graph engine.

- One canonical patch is an additive/transformational contribution to an empty graph.
- Patch contributions resolve into one immutable compiled graph.
- Triggers, typed input slots, nodes and a small store are the execution primitives.
- Optional `patch!` syntax lowers into the same macro-free builder.
- The existing selected-operation dispatcher, host effect checks, physical pending tickets and generation pinning remain the native backend, **not a second graph scheduler**.
- Graph ABI metadata must stay portable and inspectable before loading plugin code.

## Explicit outstanding gates

This branch must **not** merge until the old graph V1/V2 code is deleted, native plugin descriptors migrate to one format, all Basic/Advanced graph call sites are converted, native cancellation physically drains pending tickets and full integration CI passes.

Development is allowed to be red; do not weaken tests to force green.
