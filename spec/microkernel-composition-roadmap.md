# Usage-agnostic microkernel and composition roadmap

status: specification-only
scope: phenix-core, phenix-contract, phenix-sdk, plugins

## Architectural invariant

Phenix Core owns mechanisms needed to execute independent plugin compositions. It must not own AI-agent semantics or special-case tools, skills, frontends, memory, prompts or context windows.

Core owns canonical typed identities and value exchange, requirement/provider resolution, authority enforcement, lifecycle, generation pinning, generic event and invocation transport, and generic execution-graph compilation, composition, reconciliation and inspection.

Plugins own domain constructs and policy. They may introduce a versioned contribution *kind* and a template that lowers declarations of that kind into canonical Core requirements, providers, graph edits and event/entry bindings. The kernel must not add a case for each new kind.

A contribution is immutable input to a candidate generation, not a callback allowed to mutate the live graph. The same normalized metadata must be usable from annotated Rust, portable configuration and non-Rust plugin artifacts. Nix may generate configuration but is never the semantic authority.

## Ownership boundaries

| Concept | Owner | Requirement |
| --- | --- | --- |
| Contract identity, schema, generic requirements/provisions | Contract/SDK authoring and Core resolution | One canonical resolver |
| Graph node/edge/slot identities and graph patch validation | Core | Domain-independent, deterministic |
| Conflict policy and provenance | Core configuration and resolver | Unresolved incompatible edits fail activation |
| Provider selection and Service Layers | Existing Core resolution path | No workflow-specific provider solver |
| Kind schema and declarative lowering template | Kind-defining plugin | Kind resolution is inspectable and versioned |
| Tool catalog and model exposure | Tool/catalog plugins | No tool-specific Core dispatch path |
| Skill registration and context activation | Skill/context plugins | No skill-specific Core dispatch path |
| Agent loop, memory, compaction, UI | Agent/application plugins | Core works without these plugins |

Runtime execution may use internal libraries, including an optional scheduler, but the public graph, resolution and authority semantics must not expose a backend's types.

## Planned ordered slices

| Stage | Change | Depends on | Mutually exclusive editing area |
| --- | --- | --- | --- |
| A | Microkernel contract and ordered plan | None | This file |
| B | Typed contribution descriptors, roles, identity, schema and static normalization | A | phenix-contract + SDK authoring |
| C | Conflict-aware graph patches, stable slots, resolved provenance | B and #726 | phenix-core workflow and graph resolver |
| D | Plugin-defined contribution kinds and template lowering | B | SDK authoring + kind/template metadata |
| E | Tool/skill template migration and non-agent kernel proof | C and D | Tool/skill plugins, integration fixtures |

Stage C and D can be implemented in parallel after B once both use the agreed contribution descriptor. They must edit disjoint implementation modules. Stage E starts only after both are semantically complete.

PR #726 is a prerequisite for Stage C: it introduces generic workflow compilation but remains open with live Basic/Advanced migration and discovery work. Do not include Stage C changes in #726 merely to accelerate a merge. Stage B must avoid modifying #726's active workflow and agent-loop files.

## Dependency and review discipline

- Each stage has a separate branch and PR with a machine-readable `Depends-on: #number` section naming required earlier PRs.
- Design-only PRs document interfaces and acceptance gates, and do not claim an implementation is complete. They should not be marked complete based on green CI.
- All implementation PRs must include focused tests for their own seam and avoid changes to another active stage's owned files.
- A downstream PR is blocked from merge until its dependency PRs have landed with semantic verification. Rebase from main after each dependency merges; never create circular branch dependencies.
- No PR may introduce a second resolver, lifecycle dispatcher, tool registry, or agent-specific branch in Core.
- Shared contracts are stabilized in Stage B before parallel Stage C/D implementation; changing them requires updating affected PRs explicitly.

## Parallel runtime-host workstream

The execution-runtime packaging boundary is separate from contribution kinds and workflow graph edits. [Native plugin ABI and guest runtimes](native-plugin-abi-and-guest-runtimes.md) records a design-only target for one minimal native ABI loader plus plugin-provided Lua, Wasm, JavaScript and process guest-runtime adapters. Lua is the first conformance target because Phenix.nvim is the primary frontend, but Neovim ACP client bindings remain separate from Lua guest hosting.

- **N0 specification:** independent of Stages B-E; identifies where current `embedded` factory language, native artifact packaging, runtime adapters and reload terminology differ from the proposed target.
- **N1 native ABI contract:** isolated ABI crate/header and separate-compilation conformance, with no edits to #726's workflow files or #728's descriptor authoring files before their merge.
- **N2 native loader and metadata normalization:** depends on #728's contribution envelope and uses the existing Core resolver. Coordinate runtime edit ownership with #726.
- **N3 guest adapter proof:** a native Lua runtime adapter loads a Lua guest exporting one canonical service, invoking an imported service and preserving authority, identity and generation. Follow with a narrow Phenix.nvim ACP smoke test without restarting the kernel.
- **N4 reload/select proof:** use resident-generation staging and explicit promotion from `spec/selectable-generations.md`. Loading a new artifact must not silently switch the default graph.
- **N5 migration:** remove embedded factory/catalog and Rust product-composition ownership only after N2-N4 and product parity; coordinate with #733's cleanup inventory.

The design PR is not an implementation gate by itself. N1-N5 require runnable proofs and no second provider resolver, plugin registry or generation lifecycle.

## Required end-to-end proofs

1. Core-only non-agent workflow with two independently packaged providers; no agent plugin loaded.
2. A plugin defines a new contribution kind without modifying Core or the Rust authoring macro.
3. Two independent graph insertions compose in declared order; ambiguous order and exclusive replacement conflicts fail without a policy.
4. Identical selected contributions and policy produce identical canonical graph and generation digest regardless of plugin enumeration order.
5. Service invocation always passes through selected provider bindings, Core authority, Layers and generation-pinned dispatch.
6. A user tool and a skill are declared through third-party-kind-style templates, with no corresponding Core special cases.
7. Core inspection explains each resulting node, edge, binding and conflict resolution by its contributing plugin.
8. Live replacement or removal creates a new generation; pending invocations stay pinned to the previous generation.

## Exclusions

This roadmap does not authorize rewriting all domain code, exposing a general imperative graph mutation API, embedding Bevy ECS types in the plugin contract, or merging #726 before its product parity gate. Keep domain-specific mechanisms in ordinary plugins. The smallest adequate canonical representation is preferable to an all-purpose meta-DSL.
