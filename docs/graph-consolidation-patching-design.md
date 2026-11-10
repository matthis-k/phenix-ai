# Graph Consolidation: Patching Repair

Status: **design accepted with the Phase 2 revisions below**. This document
replaces only Batch C's rejected patch-observation proposal, not the A/B designs.
No implementation or regression results are claimed.

Phase 2 revision: use a private fieldwise comparator for stored GraphContract and
InterfaceShape metadata, including local schema text, rather than public semantic
equality. Use that comparator for observation and exact request-lift matching.
Enforcement requirements remain private and distinct from public revision events;
public event expansion is not required for write validation. The independent
re-review is in graph-consolidation-critique.md under Revised Batch C. Candidate-
owned rollback is the unchanged baseline guarantee, not rollback of captured Rust
effects. No custom callback is forced to claim Graph when precise ownership exists.

Source basis: HEAD `d758291c1db77abcff60ac3ac0e167c0f59261bd`, the baseline of
[the original design](graph-consolidation-design.md) and
[the critique](graph-consolidation-critique.md). Source edits appeared concurrently
during research in `authoring.rs`, `lib.rs`, `scheduler.rs` and `tests.rs`; they
are not part of this proposal. References below identify baseline declarations
and functions, not stable line numbers in that evolving worktree.

## Phase 0: Problem And Boundaries

### Actual Reachability

`PatchBuilder::custom` accepts an unrestricted
`FnOnce(&mut GraphBuilder) -> Result<(), String>`. Private builder fields are not
directly accessible to external callbacks, but ownership is unrestricted. Legal
callbacks include `*g = GraphBuilder::new("replacement")`, swapping in a captured
builder, and `*g = g.clone().limits(new_limits)`. A callback can also clone,
consume and reconstitute a builder, stage layouts through new handles, import
packages, edit nested claims, or perform cross-kind replacements. Instrumenting
only `connect`, `replace` and export-addition methods is not complete observation.

Current `assembly_changes` in `authoring.rs` scans after-only declarations,
factory/token changes, edge membership and new export/trigger names.
`interfaces::claim_changes` adds some claim differences. Neither compares the
complete candidate state. In particular, equal manifests do not imply equal
ports, preparation, policy configuration, routing, identities or provenance.

Current enforcement maps `AssemblyChange` to `PatchTarget`, then accepts any
declared target that `overlaps` that observation. This is wrong for a fallback
`Graph`: every narrow claim overlaps `Graph`. Planner conflict intersection is
not application write coverage.

### Required Invariant

For each selected callback, every observed candidate-owned state difference must
be accounted for by a precise target or by `Graph`. Each required target must be
covered by the footprint **already used to resolve the plan**. An observed
`Graph` requires an explicit declared `Graph`. Reject underdeclaration; do not
promote a claim after planning or rerun a consumed callback.

No-op candidate clones remain admissible with an empty footprint. Ordinary
node/instance registration and replacement, connection add/remove, exports,
triggers, effective local layout edits and definition-owned claims retain
precise coverage. Whole-builder replacement and unsupported residual accounting
require `Graph`, rather than being forbidden.

The invariant concerns final state differences, not a trace of every attempted
operation. An edit restored exactly before return need not claim a write unless
it leaves a real token, cursor, ordering or other state difference.

### Transaction Guarantee, Precisely

Keep the existing outer draft in `PatchPlanner::apply` and per-contribution
candidate in `GraphAssembly::contribute`. Commit the entire selected plan only
after all callbacks, coverage checks and splice lowering succeed. Error or
unwinding before that assignment leaves the original **candidate-owned graph
state** untouched. `apply` still does not compile; `build` compiles afterward.

This is not arbitrary callback isolation. Captured `Arc` interior state, files,
external counters and callbacks invoked during compilation cannot be rolled back.
More subtly, a captured `NodeHandle` can call `input(...)` against the original
builder's `Weak` pending store instead of the candidate's store. That is a real,
reachable out-of-transaction graph mutation, not hypothetical closure equality.
It is excluded from candidate rollback and observation just as other captured
mutable state is. Restoring it safely would require a separate handle/ownership
transaction redesign, not this minimal observer fix. Hosts must not use such
out-of-band mutations in patch callbacks. No claim of unconditional rollback for
all effects reachable from unrestricted Rust callbacks is tenable here.

## Phase 1: Proposed Design

### Coverage Is Directional

Keep `PatchTarget::overlaps` unchanged for resolution, including global `Graph`
conflicts, Node/Instance same-name overlap, and the noncompeting splice rule.
Add a private application predicate with this contract:

```rust
// Conceptual private API, not implementation.
fn covers(declared: &PatchTarget, observed: &PatchTarget) -> bool;
```

The observer emits only `Graph`, `Node`, `Instance`, exact `Connection`,
`PublicInput`, `PublicOutput` and `Trigger` requirements. It never emits a
wildcard/pattern/incident/splice requirement. Coverage is the following closed
table, not a default call to `overlaps`:

| Declared | Covers Observed |
|---|---|
| `Graph` | Every requirement |
| Any non-Graph target | Never `Graph` |
| `Node(n)` or `Instance(n)` | `Node(n)` and `Instance(n)` only; same-name cross-kind authority is preserved |
| `Connection(k)` | Exactly `Connection(k)` |
| `ConnectionPattern(p)` | Exact symbolic connections matched by every specified component of `p` |
| `IncidentConnections(n)` | Exact symbolic connections with source or destination operation `n` |
| `Connections` | Every exact connection, including low-level non-symbolic keys |
| `PublicInput(n)` | Exactly `PublicInput(n)` |
| `PublicOutput(n)` | Exactly `PublicOutput(n)` |
| `Trigger(n)` | Exactly `Trigger(n)` |
| `Splice(anchor)` | No ordinary callback connection changes; lowering remains planner-owned |

Pattern matching must parse the length-prefixed exact key, not split on dots,
colons or `->`. Reuse `ConnectionRoute::from_exact_key`; an unparseable key is
not covered by an incident or pattern claim. `Connection(k)` and `Connections`
still cover it. A missing target slot at an instance boundary is not a wildcard
that satisfies a declared concrete slot. Do not generalize symmetric
`pattern_overlaps` to coverage of two patterns.

### One Observation, Two Consumers

Replace the two partial change scans with one private field-level observation
function in the authoring layer. It returns both revision events and required
symbolic writes; the authoring layer need not import `PatchTarget` (use a private
authoring requirement enum with the seven concrete shapes above). Patching maps
those requirements to targets and uses `covers`. Revision events are not an
independent authority or a way to infer completeness.

Observe against the actual pre-edit candidate baseline, including a detached
snapshot of pending **contents**. Do not defer that snapshot until after running
the callback. Compare borrowed snapshots without invoking actions, policies,
argument builders, factories, `InputLayout::slots`, codecs or the compiler.
Implementation may traverse directly rather than allocate a full second IR.

Use explicit field destructuring without `..` for each owned struct below, so a
new field forces a review of its observation rule. Compare maps in key order and
vectors in stored order. Deduplicate requirements/events deterministically while
retaining the established event order for ordinary unchanged cases such as
`patch_lineage.rs`. Neither debug strings nor compiled manifests are equality
or completeness oracles.

### Complete Field Inventory

Notation: **P** is a precise requirement, **G** is explicit `Graph` fallback,
**N** is clone-only normalization, **E** is an external/unobservable effect.
"Accounted" below means a checked field-to-owner rule, not permission to ignore
the rest of an object after finding one convenient precise change.

#### GraphBuilder (All 10 Fields)

| Field | Observation And Ownership |
|---|---|
| `active_origin` | Compare optional opaque origin identity; changed at callback return is G. A restored `with_origin` scope itself is no write. |
| `composition_origins` | Compare every key/value, including removed/stale entries. P for checked instance-origin entries and imported definitions under the alias rule below; unrelated/orphan residuals G. |
| `module` | Every `ModuleDefBuilder` field below. A changed `scope` or root `id` is G, even if public names match. |
| `pending` | Rc allocation identity is N; compare map presence and every stored template field. Compare effective nodes as well as raw module and pending state; rules below. |
| `dependencies` | Compare union of keys, full `ModuleEntry` contents, and all before/after aliases; rules below. |
| `identities` | Compare union of names and numeric identities. Account import and reverse-ancestor refresh under all affected aliases; unexplained/orphan changes G. These are not merely manifests. |
| `definition_id` | N when root scope and id are unchanged: every `GraphBuilder::clone` deliberately mints it, including transaction and user no-op clones. Never normalize declaration/public-input/dependency identities. Changed scope/id already requires G. |
| `public_input_ids` | Add/change/remove key is P `PublicInput(key)` even if export bytes are unchanged; missing or inconsistent ownership also G. Exact cross-kind routing does not revoke these IDs. |
| `trigger_names` | Add/change/remove key or numeric value is P `Trigger(key)`; validate association with stored triggers, otherwise G. |
| `compile_limits` | Compare `max_depth`, `max_instances`, `max_work` separately; any difference G. |

#### ModuleDefBuilder (All 15 Fields)

| Field | Observation And Ownership |
|---|---|
| `declaration_ids` | P `Instance(name)` for additions/removals/revocations, including unchanged definition names. Extra keys without an instance are G. |
| `claims` | Root claim vector differences require G. Preserve the existing root diagnostic marker if useful, but it no longer grants `Instance("")` coverage. |
| `contract_requests` | Root ordered request differences require G, except the exact checked cross-kind lift below. This includes `require_contract("child", ...)`: it is root-owned storage. |
| `id` | Root definition name difference G. |
| `scope` | Difference G; replacing the whole builder must not masquerade as a narrow removal/addition. |
| `nodes` | Union of before/after names, all raw and effective fields below. Add/change/remove P `Node(name)`; unnamed/ambiguous declarations G. Survivor reordering G. |
| `node_names` | Must equal name-to-current-vector-index derived from raw nodes; checked registration/removal effects belong to those node P requirements. A residual difference/inconsistent index is G. |
| `node_ids` | Must equal raw node ID set; checked additions/removals belong to node P requirements. A residual difference/inconsistent set is G. |
| `next_node_id` | Never blanket-ignore. Check the allocation transition specified below; explained movement belongs to changed nodes, otherwise G. |
| `instances` | Union of before/after names; compare both `name` and `definition`, plus tokens/origins/dependencies. Add/change/remove P `Instance(name)`; survivor reordering G. |
| `edges` | Compare ordered records `(declaration_id, from, to)`, not set membership; exact connections and residual ordering rule below. |
| `next_connection` | Check exact allocation transition below; otherwise G. |
| `inputs` | Compare every `ModuleInput` field, presence and target order; P `PublicInput(old/new name)` except exact cross-kind retargets. Survivor leaf reordering G. |
| `output` | Compare optional presence and every `ModuleOutput` field; P `PublicOutput(old/new name)` except exact cross-kind retargets. |
| `triggers` | Compare every `ModuleTrigger` field and presence; P `Trigger(old/new name)`. Survivor trigger reordering G. |

#### NodeTemplate, Port And Leaves

These rules apply to raw root nodes, pending templates and dependency nodes.
At the root, each difference is P `Node(name)`; in a dependency, apply the alias
rule instead. A changed declaration token is a write even with the same factory.

| Struct | Every Field And Comparison |
|---|---|
| `NodeTemplate` | `composition_origin` by origin identity; `claims` by ordered exact contracts; `id`, `name`, `declaring_scope`, `declaration_id` exactly; `ports` in order; `output` fieldwise; `grants` in order by `key` and `access`; `cohort_mode` exactly; `logic_factory` by `Arc::ptr_eq`. |
| `Port` | `id`, `name`, ordered `slots`; `policy` by `Arc::ptr_eq`; `args` by optional presence and `Arc::ptr_eq`. |
| `InputSlot` | `id`, `name`, `schema` contents, `local_type`, `buffer` exactly. Compare schema text even for local identities; diagnostics are observable metadata. |
| `OutputSlot` | `id`, `name`, `schema` contents, `local_type` exactly. |
| `ResourceNeed` | `key`, `access`; duplicates and vector order are not silently normalized. |
| `GraphContract` | Existing exact equality of `id` and `required`; shape includes `inputs`, `output`, `unnamed_inputs`, and each `DeclaredValue`'s `schema` and `local_type`. Use exact contract semantics, not just contract ID or shape satisfaction. |
| `GraphOrigin` | Existing allocation-identity equality. Its immutable `OriginIdentity.parent` ancestry cannot be edited through the public API; replacement token differs. |

Pointer equality means **same opaque allocation**, not equal captured behavior.
A new equivalent factory/policy/argument builder counts as a write. In-place
interior mutation behind the same allocation is E. Never call `manifest` or run
the factory to guess equality. The same rule applies to actions and codecs
captured transitively by these opaque allocations.

Pending is not just a preferred snapshot of nodes. For every map key, compare
presence and the complete template, then resolve the effective node as
`pending[name]` if present, otherwise raw node. Both representations' differences
belong to that node; a raw-field edit hidden by pending still requires a write.
Extra pending keys without raw declarations, or key/template-name disagreement,
require G. Rc/RefCell wrapper allocation and borrow flags are not semantic data.
Clearing a live pending entry can destroy fluent reconfiguration even if its
template equals raw storage, so its presence change is not normalized away.

#### Dependency Payload (No Omitted Fields)

| Struct | Fields |
|---|---|
| `ModuleEntry` | Full `definition`, ordered `triggers`. |
| `ModuleDef` | `declaration_ids`, `claims`, `contract_requests`, `id`, ordered `nodes`, `instances`, `edges`, `inputs`, optional `output`. No builder cursor/index/scope exists in this payload. |
| `ModuleInstance` | `name`, `definition`. |
| `ModuleEdge` | `declaration_id`, `from`, `to`; endpoint enums compared by variant and all numeric addresses or symbolic strings. |
| `ModuleInput` | `id`, `name`, `port`, `slot`, `schema`, `local_type`, ordered `targets`. |
| `ModuleOutput` | `id`, `name`, `schema`, `local_type`, `source`. |
| `ModuleTrigger` | `id`, `name`, ordered `targets`. |
| `ModuleTriggerTarget` | Variant plus `PublicInput(u16)`, `Internal(InputAddr { node, port, slot })` or `Source(String)` payload. |
| `ModuleInputTarget` | Variant plus `Node(InputAddr)`, `NodeNamed { node, port, slot }`, `Instance { name, port }` or `InstanceNamed { name, port }` payload. |
| `ModuleOutputSource` | Variant plus `Node(OutputAddr { node, slot })`, `NodeNamed { node, output }`, `Instance { name, port }` or `InstanceNamed { name, port }` payload. |

`ModulePackage` is not mutable storage inside a builder, but its imports must not
escape accounting: `root` and `dependencies` become entries; `identities` and
`composition_origins` merge into the maps above. `compile_limits` belongs to the
package's own compilation, is not copied by `instance`/replacement, and is only
a root-builder write when a builder itself changes limits.

### Coupled Fields Without False Global Writes

**Registration indexes and cursors.** Validate both endpoint states' indexes
against their declarations. For `next_node_id`, start at the before cursor,
remove disappeared raw node IDs from the occupancy set, then replay new raw node
registrations in after-vector order with the actual `ModuleDefBuilder::add`
algorithm: insert the final registered ID and advance while occupied. Same-name,
same-kind replacements retain IDs; a cross-kind module-to-node registration is
new even if the name existed. Require the resulting cursor to equal after.
Unexplained intermediate allocation history or a changed existing physical ID
that does not fit this replay requires G in addition to any node requirement.
Do not recompute the cursor as minimum-free-ID; removals do not rewind it.

For connections, newly minted record IDs must be exactly
`before.next_connection + 1 ..= after.next_connection`, once each, in append
order; retained IDs are allowed only for retained records or checked in-place
substitution retargeting. Missing minted IDs, duplicate IDs or unexplained cursor
movement require G. Thus a connect followed by disconnect may leave a G cursor
write even when final topology is unchanged. That is not a no-op clone.

For edge record changes, emit the exact old and new connection requirements,
including remove/re-add at the same endpoints with a new declaration ID. Compare
the surviving record subsequence, including relative order. Pure reordering is
G; normal deletion and append are precise. Do not sort topology or fanout vectors
to hide order changes. Export/trigger/node/instance survivor order uses the same
rule; order within one named object is owned by that object's precise target.

**Dependency alias rule.** Build finite, cycle-safe reachable definition sets
for every direct instance in both before and after states. A changed existing
entry, identity, or definition-owned provenance requires `Instance(name)` for
**every** direct instance whose before or after closure contains it. Do not stop
after the first alias or inspect only after instances. Removed aliases already
require their own instance write. Identity-only changes still count, including
the reverse-ancestor refresh performed by `refresh_dependency_identities`.

New entries/identities reachable exclusively from newly added/rebound instances
are owned by those instance requirements, not automatically G. Unchanged shared
entries need no additional requirement. Changed entries shared with untouched
aliases require those aliases too. A changed map key or definition with no
before/after owning instance is G; missing references or key/id disagreement is
also G. This intentionally treats imported but unreachable historical package
entries as G, not as silently irrelevant metadata.

For origin-map keys, associate direct instance declaration tokens and dependency
`declaration_ids` with the corresponding owning instances/alias closures. Require
all owners when shared; orphan changed keys are G. Node origins live on templates
and use node/alias rules. A successful import/rebind can therefore stay precise
when its imported state is owned; arbitrary map differences cannot piggyback on
the mere existence of an instance addition.

**Cross-kind replacement.** Preserve the precise replacement API rather than
requiring every generated export write to be claimed manually. Recognize an
actual before-node/after-instance or before-instance/after-node transition for
the same name. Derive its exact public endpoint mapping from the before/after
node and module interfaces, using the same numeric/named alternatives as
`retarget_substitution`. Only matching endpoint substitutions in root input
targets and root output source are attributed to that operation's Node/Instance
requirement. Preserve export ID, name, identity, target multiplicity and order.
Other field/target differences still require PublicInput/PublicOutput writes.
Connections remain explicit connection requirements, covered by the existing
`IncidentConnections(name)` claim that `PatchBuilder::replace` adds.

For module-to-node request lifting, compute the ordered requests that the
removed module's root actually contributes, prefixing paths exactly as
`substitute_module_with_node` does. Root claims must be unchanged. Match only
those appended request entries, with multiplicity, to actual same-name
transitions covered by Node/Instance claims. Existing request prefixes may not
change. Require the exact expected lift, not an arbitrary subset. Multiple
transitions may be matched as ordered per-transition blocks, each used once;
independent leftover requests require G. Changed root claims require G separately.
No root diagnostic event, boolean `requests_lifted`, or empty-name target can
skip an unrelated G observation. A simultaneous limits edit still fails without
Graph even when all request lifts are legitimate.

### Before And After APIs

Public construction signatures stay unchanged:

```rust
PatchBuilder::custom(spec, |g: &mut GraphBuilder| -> Result<(), String> { /* ... */ })
PatchBuilder::replace(name, replacement)
PatchPlanner::apply(self, &mut GraphAssembly) -> Result<PatchResolution, PatchError>
```

Before: `assembly_changes -> Vec<AssemblyChange> -> observed -> overlaps`.
After: private full observation yields revision events plus concrete required
writes; patching validates each write with `covers`, then commits the same draft.
`PatchError::UndeclaredWrite { patch, target: Graph }` diagnoses a missing fallback
claim; no new error variant is needed. Explicit root changes now need `Graph`,
not the old accidental `Instance("")` claim. No new public mutation IR, equality
trait, action identity protocol, or capability wrapper is introduced.

Revision API choice: **add** `GraphChanged`, `NodeRemoved(String)`,
`InstanceRemoved(String)`, `InputChanged(String)`, `OutputChanged(String)` and
`TriggerChanged(String)` to public `AssemblyChange`. Retain existing addition
and replacement variants. The three `*Changed` events include same-name edits
and removals; renames emit old/new-name events. `GraphChanged` records residual
or graph-owned differences; it is not the enforcement channel. Do not mislabel
a removal as an exposure or silently omit it from successful revision history.

This is a source-breaking enum extension for external exhaustive matches:
`AssemblyChange` is public and not `non_exhaustive`. The patch API maintainer
must approve the release/API consequence. Adding `non_exhaustive` now is itself
a separate source-breaking change, not a compatibility fix. This proposal does
not add it. Existing root `InstanceRebound("")` diagnostics may be retained for
continuity, but add `GraphChanged` for non-lift root changes and do not derive
authority from that legacy marker.

### Projection Repair (Independent Subchange)

Keep public `Module::expose` and `expose_matching` signatures. Extract one private,
nonmutating leaf preflight shared by both: scope and revision validity, selected
and destination identities, endpoint resolution, existing export `port`, `slot`
and identity, duplicate target, and valid existing public-input token. Return a
planned append/new leaf without allocating IDs or changing vectors.

Manual exposure preflights one leaf and capacity, then commits. Matching exposure
keeps its complete frontier/duplicate-leaf checks, preflights **all** leaves and
aggregate new-input capacity, then commits all. Capacity is `total - 1 <=
u16::MAX` for nonzero total, allowing ID 65535. Token allocation occurs only in
commit; rejected manual or late-leaf bulk operations leave tokens and targets
unchanged. Never implement bulk projection by repeatedly calling mutating
manual exposure. `Module` dereferences to `GraphBuilder`, so adversarial existing
exports introduced through that public surface are reachable test fixtures.

## Exact Review And Test Gates

These are required tests to implement later, not tests run for this document.
Use public callbacks for reachable regressions, private observer unit tests for
individual fields not publicly addressable except by wholesale replacement.
Private field pokes must not be presented as public mutation APIs.

| Proposed Test | Fixture And Exact Assertion |
|---|---|
| `graph_fallback_requires_explicit_graph_claim` | For `*g = GraphBuilder::new("other")` and separately `*g = g.clone().limits(changed)`, test empty claims, unrelated Node, related Node plus every affected ordinary name, and Graph. First three return `UndeclaredWrite` targeting Graph; explicit Graph succeeds. |
| `underdeclared_broad_edit_cannot_bypass_resolution` | Seed a worker. A low-priority patch claims only a different node but changes limits; a high-priority patch writes worker. Both resolve as selected, application rejects low patch at Graph and rolls back high patch too. Repeat with honest Graph claim: low patch is superseded without running; equal/missing priorities conflict; explicit dependency orders both. |
| `empty_noop_and_clone_noop_need_no_claims` | Test empty callback, one/multiple `*g = g.clone()`, nested restored `with_origin`, and failed rolled-back `additions` caught inside callback. Empty requirements and no GraphChanged, with pending nodes and dependencies already present. |
| `node_payload_fields_are_not_factory_equality` | Table-driven private tests change each NodeTemplate field, each Port field and each slot/output field independently while retaining factory allocation wherever possible. Exact Node requirement, never an empty observation. Separate public same-template replacement test proves token revocation counts with identical factory. |
| `pending_changes_include_raw_and_effective_state` | Public callback declares and retargets a node through its candidate-owned handle, requiring only its Node claim. Private cases change pending template, remove equal-to-raw pending entry, change hidden raw template, add orphan pending key. Node for owned changes; orphan also Graph. Retarget failure leaves prior effective layout/token. |
| `destructive_and_same_name_routes_are_observed` | Private cases remove node/instance/input/output/trigger; independently alter every export and trigger field, reorder targets, and change public-input token/trigger-name mapping only. Assert required old/new precise names and events; malformed associations additionally Graph. Public wholesale replacement removing these objects requires Graph. |
| `edge_records_and_order_are_not_sets` | Disconnect/reconnect a public symbolic edge: exact connection claim succeeds, no claim fails, declaration ID changes. Private duplicate-ID/cursor-gap/reorder cases require Graph. Ordinary precise deletion/addition succeeds. Pattern, incident and Connections claims cover only the intended exact keys. Include numeric endpoints and punctuation in symbolic names. |
| `all_builder_bookkeeping_is_accounted` | Private table changes each of the 10 GraphBuilder and 15 ModuleDefBuilder fields individually, plus every CompileLimits member. Include cursor-only change, inconsistent indexes, origin-only change, scope change and root-name change. Assert the table's precise/G rule, not merely nonempty diagnostics. |
| `dependency_changes_cover_every_alias` | Two direct aliases of one nested shared definition, plus an unrelated package. Nested `implements` requires both aliases; claiming one fails and both succeed. Identity-only refresh and before-only alias private cases also require all owners. Unrelated instance remains absent from requirements. |
| `dependency_payload_field_matrix` | Private table mutates every ModuleEntry/ModuleDef/leaf field listed above, including entry triggers and before-only dependencies. Require every before/after alias; changed orphan or dangling entry requires Graph. New reachable package import/rebind with only its Instance claim succeeds. New unreachable historical entry requires Graph. |
| `cross_kind_retargeting_stays_precise` | Public node-to-module and module-to-node replacements with input fanout, output, incident edges, claims and request lifts. Existing PatchBuilder::replace claims suffice. Private extra target/identity/order edit requires the export claim or Graph per table; never swallowed by substitution recognition. |
| `request_lift_does_not_mask_graph_changes` | Legitimate module-to-node lift plus limits edit: narrow replacement fails at Graph. Extra root claim, changed prefix, extra request, duplicate/missing lift and two-transition cases exercise exact matching. Graph permits graph-owned residuals, not bypass of normal construction/compiler validation. |
| `candidate_failure_rolls_back_whole_plan` | Earlier selected patch adds a node/export/trigger and later patch retargets a candidate node, then returns Err or fails coverage. Assert original field snapshot, pending Rc identity/contents, effective layout, revisions, origins, IDs/cursors and contributor set unchanged. Retry earlier/later contributor names successfully. Probe cached original endpoints and fluent handle usability afterward. Repeat with outer catch_unwind for panic. |
| `rollback_scope_excludes_captured_mutation` | Deliberately mutate captured original-store layout handle or shared policy counter and then return Err. Document/test that this external mutation is not restored; do not misrepresent it as an atomicity test that should pass. Candidate-owned handle mutations in the previous row must be restored by discarding the candidate. |
| `manual_projection_preflight_is_atomic` | Install incompatible same-name export through dereferenced builder; identity or port/slot mismatch rejects manual expose with exact token/target/ID snapshot unchanged. Cover local and encoded identity, duplicate target, foreign/stale selector/destination, valid fanout and ID 65535 boundary. |
| `bulk_projection_late_failure_is_atomic` | First leaf plans an addition, second plans fanout, final leaf fails identity/revision/duplicate checks. No earlier addition/fanout/token survives. Capacity overflow fails before commit; valid bulk projection preserves leaf/target order and existing IDs. |

Retain and run `patch_planner`, `patch_lineage`, `patch_syntax`,
`patch_output_routing`, `canonical_interfaces`, `symbolic_pending`,
`typed_operations`, `matching_projection` and `module_projection_semantics`,
plus additions/authoring unit tests and compile-fail documentation. In particular
retain same-package/same-factory replacement, namespace overlap, independent
rewires, opposite-end incident claims, all splice composition/conflict/rollback
tests, and execution through projected independent children.

After separate design approval, verify `cargo fmt --all -- --check`, targeted
tests, then `cargo test -p phenix-graph` and the project's required exact-head
checks/CI. Review the implementation field-by-field against this inventory;
passing empty-footprint tests alone is insufficient. No performance improvement
is asserted; measure authoring overhead if full dependency traversal is material.

## Limitations And Acceptance

The minimal repair is still a correctness expansion, not deletion-only
consolidation: one directional coverage predicate, one complete observer, checked
ownership of coupled fields, existing candidate commit, and shared projection
preflight. It does not sandbox Rust callbacks or compare opaque behavior.

Graph fallback conservatively serializes/conflicts whole-builder changes,
limits, root claims/requests, orphan historical imports/origins, unexplained
allocation history and survivor reordering. Such edits remain possible through
an explicit Graph claim and ordering/priority, subject to normal validation.
Normal owned imports, replacements, layouts and nested claims remain precise.

Successful contribution clones already detach fluent handles pointing to the
previous pending store; this proposal does not promise that pre-contribution
configuration handles keep following a successful commit. Cached endpoint tokens
retain the existing scope/revision semantics. Failed candidate edits must not
detach or revoke original handles. Global token allocation is monotonic and is
not rewound on failure; rollback preserves stored tokens, not global counter
history. Clone-only definition IDs and pending allocation identity are explicitly
normalized, not evidence of equal package identity or equal handle connectivity.

The patch API maintainer/project owner must accept fallback conservatism and the
public enum extension. Hosts must accept the callback trust/rollback boundary.
If the requirement is instead rollback of captured original handles or arbitrary
interior state, reject this scope and review an ownership redesign separately.
Implementation remains blocked until this revised design receives independent
review; neither this document nor the previous critique grants acceptance.
