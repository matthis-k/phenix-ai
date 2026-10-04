# Plugin provider resolution

status: implemented
coverage:
  - rust/crates/phenix-core/src/provider_rebind_generation_regression.rs
  - rust/crates/phenix-core/src/provider_availability_regression.rs
  - rust/crates/phenix-core/src/provider_fallback_regression.rs
  - rust/crates/phenix-sdk/tests/plugin_component_authoring.rs


Terminology follows [the architecture glossary](glossary.md).

## Purpose

Resolve Interface providers before activation and pin the result to one immutable Generation.

Effective Authority determines eligibility. Product composition policy determines preference among eligible providers. Runtime dispatch follows the generation-owned Provider Plan rather than searching the live Plugin set again.

This document extends `plugin-contributions.md`.

## Terms

**Provider candidate.** A Component Export compatible with an Interface Import.

**Provider binding.** The resolved association between one Import and one selected provider Component.

**Resolved Provider Plan.** The generation-owned primary binding plus any explicitly enabled fallback bindings.

**Provider availability.** Runtime-local state that can make a provider unavailable for dispatch without changing the resolved generation.

## Resolution inputs

Provider resolution considers:

- the consumer Import and structural schema;
- compatible provider Exports;
- required authority and effective harness authority;
- explicit bindings and configured priority;
- whether fallback is allowed for the Interface;
- the candidate Generation.

Registration order is not provider policy.

A required Import with no eligible compatible provider fails graph construction. An optional Import may remain unresolved.

## Structural compatibility

Provider and consumer Rust types do not need to be identical.

The resolver checks the Interface identity and directional structural compatibility represented by `PhenixSchema`. Implementations remain independent and convert `PhenixValue` into their own local types at execution boundaries.

## Selection policy

Product composition may specify an explicit provider binding or priority. The selected reason is retained with the Provider Plan and provenance.

When no explicit preference changes the result, provider selection remains deterministic from the resolved candidate set rather than from registration timing.

Changing provider composition produces a new Generation. Existing work remains pinned to the generation under which it started.

## Fallback

Fallback is opt-in policy, resolved ahead of activation, and stored in the Provider Plan.

The kernel may use an already-resolved fallback when the primary provider is unavailable and fallback is enabled for that Interface.

An invocation failure after the primary provider begins executing is not an availability signal and does not trigger provider switching. `provider_fallback_regression.rs` locks this distinction down.

Fallback therefore does not perform live provider discovery.

## Availability

Provider availability is operational state for providers already present in the generation-owned plan.

Availability may prevent dispatch to a resolved provider or allow an explicitly planned fallback to run. It does not rewrite the Provider Plan, silently rebind consumers, or create a new generation by itself.

## Authority

A provider is eligible only for authority the composition can grant. Invocation then attenuates caller authority against the selected Plugin and Component limits.

Provider selection never expands authority. Fallback providers are subject to the same compatibility and authority rules as the primary provider.

## Provenance

Component invocation provenance records the Generation, resolved primary and fallback plan, selection reason, executed provider, and fallback reason when one was used.

This makes runtime availability decisions observable without treating them as topology mutation.

## Invariants

- Provider binding is resolved before activation.
- Registration order is not semantic provider preference.
- Provider and consumer implementation crates remain independent.
- Required Imports fail resolution when no eligible provider exists.
- Fallback is explicit and generation-pinned.
- Execution failure never triggers provider search.
- Availability does not silently mutate topology.
- Provider-policy changes create a new Generation.
- Selection and fallback never expand authority.
