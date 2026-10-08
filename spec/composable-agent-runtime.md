# Composable agent runtime: kernel, Basic, Full and external integrations

status: proposed
audit_revision: 65c0ea6c8805fadb283719ee831096ae6cf27261
scope:
  - contract ownership and substitution
  - first-party reference agent graphs
  - external harness forwarding and capability use
  - memory and context integration

## Minimal required runtime

**The Phenix kernel is the only mandatory runtime foundation for components composing inside a Phenix graph.** No agent loop, first-party harness, memory, context, model, tool, skill, frontend, protocol bridge or provider-specific SDK service is required by the kernel itself.

The kernel's required responsibilities are generic: declare and resolve component interfaces; select compatible provider exports for imports; enforce effective authority; dispatch calls; manage plugin and Graph Generation lifecycles; and expose the generic runtime facilities needed by the selected components. A capability such as persistence may have a minimal kernel-owned mechanism, but a concrete backend is only required when a selected component uses that capability.

A component may define its own contracts, provide an existing contract, consume other contracts, or adapt contract A into contract B. No component is required to depend on Basic or Full. There must be **no mandatory hidden first-party plugin closure** when a custom graph consists of third-party components.

The contract SDK is a development/interface library, **not an additional mandatory runtime plugin**. A non-Rust or remote implementation only needs to conform to the contract and relevant host/transport protocol.

An external application consuming a Phenix-hosted capability through an authorized gateway does **not** need to embed the kernel itself: the service host runs the Phenix kernel, and the external caller speaks the exposed contract.

Basic and Full are packaged reference selections built *on top* of that substrate, not requirements for running it. The only constraint is that each selected component's **declared required imports** must be satisfiable by the chosen graph.

## Canonical four-tier distribution model

The project distributes **four independent composition tiers**, with only the first mandatory for
a component graph hosted by Phenix:

| Tier | Package type | Purpose | Requires earlier tier at runtime? |
| --- | --- | --- | --- |
| **1. Kernel** | Minimal runtime | Contracts/interfaces, plugin graph resolution, invocation, authority, lifecycle, generic persistence primitives | No |
| **2. Harness contracts** | Standalone contract-definition library, optionally a resource-only discovery/catalog plugin | Common interfaces for agent execution, model/tool calls, session history, memory, context preparation, compaction, expansion, retrieval and maintenance; **no provider implementations or assembled graph** | No executable harness plugin; contract definitions link against kernel interface types |
| **3. Basic agent** | One simple agent-loop provider plus a Basic *reference profile* selecting naive provider plugins | Working first-party conventional agent loop; deterministic compaction and other cheap default mechanisms as needed | Uses kernel and selected contracts; provider plugins independently replaceable |
| **4. Full harness** | Declarative overlay/profile over Basic and independently packaged advanced providers | More capable implementations, optional features, replacements and Layers | Same kernel and contracts; effective provider graph is resolved after overlay application |

**Contract definitions are not themselves executable implementations.** Do not make the common-contract
catalog a compulsory runtime Plugin or process, or make its absence invalidate a graph whose providers
already declare compatible interfaces. In Rust it may be a dedicated `phenix-harness-contracts`
crate (or a cohesive module within `phenix-sdk`). An optional resource-only plugin may publish
descriptors for discovery and documentation; this is metadata, not a separate dependency resolver.

Do **not** confuse `phenix-plugin-basic-agent` (one replaceable implementation of an agent loop)
with `phenix.agent.basic` (a *profile selecting* the basic loop and other independent defaults).
The profile must not be mandatory to reuse any Basic provider. Neither the Basic loop nor Full
gets privileged kernel registration, runtime service identity, or special-case tool admission.

### No predefined graph in the contracts tier

The contracts tier does not prescribe an agent topology. A component author can provide only
`memory.retrieve`, only `context.compact`, a novel `agent.execute` implementation, or
completely different user-defined interfaces. A module needs only its own declared imports,
which may be satisfied by any compatible providers or adapters.

Contract identifiers and semantics are independently versioned. The common catalog is a
**recommended interoperability vocabulary**, not a closed set of required interfaces. Plugins
may define new contracts without waiting for first-party catalog or kernel changes.

### Basic: naive implementations are separate replaceable providers

The Basic **loop** should be intentionally simple: prepare context; invoke model; execute tools;
append history; repeat until termination. The Basic **profile** selects cheap, deterministic
first-party providers only for contracts its default agent requires. Examples:

- Basic context preparation/compaction uses bounded source-preserving trimming/checkpointing
  rather than model-backed summarization.
- Basic memory, if included, is an optional simple durable record/query provider; it is not a
  dependency of the naive loop unless the loop actually needs the corresponding contract.
- Basic tool/model implementations are independent of the loop and can be replaced without
  replacing it.
- Non-applicable optional contracts do not need stub implementations just to complete the profile.

Even when multiple Basic providers ship from one build package, each exports a logically independent
capability and its selection is represented by the graph.

### Full: composition overlay, not a second architecture

`Full = PhenixResolve(Basic defaults + Full additions + explicit replacements/disables + Layers)`

An overlay may replace or remove any inherited provider, including the Basic agent loop,
while retaining other Basic providers. An inherited default is not a hard plugin dependency.
Resolve profile defaults and overrides **before** enforcing concrete provider package closure;
after substitution, only actual selected implementation dependencies and required contract
imports are mandatory. A disabled Basic provider must not be started just because its profile
was an ancestor.

Full may offer state-of-the-art memory, compaction, retrieval, routing, planning, and observability
as first-party choices; no particular model-based algorithm should become kernel semantics.
Layer/decorator behavior is explicit, distinct from replacement, and subject to generation-pinned
one-shot continuation.

### Reusability acceptance fixtures

1. Kernel-only graph runs without loading the contracts catalog, Basic or Full.
2. A third-party plugin declares its own novel contract and runs through the kernel.
3. A standalone contracts consumer selects an arbitrary implementation, including an adapter,
   with no default graph or required agent loop.
4. A Basic loop executes against third-party model, context and tool providers with no Basic
   provider package besides the loop.
5. A third-party loop can reuse just the Basic deterministic compactor or Full memory retrieval.
6. Full retains its loop while completely *removing* native memory/context implementations and
   replacing them through compatible foreign providers.
7. A Full overlay may replace the loop while preserving chosen first-party memory/context.
8. The resolver's inspection reports the effective provider selections, inheritance and overrides,
   and tests prove inactive/disabled defaults have no startup side effects.
9. Common contracts are versioned and provider-neutral, and adapters must fulfill their target
   guarantees rather than relying on schema-only compatibility.

## Distribution and configuration: Phenix semantics, Nix packaging

**A Phenix configuration is portable and self-sufficient as a *declaration* of the desired
graph.** Nix is an optional frontend and deployment mechanism that generates or installs
this configuration, supplies plugin artifacts and launches the runtime. All actual
composition semantics belong to Phenix's existing canonical configuration/resolution
pipeline (`spec/configuration-frontends.md`).

This **supersedes** any earlier proposal to make `mkPhenix` compute provider-override
semantics, expand Basic/Full graphs, decide effective priority or resolve plugin imports.

### One configuration model, many frontends

~~~text
Nix / Home Manager ---------\
Portable TOML / JSON --------+-> ConfigContribution(s) -> Phenix resolver
CLI / TUI / GUI -------------+                           -> ResolvedGeneration
ACP / IPC / agent commands --/                           -> lifecycle/activation
~~~

The same authored configuration should be usable **without Nix**. Nix may generate the
canonical file or contributions, set environment variables and supply plugin package
paths, but it does not need to understand any contract or provider's behavior.

Phenix, not Nix, owns:
- Named profile inheritance (`Basic`, `Full`, custom profiles), defaults and overrides.
- Installable/available versus enabled versus bound/active plugin state.
- Required and optional contract imports, provider selection and adapter chaining.
- Replacement versus ordered Layers, disablement, fallback and resolution errors.
- Authority grants, validation, deterministic generation identity and lifecycle.
- Runtime plugin management, including adding/removing/replacing selected artifacts
  and reconciling affected graph generations.

The physical package manager determines which artifacts are **available**. A runtime
may dynamically acquire additional artifacts through a plugin installer/runtime adapter
where deployment policy permits it. Making an artifact available never implicitly
grants its requested authority or activates it.

### Implemented portable provider-selection frontend (initial subset)

This PR adds **native, Nix-independent provider bindings** to the harness CLI.
They are a narrow part of the eventual portable configuration model, not a
second graph resolver or a declaration that the full profile schema exists.

A UTF-8 JSON file can contain:

~~~json
{
  "bind": {
    "phenix.memory@1": "acme.memory.component"
  },
  "disable": {
    "phenix.context@1": ["phenix.context.native"]
  }
}
~~~

The names above are illustrative component/interface identities; actual providers
must exist in the selected runtime, match their contract schema and pass authority
validation. Unknown document fields and malformed identities are rejected.

Run with a plain local file, without Nix:

~~~shell
phenix --provider-policy ./providers.json
phenix --bind-provider phenix.memory@1=acme.memory.component
phenix --disable-provider phenix.context@1=phenix.context.native
~~~

`--provider-policy=FILE`, `--bind-provider=A=B` and
`--disable-provider=A=B` forms are also supported. CLI bindings apply **after**
bindings from the file; disabled provider entries accumulate. Resolution, conflict
checking and generation identities are owned by `PhenixRuntimeBuilder` and Core.

The legacy `--provider-policy` file intentionally describes **provider
bindings/exclusions only**. For profiles, plugin selection and Layers, use the
portable `--config` document described below. Full profile overlays,
package installation, and interactive plan/apply remain separate follow-ups.

### Implemented portable composition JSON and CLI (initial scope)

This PR also supports one **Nix-independent composition document** via
`phenix --config ./composition.json`, or `PHENIX_CONFIG_FILE` when deployed
through a wrapper. The current supported JSON sections are:

~~~json
{
  "profile": "phenix.product.full",
  "plugins": {
    "enable": ["phenix.debug"],
    "disable": []
  },
  "providers": {
    "bind": {"phenix.memory@1": "acme.memory.component"},
    "disable": {"phenix.context@1": ["phenix.context.native"]}
  },
  "layers": [
    {"service": "phenix.memory@1", "plugin": "phenix.telemetry", "priority": 10}
  ]
}
~~~

The service, plugin, component and interface identifiers in this example are
illustrative; **all referenced runtime providers must actually be packaged and
eligible**, or the resolver reports an error. Optional Layer fields are
`required` (default false) and `enabled` (default true). Unexpected fields
and malformed identities are rejected.

Equivalent basic native CLI choices are available:

~~~shell
phenix --config composition.json --profile phenix.product.basic
phenix --enable-plugin phenix.debug --disable-plugin phenix.planning
phenix --bind-provider phenix.memory@1=acme.memory.component
phenix --disable-provider phenix.context@1=phenix.context.native
~~~

The precedence for currently supported choices is deployment defaults
(`PHENIX_ENABLED_PLUGINS` / `PHENIX_LAYER_POLICY`), then the portable
file, then explicit CLI flags. A selected profile changes the base plugin
selection. File Layer declarations for a given service override the deployment
default list for that service. The existing provider-only
`--provider-policy` file is accepted for backward compatibility and applied
after the unified file, before CLI binding flags.

An explicit provider binding is **strict**, not a preference: if that
component is missing, disabled, structurally incompatible or unauthorized,
graph resolution now reports `UnavailableExplicitProvider` instead of
silently falling back to a different provider.

The Nix `mkPhenix` wrapper accepts `configFile = ./composition.json;` and
forwards the path via `PHENIX_CONFIG_FILE` without parsing or resolving any
provider rules itself. The same file also works through
`phenix --config ./composition.json` on a non-Nix system.

**Not yet implemented:** A fully general profiles/overlays schema (including
replacement of concrete package dependencies), package installation, automatic
standard-location config discovery, unified plugin settings in this file,
interactive `plan/apply` management commands, and exact full-layer CLI editing.
These are follow-up work, not covered by this initial JSON frontend.

### Portable configuration example (illustrative proposed syntax)

~~~toml
# phenix.toml — no Nix syntax or store paths in the semantic profile
profile = "phenix.product.full"

[plugins]
enable = ["acme.memory"]

[providers]
"phenix.memory.retrieve@1" = "acme.memory"
"phenix.context.compact@1" = "phenix.compaction.advanced"

[layers]
"phenix.memory.retrieve@1" = ["phenix.telemetry"]
~~~

The exact profile, provider and Layer syntax is **not yet an implemented file schema**.
The proposal's purpose is one explicit declarative model that a Nix module, a plain
configuration file, the CLI, a remote API and dynamic plugin management can all lower
into. The actual schema should reuse and expose the existing `ConfigContribution`,
`ProviderCompositionPolicy`, `LayerCompositionPolicy` and graph resolver rather than
inventing a parallel configuration format in Nix.

### Nix as package assembler and configuration generator

A Nix wrapper should primarily:
1. Package each plugin as an independently addressable derivation, with validated
   manifest and real executable/embedded artifact as appropriate.
2. Supply the **available** plugin artifacts, resource directories and runtime
   adapters in a reproducible deployment; installed does not imply enabled.
3. Generate or install canonical Phenix configuration (including Basic/Full
   profile selection and user overrides) without interpreting its graph semantics.
4. Wrap the same runtime executable, passing paths/configuration and, optionally,
   host environment/deployment settings.

Illustrative wrapper API (not claimed to exist today):

~~~nix
phenix-custom = mkPhenix {
  inherit pkgs;
  plugins = [
    self.phenixPlugins.${system}.memory
    myPlugins.externalMemory
  ];
  configFile = ./phenix.toml;
};
~~~

The equivalent non-Nix deployment passes the same `phenix.toml` and makes equivalent
plugin artifacts available through ordinary filesystem package directories, plugin
installation commands or a runtime API. A CLI/TUI should be able to express, inspect,
validate, preview and apply **all** operations supported by the Nix frontend. No feature
should be Nix-only because a graph could not otherwise be composed.

A product convenience wrapper such as `phenix-full` should select the Full *Phenix
profile* by emitting its profile identity; it must not carry a second independent
recipe for how Full composes providers.

### Existing code and implementation gap

- `spec/configuration-frontends.md` **already requires** one canonical Phenix
  resolver shared by Nix, Lua, TOML/JSON, IPC, GUI and other frontends. Keep it the
  normative source of configuration semantics.
- `modules/plugin-packaging.nix` defines `mkPhenixPlugin` and `mkPhenix`,
  wrapping existing runtime binaries and forwarding plugin packages, enabled
  IDs, settings and Layer policy via `PHENIX_*` variables. This is a useful
  frontend baseline, but the environment format is fragmented rather than one
  fully portable profile file.
- `modules/package-sets.nix` exports `phenixPlugins` derivations. Embedded
  `mkEmbeddedPluginPackage` currently creates a metadata derivation identifying a
  Rust crate, **not** independently runnable plugin code. The shared harness
  already links those embedded factories through its first-party catalog.
- `spec/plugin-embedded-runtime.md` explicitly prohibits loading Rust dynamic
  libraries via an unstable Rust ABI. Keep separate *packaging* from the actual
  *execution strategy*.
- Core provider and Layer resolver machinery exists, but supported user-facing
  configuration and builder surfaces do not yet expose the entire composition
  policy equally across frontends. Fix that at the Phenix configuration boundary,
  **not** by teaching Nix a special provider-selection algorithm.

### Executable distribution remains an independent choice

**Process-backed guest plugin:** Its package contains the executable, manifest
and resources. Once a suitable plugin runtime adapter is installed and permitted,
Phenix can load/reconcile it from any packaging source without relinking the kernel.

**Embedded Rust plugin:** The implementation is an independently defined Rust
crate and Nix build input, but it must be linked into the chosen product executable
ahead of time. A new embedded implementation requires rebuilding/relinking that
executable. The runtime still decides whether that available factory is enabled
and how its exports are bound. The same Basic/Full/profile file works within
the artifact choices supported by the installed product.

**Resource-only plugin:** A separate package of metadata/data with no executable
factory. It can be installed independently through Nix or another package manager.

In all cases, the kernel's PluginHost, authority, generation pinning and graph
resolution are identical. Nix store closure calculation is a **physical packaging**
operation, not semantic contract resolution. Superset packages may remain installed
but inactive; if an optimized minimal closure is desired, derive a deployment
manifest from a *Phenix-produced resolved plan*, not a Nix reimplementation of
contract semantics.

### Non-Nix and Nix parity acceptance tests

1. Kernel-only with no Basic/Full providers works without Nix.
2. A plain file and Nix-generated file with identical canonical contributions
   produce equivalent resolved provider graphs and authority, given the same
   artifact identities/revisions.
3. CLI/TUI/API can express Full profile selection, provider overrides, adapters,
   Layers, disablement, validation and reconciliation **without Nix**.
4. Adding an external process-backed plugin from an arbitrary local package path
   works with an available runtime adapter and requires no kernel rebuild.
5. Nix can package the same external plugin and select it by generating the same
   configuration, without hardcoding conversion or binding semantics.
6. Basic/Full inheritance and provider replacement are resolved by Phenix, not
   by Nix attribute merging. Disabled Basic defaults may be physically available,
   but must not become activated, bound or perform startup side effects.
7. Unknown contracts, missing required providers, cycles and insufficient authority
   fail with the same typed diagnostics through every configuration frontend.
8. Embedded plugins are available only when linked; independent guest plugin
   packages contain real loadable artifacts; metadata-only packages cannot claim
   to install runnable implementations.
9. Nix-only configuration convenience features lower into canonical contributions;
   no option requires Nix's evaluator to recover missing Phenix runtime semantics.
10. Graph changes through a live API produce generation-pinned reconciliation just
    like a subsequent run from the corresponding declarative configuration.

### Native profile defaults and replaceability (implemented slice)

`phenix-agent-configurations` now declares Basic, Advanced, Basic Product and
Full Product as *profiles* with separately inspectable default plugin lists.
Their resource-only profile identity manifests have **no concrete plugin
dependencies**. Phenix expands profile inheritance at product selection time,
then closes over true implementation dependencies.

- Full inherits Advanced, which inherits Basic, without copying or permanently
  requiring a particular Basic implementation.
- The `phenix.providers` bundle is likewise a reference **default provider set**,
  not an unavoidable dependency on every model vendor. Individual provider
  packages can be disabled and replaced independently.
- CLI `--disable-plugin` or portable `plugins.disable` removes an inherited
  *default* before concrete package selection. The plugin is not started as a
  fallback merely because an ancestor profile included it.
- A selected implementation's genuine hard dependencies remain mandatory.
  Disabling one of those is an explicit resolution error, not a silent override.
- A contract compatible foreign provider can be added after expanding defaults;
  the Full profile does not imply mandatory native memory. The new fixture
  verifies resolution and package omission, not an external memory backend's
  runtime behavior.
- An alternative agent loop must still satisfy consumer contracts and the
  application launch/agent tool wiring is currently coupled to the first-party
  loop by plugin ID. This remains a separate remediation.

The current composition API is intentionally not a second kernel graph solver:
profile expansion only adds *candidate plugin IDs*; actual import/export
compatibility, provider bindings, authority and graph generations are validated
by the canonical Core resolver.

## Implementation status of this PR

- **Implemented:** Shared SDK-owned agent execution contract, with Basic compatibility exports.
- **Implemented:** Core's complete resolver path now accepts explicit provider policy alongside Layer policies, durable schemas, process arguments, entry triggers and contributions.
- **Implemented:** `PhenixRuntimeBuilder::set_provider_policy`, `bind_provider` and `disable_provider` delegate resolution to that canonical path. The builder does not evaluate Nix or implement a separate provider solver.
- **Implemented:** Native `--config` JSON composition frontend (profile, plugin choices, provider bindings and Layer policies), `--profile`, `--provider-policy`, `--bind-provider`, and `--disable-provider`; Nix `configFile` passes the same file path through unchanged.
- **Implemented:** Explicit provider selection fails when the nominated provider is unavailable or ineligible, rather than silently routing to another component.
- **Regression source added:** Default provider selection, explicit binding, provider disablement, semantic generation identity, invalid bindings, Full/Advanced profile default exclusions, native-vs-foreign memory contract substitution (resolver fixture only), and CLI precedence.
- **Implemented in this PR:** Basic/Full reference profile defaults are expanded in Phenix before checking concrete manifest dependencies. Overridden optional defaults are not included as runtime plugins; true implementation dependencies still fail when missing.
- **Not yet implemented:** Full arbitrary plugin replacement through portable package catalogs, complete CLI/UI lifecycle management, unified plugin settings, artifact installation, or end-to-end external adapter substitution.
- **Verification:** The added tests require Rust CI execution; source-level presence alone is not a passing test result.

## Executive decision

The kernel runs a graph of contracts and capability providers. **It does not contain an agent.**
`phenix.agent.basic` is the executable first-party example agent graph, **not** a mandated implementation.
`phenix.agent.advanced` inherits Basic, and `phenix.product.full` selects the in-house Full product.
Full may replace Basic's providers or add new capabilities; it must not duplicate an unrelated agent runtime.

An independent harness can:
- provide `AgentLoopInterface` itself, using Phenix memory/context if desired;
- be invoked through a Phenix terminal provider that forwards to the external harness;
- consume authorized Phenix memory/context capabilities through a protocol adapter,
  without adopting the Phenix loop.

**Forwarding is terminal-provider replacement, not layer/decorator interposition.**
A Layer wraps the remaining service chain using the kernel's one-shot continuation.
Neither forwarding nor replacement grants additional authority.

## Audit of main (2026-10-08)

| Scope | Disposition | Evidence and correction |
| --- | --- | --- |
| Kernel provider resolution | clean | `composition/component.rs`, `composition/provider_resolution.rs`: compatible imports/exports, explicit policy, pinned generations |
| Layer and terminal separation | clean | `spec/plugin-service-layering.md` and runtime dispatch: distinct semantics |
| Basic-to-Full ancestry | remediated in this PR | `phenix-agent-configurations/src/lib.rs` now declares inheritable **profile defaults** instead of hard concrete plugin-manifest dependencies; the Phenix suite builder expands defaults before validating real dependencies. |
| Third-party provider replaceability | clean foundation | `phenix-core/src/third_party_component_regression.rs`, `phenix-plugin-basic-agent/src/component_regression.rs` |
| Agent contract ownership | finding | `AgentLoopInterface` and wire types lived in the Basic implementation crate. Extract into `phenix-sdk` without changing wire identity. |
| Application launch coupling | finding | `phenix-harness/src/application.rs` invokes `phenix.agent-loop@1` using a concrete Basic-era request. Contract types are now SDK-owned, but the application should ultimately use configurable capability bindings rather than hard-coded agent implementations. |
| Harness/plugin wiring | finding | `runtime_builder.rs` conditionally installs application agent-tool adapter and triggers when a specific Basic loop plugin is selected. Derive this from required capabilities, not plugin identity. |
| Composition override surface | partially remediated in this PR | Core supports `ProviderCompositionPolicy`; `PhenixRuntimeBuilder` exposes policy, strict binding, and exclusion. Profile defaults are expanded by Phenix, with explicit exclusion before hard dependency closure; a basic portable JSON/CLI frontend is added. Dynamic package management, full independent components, and capability-based application wiring remain outstanding. |
| Standalone memory | finding | `phenix.memory` has a required helper-invocation import even for storage/query-only use. Separate the helper-dependent mechanisms. |
| Standalone context | finding | `phenix.context` requires Phenix execution/resource providers, preventing simple external turn-preparation usage without those services. Expose a provider-neutral preparation boundary. |
| Basic context compaction | finding | No independent deterministic Basic compaction provider; Full's compaction contract is exported by `phenix.memory`. |
| External capability gateway | finding | Application SDK/callable APIs and process runtime bridges exist, but a standalone authorized memory/context gateway and independent-harness conformance fixture are not demonstrated. |
| Naming | finding | `PhenixRuntimeBuilder::with_basic_suite` is a minimal harness fixture, not the `phenix.agent.basic` agent graph. Document/rename it without breaking consumers unnecessarily. |

This is a source-level architecture audit, not a claim that all integration tests or supported products have run.
The implementation work remains outstanding except for the provider-neutral contract extraction and builder-to-Core provider-policy plumbing included in this PR. Broader product-profile substitution and portable configuration loading are not yet implemented.

## Stable contract template

Put provider-neutral types in `phenix-sdk` (or a dedicated contract crate if it later adds demonstrable value),
never in a Basic or Full implementation crate. Reserve `phenix-core` for kernel-level execution primitives.
Each contract declares:
1. Stable versioned identity and directional request/response schema.
2. Permitted authority and scope semantics.
3. Explicit domain failures versus transport/runtime failures.
4. Cancellation, idempotency and retry expectations.
5. State/identity ownership and references.
6. Provider compatibility and graph-generation pinning.
7. Contract conformance fixtures shared by native, forwarding and external implementations.

Common contracts:
- Agent execution / model invocation / tool execution.
- Session history and exact evidence references.
- Context preparation, compaction and expansion.
- Durable memory store, retrieval and maintenance.
- Optional helper inference, language code facets and evidence resolution.

These are **logical contracts**, not a requirement for one crate or plugin per operation.
Split physical providers only where independent configuration or state ownership warrants it.

## Reference graphs

~~~text
Kernel: graph resolver + authority + persistence + lifecycle + generic dispatch
  (no mandatory agent)

Basic:
  agent.execute -> basic-agent-loop
      -> model.invoke       -> basic/provider-routed model
      -> tool.execute       -> tool executor
      -> context.prepare    -> context provider
          -> context.compact -> deterministic basic compactor
      -> history            -> session event owner

Full = Basic + Extensions + Overrides:
      context.compact       -> memory-backed advanced compactor (replacement)
      memory.store          -> durable memory
      memory.retrieve       -> selective hybrid retrieval
      memory.maintain       -> source-backed extraction/consolidation
      context.expand        -> exact checkpoint expansion
      plus planning, workspace, language, jobs, observability
~~~

A custom graph may omit `agent.execute` and the entire agent loop and expose memory/context alone.
The Basic loop must depend only on contracts. The same consumer code must work when an alternate provider wins.

## Provider composition

Use the existing `ProviderCompositionPolicy` and generation resolver; do not create another resolver.
Support include, add, explicit bind/replace, disable, permitted pre-dispatch fallback, and Layer policy.
Product-specific overrides are explicit; registration order is not semantic.

A required import without an eligible provider fails resolution; an optional import may remain unbound.
Provider fallback is allowed only when both the contract and product policy permit it.
Failure after dispatch or external side effects **does not trigger implicit provider switching**.
In-flight operations remain generation-pinned; new operations resolve through the new graph.

Do not encode dependency on `phenix.agent-loop` as shorthand for availability of tools, session APIs or SDK.
Tool adapter installation should be based on declared capability requirements.

## Bidirectional substitution and contract adapters

**Symmetry is a product requirement:** Full can retain its in-house loop and use a foreign memory or context provider; an independent harness can retain all its own execution/model/tool mechanisms and consume Phenix memory or context. The implementation path must not require either caller to adopt the other's agent loop.

### First-class provider substitution (same contract)

When two providers implement the *same* versioned contract, use ordinary provider selection. No adapter is needed:

~~~text
Full agent -> memory.retrieve@1 -> external-memory-provider
Custom agent -> memory.retrieve@1 -> phenix-memory-provider
~~~

The first graph may omit Phenix's native memory implementation entirely; the second may omit Phenix's agent loop entirely.

**Composition distinction:** Product "include Basic/Full defaults" must not mean each default implementation is a non-removable hard Plugin dependency. An inherited *default provider selection* is not a required concrete runtime dependency. True implementation dependencies remain hard. Product selection should admit a replacement *before* concrete dependency-closure expansion, or have an equivalent explicit exclusion/substitution lowering with valid required imports. Merely selecting another provider while also activating a memory plugin with independent maintenance side effects is not sufficient to claim replacement.

### Different contracts: adapter as ordinary component

If the consumer requires contract A and an available provider implements contract B, use an independently owned *adapter component*:

~~~text
Consumer --requires A--> Adapter --requires B--> Provider B
                         provides A
~~~

Contract A is the adapter's outward *provided* contract; B is its inward *required* contract. A provider may be a local Phenix component, process-backed guest or external endpoint through its own bridge.

The adapter owns typed translation:

~~~text
A.Request
  -> validate caller-visible preconditions
  -> translate into B.Request
  -> invoke a resolved B capability (or a typed external endpoint)
  -> translate B.Response / B.Error into A.Response / A.Error
~~~

It may require several source contracts if the destination's operations genuinely need them. Chaining A -> B -> C is possible through declared imports and exports. Every edge is resolved and pinned by the existing Graph Generation, and required-import cycles fail resolution.

**Example: external memory provider inside Full**

~~~text
Full agent
  -> phenix.memory.retrieve@1 (selected provider)
      -> adapter.phx-from-foreign
           provides phenix.memory.retrieve@1
           requires foreign.vector-search@2
               -> external search endpoint
~~~

The adapter converts scope/temporal filters, request budgets, search results and errors into Phenix's contract. It may only advertise full compatibility when it can uphold Phenix's required visibility, freshness and provenance semantics. If the external backend cannot supply exact evidence, use an explicit narrower capability or fail the required operation; do not manufacture source authority.

**Example: an independent harness using Phenix memory**

~~~text
Foreign agent loop
  -> foreign-harness memory API
      -> adapter.foreign-from-phx (foreign process or authorized gateway)
           exposes foreign.memory@2
           calls phenix.memory.retrieve@1
               -> selected Phenix memory provider
~~~

The foreign loop never needs to import or link the Phenix agent-loop implementation. Its contract adapter may be a thin external client library or a gateway endpoint. An adapter running *outside* the Phenix graph still has to authenticate and receive explicitly scoped Phenix capabilities; it is not privileged by calling itself an adapter.

### Three conversion categories

1. **Structural representation:** Same semantic contract, compatible schema or encoding difference. Existing structural compatibility and transport codecs should be reused; no extra logical provider is necessary when no semantic behavior changes.
2. **Semantic adaptation:** Distinct contract identities or differing behavior. Use an explicit adapter that implements one contract through required imports or a declared external endpoint. It owns policy-preserving translation.
3. **Transport bridge:** Identical logical contract over stdio, socket, HTTP, MCP or another protocol. Change the transport at a gateway/runtime-adapter boundary, not by inventing another semantic contract.

A transport bridge may contain semantic adaptation only if that mapping is explicitly declared and tested.

### Adapter discovery, selection and metadata

Do not make Core search arbitrary conversion graphs or infer semantics from similar type shapes. Register adapters as ordinary providers. Existing explicit provider bindings select the adapter just like any terminal implementation.

For inspection and tooling, an adapter may declare **descriptive metadata** (not a second resolver):

- Provided contract identity/version and consumed contract identities/versions.
- Whether the mapping is exact, partial or intentionally lossy.
- Which source guarantees are preserved (scope, provenance, revisions, ordering, cancellation).
- Required authority and external endpoint identity, if applicable.
- Adapter implementation revision and contract compatibility fixtures.

No global conversion registry, auto-composed shortest-path algorithm or implicit cross-version coercion is necessary initially. If discovery is eventually added, it must generate a deterministic *candidate* graph which the product resolves explicitly, not automatically reroute live requests.

### Failure and lifecycle rules

- Schema compatibility does not prove semantic equivalence. The provider must meet the destination contract's invariants.
- An adapter cannot widen caller authority, memory scope or workspace access.
- One outer request retains a correlated identity across translation and nested invocation.
- Retries are only allowed under the destination/source idempotency contracts and cannot replay tool or storage side effects.
- Cancellation and bounded execution propagate to the underlying provider.
- Domain failures remain distinct from transport failures and unsupported operations.
- Exact source references remain exact; missing revision/history support cannot be synthesized.
- A forwarding terminal is still terminal-provider **replacement**, not a Layer using one-shot continuation.
- Avoid recursive same-interface forwarding: provide A, require B with different identities, or use an explicit Layer when intercepting A before a selected A terminal.
- Every edge follows generation pinning. If external endpoint lifetime differs, the adapter maintains connection-local state without exposing stale authority.
- If a contract cannot be implemented faithfully, fail graph composition for required capabilities or provide a different explicitly weaker contract; do not silently downgrade safety guarantees.

### Shared conformance fixtures

1. Full + foreign memory: selected foreign adapter satisfies Phenix memory retrieval; Phenix's native memory provider can be absent.
2. Full + foreign context: adapter satisfies context preparation/compaction contracts while preserving mandatory content and exact references.
3. Foreign loop + Phenix memory: no Phenix agent-loop plugin installed; external caller records and retrieves authorized memory.
4. Foreign loop + Phenix context: caller invokes explicit prepare/commit lifecycle and performs repeated compaction/recovery.
5. A -> B -> C chain resolves deterministically and preserves correlation; a cycle fails before execution.
6. Lossy/partial mappings cannot advertise stronger guarantees than the source provides.
7. Unauthorized scope expansion, stale evidence, transport interruption and cancellation fail explicitly.
8. Swapping from native to adapted provider changes the graph generation while in-flight calls remain pinned.
9. Adapter errors after a side effect cannot trigger silent provider fallback.
10. Inspection reports the chosen adapter and its underlying resolved provider and contract versions.

## External harness interoperability

**Outbound forwarding terminal**

~~~text
Phenix AgentExecution importer
  -> selected external-agent-terminal
  -> typed protocol adapter
  -> foreign harness agent loop
~~~

The terminal translates the same contract, forwards correlation/cancellation and reports transport failures
separately. It cannot recursively invoke the same service to find the old provider.
Use a Layer only for actual interception with an explicit continuation.

**Inbound authorized capability gateway**

~~~text
Foreign harness loop (model and tools remain external)
  -> typed Phenix capability client or MCP tools
  -> authorized Phenix gateway
  -> selected memory/context providers in Phenix graph
~~~

A tool-only MCP interface can expose `memory.query`, `memory.record`, `memory.recall`,
`context.expand`. Structured `context.prepare_turn`/`context.commit_turn` calls require
the foreign harness to integrate lifecycle hooks; merely adding a tool cannot automatically
intercept its context assembly. Expose only contract-approved endpoints, never unrestricted
kernel invocation over a network. Authenticate and scope each call.

Process-backed plugin runtime adapters remain transport/packaging boundaries, not alternate graph semantics.

## Memory and context constraints

- Canonical source history is not the mutable context projection.
- Basic compaction removes only eligible prior material and retains exact recovery references.
- Full uses structured, source-backed summaries and checkpoints; budget verification is mandatory.
- The projection must include **both prompt resources and the model's tool-turn continuation**.
- Tool-call/result identity and ordering must survive compaction.
- Session, workspace, global and optional agent memory scopes remain distinct, authorized and relevance filtered.
- Source changes invalidate derived memories by stable evidence references and revisions.
- Automatic retrieval is selective. Keep manual structured/semantic queries and exact expansion accessible.
- Maintenance must be event-driven, restart-safe and idempotent; no LLM on every turn by default.
- Search/indexes are derived state; preserve canonical records independently.

## Acceptance criteria

1. Kernel starts with no agent-loop provider.
2. Basic starts and completes multi-turn model/tool work; Full extends its dependency closure.
3. A third-party AgentLoop provider is selected through an explicit graph binding without changing consumer logic.
4. A standalone memory-store/query graph runs without model helpers or an agent loop.
5. A standalone context preparation graph runs without fake Phenix execution records.
6. Basic/Full compactors satisfy the same contract and preserve exact evidence after repeated compaction/restart.
7. Explicit provider bind/disable and replacement are inspectable in resolved generation provenance.
8. Forwarding terminal invocation preserves correlation, cancellation, failures and authority.
9. An external harness can query and update authorized memory; unauthorized scopes are rejected.
10. Provider changes never silently rebind in-flight calls or replay effects.
11. Static analysis rejects contracts importing implementation crates and generic application logic branching on provider identity.
12. Shared test fixtures exercise native, replaced and forwarded implementations.

## Implementation order

1. **This PR:** move agent-loop wire contract/identity into the provider-neutral SDK, preserve original Basic exports, expose kernel-owned provider composition through the product builder, and document the audit and target graph.
2. Application entry decoupling: configurable agent-execution binding and capability-driven tool wiring.
3. **Initial portion implemented in this PR:** expose `ProviderCompositionPolicy` through the harness runtime builder and add no-Nix graph selection, exclusion, invalid binding and generation-identity regressions. **Still pending:** portable file/CLI/frontend lowering and Full profile substitutions before concrete package activation.
4. Add genuinely functional deterministic Basic compaction with recoverable tool-history checkpointing.
5. Full compaction substitution, repeated-checkpoint lineage, enforced token targets and continuation integration.
6. Separate memory storage/query from helper-backed maintenance; standalone graph tests.
7. Source-resolution, scope enforcement, maintenance and retrieval improvements.
8. **Bidirectional contract adapters:** demonstrate Full with foreign memory/context and a foreign loop using Phenix memory/context, with no mandatory Basic loop in either external-only deployment; forwarding terminal + authorized capability gateway, shared interoperability fixtures.
9. Reproducible cost/quality benchmark variants; do not claim unmeasured superiority.

## Research and interoperability

Memory and context choices draw from:
- [MemGPT](https://arxiv.org/abs/2310.08560): hierarchical virtual context.
- [ACE](https://arxiv.org/abs/2510.04618): incremental context curation and avoiding summary collapse.
- [ACON](https://arxiv.org/abs/2510.00615): compression of long agent observations and interaction history.
- [LongMemEval](https://arxiv.org/abs/2410.10813): extraction, temporal updates and multi-session retrieval.
- [Self-RAG](https://arxiv.org/abs/2310.11511): conditional retrieval.
- [Lost in the Middle](https://arxiv.org/abs/2307.03172): excessive context and position effects.

The specific component graph, per-interface decomposition and forwarding-provider model are
**architectural engineering decisions**, not experimentally established by those papers.
Benchmark the tradeoffs separately. Standards are interoperability boundaries, not the kernel contract:
[MCP](https://modelcontextprotocol.io/specification/2025-11-25) for tools/resources,
[A2A](https://a2a-protocol.org/) for independent agent collaboration, ACP for application/agent integration.

Related existing Phenix contracts:
- [Agent configurations](agent-configurations.md)
- [Plugin resolution](plugin-resolution.md)
- [Service layering](plugin-service-layering.md)
- [Plugin runtime bridges](plugin-runtime-bridges.md)
- [Plugin call binding](plugin-call-binding.md)
