# Composer-Owned Registration

Status: native implementation and independent review of the bounded WIP #1259
scope; final exact-head hosted acceptance is required before superseding that WIP.
The historical branch's mutable graph/Engine authoring API is not transplanted.

## Phase 0: Actual Ownership And Failure Gap

Plugins currently register a Kernel `Contribution`, its `NetworkOffer` records and
`ContractRequirement` records independently. Offer `producer` and requirement
`consumer` keys repeat the composer's ownership identity. A plugin can catch a late
registration error after earlier members have already been installed, leaving an
incomplete declaration. Runtime ownership checks reject bad keys later, but do not
make authoring ownership automatic or bundle admission atomic.

Preserve declarative `NetworkPatch`, optional scoped `CandidateContext` preparation,
candidate artifacts/activation, semantic contracts, host-selected imports, exact
write/inspection grants, revision provenance, leases and publication lifecycle.
Import authority never grants graph-write authority.

## Phase 1: Alternatives And Selected API

- **Unchanged:** no migration, but duplicated ownership and partial registration
  remain possible.
- **Aggregate all metadata in the registered composer:** avoids generic bundle
  admission, but moves existing owner-wide offer/import namespace collision checks
  from registration to Kernel build unless another nested index is introduced.
- **Selected same-owner atomic bundle:** reuse the graph-independent Registrar and
  existing private resolver records. Preflight all incoming names and owners before
  mutating the destination registrar. This retains current namespaces and timing,
  without graph-specific hooks in the plugin crate.
- **Old WIP wholesale:** rejected; restoring mutable composition callbacks, Engine
  wrappers or triggers would reintroduce obsolete authorities.

There is one canonical public Kernel authoring route:

```rust,ignore
Contribution::new("service", declarative_patch)
    .after(dependencies)
    .route(public_inputs)
    .prepare(|candidate| Ok(CandidateArtifacts::default()))
    .offer("primary", "service", definition_key)
    .import("dependency", other_definition_key)
    .register(registrar)?;
```

The public authoring object is not a registered plugin payload. Registration derives
producer/consumer keys from `registrar.owner()` plus the contribution ID and admits
the composer/metadata together. Registered composer, offer and requirement types
are private. Remove standalone offer/import authoring and direct
`Registrar::add(kernel::Contribution)`; no compatibility registration route,
extension trait or second builder is retained. Contract definitions and unrelated
plugin contributions continue using the existing generic `Registrar::add`.

`Registrar::merge(Contributions)` is mutable, single-owner admission, not the
host-side owner-preserving inventory combination in `Contributions::merge`.
No permission is inferred from an owner string. The host still validates replaced
registrars, exact origins, imported contracts and final topology independently.

## Phase 2: Independent Critique

Independent review conditionally approved this bounded adaptation and found a new
identity bug: `Registrar::add` calls `value.id()` once to validate and again to
store. A stateful implementation can return different identities. Capture the ID
once and validate that stored value; bundle preflight validates stored entries
without invoking erased payload callbacks.

The review requires failure atomicity for both installed entries and reserved
names, including a caught late offer/import collision followed by successful reuse
of the failed bundle's fresh names. It also requires real sibling/foreign composer
origin-forgery tests, not deleting tests because explicit wrong producer keys become
unrepresentable. Failed bundles must be destroyed while provider code is alive;
ordinary Err atomicity is not rollback of arbitrary effects or destructor panics.

Repeated `after`/`route` calls append; offer/import calls accumulate and duplicate
IDs fail registration. A repeated `prepare` call replaces the prior authoring
callback rather than implicitly composing callbacks. Atomicity is per composer
bundle, not every declaration made by one PluginInstance. The plugin crate already
depends on Graph for binding; the registry/merge implementation itself remains
graph-independent and acquires no Kernel-specific hook.

## Acceptance Gate

Independent review must challenge owner substitution, caught late collisions,
names reserved after a failed merge, invalid IDs, duplicate offers/imports across
composers, contribution code destruction, namespace compatibility, self-imports,
missing/ambiguous providers, and selected-code lease retention. Final origin and
contract checking must not be weakened merely because metadata ownership is now
derived.

Migrate native callers and independently compiled fixtures explicitly. Test hosts
retain authored patches/keys and grant their exact footprints before registration;
do not expose private registered payloads through a new public inspector for tests.
Retain forged-origin coverage using a real sibling composer advertising another
composer's unit. Add a host integration proof that an independent plugin replaces
a nested authorized seam while a consumer imports the outer contract, including
failed candidate preservation and retained old-generation execution.

Completion requires registration atomicity, native composer declaration tests,
existing import/catalog/provenance/lease suites, cross-plugin nested extension,
zero obsolete public authoring routes, strict checks and exact-head CI. It does
not implement general private Add/Connect extension or incremental compilation.

## Phase 3: Implemented Migration

The authoring builder owns a private registered payload plus offer/import drafts;
registration derives linkage and admits the complete same-owner bundle. The old
public ownership-bearing records and direct `add(Contribution)` route are removed.
All current native examples, adapters, lifecycle/lease tests and independently
compiled consumer fixtures use the new route. Host fixtures retain exact authored
footprints and keys instead of inspecting private payloads. No public test-only
inspector, wildcard grant or privileged preset was introduced.

`Registrar::merge` checks destination collisions and a local incoming-name set,
then installs entries/names only after complete preflight. It does not clone the
entire destination namespace on each registration. `add` captures and validates
one ID value, with a stateful-`id()` regression for the newly found identity bug.

## Phase 4: Independent Review And Evidence

Independent post-implementation review found no runtime defect. A strict-check
failure in a migrated fixture and stale public instructions were corrected without
restoring compatibility routes. The reviewer then independently passed workspace,
no-default-feature, strict all-target Clippy, formatting and whitespace checks.

Executable proofs include:

- `composer_declarations`: caught late collisions leave all fresh names reusable;
  invalid/duplicate IDs, reserved-kind spoofs, self-import authority, appended
  dependencies/routes and replaced preparation callbacks, and consumed-bundle
  destruction/leased failure lifetime.
- Two distinct offered units from one composer remain ambiguous without host
  selection; explicit selection binds separate imports and executes results 8 and
  17 rather than collapsing offer identity into producer identity.
- `composer_nested_extension`: three independent plugins, outer-contract-only
  consumer import, exact-granted deep replacement, preserved outer/sibling origin,
  extension leaf origin, retained old-generation execution, and seven rejected
  candidates preserving active pointer and committed plugin revisions.
- Existing import/catalog/origin/preparation-inspection/generation/code-lease suites
  remain intact. Positive Rustdoc and compile-fail examples cover the one supported
  route and reject removed standalone authoring routes.

The bounded implementation criteria are met locally and independently reviewed;
the accepted hosted commit and CI links must refer to this exact assembled head.
This closes an authoring ownership/atomicity gap, not all of A1/A4/A5/A6, all plugin
composability or the production-agent milestones.
