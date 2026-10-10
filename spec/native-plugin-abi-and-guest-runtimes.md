# Native plugin ABI and extensible guest runtime bindings

status: ABI contract implemented; dynamic loader and guest runtime adapters pending
scope: native loader, runtime adapter plugins, guest bindings, resident generations
depends_on:
  - spec/plugin-runtime-bridges.md
  - spec/plugin-host.md
  - spec/plugin-contributions.md
  - spec/selectable-generations.md
  - spec/configuration-frontends.md

## Decision

Phenix has one canonical Plugin model and one canonical graph resolver. The kernel bootstrap understands a single native shared-library ABI. A native library can provide ordinary contracts or provide a *Guest Runtime* contract that executes plugins compiled for another language or artifact format.

The latter is a **native plugin**, not a built-in Lua/Wasm/JavaScript case in Core. Its guest is a separate logical Plugin with its own identity, contribution descriptors, authority, artifact revision, lifecycle and generation membership.

The kernel process remains running while plugin artifacts are rebuilt. Rebuilding or loading an artifact stages a candidate generation. A caller explicitly selects a resident candidate for trial execution or promotes it as the default. Artifact reload, generation selection and physical library unloading are different operations.

The Rust crate named `phenix-harness` is not the native loader. Product composition belongs to portable configuration, with Nix as one deployment frontend.

This document describes the target architecture. **No native ABI loader, Lua adapter or dynamic-library hot replacement is implemented by this specification.** Existing `embedded` and process-backed behavior remains authoritative until migrated and tested.

## Existing implementation and the mismatch

| Current code/specification | Current behavior | Target and precise correction |
| --- | --- | --- |
| `spec/plugin-embedded-runtime.md` | An `embedded` factory is statically linked into the executable; changing executable bytes requires relinking. | Native executable plugins are separately compiled shared-library artifacts loaded at runtime. The existing embedded path is a compatibility/performance option during migration, not the intended universal bootstrap. |
| `spec/plugin-runtime-bridges.md` | The only Core bootstrap runtime is `embedded`. Runtime adapters may be plugins, but their own factory is linked ahead of time. | The intrinsic bootstrap capability is a *minimal native ABI loader*. Runtime adapters are themselves ordinary native ABI plugins and can be replaced as artifacts. |
| `spec/plugin-process-runtime-bridge.md` | The spec explicitly excludes Rust dynamic libraries as independent plugin artifacts. | Native dynamic libraries become an additional *trusted* execution format. The process bridge remains available for isolation and must not be removed as part of the migration. |
| `rust/crates/phenix-harness/src/runtime_builder.rs` and `phenix-plugin-catalog` | Product code enumerates and links first-party factories into a Rust executable. | Discover artifact manifests, load chosen native libraries and resolve contributions generically. No first-party factory catalog in the final product executable. |
| `modules/package-sets.nix` and `modules/plugin-packaging.nix` | Embedded plugin packages include metadata naming a Rust crate while the product executable already links its code. | Packages must contain loadable artifacts and inspectable manifests; Nix chooses installed artifacts but never resolves provider bindings or graph semantics. |
| `spec/selectable-generations.md` | Multiple resident generations and explicit selection are already specified/implemented. | Reuse this machinery. The native ABI and guest adapters must participate in it; do not add a separate hot-reload state machine. |
| `rust/crates/phenix-binding-lua` and Phenix.nvim | Existing Lua bindings act as an ACP *client* for the Neovim frontend, not a Phenix guest-runtime adapter. | Keep that client role. Add a separate native `adapter-lua` plugin that hosts Lua guest modules as Phenix plugins. Both may reuse canonical contract/binding generation, not share a Neovim Lua interpreter or plugin lifecycle. |
| `spec/plugin-contributions.md` and Stage B in `spec/microkernel-composition-roadmap.md` | Canonical contribution descriptors are being made portable and inspectable. | Native and guest metadata must normalize into those same descriptors before activation, without runtime-specific contribution registries. |

### Terminology corrections

- **Native ABI** means the binary boundary between the kernel's loader and a native plugin. It is not the Rust trait ABI or a dynamically shared copy of `phenix-sdk`.
- **Native plugin** means a shared-library artifact that implements the native ABI. It may implement application contracts or the Guest Runtime contract.
- **Guest plugin** means a distinct logical plugin, hosted by a runtime adapter. Lua code uses Lua bindings to its adapter; it does **not** implement the native ABI itself.
- **Runtime adapter** means a normal plugin that provides `phenix.guest-runtime@1` for a named guest artifact format. It cannot directly change Core's graph, authority or generation choice.
- **Plugin reload** means staging a replacement artifact and generation. **Selection** chooses the resident generation for a root execution. **Promotion** changes the default for *new* roots. **Unload** means retiring instances. **Physical code unload** means releasing native executable mappings and requires additional safety conditions.
- **Bootstrap** is intrinsically implemented by the kernel's native loader. Calling that loader a plugin without a lower-level loader would create a circular definition.

These distinctions correct earlier vague descriptions such as "the kernel loads Wasm plugins", "Lua plugins share the native ABI", "a shared Rust library guarantees compatibility", and "reload automatically updates the active graph". None of those statements expresses the intended boundary.

## Target execution paths

~~~text
Kernel/Core
  +-- intrinsic native ABI loader
        +-- native provider: memory.so
        +-- native plugin: adapter-lua.so
        |     provides Guest Runtime "lua.module@1"
        |     +-- guest: memory.lua  [PluginId acme.memory]
        |     +-- guest: tools.lua   [PluginId acme.tools]
        +-- native plugin: adapter-wasm.so
              provides Guest Runtime "wasm.component@1"
              +-- guest: model.wasm  [PluginId acme.model]
~~~

The Lua and Wasm adapters use the same native plugin ABI as `memory.so`. Their hosted guests use adapter-specific bindings. Every native or guest plugin registers its own canonical contributions, not services owned by a generic `adapter-lua` proxy.

The kernel selects a runtime provider from the guest's declared runtime requirement. Every executable Guest Runtime provider is a native ABI plugin loaded directly by the intrinsic loader. Guest artifacts cannot themselves provide the executable runtime adapter that bootstraps another guest format. This restriction keeps bootstrap dependency depth finite and avoids a second host-translation lifecycle. Runtime-provider selection and all ordinary plugin dependencies must still be validated for cycles before activation.

## Implemented ABI contract (PR #726)

The separate, zero-dependency `rust/crates/phenix-plugin-abi` crate now
defines the **C-compatible ABI v1 table layout**: major/minor and size
negotiation, required feature bits, opaque call tickets, borrowed inputs,
producer-owned terminal buffers with explicit release callbacks, host-scoped
cancellation and wakeup callbacks, and mandatory prepare/start/begin/poll/
cancel/stop/destroy entries. Validation rejects foreign callback ticket IDs,
unknown statuses, malformed payload ownership, truncated tables, unknown
features and incomplete function tables. No Rust trait or allocator-owned
object crosses the C boundary.

Core's `SharedPluginInvocation::begin_component` also accepts
`PluginCallStart::Pending`; a `PluginPendingCall` has one terminal
`PluginCallCompletion`, optional nonblocking poll, and disconnect-as-error.
This Rust host bridge is exercised by a real provider completing *two*
concurrent Fork branches. Existing blocking native providers use the same
canonical dispatch with the default immediate implementation.

**Implementation boundary:** the versioned ABI layout and Rust-native
pending bridge are implemented, but the **intrinsic shared-library loader,
library residency lifetime, foreign buffer copy/release trampoline, ABI host
callback registration, and native Lua adapter are not implemented**. The ABI
crate intentionally forbids unsafe code; memory-sensitive loader operations
belong in their own auditably isolated package. Do not represent these
structures alone as a loadable native plugin system or as a completed guest
runtime bridge.

## Native ABI bootstrap contract

A small separately versioned `phenix-plugin-abi` package defines a C-compatible calling convention usable by Rust, C, Zig and other native languages. A shared library exports one known entrypoint, for example `phenix_plugin_entry_v1`, returning a versioned function table.

The exact layout belongs to the implementation PR, after tests for each platform. The contract must define:

- ABI major/minor negotiation, table-size and feature negotiation; reject unknown major versions before invoking plugin functions.
- Exact identity and revision of the artifact; distinguish artifact availability, enabled status, active instance and resident-generation membership.
- Inspectable descriptor bytes provided out-of-band in the package; do not execute arbitrary native code just to discover dependencies or authority.
- Opaque instance, request, host-scope and error handles. No `&dyn Trait`, `Vec`, Rust enums, unwinding or allocator-owned Rust objects cross the ABI.
- Explicit ownership of input/output buffers, allocation/free pairing, bounded lengths, lifetimes, alignment and thread-safety promises.
- A host callback table granting only attenuated operations: imported-contract invocation, event publication, permitted resource access, task/cancellation checks and structured results.
- Lifecycle methods for candidate preparation, start, invoke, stop and destroy, with typed failure stages and cancellation correlation.
- Correlated streaming/event messages over the existing canonical event model, without imposing a Rust async runtime at the ABI.
- Panic containment at the Rust SDK trampoline. Undefined behavior, memory corruption or plugin-created uncontrolled threads cannot be contained inside the kernel process.

A Rust author uses ordinary typed traits and `phenix-sdk` macros. Generated native bindings lower them into this ABI; `phenix-plugin-abi` stays a small standalone contract crate/header, not a runtime implementation shared with the kernel.

Only the native loader is intrinsic. It provides neither model/tool/session contracts nor built-in interpreters.

## Extensible Guest Runtime contract

A native adapter provides a versioned, generic runtime-provider Interface, tentatively `phenix.guest-runtime@1`. The final stable Interface name/schema must be registered through the ordinary contribution contract system.

The adapter implements:

1. `inspect`: accept an immutable guest artifact reference and check its declared guest format, version and runtime feature requirements. Normalize the already-inspectable guest manifest into canonical contribution descriptors without executing guest code.
2. `prepare`: stage one guest instance under a fixed `PluginId`, artifact revision, generation and attenuated guest `PluginHost`.
3. `invoke`: convert canonical request/result/event values to the guest binding and back. Preserve call identity, contract version, cancellation, provenance and generation.
4. `stop` and `destroy`: release guest resources when generation retirement allows it.

The kernel still owns provider selection, graph construction, permission grants, durable namespaces, entrypoint routing and root generation selection. An adapter cannot register additional providers during `start` or claim services for every guest under its own identity.

Adapter authority and guest authority are intersected independently. A Lua guest may invoke only its resolved imports through the guest host binding; adapter host privileges never flow automatically to guests. An in-process native Lua adapter is trusted native code and cannot promise OS sandbox isolation for itself.

### Illustrative cross-language package pair

The native adapter package contributes a normal plugin plus a runtime provider:

~~~toml
# adapter-lua/plugin.toml, proposed portable manifest syntax
id = "phenix.adapter.lua"
runtime = "native.abi@1"
artifact = "adapter-lua.so"
provides_runtime = "lua.module@1"
~~~

The guest package requires that runtime and independently provides a service:

~~~toml
# my-memory/plugin.toml
id = "acme.memory"
runtime = "lua.module@1"
artifact = "memory.lua"
provides = ["example.memory@1"]
~~~

The guest author can use an adapter-provided Lua SDK under the same top-level module name as the client, but with a different runtime implementation:

~~~lua
-- memory.lua, illustrative guest-side Lua API, not an existing implementation
local phenix = require("phenix")

return phenix.plugin({
  id = "acme.memory",
  provides = {
    ["example.memory@1"] = {
      recall = function(query, host)
        return host:invoke("example.store@1", "search", { query = query })
      end,
    },
  },
})
~~~

Those files illustrate the boundary, not a second source of registration authority. The versioned manifest fixes guest contract declarations before `memory.lua` executes. The Lua adapter must validate that the module's exports match the pinned declarations.

### Neovim frontend compatibility and Lua runtime boundary

Phenix.nvim is the primary frontend for the first conformance slice, making Lua a better initial guest-runtime example than JavaScript. **The editor's Lua client and a Lua guest hosted by the kernel are separate runtimes:** the current `phenix-binding-lua` crate supplies client-side ACP bindings, while `adapter-lua.so` would independently host one or more Lua guest plugin instances. A Lua module in Neovim is not automatically a plugin in the kernel's graph.

The adapter should initially specify and test a concrete Lua language/version compatibility target, preferably compatible with the existing Lua 5.1 binding ecosystem. LuaJIT support, if chosen, must be tested rather than assumed. The ABI must not expose Lua state pointers or assume that the editor and server share an address space.

Prove an editor-independent invoke/import/event round trip first. Then add a narrow Phenix.nvim ACP smoke test: keep the editor connected while staging Lua guest revision B, invoke B from a deliberately selected new root, and confirm the preexisting root remains pinned to A. The frontend must never rebuild or restart the kernel to observe the new generation.


### Bidirectional Rust and Lua integration

There are two complementary call directions. Both present an ergonomic Lua-facing Phenix API while retaining distinct execution and authority boundaries.

~~~text
Phenix Core (Rust)
  -> Native ABI -> adapter-lua.so -> Lua plugin callback
  <- Native ABI <- adapter-lua.so <- Lua plugin result

Lua guest code
  -> require("phenix") guest SDK -> adapter-lua.so
  -> authorized PluginHost import -> Phenix Core (Rust)

Neovim Lua code
  -> require("phenix") client SDK -> ACP client binding
  -> ACP transport -> Phenix application / Core
~~~

- **Rust -> Lua:** A selected Phenix service, event or graph node invokes a Lua function hosted by the native Lua adapter. The adapter translates arguments, cancellation and results.
- **Lua guest -> Rust:** Inside the hosted Lua plugin, `require("phenix")` provides contract invocation, event publication, and permitted host capabilities. Those calls return through the adapter into ordinary generation-pinned Core dispatch.
- **Neovim Lua -> Rust:** The existing `require("phenix")` client module keeps its ACP/application operations. The editor is an independent process/client, not the runtime hosting guest plugin callbacks.

The top-level import name can be **the same in both Lua environments** because they are separate module loading contexts. Shared public contract shapes should come from generated descriptors. The guest SDK has `phenix.plugin(...)` and a scoped host API, while the client SDK has `phenix.connect(...)` and negotiated application operations. They are related APIs, not interchangeable transport mechanisms. Never attempt to auto-detect a missing guest host and silently connect through ACP, or allow a client to impersonate a privileged plugin host.

The first integration test must exercise both directions in a single invocation: invoke a Lua-provided service from Rust, have its handler call a second imported Rust service through the Lua guest SDK, and propagate the typed result, generation and effective authority back to the original caller.

### Preserve the Neovim Lua client module

The existing **client-facing** `phenix-binding-lua` crate exports a native Lua module via `#[mlua::lua_module(name = "phenix")]`. Its public import remains `require("phenix")`; existing `phenix.connect(...)`, `phenix.application`, `phenix.tools`, descriptor and callback behavior remain supported. This is an independently loadable Lua client library for Neovim or any compatible Lua 5.1/LuaJIT host. It must not depend on loading `adapter-lua.so` into the editor.

The proposed `adapter-lua.so` is a **server-side native Phenix plugin** that hosts separately packaged Lua guest plugins. A Lua guest should also be able to use `require("phenix")` as its primary authoring library, but its module comes from the guest adapter. The existing editor client library remains a separate module implementation. The two environments must not share a Lua state, native module instance, ACP connection or runtime lifetime; guest calls go through an attenuated `PluginHost`, not through Neovim or client ACP.

The public Neovim API should remain ergonomic and client-oriented, e.g.:

~~~lua
local phenix = require("phenix")
local client = phenix.connect({ command = "phenix", args = { "--mode", "acp" } })

-- Existing ACP and Phenix application operations stay available.
-- Future generation/plugin management helpers are generated from the
-- negotiated application descriptor, not handwritten alternate protocols.
~~~

`phenix-binding-generator` and the fixed application descriptor should remain the source of truth for client-side operation names, errors and negotiated capability metadata. Common contract schemas may be shared by the client and guest binding generators, but transport, threading, lifetime and permission handling stay on their respective sides.

After Stage N4, the Lua client should expose typed, capability-negotiated operations to **list resident generations, inspect a candidate graph and its diff, explicitly select a candidate for a new root, and promote a candidate if authorized**. Naming and exact return shapes are governed by the application descriptor, not by special Lua-only commands. Unavailable management extensions must fail with the binding's existing typed unsupported-capability error; no implicit fallback or direct kernel mutation is permitted.

Acceptance includes:
- A Neovim session retains its existing `require("phenix")`, ACP connection, requests, event delivery and callback behavior through a Lua guest reload.
- The same `require("phenix")` module works from a non-Neovim Lua 5.1/LuaJIT fixture without `vim` globals.
- The server-side `require("phenix")` guest module can implement and invoke canonical services without pulling `phenix-client-acp` into its guest runtime.
- A candidate can be selected via negotiated Lua client operations without an editor/kernel restart or accidentally changing existing root generation bindings.

### Adapter dependency rules

Executable Guest Runtime adapters are native ABI plugins. An adapter may depend on another ordinary native plugin service through a resolved, attenuated import, but the adapter's own execution never depends on a guest interpreter. Guest plugins may implement services that other guests import; they cannot become executable runtime providers in this design. Cyclic adapter or service dependencies fail during candidate validation. Supporting nested interpreter-hosted runtime adapters would require a separate design and conformance gate.

## Reload, residency and promotion

The running kernel never rebuilds itself during ordinary plugin development.

~~~text
build guest or native plugin artifact revision B
  -> store immutable artifact and inspect package manifest
  -> resolve candidate graph B using the exact ABI/runtime adapter
  -> validate contracts, authority, pinned bindings and durable compatibility
  -> prepare and stage B as a resident generation (default stays A)
  -> choose B explicitly for a trial root under original root constraints
  -> promote B to default for new roots only when requested
  -> drain/retire A when no pinned consumers remain or cancellation is authorized
~~~

This uses `GraphReconciler` and `RootExecutionConstraints` defined by `spec/selectable-generations.md`. No second plugin-manager graph, live mutable registry or alternate root-authority path is allowed. Existing executions continue on A. Changing a session's generation is explicit and only occurs between root executions.

Candidate *inspection* must be side-effect-free. Candidate *activation or trial execution* may have external effects and may share durable resource identities with A. Isolation must therefore be provided by an explicit test backend/workspace or approved capability policy; generation residency alone is not a transaction across the filesystem/network.

For native libraries, use immutable revision-specific paths, retain each library handle while any code/instance/callback/task can reach it and only physically unload after an auditable teardown. The initial production-safe policy may retain old library mappings until process exit. This is code replacement, not guaranteed physical unloading. A native crash or memory corruption can still terminate the kernel; untrusted plugins use a process or sandboxed runtime.

If candidate preparation fails, A remains the default and no candidate callbacks remain active. After promotion, runtime failures must not silently switch provider implementations mid-request or replay ambiguous mutating calls.

## Delivery order and source ownership

This is an **independent design PR**. Runtime implementation must follow the existing typed-contribution and resident-generation contracts rather than inventing a competing model.

| Stage | Deliverable | Dependency and files owned |
| --- | --- | --- |
| N0 | This design and current-vs-target mismatch audit | Independent of #726-#731; specification only |
| N1 | `phenix-plugin-abi` C header, Rust SDK trampolines and ABI conformance fixtures | Can begin independently; avoid #728 SDK macro/contract files until Stage B stabilizes |
| N2 | Generic native artifact loader, manifest discovery, adapter bootstrapping and validation | #728 normalized descriptor contract; Core runtime/loader files, not #726 workflow graph internals |
| N3 | Native `adapter-lua` with one independently declared Lua guest, plus frontend-independent conformance | N1+N2; new adapter/example packages only |
| N4 | Side-by-side revision loading, stage/test/select/promote and safe retirement via existing resident generations | N2 and `spec/selectable-generations.md`; add tests without a second reconciliation API |
| N5 | Optional native `adapter-wasm`, `adapter-js` and process guest bindings; package/frontend parity | N2+N3 conformance pattern; independent adapters |
| N6 | Retire embedded factory catalogs and Rust `phenix-harness` product composition after all first-party providers and frontends migrate | N2-N4 plus parity with Basic/Full and non-agent compositions; coordinate with #733 cleanup inventory |

N2 and N4 may require shared Core runtime edits. Land N2 first or isolate owners before parallel work. N3 can progress separately from #726 agent-loop work because guest execution uses ordinary service contracts. #729/#730 graph kinds are not prerequisites for the first service-call-through-adapter proof, but adapters must adopt their canonical schema if those contracts change before merge.

## Semantic acceptance gates

A passing build does not establish migration completion.

1. Kernel-only binary starts with no product plugins; independent native library supplies an ordinary contract without relinking Core.
2. C-compatible ABI fixture is compiled separately from the runtime and completes negotiation, lifecycle, typed invocation and cleanup. Wrong major version, table size, contract declaration or ownership rule fails safely.
3. Native Lua adapter is itself loaded as a normal plugin. A guest Lua plugin implements a service, imports another service, emits an event, and returns a typed result through two boundaries, with no Lua branch in Core.
4. Two distinct Lua guests hosted by the same adapter preserve separate `PluginId`, authority, artifact revision, contributions and durable ownership.
5. Missing adapter, adapter-cycle, forged guest declaration, undeclared provider, unauthorized host callback and incompatible contract all fail before graph commit or the unauthorized operation.
6. Kernel remains live across native artifact B staging; A is still default, trial B uses explicit root selection, promotion changes new roots only, and old in-flight calls remain pinned.
7. An adapter implementation reload stages its dependent guests in B; retirement never invalidates callbacks or plugin instances belonging to A.
8. Candidate failure leaves A usable; native unload is not asserted until a teardown/liveness audit proves it. No replay of uncertain external mutations.
9. Nix and non-Nix installations load the same exact artifacts and produce equivalent resolved graphs without a second packaging-time resolver.
10. At least one non-agent plugin fixture proves the bridge does not depend on agent, prompt, tool or memory-specific kernel behavior.

## Out of scope for this design PR

No production shared-library loading, `dlclose` guarantee, Wasm interpreter in Core, automatic promotion, new product behavior, Rust ABI exposure or native sandbox guarantee. Code implementation must earn completion against the acceptance gates above.

## Kernel RFC alignment: asynchronous ABI and leases

The [kernel RFC](kernel-runtime-rfc.md) fixes the semantic constraints for native, process and guest providers. They apply to the future ABI loader; they are not claims about currently available loader behavior.

- A native service call may settle immediately or remain pending. The ABI must express exactly one eventual completion, cancellation request correlation and host wakeup/poll readiness. Callback versus poll representation is deferred; asynchronous dispatch and lost-wakeup avoidance are mandatory.
- The host scheduler must not block on arbitrary provider code. Blocking providers use a managed blocking executor or an isolated process boundary. Dropping a waiting client does not prove a native invocation stopped.
- Pending calls, outstanding callbacks, guest children and managed blocking work retain the generation lease and root scope until settlement or authorized isolation-level termination. A native call ignoring cooperative cancellation can delay retirement indefinitely.
- No unwind or panic may cross the C-compatible ABI. Buffer allocation/free ownership, alignment, thread affinity, legal reentrancy, callback lifetime and late-completion rejection are versioned ABI contracts. Host TLS cannot be an authority source.
- Guest authorization remains per guest identity. Adapter permissions do not become guest grants. Data-only frame values and `DataRef` locators carry no authority; host capabilities never cross through serialized frames.
- The in-process native adapter is trusted code, not a sandbox. An isolated process adapter is a first-class, separately deployable trust boundary through the same logical Plugin contract. OS process termination may provide stronger containment than cooperative in-process cancellation.
- Client-side `require("phenix")` in Neovim and guest-side `require("phenix")` remain separate Lua environments and roles, with no automatic ACP fallback or shared interpreter state.

First-party ABI conformance must cover pending/immediate completion, concurrent callback races, ownership and reentrancy, cancellation and late results, lease-preserving retirement, malformed value decoding, Lua-to-Rust imports and process-isolated termination. Do not remove embedded/process compatibility implementations until the separate artifact and product-level parity tests pass.
