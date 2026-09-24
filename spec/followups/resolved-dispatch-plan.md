---
status: active
source: kernel-runtime-mechanism-audit-2026-09-23
base_sha: e4d653a0d9f6b8ef908a06cb59a4ac7766baf66b
depends_on: runtime-generation
implementation_status: generation topology and invocation hot paths migrated; exact-head validation pending
---

# Precomputed resolved dispatch plans

## Goal

Resolve static dispatch topology once per runtime generation.

Component imports are already resolved to generation-pinned provider plans by #468. Service invocation still rebuilds another representation on every call.

The current component call path:

```text
SdkClient::invoke
  -> PluginHost::invoke_import
  -> ResolvedComponentGraph::provider_plan
  -> choose active primary/fallback
  -> InterfaceId -> ServiceId parse
  -> attenuate authority
  -> KernelConfig::resolve_component_chain
  -> resolve layers
  -> filter active optional/required layers
  -> invoke
```

The graph has already selected the component provider. The service registry should not re-resolve static topology after that point.

## Static versus dynamic facts

Resolve these at generation construction:

- interface to service identity
- component import primary and permitted fallbacks
- terminal plugin for a component-bound call
- candidate terminals for an unbound ABI service call
- ordered configured layers per service
- required versus optional layer status
- each participant's declared required authority
- policy identity and provider selection reason

Evaluate these at invocation time:

- caller/effective authority
- plugin active state
- implementation instance availability
- cancellation
- causal re-entry
- fallback because the pinned primary is unavailable
- continuation position and one-shot use

Do not freeze dynamic authorization into the generation.

## Target types

Use one generation-owned dispatch representation.

Exact names may follow local conventions, but the data model should be equivalent to:

```rust
pub struct ResolvedDispatchTopology {
    services: BTreeMap<ServiceId, ResolvedServicePlan>,
    component_imports: BTreeMap<ComponentImportKey, ResolvedComponentDispatchPlan>,
}

pub struct ResolvedServicePlan {
    service: ServiceId,
    layers: Vec<ResolvedLayerPlan>,
    terminals: Vec<ResolvedTerminalPlan>,
    policy_identity: KernelPolicyIdentity,
}

pub struct ResolvedComponentDispatchPlan {
    component: ComponentId,
    interface: InterfaceId,
    service: ServiceId,
    providers: ResolvedProviderPlan,
    layers: Vec<ResolvedLayerPlan>,
    policy_identity: KernelPolicyIdentity,
}
```

Do not duplicate provider endpoint data already owned by `ResolvedProviderPlan` unless an owned flattened form deletes more code than it adds. Prefer references or shared immutable values inside one `RuntimeGeneration`.

The key invariant is one lookup from the generation to a dispatch plan. No runtime scan/sort/reparse follows it.

## Component-bound invocation

Target call flow:

```text
SdkClient::invoke
  -> PluginHost::invoke_import
  -> generation.component_dispatch(component, interface)
  -> choose first runtime-available provider from pinned plan
  -> authorize selected provider and layers for caller
  -> invoke prepared chain
```

The selected component provider is the terminal. Never pass it into a generic terminal resolver that can select another provider.

Fallback remains limited to the fallbacks pinned in the generation by #468. Runtime unavailability may choose a pinned fallback. Provider execution failure must not trigger generic fallback.

## Unbound service ABI invocation

`PluginHost::invoke_service_abi` still needs service-provider selection.

At generation construction, store the deterministic ordered terminal candidates for each service. Runtime invocation filters that fixed list by:

- optional binding
- caller authority
- active state
- implementation availability

The runtime does not scan all manifests or sort providers.

If no eligible candidate remains, preserve the existing typed errors such as bound-provider unavailable and no eligible provider.

## Layer resolution

`KernelConfig::resolve_layers` currently walks layer policy on every call even though policy order is already sorted during harness resolution.

Store ordered layer candidates in the dispatch plan.

At invocation:

1. filter candidates that caller authority may invoke
2. if an active required layer is unavailable, return the existing required-layer error
3. skip unavailable optional layers
4. preserve declared order
5. attenuate authority for each invoked participant

Do not sort layers during invocation.

Be precise about required layers whose required authority is not permitted by the caller. Preserve current semantics from `resolve_layers`; the composition plan stores the requirement but authorization still runs per caller.

## Interface identity

Convert `InterfaceId` to the corresponding `ServiceId` once when the component dispatch plan is built.

Delete per-call:

```rust
ServiceId::parse(interface.as_str().to_owned())
```

A malformed interface/service identity should fail generation resolution, not a model/tool request later.

If the repository contract intends InterfaceId and ServiceId to share the same validated syntax permanently, consider one typed conversion implementation. Still perform the conversion during plan construction.

## Authority

Keep both layers of authority:

1. component provider plan effective authority from graph resolution
2. per-call caller authority attenuation and service/layer required-authority checks

Do not replace them with one cached effective authority.

The static plan says what a provider may receive. The call scope says what this caller may delegate now.

## Provenance

The invocation provenance must still record:

- generation
- policy identity
- service
- planned participant chain
- component provider primary/fallback set
- actual executed provider
- selection reason
- fallback reason
- effective authority
- participant outcomes

Update the provenance type so it records the prepared plan without forcing the runtime to reconstruct a `ResolvedServiceChain` solely for diagnostics.

The trace/provenance PR should make this bounded. This PR should not reintroduce a second chain representation only because an old provenance field expects it.

## KernelConfig after this PR

`KernelConfig` remains the resolved manifest/runtime-binding configuration owner where other kernel operations need it.

Move or narrow these methods away from the invocation hot path:

- `resolve_chain`
- `resolve_component_chain`
- `resolve_layers`

Preferred outcome:

- composition-time plan builder owns static dispatch resolution
- invocation code receives a resolved plan and only applies live state/authority

Delete old methods if no non-dispatch caller needs them. Do not keep them as aliases.

## Continuation

A layer continuation must resume the already selected plan at the next layer position.

Do not call plan resolution again from `continue_service`.

The continuation may hold:

```text
Arc<ResolvedInvocationPlan> + next_position
```

or a generation reference plus plan key and position. Pick the form that avoids cloning the full chain per layer while keeping the generation pinned.

The later call-scope PR may refine the cursor. This PR must at minimum stop re-resolution.

## Files and mechanical changes

Primary files:

- `rust/crates/phenix-core/src/resolver.rs`
- `rust/crates/phenix-core/src/component.rs`
- `rust/crates/phenix-core/src/registry.rs`
- `rust/crates/phenix-core/src/runtime/host.rs`
- `rust/crates/phenix-core/src/runtime/dispatch.rs`
- `rust/crates/phenix-core/src/runtime.rs`

Mechanical sequence:

1. Define generation-owned dispatch topology types.
2. Build service terminal and layer plans during resolved-harness construction.
3. Bind component import plans to prevalidated service identities.
4. Store dispatch topology in `RuntimeGeneration`.
5. Convert component invocation to one component-plan lookup.
6. Convert unbound service invocation to one service-plan lookup.
7. Replace `prepare_active_chain` with live selection over the resolved plan.
8. Make continuation resume the selected plan.
9. Update provenance to consume the plan directly.
10. Delete hot-path `KernelConfig::resolve_*chain` and repeated interface parsing.

## Invariants from earlier PRs

Preserve #468:

- composition policy owns provider choice
- provider plans are generation pinned
- availability never triggers live provider search outside the pinned plan
- execution failure does not trigger generic fallback
- provenance records selected plan and actual provider

Preserve #494:

- repeated component endpoint re-entry is rejected
- distinct same-plugin component endpoints may call each other
- legacy mutable-plugin re-entry fails before lock reacquisition

Preserve service layers:

- required layer unavailable is a typed failure
- optional unavailable layer is skipped
- layer continuation is one-shot
- layer authority attenuation remains per participant

## Tests

Add lookup-count and behavior regressions that prove:

- component invocation does not call `KernelConfig::resolve_component_chain`
- unbound service invocation does not scan/sort manifests
- interface to service conversion happens at generation resolution
- primary unavailable selects only a pinned fallback
- primary execution failure does not select fallback
- required inactive layer fails
- optional inactive layer skips
- caller without required authority cannot invoke an otherwise active participant
- continuation resumes the same plan without resolution
- plan/provenance generation changes after reconciliation
- deterministic provider/layer order is unchanged from current behavior

A test-only resolver counter is acceptable if it is local to tests. Do not add production instrumentation only to prove absence of work.

## Deletion target

Delete the per-call static resolution sequence:

```text
provider_plan clone
InterfaceId -> ServiceId parse
KernelConfig::resolve_component_chain
resolve_layers
sort/select static candidates
```

Runtime work should be limited to lookup, live authorization, active availability, and execution.

## Non-goals

- no routing-profile or model-provider redesign
- no change to #468 fallback policy
- no plugin lifecycle redesign
- no call-stack redesign yet
- no EventBus change
- no cache keyed outside the immutable generation

## Acceptance criteria

- [ ] Every component-bound invocation uses one generation-owned resolved dispatch plan.
- [ ] Component terminal selection is not repeated by the service registry.
- [ ] Unbound service calls use preordered generation-owned terminal candidates.
- [ ] Layer ordering is precomputed.
- [ ] Dynamic authority and active availability remain invocation-time checks.
- [ ] Continuations never re-resolve the chain.
- [ ] Old hot-path chain resolver methods are deleted or have no invocation caller.
- [ ] Provider/layer/fallback behavior remains covered by exact regressions.
- [ ] Source, Rust, Clippy, Product, Integration, Docs, and Maintenance checks pass at exact head.

## Follow-up relation

The call-scope/endpoints PR consumes these resolved plans through an explicit invocation scope and removes the remaining copied guard/context state.
