# Typed plugin contributions: canonical metadata and authoring seam

status: partial
coverage:
  - rust/crates/phenix-contract/src/contribution.rs
  - rust/crates/phenix-sdk/src/authoring/plugin.rs
  - rust/crates/phenix-sdk/tests/plugin_contribution_authoring.rs
  - rust/crates/phenix-sdk/tests/plugin_dependency_authoring.rs
stage: B of spec/microkernel-composition-roadmap.md
depends-on: #727
related: #726 may remain in progress; do not modify its workflow code in this slice

## Goal

Provide one immutable, versioned contribution representation that both first-party metadata and future plugin-defined kinds can use. Its consumers include provider resolution, graph patches, kind-template lowering and inspection. Do not create another resolver or registration system.

A plugin contribution is **data**. No `fn apply(&mut Graph)` or runtime callback is permitted to mutate candidate topology directly. Core composes normalized contributions before activation.

## Implementation progress

- `phenix-contract` now defines `Contribution`, `ContributionRole`, and a canonical `ContributionSet` with versioned identities and portable `PhenixValue` payloads.
- Serialization normalizes contribution-set order and rejects duplicated identities, including duplicates in decoded external inputs.
- `ContributionSet::decode_owned` decodes portable JSON against an independently verified manifest `PluginId`, rejecting forged serialized owners and duplicate declarations before a caller interprets kinds. This is a reusable contract API; the normal candidate loader does not call it yet.
- `ContributionSet::decode_selected` now prepares a complete selected portable artifact set against manifest-verified plugin owners, rejecting duplicate plugin envelopes, forged owners and cross-artifact stable-ID conflicts. Its canonical output is invariant under artifact enumeration. This remains a reusable preparation boundary; the live candidate loader does not call it and no non-Rust executable has been activated by the test.
- Static authoring supports `#[phenix(contribute)]` on annotated module constants, including modules that are `ResourceOnly` and have no embedded component or factory.
- The SDK exports `#[phenix_sdk::contract(...)]` as the contract-first spelling of the existing `#[phenix_sdk::interface(...)]` macro. Both lower into one versioned typed Interface descriptor; they do not define separate registries or resolution rules.
- Stateless and component provider methods now accept `#[phenix(provide(Interface))]` as the contract-first spelling. It lowers to the existing Export metadata and terminal Service role, without an independent registration or dispatch path. A service-only fixture exercises the generated descriptor and actual kernel invocation with no workflow or agent plugins. Literal `export(...)` remains available during migration.
- The existing `StaticPluginGraph` traverses the selected dependency closure and collects all statically authored contributions, rejecting cross-plugin identity collisions and mismatched owners.
- This is a **data-only foundation**: `Contribution` does not yet contain explicit requirement and ordering fields from the illustrative schema below. Contract requirements and provider resolution remain represented by the existing manifest types.
- Struct-field authoring uses `#[phenix(contribute(value = NAMED_CONST))]`; the typed const is extracted from the selected plugin's dependency closure.
- Static duplicate plugin IDs are checked for version, execution, authority, dependency set, factory availability and canonical contributions. Identical dependencies remain equivalent regardless of declaration order. Error provenance is stable across dependency order.
- The Rust-only dependency walker memoizes validated `TypeId` origins to avoid exponential traversal of nested diamonds. This is **not** a portable plugin identity or a shortcut around comparing different definitions with the same plugin ID; contribution bytes and provider requirements remain the canonical cross-runtime inputs.
- Static dependency descriptors are lowered once per Rust `TypeId` within a candidate graph and their normalized dependency identities are snapshotted alongside each selected plugin. Equivalent aliases and diamonds reuse a frozen descriptor; duplicate checks and recursion consume the same selected metadata. A count-based test covers shared dependency types without repeated author-code execution. These type keys are process-local memoization, not portable plugin identity.
- `StaticPluginGraph::compose` now rejects cross-plugin contribution conflicts and forged owners before returning a graph. It freezes the validated contributions once. Later `graph.contributions()` calls read the snapshot without executing plugin-provided lowering code again. This covers static SDK preparation only; normal kernel candidate activation still needs these declarations wired into generation identity, schema checks, and reconciliation.
- Portable JSON conformance now covers multiple independently authored selected artifact envelopes matching the canonical contribution stream of a typed Rust dependency graph, including modules, typed constants and struct-field contributions. A negative owner-spoofing case rejects the portable artifact. This proves normalization parity, not dynamic non-Rust artifact activation.
- Explicit stateful instance preloading requires the Rust definition to belong to the validated static graph. A matching ID and definition string alone cannot authorize an unselected type.
- Static preparation rejects resource-only and external-runtime descriptors that carry an embedded factory, before a kernel attempts to preload executable code.
- Duplicate embedded Rust plugin identities with executable factories are rejected across distinct types. Matching strings and factory presence cannot prove that two binary implementations are equivalent.
- Contribution lowering is evaluated once per selected Rust definition. Duplicate comparisons and final graph collection reuse snapshots, so mutable author callbacks cannot silently change the prepared contribution set through repeated reads. Selected duplicate declarations must agree; only the original canonical snapshot enters generation preparation.
- Explicit embedded state preloading uses the frozen selected Rust type and descriptor snapshot. Loading a selected instance cannot rerun descriptor or dependency author callbacks, even if their values change after preparation. A changed declaration requires a new candidate composition. These SDK checks still do not replace kernel candidate-generation enforcement.
- Before installing any embedded factory, SDK preloading compares the entire selected static dependency graph against the kernel candidate's manifests. Plugin versions, execution kinds, maximum authority, and frozen dependency identities must match; any discrepancy rejects the batch before the first factory is installed. Explicit state preloading enforces the same check. This guards executable revisions against stale manifest selection; it does not yet freeze or verify generated service and resource declarations against the complete candidate.
- Missing: general contribution-kind schema validation, automatic candidate-generation integration, arbitrary-runtime artifact discovery and full non-Rust conformance. Do not claim Stage B integration is complete.

## Canonical envelope

A possible implementation shape is:

```rust
struct Contribution {
    owner: PluginId,
    identity: ContributionId,
    kind: ContractId,
    role: ContributionRole,
    payload: ContractValue,
    requirements: Vec<ContributionRequirement>,
    ordering: Vec<ContributionOrder>,
}

enum ContributionRole {
    Declare,
    Require,
    Provide,
    Modify,
    Observe,
}
```

This is a suggested boundary shape, not a commitment to these identifier names. Keep exact plugin selection, contract provider requirements and contribution kinds distinct. `#[phenix(dep)]` is still a concrete Plugin dependency. `Require<T>` requests a compatible capability without pinning an implementation; `Provide<T>` offers one; `Declare<T>` publishes descriptive metadata; `Modify<T>` supplies immutable edits. A single authored method may lower to several canonical contributions where that is already Phenix behavior.

The envelope must be serializable across non-Rust plugin runtimes, reject invalid or duplicate identities, carry schema/version provenance, retain owner for unload/reconciliation, and canonicalize deterministically. Dynamic payload decoding is schema-checked before resolution; typed Rust interfaces remain ergonomic at authoring time.

Avoid representing all product-domain concepts as Core enum variants. Kinds are contract identities and schema values, not cases of a `Tool | Skill | Frontend` enum.

## SDK authoring contract

Reuse `#[phenix_sdk::plugin]` on structs/modules, `#[phenix(component)]`, `#[phenix(require)]`, `#[phenix(provide)]`, `#[phenix(layer)]` and `#[phenix(listen)]`. Existing `import/export` names remain aliases where supported.

Add one typed static contribution role to fields and module constants:

```rust
#[phenix_sdk::plugin("acme.profile")]
mod plugin {
    #[phenix(contribute)]
    const ROUTE: GraphPatch = graph_patch! {
        graph: "phenix.tool.invoke@1",
        edits: [Replace {
            node: "execute",
            with: RemoteExecute,
        }],
    };
}
```

The example presupposes later Stage C graph patch types. The purpose of Stage B is that the annotation extracts a type that implements one common static contribution contract, not that it knows graph syntax or Tool/Skill kinds. Authoring should also accept an ordinary `const GraphPatch { ... }` when the data is practical as a struct literal. Builder APIs are opt-in helpers.

A plugin should be able to contribute several instances of the same kind and mix these with components and requirements without writing `StaticGraphContribution` impls for marker types.

## Lowering and authority

- Macros generate typed descriptors, not startup registration code that silently runs.
- Static Rust and portable package metadata normalize to exactly the same envelope and resolver input.
- A contribution does not grant new authority. Exports, imports, graph edges and generated bindings still face existing authority checks.
- Canonical payload/schema and contributor provenance must be included in candidate generation identity. Plugin discovery order must not alter the result.
- A manual dynamic SDK path may submit the same typed contribution data for a fresh generation or an explicitly supported session-scoped catalog. It cannot mutate an active resolved generation.

## Implementation ownership

Stage B may change `phenix-contract` identities/schema, `phenix-sdk` static descriptor authoring, `phenix-sdk-macros` and additive normalization in Core. Keep `phenix-core/src/workflow.rs`, `phenix-core/src/composition/resolver.rs`, `phenix-harness/src/runtime_builder.rs` and the agent plugins untouched while #726 is active. If a shared integration seam is required, use a separate additive module and defer wiring to Stage C.

Do not introduce domain-specific SDK default tool/skill registration in Stage B; Stage D/E own that.

## Acceptance tests and completion

1. A Rust plugin with an annotated const and an annotated field lowers both into one inspectable contribution stream.
2. Equivalent portable metadata yields byte-for-byte canonical descriptors and matching generation input identity.
3. Independent contributions are input-order invariant; duplicate stable IDs and mismatched kinds fail.
4. A missing required contract rejects candidate resolution, whereas an optional contract remains unbound.
5. An exact `#[phenix(dep)]` remains distinct from an interface requirement.
6. Ownership follows plugin removal, and a contribution cannot grant authority by itself.
7. Existing component/service/layer/listener and `StaticPluginGraph` authoring regressions still pass.
8. No alternate provider resolver, activation hooks, or hidden domain catalog is introduced.

Completion requires implemented code, focused tests and verified Core/SDK behavior. This document alone does not satisfy the merge gate for Stage B.

### Authoring and boundary update

`#[phenix(contribute)]` is supported on module constants. For plugin struct fields whose type is metadata-only, use an explicitly named static constant:

```rust
const POLICY: StaticRawContribution = /* versioned contribution */;

#[phenix_sdk::plugin("acme.policy")]
struct Plugin {
    #[phenix(contribute(value = POLICY))]
    policy: StaticRawContribution,
}
```

The const-path requirement prevents metadata gathering from calling an arbitrary factory supplied as an attribute expression. Both authoring paths still use `StaticContributionDefinition` to lower typed values; a selected contribution-kind provider must subsequently validate domain-specific schema and lowering. Procedural lowering purity remains an open concern and is not an implicit guarantee from the macro alone.

The contribution envelope belongs to `phenix-contract`, and `phenix-sdk` depends directly on this passive crate. No `phenix-core/src/lib.rs` changes are required by this PR, avoiding an implementation overlap with #726. Shared `Cargo.lock` edits must be reconciled after #726 merges.

The portable-form conformance test demonstrates that equivalent JSON and annotated Rust descriptors serialize to identical canonical bytes; it does not yet prove full non-Rust artifact activation.

## Service-only authoring conformance

`rust/crates/phenix-sdk/tests/service_only_provide_conformance.rs` uses a typed `#[phenix_sdk::contract]`, one stateless `#[phenix(provide(Echo), public)]` function, and no plugin dependencies. The test prepares its generated descriptor, resolves Core imports, starts the embedded provider and invokes it through the existing kernel dispatch. The annotation's terminal role is inherent in `provide`; authors do not need to specify a second `terminal` flag. This proves the minimal Rust common path at the current embedded SDK boundary. It does not prove non-Rust artifact loading or RFC plan-backed execution. The host declares a resource-only root component with a typed service import. `RootExecutionHandle::invoke_import` uses that canonical selected binding without a plan or the legacy service registry. Resource-only plugin root fields may declare imports; embedded component fields and executable handlers remain rejected.

## Target-state contribution completeness

**Normative target: [kernel RFC #736](https://github.com/matthis-k/phenix-ai/pull/736).** The current typed contribution stream is an intermediate authoring representation. Candidate activation has not yet integrated all of these contracts; this section adds semantic gates without claiming they are implemented.

- A versioned, plugin-owned `ExecutionPlanDefinition` is an ordinary canonical contribution. Its metadata includes `PlanId`, revision, IR semantic version, typed entry and output, frame schema revision, step/edge/slot identities, scope requirements, imports and provenance. The resolved plan body and applied patches contribute to generation identity.
- A versioned `EntryBinding` is a separate contribution associating a typed trigger, command or application entry with one `PlanId` and revision constraint, validated input projection and admission authority. Ambiguous, stale or unauthorized plan-backed entries fail candidate resolution or admission. Direct service-only roots remain valid without any plan.
- A plan's owner alone may revise entry, global input/output contracts and root scope policy. Ownership and version validation occur before subplan expansion or patching. Multiple competing definitions for a plan identity require explicit selection or candidate failure.
- Selected contributions are collected from Rust, portable configuration and guest artifacts through one canonical preparation path, verified against manifest owners, normalized and **frozen before the resolver starts**. Neither provider selection nor graph composition executes plugin author code. Candidate resolution reads a single frozen snapshot and includes its canonical bytes and provenance in generation identity.
- Exceptional procedural preparation, if later supported, runs in a separate bounded, no-ambient-authority stage. Its validated result is frozen and digested. Re-execution equality is a separate reproducibility policy, not an implicit guarantee.
- A normal service-only plugin requires a contract declaration, implementation and `provide` annotation. The SDK derives its descriptors and executable wrapper without author-written registration tables, workflow declarations, graph patches or kind providers.
- Interface contract result schemas distinguish declared recoverable result variants from the execution envelope `Completed` / `Failed` / `Cancelled`. Language binding notation such as Rust `Result` cannot silently reclassify host failures as business outcomes.

**Additional tests before semantic completion:** portable and Rust plan/entry descriptor parity; two conflicting plan owners; stale or ambiguous entries; owner-only metadata; frozen-data-only resolution; service-only plugin with no workflow dependency; and no second registration or resolver code path. Execution semantics and IR belong to #726, structural patching to #729, and domain kinds to #730.
