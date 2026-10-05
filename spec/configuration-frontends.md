# Configuration frontends and graph generations

status: implemented

## Purpose

Allow Phenix to expose arbitrary configuration APIs without making Nix, Lua, IPC, TOML, a GUI, or any other frontend part of the runtime architecture.

The architectural rule is:

> Configuration frontends may expose arbitrary user-facing APIs, but they only emit canonical declarative contributions. One resolver owns runtime topology, authority, validation, and graph construction.

This preserves a stable production model while allowing development mode to dynamically add, remove, or replace plugins, components, skills, resources, and configuration through validated graph generations.

## Vocabulary

Use these terms consistently:

```text
Plugin
  package / lifecycle / trust owner

Component
  runtime composition unit

Interface
  typed runtime import/export contract

Configuration frontend
  adapter from an external configuration API or source into canonical Phenix configuration contributions

ConfigContribution
  declarative, attributable configuration data consumed by the canonical resolver

ResolvedGeneration
  immutable, inspectable resolved runtime composition with a stable semantic identity

Graph generation
  one activated ResolvedGeneration revision

Reconciler
  plans and applies safe transitions between valid graph generations in development mode
```

Do not call configuration frontends `interface plugins`; `interface` is reserved for runtime component contracts.

## Configuration frontends

A configuration frontend may adapt any external representation or API, including:

```text
Nix modules / flakes
Lua DSLs
TOML / JSON / YAML
project-local configuration
filesystem / skill-directory discovery
IPC protocols
GUIs
remote control planes
agent-generated configuration
company-specific DSLs
```

Examples of first- or third-party packages may include:

```text
phenix-config-nix
phenix-config-lua
phenix-config-file
phenix-config-project
phenix-config-ipc
phenix-config-dev
```

A frontend may embed substantial logic such as a Lua VM, parser, IPC client/server, schema-aware GUI, or domain-specific convenience API. That logic is not privileged runtime composition logic.

Its output is canonical declarative data, for example:

```text
ConfigContribution {
  source_identity
  source_revision
  namespaced_settings
  requested_plugins/components
  bindings
  interposition policy
  resources/skills
  requested authority
  environment/deployment bindings
}
```

The exact internal shape may differ, but it must preserve source attribution and be deterministic enough for resolution, diagnostics, and semantic identity.

## Arbitrary configuration API, strict runtime semantics

Plugins may extend the configuration API without core changes.

A Lua frontend may expose:

```lua
acme.engineering {
  team = "compiler",
  review = "strict",
}
```

A Nix frontend may expose the equivalent:

```nix
acme.engineering = {
  team = "compiler";
  review = "strict";
};
```

Both may lower to the same versioned namespaced configuration payload:

```text
acme.engineering@1 {
  team = compiler
  review = strict
}
```

The owning plugin/component interprets that payload through its declared configuration contract. Core does not need an `engineering` concept.

Configuration frontends may therefore invent arbitrary syntax, abstractions, aliases, macros, presets, and domain-specific APIs.

They may not create a second runtime extension system. New runtime behavior still enters through ordinary components, typed interfaces, events, controllers, resources, persistence, and interposition.

The invariant is:

> Arbitrary API on the configuration side. Canonical typed semantics on the runtime side.

## Canonical resolver

Only the canonical Phenix resolver may turn configuration contributions and package metadata into runtime topology.

The resolver owns:

```text
contribution merge/precedence
plugin/component selection
component import/export resolution
terminal/provider binding
interposition chain construction
authority grant calculation
resource/skill dependency resolution
compatibility validation
durable ownership validation
environment-binding validation
semantic identity calculation
ResolvedGeneration construction
```

A configuration frontend must not directly mutate the live component/service registry, install providers, grant authority, or alter bindings.

The wrapper and runtime APIs may be implementation layers around this resolver, but there must be one semantic owner.

## Wrapper and modules

The public wrapper remains the normal stable composition entry point. NixOS/Home Manager modules are thin host/configuration frontends that pass settings/package selections to the wrapper/resolver.

```text
Nix module -----------\
Lua config ------------+
TOML config -----------+--> configuration frontends --> ConfigContributions
IPC / GUI -------------+                              |
project discovery -----/                               v
                                                canonical resolver
                                                       |
                                                       v
                                                ResolvedGeneration
                                                       |
                                                       v
                                                    runtime
```

Modules do not own provider selection, graph construction, authority, skill dependency resolution, or runtime manifest generation.

The default Harness, custom flakes, host modules, and third-party configuration frontends all converge on the same resolver semantics.

## Plugin and component metadata

Safe composition and dynamic reconciliation require enough metadata to understand a candidate graph before executing plugin behavior merely to discover its basic shape.

Separate metadata by responsibility rather than growing one universal manifest.

### Plugin/package metadata

At minimum describe:

```text
identity/version
packaged components
packaged resources/skills
execution/isolation kind
compatibility constraints
durable namespaces and migration ownership
maximum/requestable authority
configuration frontends supplied by the package
component hosts supplied by the package
lifecycle/reload constraints
```

### Component metadata

At minimum describe:

```text
component identity/version
typed imports/exports
required versus optional imports
configuration schema/contracts
requested capabilities/authority
state class: stateless / ephemeral / durable
restart/unload/reload behavior
interposition compatibility
event/controller contributions where applicable
```

### Resource/skill metadata

At minimum describe:

```text
identity/version/content identity
dependencies/conflicts
triggers/scope/priority where applicable
required tools/interfaces/capabilities
compatibility
invalidation/reload semantics
```

### Configuration frontend metadata

At minimum describe:

```text
frontend identity/version
accepted input/source kinds
configuration namespaces/schemas exposed
source identity/revision rules
stable-mode materialization rules
watch/reload support
required authority for reading external sources
```

## Metadata must be inspectable before activation

Basic composition metadata must be readable without first granting the plugin its requested runtime authority or activating arbitrary plugin behavior.

External executable plugins may expose a manifest through packaging metadata, a restricted metadata handshake, or another deterministic mechanism. Metadata discovery must not become an authority bypass.

If metadata itself is generated dynamically, the generation mechanism and all composition-relevant inputs become part of the candidate configuration identity.

## Authority

Requested authority and granted authority are distinct.

A frontend may request:

```text
workspace.write
network.openai
process.spawn
```

but only Harness/resolver policy grants capabilities.

Configuration frontends do not gain authority merely because they can request components, plugins, bindings, or resources.

A dynamically discovered component is subject to the same authority ceiling and attenuation rules as a statically configured one.

## Stable mode

Stable operation resolves once and activates one immutable graph generation:

```text
external config + package metadata + resources
                    |
                    v
              frontends/adapters
                    |
                    v
             ConfigContributions
                    |
                    v
                 resolver
                    |
                    v
        ResolvedGeneration generation N
                    |
                    v
                  runtime
```

Stable runtime execution does not continue to reinterpret the original Nix/Lua/TOML/IPC configuration source.

Any external input that can affect semantic composition must either:

1. be materialized into the resolved artifact/identity, or
2. be explicitly classified as an environment/deployment binding that is not allowed to alter semantic policy.

A frontend must not allow ambient filesystem state, environment variables, remote IPC responses, current Git state, or wall-clock-dependent evaluation to silently change stable semantics without changing the resolved identity.

## Graph generations

Each valid `ResolvedGeneration` is a graph generation with its own semantic identity.

Running work is pinned to the generation whose semantics it started under unless a contract explicitly defines safe migration.

```text
execution A -> generation 41
execution B -> generation 41

configuration changes

execution C -> generation 42
```

A model call, orchestration, task, worker, or session projection must not silently switch providers, authority, skills, or interposition policy halfway through an invocation merely because a new graph generation became available.

## Development mode

Development mode uses the same resolver. It does not expose a mutable bypass.

```text
active generation N
        |
 source/package/resource change
        |
        v
 re-evaluate frontends
        |
        v
 candidate generation N+1
        |
   full validation
        |
        v
 graph/resource diff
        |
        v
 transition plan
        |
        v
 activate safely
        |
        v
active generation N+1
```

If candidate `N+1` is invalid, activation is rejected and generation `N` remains active.

Do not partially mutate the live graph and then attempt to recover validity afterwards.

## Reconciliation

Dynamic loading/unloading is graph reconciliation between two valid resolved generations.

The diff model should distinguish at least:

### Nodes

```text
unchanged
added
removed
reconfigured
upgraded
invalidated
restart-required
```

### Edges

```text
unchanged
added
removed
provider-changed
authority-changed
interposition-changed
```

The reconciler computes the affected dependency closure and performs the smallest safe transition allowed by lifecycle metadata and live runtime state.

Possible transition actions include:

```text
activate component
stop component
drain in-flight calls
restart component
restart dependents
retain old generation for pinned executions
cancel affected tasks when policy allows
run durable migration
rebuild skill/resource indexes
reject candidate generation
```

Runtime failure is not provider fallback. Reconciliation changes bindings only by activating a new validated generation.

## Prefer full resolution before incremental solving

The initial implementation should recompute the complete candidate graph on each relevant development change, then use a semantic graph diff to minimize runtime disruption.

Do not build a complex incremental dependency solver until measurement shows full deterministic resolution is a bottleneck.

Correctness and inspectability are more important than optimizing resolution of hundreds or low thousands of metadata nodes.

## Dependency graph cycles

Required synchronous component imports should form a resolvable acyclic dependency graph wherever possible.

Do not solve ordinary hard-import cycles through hidden late service lookup.

Genuine cyclic interaction must use an explicit mechanism whose lifecycle semantics are understood, such as:

```text
events/subscriptions
controller reconciliation
explicit late-bound handles designed for cycles
shared runtime primitives that do not create ownership cycles
```

The resolver must report dependency cycles with a concrete path.

## Skills and resources

Skills and resources participate in semantic identity and graph/resource reconciliation according to their metadata.

A skill edit should normally create a new content identity and invalidate only declared derived state such as skill indexes or future context projections.

It must not rewrite unrelated durable session/artifact state or retroactively mutate executions pinned to an older generation.

A resource-only plugin may be added or removed without inventing a runtime service when its metadata shows that only resource indexes/projections are affected.

## Component hosts

Plugins may also provide alternate component hosts such as:

```text
embedded Rust
embedded Lua
external IPC process
Wasm
remote service
```

A configuration frontend and a component host are distinct roles even when one plugin package provides both.

For example, a Lua plugin may provide:

```text
Lua configuration frontend
Lua component host
Lua runtime modules/resources
```

Lua-defined runtime components still declare ordinary Phenix component identities, imports, exports, authority requirements, and lifecycle metadata.

External/IPC-hosted components obey the same resolved graph and capability rules as embedded components.

## Inspectability

The active system must expose enough information to explain why the runtime has its current shape.

At minimum inspect:

```text
active graph generation/semantic identity
configuration contribution sources and revisions
selected plugins/components/resources/skills
component imports and resolved providers
interposition chains
requested versus granted authority
component host/execution kind
state/lifecycle class
changes between candidate and active generations
reconciliation plan/rejection reason
```

Diagnostics must preserve source attribution so a user can trace a resolved setting or binding back to the Nix/Lua/IPC/project source that contributed it.

## Required regressions

- Equivalent semantic Nix and Lua configuration contributions resolve to equivalent component graphs and semantic identities.
- A configuration frontend can expose a plugin-defined namespaced configuration API without core changes.
- A configuration frontend cannot directly mutate the active component registry or grant authority.
- A third-party plugin can define a new typed runtime interface without core changes.
- Basic plugin/component metadata can be inspected before arbitrary plugin behavior is activated.
- Requested authority can be denied or attenuated by Harness policy without changing frontend syntax.
- Stable mode materializes all composition-relevant external input into the resolved identity or rejects the configuration as non-reproducible.
- Each execution records/pins the graph generation that defines its semantics.
- A valid dev change produces a fully validated candidate generation before activation.
- An invalid candidate leaves the previous generation active and unchanged.
- Adding an optional compatible plugin/component changes only the required dependency closure.
- Removing an unused optional component does not restart unrelated components.
- Removing a required provider either retains the old generation for pinned work, drains/restarts the affected scope, or rejects activation according to policy; it never leaves an unresolved live graph.
- Changing a provider binding is represented as a new generation, not runtime fallback after a failed call.
- A skill content change invalidates declared derived indexes/projections without mutating unrelated durable state.
- Dependency cycles are diagnosed explicitly rather than resolved through hidden service lookup.
- Lua/IPC/Wasm/external component hosts use the same component import/export and authority model as embedded components.
- The default Harness, Nix modules, direct wrapper use, and third-party configuration frontends converge on the same canonical resolver.

## Completion gate

This contract is complete when:

- there is one canonical resolver from declarative contributions + metadata to `ResolvedGeneration`;
- arbitrary configuration frontends can extend the user-facing API without adding core-domain concepts;
- configuration frontends cannot bypass runtime component/interface/capability semantics;
- manifests are rich enough to validate and diff candidate compositions before activation;
- stable mode activates one reproducible immutable graph generation;
- development mode repeatedly produces complete candidate generations and reconciles only between valid graphs;
- running work is pinned to explicit graph generations;
- the active graph and proposed transitions are inspectable with source attribution;
- Nix remains a first-class frontend but is not a privileged composition engine;
- Lua, IPC, or another frontend/host can be added as ordinary plugins without changing the canonical resolver or core runtime semantics.

## Selectable development generations

`spec/selectable-generations.md` defines the proposed runtime extension for retaining more than one validated `ResolvedGeneration` during development.

Configuration frontends still only produce declarative candidate input. They do not choose a generation for an in-flight call, widen the root authority ceiling, or replace pinned host bindings. Promotion changes the default generation only after the candidate passes those runtime checks.
