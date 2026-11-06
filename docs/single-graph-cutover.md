# Single graph cutover (draft, deliberately red)

## Architectural invariants

- **One graph engine:** `phenix-graph` compiles canonical `Patch` contributions over an empty graph, and runs one `GraphSession` scheduler.
- **One native execution bridge:** `phenix-kernel::graph::NativeGraphRun` resolves node provenance to selected plugin operations and drives the *same* `GraphSession`. It does not schedule a competing graph.
- **One agent host:** `phenix-basic-runtime` handles Terminal, JSONL and ACP through the same `NativeGraphRun`. Nix launchers are configuration wrappers around that executable, not independent agent apps.
- **One Basic plugin artifact:** no `staged-graph` feature or second compiled Basic-agent variant. The single graph contribution enters at `phenix.basic.prompt`.
- **One descriptor vocabulary:** native ELF graph contributions carry portable `Patch` metadata. Definition-only packages carry definitions, never an alternate graph section.
- **Host authorization remains host authorization:** untrusted patch node labels cannot grant effects; selected operation scope and physical ticket leasing remain in the kernel.

The old Graph V1/V2 compiler/executors, `phenix-m1-graph` V1/V2 conformance executable and duplicate graph fixture app builds are retired. The single graph engine has a direct Nix conformance check. `tools/check-single-graph.py` prevents accidental reintroduction.

Independent native provider plugins, the generic kernel CLI, and standalone **product acceptance binaries** serve distinct purposes and are not alternative agent/graph runtimes. Do not delete them simply because they are executables.

## Current implementation

- Portable patch IR, symbolic endpoints, deterministic patch ordering/identity and contributor provenance
- One elementary compiler and queue-based scheduler with readiness `All`/`Any`
- Incremental activations, out-of-order diamond execution, graph cancellation and failure isolation
- Selected-native-operation binding and ticket-aware dispatch/poll/release for pending activations
- Basic and Advanced plugin Patch metadata generated from one source declaration
- Basic Terminal/JSONL/ACP controller now using `NativeGraphRun`
- Per-pump native admission quota for frontend fairness; no total graph execution budget
- Duplicate Basic feature/derivation removed; direct legacy submit fails closed

## Outstanding blockers (do not merge)

1. Complete and verify native pending cancellation, physical drain and nested-call behavior against the prior native acceptance fixtures; do not treat a logically cancelled wave as quiescent.
2. Restore graph-level state triggers/epoch observation: the Advanced preparer must be activated by watched state changes without a second scheduler.
3. Replace the old Basic imperative model/tool continuation with graph nodes and remove the retired BasicInput entry contract, poll/cancel handlers and dead helpers. Currently that direct handler only rejects work.
4. Port recursive modules, public boundaries, input policies, coloring, precise patch edit conflict handling and optional `patch!` macro from the separate prototype. None of these should introduce a second graph implementation.
5. Port the *guarantees* tested by retired V1/V2 native conformance into tests for the sole engine. Keep authority, failure isolation, state scope, cancellation and generation pinning checks.
6. Restore cargo fmt, compiler checks and all required CI. The current branch is not production-ready.

No compatibility scheduler, second Basic app or alternate graph IR should be introduced to make CI green.
