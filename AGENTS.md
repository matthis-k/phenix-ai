# Experimental Phenix graph scope (migration quarantine)

This branch intentionally builds **only** the standalone graph prototype.
The project-wide kernel, artifacts, plugins, frontends, and old graph have
been removed from this branch; restore domain code from the parent commit
only after the prototype passes its own tests and native binding is designed.

Rules:
- One `GraphBuilder` / `GraphAssembly` / `PatchPlanner` authoring pipeline.
- One `GraphBuilder::node` for typed actions; plain values broadcast and
  `Output<T>` optionally selects existing connected targets.
- No branch, optional, or fallible node categories. `Option<T>` and
  `Result<T,E>` remain domain payloads unless the action explicitly handles them.
- One `compile` returning a structured `CompileDiagnostic`; do not restore `compile_detailed`.
- Output routing belongs to the returned payload's `Output<T>` selection.
  Patches may replace the node action; do not restore a port-level router.
- One compiled graph and `Engine` / `ExecutionSession` runtime.
- `GraphAssembly::finish()` is the sole assembly finalization API; no
  provenance-dropping convenience compiler.
- `ExpectWave` assertions must identify the producing output. Never infer
  correctness from the last publication in a multi-output graph.
- Topology manifests are structural and diagnostic, never trusted generation
  or executable-behavior identities.
- A patch is a pre-compilation contribution, never another scheduler.
- No second `Patch` contract or proxy `GraphSession`.
- Host authority and native ticket/drain integration are **not yet implemented**.
- Never restore an old graph call site to make CI green.
- Do not claim full Phenix product acceptance from graph-only tests.

- `Output<T>` is an optional produced value plus target selection. No
  Option/Result payload interpreters in routing. `Output::none()` means no
  value was produced; `Output::to_all([], value)` means a value was produced
  but selected zero edges. These must never be conflated.
- A domain `Err` is data unless a node action explicitly returns an invocation
  error. Do not silently convert one into the other.

- `PortArgs` types are themselves `InputLayout`/`DecodedInputLayout` types;
  do not reintroduce a wrapper-only `PortArgsLayout` or `Port::for_args`.
- Low-level `NodeAction` currently allows multiple emissions in pre-existing
  conformance fixtures. The full single-emission cutover is an outstanding
  contract migration, not an excuse for another adapter or hidden fallback.
- Typed `OutputSchema::output_slot()` always returns exactly one `OutputSlot`.
  `IntoEmission::into_emission()` always returns exactly one `Emission` on
  typed wire-handler paths. A plain unit return publishes `()`; an explicit
  `Output::none()` is a successful invocation with no publication, but the
  declared output slot still exists. It is not `Option<T>` domain data.

- Typed `NodeHandle` must retain the exact selected `LocalInputLayout` type after `.input(layout)`. Both inferred and explicitly selected atomic layouts expose `inlet()`. Field layouts expose typed fields, not a fabricated atomic inlet.
- `NodeDefinition::signature` declares input/output identities with `NodeSignature`, not positional booleans; the local/wire distinction must not be guessed from string names.
