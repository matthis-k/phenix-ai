# Plugin-defined contribution kinds and templates

status: partial
stage: D of spec/microkernel-composition-roadmap.md
depends-on: #727, Stage B typed-contribution PR
parallel-with: Stage C graph patch composition, after Stage B
blocks: Stage E consumer migration

## Implementation slice: portable pure lowering

`phenix_sdk::kind_lowering::lower_kinds` implements a data-only
selected-definition/input lowering stage with explicit output bounds,
versioned kind identity, original author and provider provenance, literal or
source-field projection, recursive kind expansion and terminal canonical
output. Definitions and inputs sort independently of enumeration order and
serialize to stable structural bytes. Missing kinds, duplicate identities,
schema failures, absent fields, cyclic expansion and exceeded output bounds
reject preparation. Tests include independent non-agent authors and reversed
discovery.

This is **not kind selection or live generation activation**. Binding the
typed definitions to #728's frozen contribution envelope, verifying selected
provider authority, and passing all terminal output through canonical Core
resolver validation remain open. No generic Rust `#[phenix(kind)]` macro,
third-party provider discovery, or end-to-end runtime kind consumption has
yet been implemented. #730 is not merge-ready.

## Goal

Any selected plugin may define a new versioned contribution kind used by other plugins. The Core resolver knows how to resolve typed kind definitions and process their canonical output without hardcoding Tool, Skill, Frontend, Memory, Agent or application semantics.

A kind defines a versioned schema and a template for lowering authored declarations into already-known canonical contract requirements, providers, resources, events, triggers and optional graph edits. Kinds are ordinary plugin capabilities with owners and provider-selection rules, not a second registry with hidden activation behavior.

## Bootstrap and selection

The kernel has a fixed, minimal metamodel for `Contribution` and `ContributionKindDefinition`. It validates and resolves selected kind providers in a preparation phase. Ordinary contributions are schema-checked against their selected kind, then lowered. Template output is normalized into the SAME canonical contribution model and enters the SAME kernel resolution.

- A missing required kind provider rejects generation preparation.
- Multiple competing definitions for one versioned kind follow explicit provider binding or conflict policy; no nondeterministic winner.
- Kind dependencies are explicit; expansion cycles and unbounded recursively generated contributions fail.
- Every output retains original plugin provenance, template version, and versioned kind identity.
- Lowering cannot grant authority and cannot invoke an unvalidated plugin service or introduce new unseen providers at runtime.
- The kind-provider plugin is allowed to be resource-only; it need not execute model-, tool- or other domain-specific code.

## Ordinary Rust authoring

```rust
#[phenix_sdk::kind("phenix.tools.tool@1")]
pub struct Tool {
    pub id: ToolId,
    pub callable: InterfaceId,
    pub description: String,
}

#[phenix_sdk::plugin("acme.workspace")]
mod plugin {
    #[phenix(contribute)]
    const READ: tools::Tool = tools::Tool {
        id: ToolId::new("workspace.read"),
        callable: ReadFile::ID,
        description: String::new(), // example placeholder
    };
}
```

This illustrates semantics, not a claim that `kind` or `contribute` is implemented. Static metadata must remain representable as portable serializable declarations. Rust proc macros cannot be dynamically installed by plugins, so the *generic* `#[phenix(contribute)]` annotation must discover the kind through its Rust type's generated metadata, not by a kind-specific macro parser.

For the common case, define templates through typed Rust data constants or struct literals with optional minimal helper macros. Do not require one handwritten marker struct + `impl` per static declaration.

## Template behavior

A template should produce canonical descriptions such as contract requirement, provider candidate, callable registration requirement, event binding or graph patch. For a `tools::Tool`, the default tool plugin owns mapping to tool descriptors, catalog access and model exposure contracts. A separate agent/model plugin decides whether to activate/expose those tools on a given model call. The kernel never calls a tool-catalog API directly.

A `skills::Skill` kind similarly maps static content/resources to standard skill and context contracts; frontend kinds may map capabilities to entry triggers or front-end component imports.

The default template is declarative and independent of plugin executable code. If dynamic transformation is unavoidable, make it an explicitly versioned preparation provider with fixed inputs, bounded output, sandboxed authority and output digest/provenance. Dynamic transformations cannot weaken repeatability guarantees or mutate an already active generation; no unbounded code execution inside Core graph resolution.

## Manual/dynamic path

The SDK may support explicit dynamic contribution publishing for runtime-discovered constructs. It uses the same schema, owner, authority and provider machinery as statically authored contributions. Distinguish:
- new topology/provider contributions, which require constructing a fresh generation;
- domain-level catalog resources that an application plugin may manage within its declared mutable contract.

Do not add a second implicit registration hook for every kind.

## Implementation ownership

Stage D owns kind-definition contract schema and SDK/macro kind authoring, template metadata and kind-provider selection. Stage C owns graph patch application and its Core resolver. Both stages consume the stable Stage B descriptor, and should not modify each other's source files concurrently.

Stage E owns first-party tool/skill migration. Do not move the tool catalog, skill loader or frontend semantics into Core.

## Acceptance tests

1. A third-party kind type with schema and template is introduced without modifying the Core enum, SDK plugin macro parser or kernel domain branches.
2. Two independent consumer plugins contribute values of that kind, including resource-only contributors.
3. Missing, duplicate/incompatible, cyclic or unauthorized kind definitions fail with owner/kind provenance.
4. The same declaration authored in Rust or portable config yields matching normalized contribution data.
5. Template output gets canonical provider, authority and generation checks and can be inspected before activation.
6. A template producing graph edits delegates application to Stage C; no direct graph mutation callback.
7. A non-agent domain kind is implemented through the same path and proves the kernel boundary.

Design approval is not implementation completion.

## Closed preparation boundary and lowering limits

**Normative target: [kernel RFC #736](https://github.com/matthis-k/phenix-ai/pull/736).** The default kind template is *data*, with a bounded, total, pure, deterministic, non-Turing-complete lowering rule. It consumes a schema-validated kind declaration and emits a finite set of canonical contributions. No template may call executable PluginHost operations, choose providers as a side effect, mutate the active graph or create a new kind-specific dispatch path in Core.

- Kind definitions are plugin-owned, independently versioned, selected through canonical contribution/provider rules and checked for compatible schemas. Missing required providers, definition conflicts, expansion recursion and cycles reject candidate preparation.
- Freeze normalized template input, selected kind/provider version, output, provenance and digest **before** the resolver selects services or composes IR. Resolver inputs are immutable data only. This is distinct from static Rust macro expansion, which merely generates descriptors.
- Any exceptional procedural preparation uses a separately authorized and bounded preparation stage with **no ambient host authority** and immutable, size-limited inputs. It returns versioned, schema-checked finite output with a digest. The resolver never executes this procedure. Freezing its output does not prove the procedure deterministic; reproducible preparation is a separate policy/test.
- Templates may produce ordinary `ExecutionPlanDefinition`, `EntryBinding` or IR patch contributions where their kind requires it. Structural edits obey #729's closed IR patch algebra. They cannot add Core node kinds, retarget owner-only plan metadata, or provide a second execution engine.
- Tool, Skill, Frontend, Agent and other domain meaning stays with plugin-defined kinds. The Core parser must not acquire a new match arm for each kind, and the SDK common service-only path must not require kind declarations.

**Added acceptance:** third-party non-agent kind plus independently authored consumer; bounded-expansion rejection; template replay/canonical-byte tests; procedural output freeze without resolver-time callbacks; incompatible kind schema/version rejection; and a service-only SDK fixture that selects no kind provider. Stage B (#728) must stabilize canonical descriptors before implementing these rules.
