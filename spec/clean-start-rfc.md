# RFC: Phenix Minimal Plugin Microkernel — Clean-Start Specification

**Status:** Final review draft; normative design target, **not an implementation claim**  
**Version:** 1.0-rc1  
**Date:** 2026-10-10  
**Target:** Linux-first, Rust-authored native plugins, Nix packaging and deployment  
**Initial release:** M0.0–M0.4, accepted incrementally  
**Basis:** Existing Phenix AI specifications, source code and regression tests are *references*, not compatibility constraints  
**Primary deliverable:** An independently usable, non-AI, Nix-packaged plugin microkernel

## 1. Status, terminology and precedence

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **NOT RECOMMENDED**, **MAY** and **OPTIONAL** are interpreted as BCP 14 (RFC 2119 and RFC 8174) only when written in capitals.

Normative requirements carry a milestone tag, either in their section heading or inline. **M0.0** is the first executable release; **M0.1** adds richer selection and authority; **M0.2** adds asynchronous lifecycle; **M0.3** adds selectable generations; **M0.4** closes product conformance. A requirement tagged **ALL** applies once its referenced mechanism exists, including M0.0. A requirement tagged **DEFERRED** MUST NOT be interpreted as an M0.0 prerequisite.

This document is both the **architectural RFC** and the **M0 implementation-contract schedule**. The specific M0.0 wire/API profile in §§8–16 is binding on M0.0; the portions explicitly marked **M0.1+**, **M0.2+**, or **M0.3+** become binding at their respective gates. A narrowly scoped ABI amendment MAY define C layout details without changing ownership or semantic requirements. It MUST NOT introduce a second runtime or an undeclared compatibility path.

This RFC supersedes earlier *clean-start draft* milestone and descriptor-mechanism assumptions. The existing repository's target RFC and implementation branches remain non-normative references. No existing Rust crate, macro syntax, serialized message or plugin identity is grandfathered into this implementation.

### 1.1 Vocabulary

| Term | Meaning |
| --- | --- |
| **Contract / interface** | Versioned operations, request schemas, result/domain-error schemas and semantic guarantees |
| **Plugin** | Independently identified owner of declared contributions, executable code and lifecycle |
| **Native artifact** | Exact loadable shared-library file, independently versioned from a descriptor |
| **Contribution** | Immutable, plugin-attributed declarative input to preparation, never a registration callback |
| **Descriptor** | Canonically encoded collection of a plugin's static contributions and ABI requirements |
| **Package record** | Nix-produced binding of exact artifact bytes, descriptor bytes and deployment paths |
| **Import / export** | Typed required operation / provider of that operation |
| **Binding** | Explicit mapping of a selected import to an eligible provider in a resolved composition |
| **Root** | Trusted admitted unit of execution, host-held identity, authority and selected binding snapshot |
| **Host import** | Plugin request to perform an operation through the scoped, kernel-enforced host boundary |
| **Generation** | Immutable selected artifacts, bindings, deployment policy and admission state (M0.3 semantics) |
| **Candidate** | Validated prospective generation not selected as the default |
| **Physical lease** | Host ownership that prevents code or callback resources from being unloaded while they can execute |
| **Control plane** | Trusted installation, selection, admission, promotion and inspection functions |
| **Plugin plane** | Narrow, root-scoped invocation functions made available to plugin code |

## 2. Decision and hard scope boundary

**Phenix SHALL start as a domain-neutral typed-service microkernel**, with a procedural-macro Rust SDK, independently loadable native plugins, and Nix as the canonical build, packaging, development and deployment layer. The first acceptance artifact is a real pair of externally authored non-AI plugins, not an agent harness and not a static in-process mock.

The kernel MUST ultimately own one canonical mechanism for: selected provider resolution; host-mediated invocation; operational authority attenuation; root/task ownership; and plugin/generation lifecycle. Modules MAY be reorganized for readability, but no additional registry, runtime authority root, provider resolver, or competing root executor MAY be introduced.

M0 MUST NOT implement AI agents, model routing, tools, skills, prompt orchestration, context/memory strategies, a declarative workflow IR, graph patches, user-extensible contribution kinds, general durable storage, guest-language VMs, ACP clients or an event bus. The kernel MUST NOT interpret Nix expressions or choose providers based on Nix package names.

The project is pre-stability. The clean-start implementation MUST NOT include compatibility readers, historical wire aliases, legacy factories, shadow dispatch paths or adapters solely to support the old Phenix implementation. Existing algorithms and tests MAY be selectively ported only after proving their necessity within the new contracts.

### 2.1 Kernel minimality principle

An ordinary plugin MUST be able to export and import a typed service **without** any workflow, framework, AI plugin, contribution-kind provider, event listener, Layer or durable database. Kernel facilities MUST exist to support an independently demonstrated generic capability, not to anticipate an eventual AI product.

A domain-specific scheduler or algorithm inside a plugin is permitted so long as it cannot bypass kernel-owned host effects, root ownership, cancellation or executable-lifetime rules. A second kernel execution scheduler is forbidden.

## 3. Ownership and dependencies — ALL

| Boundary | MUST own | MUST NOT own |
| --- | --- | --- |
| Portable contract crate | Typed value model, stable IDs, schemas, descriptor/error encoding | Provider selection, live state |
| Rust SDK and proc macros | Author annotations, type validation, canonical static contributions, ABI glue | Runtime selection, host admission policy |
| Kernel | One resolver, scoped invocation, authority enforcement, root identity and lifecycle | Domain logic, Nix semantics, plugin-specific hardcoding |
| Intrinsic native loader | Offline verification boundary, dynamic loading, ABI negotiation, physical image ownership | Provider resolution, policy grants, guest-language semantics |
| Plugins | Interface implementations and private state; declared imports/exports | Global factory registry, authority self-grant, kernel task/lease bookkeeping |
| Nix | Hermetic builds, artifact paths, package records, deployments, checks and CI tools | Alternate descriptors, runtime resolver or implicit permissions |
| Conformance fixtures | Independent external-facing proof of positive and negative behavior | Permanent kernel special cases |

An intrinsic native loader is permitted because the first native executable cannot be loaded by a plugin that has not yet been loaded. Lua/Wasm/JS **guest runtimes** are later ordinary plugins, not intrinsic loader modes.

At M0.0, the dependency direction SHOULD be `contract <- sdk/macros <- external plugins`, with `kernel -> contract + native loader`. The Nix installation supplies concrete compiled plugins as artifacts. A plugin MUST compile without depending on private `phenix-core` or loader crates.

## 4. Normative milestones and contract freezes

**Every milestone MUST leave a working, reproducible, executable deployment.** No milestone MAY justify a temporary second resolver, executor or manifest. Existing features MUST remain valid after adding the next milestone unless deliberately superseded through a change to this RFC.

| Gate | First required behavior | Contract sections frozen before coding | Exit evidence |
| --- | --- | --- | --- |
| **P0: descriptor feasibility** | Offline metadata verification works for real optimized native file | §10 extraction/provenance | Proof under actual release/LTO/strip configurations, or documented single-source fallback |
| **M0.0: synchronous vertical slice** | Two separately built native plugins: consumer imports provider, Nix packages both, trusted CLI invokes consumer | §§8–16 M0.0 profiles | Correct response; missing/ambiguous binding, schema, denied operation, invalid descriptor, ABI rejection; machine-readable inspect |
| **M0.1: composition** | Multiple candidates, optional imports, substitutions/exclusions, nested attenuation | §§11, 13 M0.1 addenda | Deterministic resolution independent of enumeration order; denied effects never execute |
| **M0.2: async and lifecycle** | Pending calls, wake/poll, cancellation, callback leases, failure-atomic start/stop | §§15, 17–18 M0.2 | Exactly-once settlement, late callback rejection, native residency under cancellation, rollback/leak tests |
| **M0.3: generations** | Prepare candidate, trial, promote/rollback, pin roots, retire safely | §19 M0.3 digest and selection | A/B revisions concurrently resident; immutable old roots; failed promotion leaves prior default intact |
| **M0.4: shipping conformance** | Reproducible full Nix closure and public SDK product tests | Entire M0 contract | One pinned integrated run; no handwritten manifests, hidden registrations or AI dependencies |

**M0.0 already includes a *minimal* resolver and *minimal* authority enforcement**: it MUST have an explicit binding for the consumer's provider import, one authorized nested operation and a corresponding denial case. M0.1 extends rather than introduces these concepts.

No full generation digest is required for M0.0: the admitted process MAY use one frozen binding snapshot. A root MUST NOT dynamically rediscover a provider during invocation. M0.0's one active snapshot becomes the initial instance of the M0.3 generation model; no separate execution path may be introduced during that transition.

### 4.1 What is frozen, and when

An M0.0 contract MUST be defined *concretely* before its implementing PR begins. Future contracts MUST be defined immediately before their milestone, not seven speculative standalone RFCs in advance. Once exercised by external fixtures, a published wire/ABI profile MUST remain stable for that milestone's test artifacts; incompatible changes MUST bump their version rather than silently reinterpreting existing bytes.

## 5. The first executable proof — M0.0

Build **two actual native shared libraries**, from a separate example workspace with its own Cargo manifest and lockfile, using only the published `phenix-contract` and `phenix-sdk` APIs:

1. `example.provider` exports `example.transform@1.apply`: a typed request containing UTF-8 text yields a deterministic transformed text result.
2. `example.consumer` exports `example.consumer@1.run` and declares a required import of `example.transform@1.apply`. The consumer invokes that import through a **scoped host API** and returns the provider result.
3. The trusted CLI selects `example.consumer@1.run` as the root entry and binds `example.transform@1.apply` explicitly to `example.provider`.
4. Nix independently builds the kernel and both plugin artifacts, verifies metadata without `dlopen`, packages them and starts the kernel with an explicit deployment document.
5. The invocation succeeds **without kernel source changes, handwritten registration, a workspace-only factory, product-specific code or AI dependencies**.

The fixture MUST also test: absent required provider; two eligible providers with no explicit disambiguation; schema mismatch; a denied nested operation with an effect counter unchanged; descriptor/artifact tampering; and invalid ABI requirements. Missing/ambiguous imports MUST fail preparation before calling the consumer.

The fixture MUST exercise two ABI directions: trusted host -> consumer export and consumer -> kernel scoped import -> selected provider export. A host that invokes only `example.provider` does not satisfy M0.0.

## 6. Public authoring experience — M0.0

The Rust SDK MUST offer an ergonomic typed annotation path with generated ABI bindings and contributions. An illustration of the **intended semantics**, not yet final macro spelling:

```rust
#[phenix::contract(id = "example.transform@1")]
pub trait Transform {
    fn apply(&self, request: TransformRequest) -> Result<TransformResult, TransformError>;
}

#[phenix::plugin(id = "example.provider", version = "1.0.0")]
pub struct Provider;

#[phenix::provide(Transform)]
impl Transform for Provider {
    fn apply(&self, request: TransformRequest) -> Result<TransformResult, TransformError> {
        /* domain behavior */
    }
}

#[phenix::plugin(id = "example.consumer", version = "1.0.0")]
#[phenix::require(Transform)]
pub struct Consumer;
```

The SDK MUST generate: validated contract and operation identities; typed import/export stubs; ABI entry glue; a canonical descriptor representation; and a defined error translation. Plugin authors MUST NOT write a parallel JSON manifest, manual C vtable, global factory registration, or hidden string dispatch table.

The M0.0 vocabulary MAY be deliberately smaller than the final SDK. It MUST support one plugin per native artifact, synchronous `fn` exports, typed structs of supported portable fields, declared domain errors, required imports and exports. `async fn`, optional imports, Layers, arbitrary contribution kinds and advanced lifecycle annotations are later features.

Macros MUST be deterministic for equivalent source/configuration. Macro output MUST NOT depend on filesystem enumeration, timestamps, random values, unordered hash-map iteration, ambient network state or changing host machine identity. Identical declarations compiled under equivalent supported settings MUST emit byte-identical canonical descriptor data. The conformance suite MUST compare generated bytes across independent clean builds and changed discovery ordering.

A documented **manual descriptor-lowering API** MAY exist for unsupported Rust syntax, but MUST produce exactly the same validated canonical representation; it MUST NOT be a second registration mechanism.

## 7. Contributions and schema — M0.0

The initial immutable contribution roles are `Declare`, `Require` and `Provide`. An inspectable descriptive `Observe` role MAY be added when demonstrated; `Modify` and extensible contribution-kind languages are **DEFERRED**.

Each contribution MUST have a stable contribution ID, owning plugin ID, role, versioned kind, typed payload and provenance. A contribution MUST NOT mutate the active graph or invoke its author during resolution. Duplicate identity, conflicting owner, incompatible contract revision, missing required import, or unrecognized required metadata MUST fail preparation.

A service contract identifies an operation by a stable versioned interface ID plus operation ID. Compatibility MUST be checked structurally and by declared major revision; a matching string alone MUST NOT bypass schema comparison. The initial schema vocabulary needs only `Bool`, `I64`, `U64`, `String`, `Bytes`, named-field `Struct` and declared variants for results/errors. More expressive schemas are **DEFERRED**.

Canonical contributions are a **single source of truth**. Compiler-generated metadata, binary extraction output and package records are representations of the same declarations; no representation MAY silently add or change an export or import.

## 8. Portable value and wire profile — M0.0 FROZEN BEFORE IMPLEMENTATION

The first service wire profile is `phenix.value.v1` and is intended as a deliberately small, target-independent encoding. It MUST be implemented identically by the SDK, loader and kernel. The following table defines its byte-level tags (all multi-byte integers are **big-endian**):

| Tag | Value | Bytes after tag |
| --- | --- | --- |
| `0x00` | Null | None |
| `0x01` | False | None |
| `0x02` | True | None |
| `0x03` | Signed 64-bit integer | 8-byte two's-complement value |
| `0x04` | Unsigned 64-bit integer | 8-byte integer |
| `0x05` | UTF-8 text | 4-byte unsigned length, then exact UTF-8 bytes |
| `0x06` | Opaque bytes | 4-byte unsigned length, then exact bytes |
| `0x07` | List | 4-byte element count, then encoded values |
| `0x08` | Map with text keys | 4-byte pair count; each pair is 4-byte UTF-8 key length, key bytes, encoded value |

The map keys MUST be unique and strictly ascending by unsigned UTF-8 byte ordering; strings are not Unicode-normalized implicitly. Lists preserve order. Floating-point values, raw pointers, native handles, environment references and authority capabilities are unsupported in v1. Null is legal only where the associated schema permits it. A decoder MUST reject unknown tags, invalid UTF-8, duplicate/out-of-order map keys, truncated or trailing bytes, integer/length overflow and schema mismatch. A value occupies **exactly one** complete message payload; arbitrary concatenated values are invalid.

For M0.0 conformance, the accepted value payload limit is **1 MiB**, the maximum nested depth is **32**, and the maximum count in any individual list/map is **65,536**. These are explicit decoder safety limits, not default execution timeouts. Deployment MAY enforce stricter limits. Raising them in a later compatible implementation MUST remain safe for a decoder that advertises smaller supported limits. A future larger-data contract MAY introduce authorized data references separately; capability handles MUST NEVER become ordinary portable values.

Typed Rust schemas MUST be lowered deterministically to this wire vocabulary, with stable field/variant names, versions and explicit missing/unknown-field handling. The descriptor schema itself SHALL use a versioned, deterministic encoding; it MAY use `phenix.value.v1` maps/lists provided that its complete field layout is frozen in the M0.0 implementation contract before authoring code is merged.

## 9. Descriptor identity and offline extraction — P0, M0.0

### 9.1 Descriptor fields

The canonical descriptor profile is `phenix.descriptor.v1`. It MUST contain at least:

- `format`, `plugin_id`, `plugin_version`, `required_native_abi_major`, `required_native_abi_minor`, required feature bits;
- canonical sorted contract definitions and versions;
- canonical sorted `Require` and `Provide` contributions, including operation schemas and plugin ownership;
- minimum declared policy/authority ceiling and supported execution mode;
- a descriptor schema version and canonical payload bytes.

The **descriptor digest** is `SHA-256` of the exact canonical descriptor bytes. It MUST NOT include a hash of the final executable that embeds it. This avoids self-referential artifact hashing.

### 9.2 Extraction mechanism: prove-first decision

Before substantial kernel implementation, **P0 MUST prove a reliable offline extraction mechanism on the supported Linux target**. The preferred candidate is a macro-generated static placed in a designated ELF section, extracted by `phenix-describe` **without** `dlopen` or calling code. Because `#[used]` alone does not guarantee linker retention, the proof MUST cover optimized builds, LTO settings selected for the product, linker garbage collection, stripping, different supported linker versions and independent external plugin builds.

If ELF retention/extraction is unreliable or excessively fragile, the implementation MUST switch to a **sidecar produced from the same canonical macro-generated representation in one reproducible build pipeline**, with no manual manifest and no second parser of source semantics. The fallback MUST demonstrate how the descriptor bytes were generated and bound to the exact compiled artifact; merely writing matching user-supplied hashes into two files is not sufficient provenance proof.

Whichever mechanism passes P0 MUST be selected as the **single M0.0 production path**; the other MUST NOT remain as a permanent automatic fallback unless it is explicitly standardized and independently justified. The choice, relevant target toolchain versions, extraction tests and failure conditions MUST be recorded in the P0 acceptance evidence.

A Cargo `build.rs` MUST NOT be assumed capable of extracting its own package's macro-expanded declarations: it executes before compiling that package. A sidecar generator requiring a pre-compilation source generation step MUST make that step explicit and still preserve one canonical authoring input.

### 9.3 Package record and artifact verification

The M0.0 `phenix.package.v1` record MUST bind at least the relative loadable artifact path, descriptor path, target ABI/triple, `artifact_sha256`, and `descriptor_sha256`. For M0.0, **artifact digest means SHA-256 over every byte of the exact final loadable `.so` file**, after all packaging-affecting transformations. It does not mean the descriptor digest, Nix store-path hash, NAR hash or generation identity. A rebuild MAY produce a different artifact digest even when logical declarations are unchanged.

Nix MUST verify the extracted/generated descriptor digest and artifact digest when assembling the package. The kernel MUST validate the same package record and exact artifact bytes during preparation before loading. It MUST reject missing files, modified content, malformed paths, incompatible ABI metadata, unsupported descriptor versions and provenance mismatches. Deployment MUST use trusted immutable Nix store inputs (or another explicitly trusted read-only artifact source); merely matching hashes does **not** establish authorship or make native code safe.

**All descriptor extraction, bounds checking, package-policy checks and file hashing MUST occur before `dlopen`.** Linux `dlopen` may run native constructors before returning; no contract may claim that an ABI entrypoint validation performed after `dlopen` protects against pre-load arbitrary native execution. Native artifacts are trusted executable process code, and source/package trust is a separate deployment decision.

The kernel MAY validate an ABI table after trusted loading and before normal plugin entry calls. It MUST distinguish metadata verification from executable ABI negotiation in inspection and errors.

## 10. Nix deployment and package products — M0.0

Nix MUST be the authoritative **build/deployment system**, not a second semantic runtime resolver. Nix SHALL build the kernel, a public SDK distribution, external fixtures, descriptor tooling and a runnable deployment closure. Nix MAY select which packages are installed, but logical imports, provider binding and host grants MUST remain explicit kernel-consumed deployment data.

At minimum the flake MUST expose runnable derivations equivalent to:

- `packages.<system>.phenix-kernel`;
- `packages.<system>.phenix-describe`;
- `packages.<system>.phenix-m0-plugins` (provider and consumer as distinct artifacts);
- `packages.<system>.phenix-m0-conformance`;
- `checks.<system>.m0-descriptor`, `.m0-contract`, `.m0-native`, `.m0-product`;
- `devShells.<system>.default`.

Concrete output names MAY be finalized in the first implementation PR, but their separation and reproducible testability are REQUIRED. Native artifacts SHOULD be independently reusable between kernel rebuilds. One root flake and Cargo dependency graphs SHOULD remain the source of build dependency truth; Nix package metadata MUST NOT independently enumerate logical exports/imports. Formatting, static checking and CI runners SHOULD be Nix-declared tools rather than bespoke unmanaged `scripts/` paths.

The independent plugin example MUST be buildable in a separate Cargo build unit with its own lockfile, and consume only published contract/SDK APIs. CI MUST prove the executable works in the Nix-built closure; testing a plugin as a kernel workspace module is insufficient.

A deployment document SHOULD be versioned canonical data, for example:

```json
{
  "format": "phenix.deployment.v1",
  "artifacts": ["/nix/store/...-provider", "/nix/store/...-consumer"],
  "entry": {"interface": "example.consumer@1", "operation": "run", "provider": "example.consumer"},
  "bindings": [
    {"interface": "example.transform@1", "operation": "apply", "provider": "example.provider"}
  ],
  "grants": [
    {"caller": "root", "interface": "example.consumer@1", "operation": "run"},
    {"caller": "example.consumer", "interface": "example.transform@1", "operation": "apply"}
  ]
}
```

The example Nix store paths are placeholders, not literal required paths. The exact JSON field set MUST be frozen as `phenix.deployment.v1` before the first deployed fixture is accepted; unknown policy-relevant fields MUST reject, never silently grant authority. No omitted field MAY imply a default provider or grant.

## 11. Resolution and selection — M0.0, M0.1

### 11.1 M0.0 explicit selection

The kernel MUST parse package records, validate descriptors offline, enumerate available exports/imports and create **one immutable binding snapshot**. Every required import MUST resolve to exactly one compatible selected provider. An explicit binding uses `(interface_id, operation_id) -> provider_plugin_id`, with plugin identity and descriptor checked against the selected artifact. The root entry is explicitly selected with the same identity vocabulary.

Zero providers for a required import MUST produce `MissingBinding`. Multiple eligible providers **without an explicit selection** MUST produce `AmbiguousBinding`. An explicit selection of an absent/incompatible provider MUST fail rather than selecting the only available alternative. No provider preference based on discovery order, package name, priority heuristic or previous process state is allowed.

No plugin author callback may be used during resolution. The selected binding map MUST be frozen before invoking any plugin; nested host imports MUST use this same prepared map, not run an ad hoc dynamic resolver. Static startup and later in-process candidate installation MUST converge on the same preparation/resolve/dispatch functions, even though candidate installation does not exist until M0.3.

### 11.2 M0.1 additional policy

M0.1 adds required-versus-optional imports, exact implementation requirements, explicit exclusions and substitutions, and multiple independently selected implementations of a compatible interface. Exclusions MUST run before required dependency closure; excluded defaults MUST NOT be silently reinstated. Required preparation dependency cycles MUST fail with an inspectable path. Reversing artifact discovery order MUST preserve selected bindings and errors.

## 12. Authority and security — M0.0, M0.1

### 12.1 M0.0 grants

The trusted deployment/admission configuration MUST explicitly grant named `(caller, interface, operation)` operations; all other host effects are denied. At M0.0, `caller` is either the trusted root-entry principal or an authenticated selected plugin identity. The kernel MUST derive caller identity from the current host call scope, **never** from untrusted request data. The selected provider's declared ceiling is an additional upper bound, not a permission source.

A child/import call's effective grant MUST be at most the intersection of trusted root ceiling, current caller grant, deployment selection policy, provider ceiling and applicable environment restrictions. M0.0 MAY use only the empty or explicitly named environment resource scope; it MUST NOT accidentally grant process credentials, arbitrary filesystem or network access as a result of host policy validation.

M0.0 MUST test authorized `example.consumer -> example.provider` invocation and a denied operation against a protected counter. The denied call MUST return `Denied` **before** invoking the protected provider; the counter MUST remain unchanged. A plugin MUST NOT be able to create a new unrestricted root, forge an import caller or override the selected policy by supplying new JSON grant objects.

### 12.2 M0.1 delegated scopes

M0.1 introduces typed scope restrictions (such as a named workspace/resource) and explicit attenuation for nested calls. Policies MUST be applied at the **point of effect**, not only while validating contributions. Passing an opaque capability through portable value encoding is forbidden. If capability handles become necessary, they MUST remain host-side nonserializable references, scoped to the admitted root/call and bound to actual selected owner identity.

**Threat model:** An in-process native plugin is *trusted executable code*. Kernel-host authority mediation constrains properly cooperating plugin API calls; it does **not** sandbox malicious native libraries from process memory, system calls, other plugin memory, or secrets. Process/OS isolation is separate and **DEFERRED**. Signed packages or trusted Nix inputs establish deployment trust; hashes alone establish consistency, not authorship.

## 13. Control plane, plugin host and inspection — M0.0

### 13.1 Trusted control-plane API

The public conceptual API is:

```rust
prepare_deployment(verified_packages, explicit_selection, trusted_policy) -> PreparedSnapshot
admit_root(prepared_snapshot, entry_operation, trusted_environment) -> RootHandle
invoke(root, typed_request) -> Result<DeclaredOutcome, KernelFailure>
inspect() -> InspectionSnapshot
```

These signatures are semantic contracts, not stable Rust trait signatures. The control plane MUST authenticate the caller that supplies the deployment policy and entry selection. A `RootHandle` MUST NOT be represented as an arbitrary caller-constructible ID or exposed to ordinary plugin code with a configurable authority ceiling.

For M0.0, the trusted CLI MAY be the only control-plane client. The process MAY admit a single root serially. Generic install/trial/promote functions are **M0.3**, not M0.0 requirements.

### 13.2 Scoped plugin-plane host API

The plugin's generated import binding MUST call an equivalent of:

```rust
invoke_import(current_call_scope, declared_import, typed_request)
    -> Result<DeclaredOutcome, KernelFailure>
```

`current_call_scope` MUST be issued and validated by the host; the plugin cannot construct a larger scope or select another root. During a nested invocation, the kernel MUST check: authenticated current selected plugin; declared import; compatible frozen binding; request schema; effective grant; and cancellation state when that concept becomes available. Then it invokes the selected provider through the *same* central dispatcher.

### 13.3 Inspection schema

M0.0 MUST emit **machine-readable, versioned JSON** (`phenix.inspect.v1`), not a debug-only printout. It MUST include: selected descriptor and artifact digests, installed plugin IDs, root-entry identity, frozen provider mappings, root/call IDs or terminal summaries, last typed failure class and denied-operation counters. It MAY omit live task and lease fields until those features exist. M0.2 adds ticket/cancellation/lease summaries; M0.3 adds candidate/default generation identities, pin counts and retirement blockers.

Inspection MUST have a stable, deterministic ordering for unordered collections, and MUST NOT reveal credentials, private host handles, arbitrary plugin memory, raw capability tokens or unredacted request payloads. The schema MUST be supported by parseable golden fixtures and backwards-compatible versioning rules.

## 14. Immediate native ABI — M0.0 FROZEN BEFORE IMPLEMENTATION

Native plugins MUST use a versioned C-compatible ABI, not Rust `dyn Trait`, Rust struct layout or Rust allocator compatibility across separately compiled artifacts. The M0.0 ABI MUST have:

- an explicit exported entrypoint identity and table header containing major version, minor version, table byte length and required feature bits;
- statically known functions for creating an instance, invoking a declared operation synchronously, stopping an instance and releasing owned buffers/resources;
- an opaque, host-supplied per-call context for nested host imports; an import must be correlated to the same pinned root and authenticated plugin owner;
- length-bearing borrowed input bytes and result buffers with exactly specified producing-side release responsibility;
- explicit success, declared domain error and kernel/ABI failure representation; incompatible ABI requirements reject before normal invocation;
- no unwinding across `extern "C"` boundaries, no Rust-allocator ownership transfer between independently compiled libraries and no TLS-derived authorization.

The final C header and Rust `repr(C)` definitions MUST fix integer widths, alignment, null/zero-length buffer rules, table validation order, pointer validity, error/result ownership and callback lifetime before M0.0's native loader PR is merged. Header major/minor and table length MUST be validated before reading or calling unadvertised ABI entries. Pre-load descriptor verification and **post-load** ABI pointer/table validation are distinct phases; metadata checks do not eliminate constructors executed by trusted native code during `dlopen`.

### 14.1 M0.0 threading and reentrancy

The host MUST invoke a given synchronous plugin call on one identified host admission thread. The plugin MAY make a **synchronous nested host import on that same thread** through the issued call scope. Such an import may enter a *different* plugin selected by the resolver and MUST return before the caller resumes. Reentrant invocation of the **same stateful plugin instance**, plugin-created thread callbacks, host imports from unregistered threads, detached callbacks, and arbitrary cross-thread reuse of a host context MUST be rejected in M0.0. These capabilities are reserved for versioned M0.2 tickets/permits and explicit reentrancy policy.

Blocking during a synchronous M0.0 call may block that host thread; the CLI does not promise concurrent throughput. Nested call cycles MUST reject rather than deadlock. No implicit event loop, wake/poll scheduler or callback lease registry is required for this stage.

### 14.2 Process-fatal events

Plugin code MAY return typed recoverable failures. Rust unwinding panics MUST be prevented from crossing FFI and translated to `PluginFailed` only where catchable and safe. `panic=abort`, `abort()`, `_exit()`, `SIGSEGV`, process termination and other process-fatal behavior **terminate or compromise the process**. M0.0 MUST NOT promise cleanup, stop callbacks, root settlement or rollback after such events. In-process native plugins are not fault-isolated; any stronger recovery requires an isolation boundary not present in M0.

## 15. Error taxonomy — M0.0

The kernel MUST distinguish preparation errors, domain-declared results/errors and host invocation failures. At minimum, machine-readable error codes MUST include:

| Code | Classification / required behavior |
| --- | --- |
| `DescriptorInvalid` | Malformed/offline descriptor, invalid ownership, provenance or digest |
| `ArtifactInvalid` | Missing/changed loadable file, untrusted source or incompatible artifact |
| `AbiIncompatible` | Unsupported ABI, required feature, invalid table or entrypoint |
| `MissingBinding` | Required compatible provider absent |
| `AmbiguousBinding` | Multiple eligible providers not explicitly disambiguated |
| `SchemaRejected` | Invalid request/result shape or incompatible schema |
| `Denied` | No effective grant; protected operation MUST NOT execute |
| `PluginFailed` | Provider-returned host failure or caught recoverable plugin boundary failure |
| `InfrastructureFailure` | Host or packaging/runtime infrastructure could not fulfill operation |
| `Cancelled` | **RESERVED until M0.2**; terminal cancellation state, not a domain variant |

The result envelope MUST preserve a distinction among `Completed(DeclaredResult)`, `DeclaredError(DeclaredDomainError)` and `Failed(KernelFailure{code, phase, context})`. From M0.2 it also includes `Cancelled`. Declared domain errors MUST NOT be converted to host errors simply because they use a Rust `Result`; the contract annotation defines the mapping.

A kernel failure MUST have a stable code, phase and redacted context suitable for inspection. Unknown codes/versions MUST reject or be surfaced as typed incompatible errors; they MUST NOT silently become `Completed`. M0.0 negative tests MUST assert actual error code **and** whether protected provider code executed.

## 16. M0.0 startup and lifecycle

For the first synchronous deployment, preparation MUST verify packages, decode descriptors, validate selections and policy, prepare one frozen binding snapshot, load verified trusted native artifacts, establish plugin instances and activate the selected entry. On failure, preparation MUST NOT admit a partially bound root.

The canonical lifecycle vocabulary is `Discovered -> Verified -> Prepared -> Started -> Active -> Draining -> Stopped -> Released`. The implementation MAY collapse unobservable transient phases in M0.0, but MUST NOT conflate verification with executing artifact code or permit activation before required bindings are valid. Every failed phase MUST produce `Failed { phase, reason }` or an equivalent typed outcome identifying resource ownership and rollback action; it need not add one enum state per possible error.

M0.0 MUST release all **actually acquired host-owned resources** on recoverable failed startup and must not call an uninitialized provider. Stop/release MUST be idempotent or explicitly reject double invocation without double free. M0.2 defines reverse-dependency stop ordering, pending callbacks and full draining. M0.3 makes activation/promotion failure atomic with respect to an existing selected generation.

Native process-fatal termination remains outside cleanup guarantees (§14.2).

## 17. Pending calls, cancellation and leases — M0.2

M0.2 MUST extend the existing selected dispatch/host model; it MUST NOT introduce a second scheduler or provider resolver. The ABI extension MUST define the following ticket state machine with a versioned set of operations:

`Created -> Running -> Pending -> {Ready, Failed, Cancelled} -> PhysicallyQuiescent -> Released`.

The logical terminal state (`Ready`, `Failed` or `Cancelled`) MUST be selected **exactly once**. Repeated settlement, wrong ticket/correlation, invalid owner, or wake after release MUST be rejected without invoking a protected effect. Wake means *request host polling*, not delivering a second result. Cancellation is a request and may logically settle while native code remains physically active; cancellation MUST NOT be interpreted as proof of unload safety.

The extension MUST specify which callbacks can run on which threads, whether/when host imports can reenter, how threads register under a scoped permit, how wake/poll traffic is serialized, ownership of buffers, and how pending operations hold physical executable leases. A stale or late callback MUST never dereference a retired host context or expand authority. `drop` of a caller future/channel is not sufficient proof that native work has ended.

A plugin that does not cooperate with cancellation MAY prevent safe unload indefinitely. The kernel MUST report the blocking ticket and physical lease rather than force `dlclose` unsafely. Only an independently isolated runtime can promise forced termination. M0.2 conformance MUST include double-settlement, wrong-owner, delayed callback, cancel-race and leak tests.

## 18. Generation selection, identity and retirement — M0.3

M0.3 MUST define an immutable generation formed from the exact selected descriptor digests, artifact digests, canonical binding map, authority/policy snapshot, compatible ABI and relevant environment **identity** (not secret values). The exact canonical tuple encoding and domain-separated hash computation MUST be frozen and regression-tested **before implementing M0.3**, not speculated into M0.0.

The generation identity MUST differ when any selected executable artifact bytes affecting dispatch change, even if the descriptor's declarations are the same. Nix store-path names MUST NOT serve as generation identities. Identity equality is not a claim of source authenticity.

The control-plane extension introduces conceptual operations:

```rust
install_candidate(verified_package_set, policy) -> CandidateId
inspect_candidate(candidate) -> CandidateInspection
admit_root_on(candidate_or_resident_generation, entry, environment) -> RootHandle
promote(candidate) -> Result<GenerationId, PromotionFailure>
retire(generation) -> Result<RetirementStatus, KernelFailure>
```

Candidate preparation MUST use exactly the same descriptor validation, selection, authority and invocation machinery as ordinary startup. Explicit trial and promotion MUST be separate operations; neither Nix rebuild nor artifact discovery silently promotes a generation. Promotion MUST atomically change the default for **new roots only**. In-flight roots MUST continue using the original pinned generation; callbacks and code mappings remain resident while physically live. Failed candidate start/promotion MUST NOT corrupt the previous default. Rollback is selection of an eligible prior immutable resident generation, not mutation of an existing one.

Inspection MUST report default/candidate identities, exact selected artifacts and bindings, in-flight root/call counts, outstanding physical leases and retirement blockers without exposing capability tokens or secret material.

## 19. Non-AI conformance suite and release evidence

Conformance MUST execute actual compiled artifacts; mocked providers alone are insufficient. Fixtures remain ordinary externally authorable plugins.

| Gate | Required fixture | Positive proof | Required negative proof |
| --- | --- | --- | --- |
| P0 | `example.descriptor` | Offline extraction on release/LTO/stripped artifact and exact digest | Lost/modified section, mismatched descriptor, malformed ELF/sidecar |
| M0.0 | `example.consumer` + `example.provider` | Host -> consumer -> provider typed request/result across two native artifacts | Missing/ambiguous binding, malformed wire/schema, denied effect, bad ABI, tampering |
| M0.1 | Two interchangeable providers and a restricted client | Explicit selection, optional imports, scoped attenuation | Exclusion closure, unauthorized nested call, order-dependent resolution |
| M0.2 | `example.pending` + `example.lifecycle` | Pending completion, cooperative cancel, rollback/stop | Wrong ticket, double settlement, late callback, leaked physical lease |
| M0.3 | `example.revision-a` / `revision-b` | Concurrently resident revisions, pinned calls and explicit promotion | Failed candidate, stale callback, premature unload, invalid policy |
| M0.4 | Independent SDK workspace and reproducible Nix closure | All checks on one pinned artifact set, no AI dependencies | Hidden factory/manifest, private-core dependency, skipped real package tests |

Required supporting tests: proc-macro compile-fail cases; duplicate contribution IDs; identical canonical descriptor bytes across independent clean builds; decode overflow/duplicate-map rejection; binding determinism under artifact order permutation; authority denial without side effects; failure-atomic rollback; ticket settlement properties; and leak/retirement tests when those features exist.

M0.0 inspection MUST be parsed as JSON and compared on semantic fields, not free-form log strings. Tests MUST record the kernel commit, SDK/contract versions, native artifact digests, descriptor digests, Nix derivation outputs and the exact CI run. Green reduced CI is not sufficient to certify a milestone with skipped required packages.

## 20. Complexity gate and PR review contract — ALL

Every new intrinsic or central abstraction MUST accompany a non-AI executable need and a documented rejected simpler alternative. Every owner PR MUST answer:

| Review question | Acceptance criterion |
| --- | --- |
| Why does this exist? | One named invariant and test fails without it |
| Could an ordinary plugin supply it? | If yes, keep the behavior outside kernel |
| Does it create another resolver or provider lookup? | If yes, reject and consolidate |
| Does it introduce another root scheduler, authority store or generation owner? | If yes, reject and consolidate |
| Does it require a second source of metadata? | If yes, reject or generate it from canonical contributions |
| Must its state persist? | Name the precise lifetime and teardown rule |
| Is a simpler contract sufficient for this milestone? | Implement the simpler contract; defer speculative flexibility |
| Does it grow the kernel while leaving an old mechanism live? | Include the deletion or bounded migration owner and acceptance proof |
| Can an external plugin exercise it? | Require public-SDK/Nix fixture, not just internal mocks |

This table MUST be incorporated into the clean-start repository's PR template. Mechanism count, independent mutable state ownership, duplicate declarations and end-to-end correctness matter more than raw line-count targets. No change MAY be called complete solely because local/unit CI is green.

Each milestone MUST maintain a short acceptance matrix with concrete tests and final artifact identities. A failing test MUST be routed to its owning mechanism; tests MUST NOT be rewritten merely to conceal a real behavioral regression.

## 21. Optional generic facilities — DEFERRED

After M0.4, new RFCs MAY introduce Service Layers, event observation, durable resources, guest-runtime adapters (Lua/Wasm), richer environment/workspace providers, portable kind templates, graph patches, workflow IR and AI-agent plugin packages. None is required by the ordinary typed-service kernel.

A proposed future intrinsic MUST reuse the existing root, provider binding, authority, native host and generation machinery. If a service or plugin can implement the behavior without compromising the kernel's invariants, the new capability MUST live outside Core. New domain-specific concepts MUST NOT be added to a privileged kernel enum merely to make an AI use case easier.

## 22. Reference and implementation notes (non-normative)

Existing Phenix AI is useful for behavioral regression cases and mature algorithms, not as a compatibility baseline:

- [Phenix target kernel RFC](https://github.com/matthis-k/phenix-ai/blob/main/spec/kernel-runtime-rfc.md).
- [Plugin authoring spec](https://github.com/matthis-k/phenix-ai/blob/main/spec/plugin-authoring-macro.md).
- [Native plugin ABI proposal](https://github.com/matthis-k/phenix-ai/blob/main/spec/native-plugin-abi-and-guest-runtimes.md).
- [Selectable generations](https://github.com/matthis-k/phenix-ai/blob/main/spec/selectable-generations.md).
- [Configuration frontends](https://github.com/matthis-k/phenix-ai/blob/main/spec/configuration-frontends.md).
- [Cargo build script execution order](https://doc.rust-lang.org/cargo/reference/build-scripts.html).
- [Rust `#[used]` and `#[link_section]` guarantees](https://doc.rust-lang.org/reference/abi.html).
- [Linux `dlopen(3)` constructor semantics](https://man7.org/linux/man-pages/man3/dlopen.3.html).

**Final acceptance statement:** M0.0 is complete only when two independently built, proc-macro-authored Rust native plugins can be Nix-packaged, verified offline, selected with an explicit single provider binding and scoped operation grant, loaded and invoked through the kernel's one dispatch path—without handwritten registration, AI code, private kernel SDK dependencies or uncontrolled native reentrancy. M0.1–M0.4 extend that proven foundation rather than replace it.
