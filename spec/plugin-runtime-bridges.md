# Plugin runtimes and live management

status: implemented
coverage:
  - rust/crates/phenix-core/src/plugin_build_loading_regression.rs
  - rust/crates/phenix-core/src/plugin_management_regression.rs
  - rust/crates/phenix-core/src/plugin_runtime_adapter_regression.rs
  - rust/crates/phenix-core/src/plugin_runtime_adapter_host_regression.rs
  - rust/crates/phenix-core/tests/kernel_concurrency_contract.rs

## Purpose

Define the current canonical Plugin runtime boundary and kernel-owned live Plugin-management semantics.

Execution runtime changes how executable behavior is hosted. It does not create a second Plugin model, component graph, authority system, persistence model, or lifecycle.

## Canonical runtime model

Core has one executable `PluginInstance` contract and one `PluginHost` boundary.

The built-in bootstrap runtime is `embedded`. Runtime identity is open: non-embedded execution is provided through the kernel Runtime Provider Interface rather than a closed Core enum of supported languages or engines.

A Runtime Provider translates a guest artifact into the canonical `PluginInstance` contract. The guest receives its own attenuated Plugin Host. Runtime Provider authority and guest Plugin authority are separate.

No concrete non-embedded bridge package is required by the current baseline.

## Inspectable candidate

A Plugin candidate is inspectable before its executable behavior runs.

Candidate metadata includes:

- Plugin identity and version;
- execution runtime identity;
- exact artifact revision;
- Components, Imports, Exports, Layers, and Listeners;
- authority limits;
- Plugin Resources and durable schemas;
- runtime dependencies.

Runtime-specific executable code does not choose its own effective authority or component contract after activation begins.

## Runtime Provider resolution

Every non-embedded runtime resolves through a Runtime Provider already available to the candidate graph.

Runtime-provider dependencies are acyclic and must terminate at `embedded`. Missing providers and dependency cycles fail candidate preparation before graph commit.

The Provider's own authority is derived independently from the guest Plugin's authority. `plugin_runtime_adapter_host_regression.rs` verifies this separation and the host cancellation surface.

## Plugin management

Kernel-owned desired-state management supports:

```text
Load
Unload
Reconcile desired Plugin set
```

Loading an already-active `PluginId` is replacement. There is no separate semantic reload operation.

Management is implemented through `GraphReconciler::manage` and the same candidate-generation machinery used by ordinary graph reconciliation.

Lifecycle methods such as `start` and `stop` are internal transition phases, not public management operations.

## Artifact inputs

A load request normalizes either:

```text
Ready PluginArtifact
Build PluginBuildPlan
```

into one exact `PluginArtifact` before runtime preparation.

Artifact revision is derived from exact artifact bytes and pinned by the candidate Graph Generation.

## Build plans

Build plans are typed candidate-preparation input. Steps use an executable and argument vector rather than one shell command string.

Core receives an explicit build executor and content-addressed storage capability from trusted management context. Core does not gain ambient shell access to perform builds.

Build authority is attenuated independently from both guest Plugin authority and Runtime Provider authority.

A successful build produces one declared output artifact plus bounded provenance. A later runtime or activation failure preserves that build evidence while leaving the active generation unchanged.

## Atomic replacement

Load, replacement, unload, and desired-set reconciliation use the same transactional generation model:

```text
resolve desired state
  -> authorize management request
  -> obtain exact artifact when needed
  -> resolve Runtime Providers
  -> resolve component graph
  -> validate authority and resources
  -> prepare candidate instances
  -> start candidate
  -> commit Graph Generation
  -> stop retired instances
```

Any failure before commit leaves the previous active Graph Generation unchanged.

Retired instances are stopped synchronously after commit in the current baseline.

## Generation pinning

New invocations bind to the active generation at start.

Existing invocations and spawned Plugin task scopes retain their starting generation until completion or cancellation. Replacement does not mutate the implementation under an already-started call.

Core owns live-call tracking, task cancellation, and late-result rejection. See `plugin-threading.md`.

## Persistence ownership

Persistent state belongs to Plugin and resource identity rather than artifact revision or Runtime Provider identity.

Changing executable artifact or execution runtime does not implicitly create a new persistence namespace.

Runtime Providers translate guest access to kernel-owned persistence capabilities; they do not own product state semantics.

## Error boundary

Management errors identify the failed phase and preserve the previous active generation when commit has not occurred.

The current contract distinguishes failures including:

- authorization denial;
- build or declared-output failure;
- unavailable artifact storage;
- unavailable Runtime Provider;
- runtime dependency cycle;
- graph-resolution failure;
- candidate preparation or start failure;
- stale expected artifact revision;
- unknown unload target.

Post-commit retirement failure is operational failure and does not roll the graph pointer back to an already-retired generation.

## Invariants

- One canonical Plugin API serves every execution runtime.
- `embedded` is the only Core bootstrap runtime.
- Runtime identity is open and resolved through Runtime Providers.
- Runtime Providers do not gain composition authority.
- Guest and Runtime Provider authority remain distinct.
- Desired-state management is kernel-owned.
- Builds are explicit candidate preparation, not ambient shell execution.
- Exact artifact revisions are generation-pinned.
- Pre-commit failure preserves the previous active generation.
- Active calls remain pinned across replacement.
- Persistence ownership does not follow execution runtime or artifact revision.
- Consumer component contracts do not expose provider runtime choice.

## Resident replacement trials

The implemented baseline commits replacement and then stops retired instances. `spec/selectable-generations.md` proposes a development extension where compatible old and candidate generations may remain resident for explicit root execution before promotion.

This does not add a reload lifecycle. Build, resolution, Plugin preparation, authority, persistence ownership, and Runtime Provider semantics remain the same. The extension changes generation residency and root selection, not the Plugin API.
