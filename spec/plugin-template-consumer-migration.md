# Plugin-defined tools and skills: consumer migration and non-agent canary

status: partial implementation (independent non-agent provider canary)
stage: E of spec/microkernel-composition-roadmap.md
depends-on: #727, #728, Stage C graph-patch PR, Stage D plugin-defined-kind PR
prerequisite: #726 must be semantically complete through Stage C

## Implementation slice: headless non-agent Core canary

The new `plugin_template_consumer_canary` Core regression instantiates one
resource-only composition owner importing two independently authored
application interfaces from two embedded provider plugins. It asserts both
typed imports resolve without selecting any first-party Tool, Skill, Model
or Agent plugin and that removing a required provider fails admission rather
than silently reinstalling a default. This is the *prerequisite neutral
execution boundary* for Stage E, not a substitute for plugin kind templates.

Remaining: wait for #729 and #730 to provide actual selected templates,
migrate Tool and Skill declarations and catalog consumers, prove dynamic
compatibility/model visibility, exercise runtime provider execution, and
retire stale first-party registrations. This PR remains blocked on those APIs.

## Goal

Prove the microkernel boundary against real consumers rather than stopping after abstract Core metadata. Move the first-party tool and skill authoring path to contribution kinds defined and handled by their respective plugins. Keep model visibility, skill activation and domain policy outside Core. Preserve portable plugin declarations and dynamic/manual APIs.

## Dependency gates

Stage E must not begin integration implementation until:
- Stage B supplies canonical typed contribution descriptors and stable Rust authoring lowering;
- Stage C supports deterministic graph patches and the generic workflow seam from #726;
- Stage D supplies plugin-defined kind schemas and template lowering.

C and D can be reviewed/implemented in parallel, but E cannot merge with either unfinished. All PRs must state the dependency explicitly and avoid touching overlapping files while predecessors remain open.

## Consumer ownership

| Domain | Kind provider | Owns |
| --- | --- | --- |
| Tool | Tool/catalog plugin | Callable descriptor, tool-definition materialization and visibility semantics |
| Skill | Skill/context plugin | Skill content metadata, registration, context activation and invalidation |
| Model-facing tools | Agent/model integration plugin | Which tools are offered to model, in which run and under which frontend capabilities |
| Frontend | Frontend integration plugin | Frontend capabilities, UI transport and subscriptions |
| All domains | Core | Generic contribution resolution, graph application, authority, execution and lifecycle |

A frontend kind can be a third test case, but do not enlarge this slice into a frontend rewrite. No new tool/skill policy branch enters Core.

## Representative static authoring

```rust
#[phenix_sdk::plugin("acme.workspace")]
mod plugin {
    #[phenix(provide(ReadFile))]
    fn read(request: ReadRequest) -> ReadResult {
        /* ordinary executable component implementation */
    }

    #[phenix(contribute)]
    const READ_TOOL: tools::Tool = tools::Tool {
        /* ID, contract, description, schema metadata */
    };

    #[phenix(contribute)]
    const REVIEW_SKILL: skills::Skill = skills::Skill {
        /* ID, static content and activation metadata */
    };
}
```

Normal plugin authoring must not require a manual registration list, module-specific proc macro parser, duplicated factory, custom graph runner, `impl StaticGraphContribution` per value or a domain-specific Core API.

For runtime-generated tools and skills, retain explicit SDK publication through the selected provider contract. The static and dynamic paths share schema/identity/ownership validation, with runtime catalog changes respecting the catalog's own lifecycle. Adding new topology/provider bindings requires a fresh Core generation.

## Non-agent proof product

Build a small headless integration fixture or test crate using **only** the kernel, generic contract metadata, a workflow owner, two independent service providers and an optional graph patch. No agent/model/tool/skill plugin is installed in this fixture.

Prove:
- custom contribution kind and generic workflow declaration, with distinct Rust and portable manifests;
- one provider replacement and one Service Layer or Event Listener;
- ordered graph insertions and conflict rejection;
- authority checks, cancellation, plugin removal and in-flight generation pinning;
- consistent result across different plugin enumeration orders.

A separate agent-harness fixture then verifies the real tool and skill consumer paths, including catalog discoverability, schema fidelity, model tool exposure constraints, provenance and removal. No mocked Core special-case registration.

## Verification and release criteria

1. Annotating a normal tool or skill declaration produces one canonical contribution, accepted by the selected kind provider and materialized via its contracts.
2. Static tool callable is discoverable and executes through normal kernel dispatch and Layers; model exposure remains optional and frontend/capability gated.
3. Static skill content is discoverable through the skill provider; lifecycle and invalidation ownership remain correct.
4. No stale catalog entry survives a plugin generation removal or replacement.
5. Missing tool/skill kind provider fails during resolution where the requirement is mandatory.
6. Manual dynamic registration and non-Rust manifest paths remain supported without double registration.
7. Non-agent Core test runs independently and passes without agent crates.
8. Build/CI proof is a focused test shard first; full integration on merge as permitted by repository CI policy.

## Out-of-scope

No bulk rewrite of every first-party plugin, no Bevy/ECS adoption, no model-specific Core logic, no new provider selection mechanism, and no implicit activation of optional tools. The first migration should be representative and narrow.

## Post-redesign conformance gate

The migration must verify the [kernel RFC #736](https://github.com/matthis-k/phenix-ai/pull/736) against **two independent user-level fixtures** rather than simply demonstrating that generated Tool and Skill metadata is present. This extends the existing acceptance criteria; implementation remains blocked on #726, #728, #729 and #730.

### Minimal authoring fixture

One external service-only plugin declares a typed Interface, one implementation and one provide annotation. The SDK derives canonical descriptors, schemas and runtime bindings. The plugin starts, executes and unloads without an ExecutionPlan, Basic/Full, a kind provider, a graph patch, manually maintained factory/registration table or a domain-specific Core branch.

### Headless non-agent composition fixture

A separately packaged plan-owning plugin supplies a versioned `ExecutionPlanDefinition`, typed `EntryBinding`, one compile-time reused subplan and at least two independent providers. A non-agent root exercises a cyclic control-flow path; a finite bounded dynamic map `Fork` and each closed `Join` policy; typed frame extensions and immutable data references; service-layer decoration and listener observation; provider replacement and contributor-owned IR patches.

The fixture must reject ambiguous plan entries, owner-only plan metadata patches, unauthorized capability values in frames or join results, stale base revisions, missing imports and conflicting ordered edits. Reversed discovery order must yield identical canonical composition. All work remains generation-pinned, child scopes and leases settle, and retirement never preempts an unsafe in-process native call.

### Lifecycle and persistence assertions

Use a direct service-only root alongside the plan-backed root. Trial a candidate generation without promotion, then promote and retire it while a prior root remains pinned. Verify ambient versus root-scoped event routing, ordered and unordered durable event modes, explicit shared/isolated/read-only storage policies, migration fencing rejecting stale writes, and pinned Environment/secret authority.

Do not broaden this PR into implementing Core graph, SDK descriptors, or runtime ABI mechanisms owned by other PRs. These are cross-module integration **acceptance** gates. Claims of completion require runnable tests with evidence, not a green documentation job.
