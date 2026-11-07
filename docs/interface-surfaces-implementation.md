# First implementation slice: portable graph interfaces

The new `phenix_contract::interface::InterfaceSurface` models the **same named public module ports** used by the new Rust graph builder. It is **not** a new graph executor, scheduler, plugin registry, or authority system.

`InterfaceSurface::input::<T>(port)` and `output::<T>(port)` use the existing `Portable::SCHEMA` (and the existing sorted `StaticSchema` representation). The comparison checks semantic interface identity and exact named/directional schema shapes. A provider can use a Rust type in crate B, and a consumer a separately declared type in crate C, provided both derive/implement matching portable shapes. Tests verify real canonical `Value` round trips, not just matching IDs.

The original `phenix-graph` Iteration 12 API uses `GraphBuilder::node`, `expose_input`, `expose_output`, `GraphBuilder::instance`, and `PatchPlanner`. The next integration slice will attach this *surface* to its existing module public ports and reject unresolved imports at final graph compilation. The graph prototype itself is not part of the production workspace on this branch; do not create a parallel graph runtime.

## Explicitly incomplete

- No independent native `.so` action/codec bridge yet.
- No required/exported module registration or selection in the production graph compiler yet.
- No directional structural subtyping, just exact named structural schemas.
- Only static inspectable surfaces are checked offline; dynamically generated patch topology needs deterministic staging and graph compilation.
- `InterfaceSurface` is not a capability grant; kernel selection and authority remain necessary.

## Nix acceptance target

A Nix composition check runs the **real** surface checker on exact provider/consumer metadata before loading code. The CLI entrypoint is `phenix-interface-check`. Generated descriptors must be pinned to verified plugin artifacts. Full structural checking still uses the actual graph `PatchPlanner` when selected static contributions are available.

No new `*-types` crate is required.
