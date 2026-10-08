# Process-backed plugin runtime bridge

status: implemented

## Relation to the native ABI proposal

The statement below that Rust dynamic libraries are unsupported describes the **current implementation**, not a permanent ban on an explicitly versioned native ABI. See [native plugin ABI and guest runtimes](native-plugin-abi-and-guest-runtimes.md). A future native shared-library loader is a *trusted in-process* execution mechanism. This process bridge remains separately useful for OS isolation and crash containment, including for untrusted guest runtimes.

A process runtime adapter may itself be distributed as a native adapter plugin after the native bootstrap exists. That does not make the guest process a native-ABI implementer or add process-specific composition logic to Core.

## Purpose

Run executable plugin implementations in a separate process when they need independent distribution or enforceable isolation, while preserving the same logical Plugin API used by embedded implementations.

Requires `spec/plugin-host.md` and follows the plugin-runtime-adapter model in `spec/plugin-runtime-bridges.md`.

## When to use a process-backed bridge

A process-backed runtime bridge is a distribution and security boundary, not the default concurrency model.

Use it when an executable plugin needs one or more of:

- independent installation or upgrade without rebuilding the Phenix product;
- implementation in another language;
- enforceable filesystem, network, secret, IPC, crash, or memory isolation;
- an ownership/release lifecycle that should not be linked into the normal product.

First-party status does not force embedded execution. Third-party status does not prevent a custom trusted product from embedding source at build time. A process-backed runtime bridge is the normal execution arrangement for independently supplied executable plugins that require a process boundary.

## Transport

The first process-backed bridge uses a local subprocess protocol over stdio or a Unix-domain socket. The transport sits behind the canonical PluginHost/plugin-runtime-adapter boundary; it does not define alternate plugin semantics.

The bridge transport uses blocking reads/writes on dedicated threads. It does not require an async executor.

The protocol is typed and versioned. It carries only transport-safe values defined by the logical plugin contracts.

Rust dynamic libraries are not a supported plugin ABI. Independently distributed Rust executable plugins use this process protocol rather than `.so`, `.dylib`, or `.dll` loading. WASM is not required for the first implementation. A later WASM plugin runtime adapter may implement the same logical contract if a concrete need justifies it.

## Handshake

Before a plugin becomes `ready`, host and guest exchange and validate:

- `PluginId`;
- plugin package/version identity;
- plugin protocol version;
- supported contribution/capability contract versions;
- declared capability implementations;
- runtime feature support such as cancellation or streaming;
- health/startup result.

A protocol or contract mismatch fails before contribution activation.

The runtime handshake cannot add contributions, permissions, or priority beyond the pinned configuration and validated manifest.

## Process ownership

The kernel owns process-backed plugin lifecycle through `PluginHost`.

Each start creates a new plugin runtime generation. Process PID, transport handles, pending requests, and cancellation handles are process-local state and are not persisted.

Unexpected exit:

- marks the generation unavailable;
- fails in-flight requests with a typed disconnect/crash error;
- releases live scopes;
- does not crash the Phenix process;
- does not automatically repeat ambiguous mutating requests through another provider.

Restart creates a new generation and re-runs handshake.

## Permission enforcement

Process-backed plugin permissions are enforced by the OS sandbox where the bridge claims enforcement.

At minimum map applicable plugin grants to existing Phenix isolation mechanisms for:

- filesystem access;
- repository metadata access;
- network access;
- explicit IPC endpoints;
- secrets;
- writable temporary/scratch state.

If a requested restriction cannot be enforced for the selected process-backed runtime, startup fails closed instead of silently broadening host access.

Host API permission checks remain mandatory even when the OS sandbox also restricts the process.

## Environment

The guest process receives only declared non-secret environment plus explicitly granted secrets and runtime metadata required by the protocol.

Host credentials, arbitrary HOME state, ambient agent credentials, frontend sockets, workspace write access, or network access are not inherited unless explicitly granted by the resolved plugin permission set.

## Requests

Each request carries:

- request ID;
- plugin runtime generation;
- capability/operation identity;
- contract version;
- normalized input;
- scoped host-operation token/handle identity if needed;
- cancellation correlation.

Responses echo request identity and generation. A response from an ended or mismatched generation is rejected as stale.

## Concurrency and cancellation

The bridge may handle concurrent requests when the guest declares support. Correlation is explicit; response ordering is not semantic.

Each blocked connection/request worker follows the ordinary threaded runtime model. Streaming is a sequence of correlated protocol events, not an async Rust stream.

Cancellation uses the ordinary `PluginHost` live scope. If the bridge/guest supports cancellation, the host sends a correlated cancellation request. Otherwise the host may terminate the plugin runtime generation when policy requires hard cancellation.

A cancelled request cannot later commit a result into canonical state.

## Security boundary

A process-backed plugin is not trusted merely because it is installed or first-party. Permission grants are explicit product/user policy.

The guest process can affect canonical Phenix state only through allowed protocol operations. Direct workspace, network, IPC, and secret access is additionally bounded by its sandbox.

Embedded native plugins do not provide this OS-isolation guarantee. A plugin requiring enforceable isolation must use an execution runtime that supplies the required boundary.

## Invariants

- Process-backed and embedded executable plugins implement the same logical Plugin API.
- A process-backed runtime is chosen for distribution or isolation, not ordinary concurrency.
- Protocol handshake cannot expand configuration semantics.
- Process-backed permissions are enforced both at host-operation boundaries and by OS isolation where applicable.
- Unsupported isolation fails closed.
- Guest crash/disconnect cannot crash Phenix.
- Plugin runtime generations make stale responses detectable.
- Restart does not restore process-local handles.
- Ambient host credentials and IPC are absent without explicit grants.
- Ambiguous mutating requests are never automatically replayed through another provider.
- Bridge transport does not introduce an async runtime requirement into the canonical Plugin API.
- Rust dynamic libraries are not a supported independently distributed plugin format.

## Required regressions

- compatible guest completes handshake and becomes ready;
- incompatible protocol version is rejected before contribution activation;
- handshake that advertises an undeclared capability cannot expand the pinned manifest;
- guest process crash fails in-flight calls and leaves Phenix alive;
- restart increments generation and stale old-generation response is rejected;
- no-filesystem guest cannot read workspace source;
- read-only guest cannot write workspace source;
- no-network guest cannot reach host/outbound network;
- ungranted secret is absent from the guest environment;
- explicit allowed IPC endpoint works while undeclared host IPC is unavailable;
- cancellation prevents a late bridged result from entering canonical state;
- process-backed and embedded adapters pass a shared conformance fixture for the same logical capability contract;
- blocking bridge transport supports incremental streaming without an async host runtime.

## PR boundary

This slice adds subprocess transport, handshake, lifecycle integration, sandbox mapping, cancellation correlation, and process-backed conformance tests. It does not add remote-network plugins, runtime package downloads, Rust dynamic-library loading, WASM, marketplace semantics, or automatic plugin updates.
