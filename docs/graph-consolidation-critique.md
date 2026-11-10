# Graph Consolidation Critique

## Review Basis

Independent preimplementation Phase 2 review of
[the design](graph-consolidation-design.md), batches A, B and C separately.
Inspected workspace HEAD: `d758291c1db77abcff60ac3ac0e167c0f59261bd`, matching
the design baseline. The design was untracked at review time. No source changes
or test execution were performed; source inspection is not regression evidence.
References below are baseline locations under `packages/phenix-graphs/src/`.

**Decision:** A and B receive conditional design acceptance, not implementation
acceptance. C is rejected as written because its proposed fail-closed fallback
does not work with current footprint enforcement. Each batch must satisfy its
own gates; success in another batch cannot compensate for a failed invariant.

## Batch A: Phase 2

### Challenges

1. **Does this delete execution duplication or merely hide it?**
   `lib.rs:2133-2317` has six named private action structs, not seven: typed,
   encoded function, local function, local prepared function, contextual and
   multi-port. Source closures are another lowering path, not another named
   struct in this inventory. The existing closure blanket implementation already
   supplies the execution interface. Replacing structs with six closures that
   each allocate a vector and run their own lowering is a facade change.
   A real reduction centralizes owned-context sequencing and zero-or-one transport
   construction, while retaining genuinely different preparation/conversion.
2. **Preparation does not all belong at the same time.**
   `scheduler.rs:1260-1367` validates selection and retirement, then prepares
   encoded arguments before consuming deliveries. Local structural preparation
   instead takes reserved raw values during execution (`lib.rs:2225-2237`).
   The helper's preparation closure must mean extraction from an admitted context,
   not relocation of all decoding into execution. Otherwise invalid encoded input
   changes from a rejected cohort with intact buffers into a consumed invocation.
3. **Ownership and auto-trait bounds are part of the contract.**
   `InvocationCtx::take_prepared` accepts `Send`, not `Sync` or `Clone`
   (`lib.rs:2022-2035`); local structural fields can be move-only. Capturing
   phantom values or requiring the extracted argument to be `Sync` would reduce
   expressiveness just to satisfy a new closure type. The handler must receive
   the shared context only after its mutable preparation borrow ends, without
   cloning the context or introducing another `Any` transport.
4. **A safe constructor does not make completion trusted.**
   `NodeAction` is object-safe and external implementations can return arbitrary
   vectors. `scheduler.rs:1465-1486` remains the authoritative whole-result check.
   A private `Option<Emission>` cannot justify changing the public trait,
   short-circuiting validation, or accepting only library-generated actions.
5. **Factory lifetime must not become shared handler lifetime.**
   `lib.rs:2339-2376,2602-2619` deliberately distinguishes shared actions from
   per-instance factories. Delegation through `with_layout` must not eagerly call
   a factory and capture one handler for all physical nodes. Multi-port dispatch
   still needs its heterogeneous adapter map; deleting that map is not a useful
   consolidation target.

### Required Revisions And Gates

- Correct the adapter inventory and provide a constructor-to-preparation-to-return
  mapping, including factories and plain/directed sources. Define the helper's
  conceptual signature as mutable-context preparation followed by invocation with
  owned prepared input and shared remaining context, returning
  `Result<Option<Emission>, String>` before the single vector boundary.
- State explicitly that pre-admission encoded validation stays in the port-owned
  argument builder. Capture local layout slots once from the declaration; do not
  call arbitrary `L::slots()` during an admitted invocation.
- Delete the old private executors and duplicated typed/contextual construction
  bodies. Retaining public constructor spellings is acceptable; adding a second
  executor, return trait, erased intermediate, or helper layer per old struct is
  not. Explain any source path excluded by "where practical" in the removal ledger.
- Verify `Send`-only prepared arguments, move-only local structural fields,
  ordinary `Option`/`Result` payloads, suppression versus an emitted value selecting
  no destinations, source contexts with no input port, and frozen layout behavior.
  Check the context still exposes the same metadata and grants after extraction.
- Exercise factory-created state across two physical instances and engine
  instances, heterogeneous ports, and existing compile-fail layout/handler probes.
  Preserve external `dyn NodeAction` and invalid external-completion coverage.
  Preparation failure must not invoke the handler or publish output.

### Verdict

**ACCEPT WITH CONDITIONS** as an internal lowering refactor. The direction is
real simplification if the shared sequencing and vector boundary actually replace
the old bodies. It is not a new execution model or a reason to narrow public
actions. Required revisions above must be made explicit before implementation;
actual diff and exact-head tests remain separate acceptance gates.

## Batch B: Phase 2

### Challenges

1. **One adjacency is justified, but output validation must survive its key.**
   `lib.rs:3694-3721` validates slot zero before inserting identical edges into
   `by_output` and `by_node`. This is a genuine duplicate owner. Select one retained
   representation, not a lazily rebuilt second cache. If it is keyed only by node,
   callers accepting `OutputAddr` must not silently treat slot 1 as slot 0.
   In particular, audit `scheduler.rs:625-639` as well as deployment resolution
   (`deployment.rs:83-112`), publication, structural indexing and manifests.
2. **Removing activity summaries is sound; deleting their assertions is not.**
   `WaveActivity` counts/nodes/active partitions are maintenance-only in production
   (`scheduler.rs:816-943`); tests consume them extensively. A replacement that
   derives pending activity only from `pending_ports` cannot detect a stale pending
   index. Buffers include retained empty queues, so counting map entries is also
   wrong. A buffer scan must count nonempty slots, not deliveries or queue keys.
3. **Admitted, dispatched and reserved are different facts.**
   A ready but undispatched invocation is already an open producer. A dispatched
   ticket represents completion authority, not all producer activity. Reservation
   history survives completion and can describe future staged obligations with
   no buffered inputs (`scheduler.rs:1719-1732`). Neither history nor settled colors
   can be reconstructed just from live tickets or nonempty buffers.
4. **Mutation sharing must not fuse policy powers or transaction phases.**
   `reserve_all` validates all picks and all retirements before decoding and
   consumption (`scheduler.rs:1260-1441`). Selected values become arguments;
   retired alternatives do not. A removal helper must preserve that distinction,
   queue order and occupied-to-empty transitions. "If straightforward" is not
   sufficient permission to combine these phases or move reevaluation between
   source and port admission updates.
5. **A smaller policy result is not automatically a simpler policy model.**
   `PortPolicy` has independent `select`, `retire`, and `incomplete` methods
   (`lib.rs:1427-1457`). Keeping this object-safe boundary is justified. A simple
   Wait/Invoke/Fail enum cannot express explicit disposal and reservation-only
   future obligations. This rejects that particular replacement, not every
   possible richer future decision representation.

### Required Revisions And Gates

- Name the retained adjacency and enumerate all removed reads/writes. Preserve
  edge vector order and IDs, zero-outdegree nodes, sparse IDs, duplicate-successor
  handling, stable cyclic ordering fallback and exact node-to-port reachability.
  If node-keyed, reject invalid output slots at address-taking boundaries.
- Derive diagnostic node state with admitted producers taking precedence over
  nonempty buffers. Derive active nodes as their union and partitions by distinct
  active nodes, not invocation count. Keep these scans out of scheduling hot paths.
- Preserve an independent test oracle: scan actual buffers and ready/dispatched
  work to check pending indexes and producer membership at stable transition
  boundaries. Comparing two views computed from the same accelerator is circular.
  Adapt the capture harness at `tests.rs:8923-8978`, which manually drives
  invocations, rather than assuming it has production ticket bookkeeping.
- Keep optional buffer-removal/admission consolidation out of the initial state
  deletion unless its invariant and phase ordering are explicit. It is not needed
  to obtain the adjacency and summary-state simplification.
- Retain regression coverage for queued versus dispatched producers, malformed
  picks/retirements without partial consumption, decoder failure, rejected cohorts,
  reservation-only staged incompleteness, routed-away destinations, partial fanout
  rejection, cancellation, duplicate/foreign tickets, independent waves and cycles.
  Keep differential targeted-versus-conservative reevaluation coverage; do not
  replace it with assertions about newly derived summaries alone.

### Verdict

**ACCEPT WITH CONDITIONS** for removing the duplicate adjacency and eager activity
summaries. This reduces stored facts and transition obligations, rather than just
renaming them. Optional mutation consolidation is not independently approved until
its ordering is specified. No policy expressiveness reduction is accepted.

## Batch C: Phase 2

### Challenges

1. **Blocking: `Graph` observation currently fails open for narrow claims.**
   The design proposes a `Graph` fallback for unrepresentable edits.
   `PatchTarget::overlaps` makes Graph overlap everything
   (`authoring/patching.rs:96-100`), while application accepts an observed change
   when *any* declared target overlaps it (`authoring/patching.rs:808-823`).
   Therefore a patch claiming only `Node("unrelated")` would pass an observed Graph
   write. Empty-footprint tests would pass while the actual fail-closed property
   remained broken. Conflict intersection and declared-write coverage are not the
   same relation. An observed Graph write must require an explicit Graph claim
   or reject the mutation; a Graph declaration must still conflict globally during
   planning. Do not weaken symmetric planner overlap to fix application checking.
2. **Observation needs a complete, field-level mutation contract.**
   `assembly_changes` walks after-only nodes/instances, compares factories/tokens,
   compares edge membership, and notices only new input/output/trigger names
   (`authoring.rs:1742-1823`). It misses removals, same-name routing/identity changes
   and configuration changes without factory replacement. Pending layouts,
   declaration/public-input IDs, trigger groups/source triggers, origins, limits,
   and dependency definitions/aliases also need an explicit observation rule.
   A custom callback receives `&mut GraphBuilder` and can replace the whole builder;
   a narrow survey of normal convenience methods cannot establish completeness.
3. **Not every structural inequality is a write.**
   `GraphBuilder::clone` intentionally changes `definition_id` and isolates the
   pending map (`authoring.rs:796-813`). Every contribution clones before editing
   (`authoring.rs:1858-1866`). A naive all-fields comparison reports Graph even
   for no-op patches. Conversely, a deterministic manifest omits captured policy
   configuration (`lib.rs:1429-1434`) and cannot establish mutation equality.
   Normalize transactional bookkeeping only; do not normalize real revocations,
   routing changes or observable edge/target order away.
4. **Manual projection must validate the existing export, not just endpoints.**
   `Module::expose` checks selected and destination identity but appends to a
   same-name input without checking that input's port, slot or identity
   (`authoring/operation.rs:936-974`). `expose_matching` does those checks and
   preflights capacity before commit (`authoring/operation.rs:1002-1078`).
   Sharing validation is useful; implementing bulk projection by repeatedly calling
   a mutating manual method destroys bulk atomicity when a later leaf fails.
5. **Candidate rollback is graph-state rollback, not callback isolation.**
   `PatchPlanner::apply` clones and commits once (`authoring/patching.rs:730-849`).
   That does not undo captured `Arc` interior mutation or arbitrary external effects
   by custom callbacks. The root-request-lifting exception at lines 789-811 also
   must not swallow an unrelated fallback observation merely because legitimate
   request lifting occurred in the same edit.

### Required Revisions And Gates

- Specify application coverage separately from planner overlap, at minimum making
  an observed Graph write require a declared Graph claim. Test empty claims,
  an unrelated nonempty narrow claim, a related narrow claim that still does not
  cover the entire edit, and an explicit Graph claim. Include a multi-patch case
  proving a broad mutation cannot bypass priority/conflict resolution by declaring
  only a narrow footprint. Observation happens after resolution, so reject
  underdeclaration rather than silently promoting its claim after planning.
- Inventory every mutable builder/dependency field and classify semantic changes
  as an existing precise target, explicit Graph fallback, normalized clone-only
  bookkeeping, or a documented unobservable external effect. Include before-only
  declarations, changed same-name exports/triggers, pending effective layouts,
  nested/shared aliases and order-sensitive topology. No-op custom patches must
  remain admissible without a Graph claim.
- Choose how fallback changes are recorded and validated. `AssemblyChange` is
  public (`authoring.rs:1702-1712`); adding a variant affects exhaustive external
  matches. Record that API consequence or keep conservative validation internal.
  Do not disguise new mutation authority in a diagnostic-only event or the
  existing root-instance/request-lift exception.
- Share a nonmutating projection preflight; preserve bulk validation of all leaves,
  duplicate targets, scope/revision, existing export identity and capacity before
  commit. Manual failure must leave IDs and target vectors unchanged. A late-leaf
  bulk mismatch must roll back all earlier planned additions/fanout.
- Verify failed patch application preserves topology, metadata, origins, effective
  layouts, usable old handles, revisions and contributor-name availability,
  including rollback of an earlier successful selected patch. Also verify precisely
  declared destructive/configuration edits still succeed where supported.

### Verdict

**REJECT AS WRITTEN.** Reconsider after explicit Graph coverage and complete
observation rules are specified. Projection preflight reuse is independently
acceptable with the stated atomicity gates. Footprint work is primarily a
correctness repair, not evidence of simplification: reducing duplicated validators
is a gain, while a larger observer should be justified by the invariant it closes.
No new IR or kernel authorization layer is necessary to address these blockers.

## Limitations And Authority

These are proposed acceptance boundaries, not waivers already granted by a named
maintainer. The design names no accepting individual. The project owner must
designate the relevant maintainer/reviewer before delivery; this critique cannot
approve reduced expressiveness on the user's behalf.

| Limitation / Affected Cases | Rationale | Consequence | Accepting Authority / Condition |
|---|---|---|---|
| A: heterogeneous port dispatch and erased public actions remain | Different port layouts and external executors require runtime boundaries | Some dynamic dispatch remains; no claim of a fully static executor | Graph API maintainer, provided object safety and external-action tests survive |
| A: local preparation occurs after reservation; encoded preparation before it | Move-only raw ownership differs from borrowed decoding | Local preparation failure cannot promise restoration of consumed raw inputs | Graph runtime maintainer, only as unchanged baseline behavior; no broad rollback claim |
| B: pending accelerators, tickets, producers, reservations and settled colors remain | They encode different lookup needs, completion authority and policy history | Not a single-map scheduler; retained history and indexes still cost memory | Scheduler maintainer, with independent index/lifecycle conformance tests |
| B: cycles have stable ordering, not guaranteed liveness | Topological order is a preference, not a DAG restriction (`lib.rs:5327-5367`) | Consolidation cannot promise termination or discard cyclic reachability | Scheduler maintainer and project owner, only with unchanged cycle expressiveness |
| C: fallback requires a broad Graph claim when precision is unavailable | Existing vocabulary cannot describe every configuration/destructive edit safely | Some otherwise independent patches conflict globally or require explicit ordering; document affected edits | Patch API maintainer and project owner must accept conservatism, not silent rejection of supported edits |
| C: callback-captured interior state and external side effects are not rolled back or behaviorally compared | Cloning declarations does not clone arbitrary closure state | Trusted custom callbacks remain necessary; footprint checks are not a security sandbox | Host/application owner accepts callback trust; Graph maintainer must bound rollback documentation |
| Cross-cutting: builder-owned pending store, Weak handles, frozen books and revocation tokens remain | Registered fluent retargeting and stale-handle rejection are distinct requirements | Pending staging and dynamic frontier/revision checks remain; A-C are not a full ownership redesign | Graph authoring maintainer, after confirming no duplicate authority is introduced |
| Cross-cutting: local/encoded identities and structural manifests remain distinct from behavior identity | Local type equality, portable schema equality and closure honesty differ | No codec-honesty certificate or captured-behavior equivalence follows from equal manifests | Graph/schema maintainers; no expanded guarantee is accepted |

## Delivery Gates

- Before implementation, revise the design to resolve C's coverage blocker and
  specify the A/B conditions above. Retain explicit projections, independent child
  readiness, custom policies, fluent retargeting and cross-kind replacement; no
  reduced expressiveness is approved as a simplification tradeoff.
- After each batch, record the actual deletion/retention ledger, invariant ownership,
  independent diff review and regression results. Measure state/allocation changes
  where claimed; fewer type names alone is not a measurement. Report compilation,
  code-size or runtime regressions if observed, without inventing performance wins.
- Implementation acceptance requires formatting and relevant tests at the exact
  final revision, followed by the project's required checks and exact-head remote
  CI. Baseline shard results and this static critique provide no such acceptance.

## Revised Batch C

### Review Basis

Independent Phase 2 review of `graph-consolidation-patching-design.md`, not an
implementation review or approval of A/B. Inspected current workspace source at
HEAD `d758291c1db77abcff60ac3ac0e167c0f59261bd`, including the concurrent changes
in `authoring.rs` and `lib.rs`. Source references below describe that worktree.
Existing source changes and untracked documents were left intact. No source edits
or tests were performed for this review; this section is the only review edit.
Additional concurrent edits to `authoring/operation.rs` and
`tests/matching_projection.rs` appeared by the final status check. Projection
references below are to the inspected pre-edit version, not acceptance of those
new implementation changes.

**Decision: ACCEPT WITH ONE REQUIRED CORRECTION.** The original coverage and
candidate-rollback objections are resolved at design level. Complete field
accounting is warranted for unrestricted custom callbacks; the entire proposed
observer/revision architecture is not mandatory. The smaller accepted
implementation below preserves custom precise edits, rather than reserving
precision for library-generated patches. The contract-comparison correction is
required before implementing the inventory literally. Tests and independent
implementation review remain delivery gates.

### Blocker

1. **Stored contract equality is not the existing `PartialEq`.** Design line 191
   says both "Existing exact equality" and to include every `DeclaredValue`'s
   schema and local type. Those are different rules in current code.
   `GraphContract` derives equality (`interface.rs:321-325`), but
   `InterfaceShape::eq` calls `same_shape` (`interface.rs:86-90,198-202`). Its
   checks convert named values to `TypeIdentity`, so local values with the same
   Rust type and different stored schema text compare equal. Direct
   `DeclaredValue` equality would notice that difference
   (`authoring.rs:72-76`). Reusing `claims == claims` or request-vector equality
   therefore does not implement the promised field inventory. Use one private
   observation-only fieldwise contract/shape comparator, including ordered claim
   and request vectors, map keys, names, optional output, unnamed identities and
   named values' schema text/local type. Use it also in cross-kind prefix/lift
   matching. Do not change public semantic contract equality as collateral work.
   Add a private same-local-type/different-schema observation regression. This is
   a concrete inventory/comparator mismatch, not a claim that public
   `implements` currently allows arbitrary in-place replacement of a contract.

### Resolved Challenges

- **Directional coverage is correct.** The table at design lines 93-105 separates
  write authority from conflict intersection. Keep planner `overlaps` unchanged
  (`patching.rs:96-134,650-655`); application must stop using it at
  `patching.rs:808-823`. Graph observations require Graph declarations, while
  Graph declarations cover all concrete observations. Same-name Node/Instance
  authority is intentional and necessary for replacement. An incident claim is
  topology authority, not authority over the operation payload or its exports.
- **Pattern matching must actually be directional component matching.** Parse
  exact keys with `ConnectionRoute::from_exact_key`
  (`patching/route.rs:83-131`). For each declared optional component, absence
  means unconstrained; presence requires equal presence/value in the exact
  route. Check every component, including `from.slot` if a caller manually
  constructs the public selector. `pattern_overlaps` currently ignores that
  component and treats missing destination slots symmetrically
  (`patching/route.rs:73-81`); it is not the implementation. A concrete node-slot
  pattern cannot cover an instance boundary that has no slot. Exact low-level
  keys and Connections still cover numeric/non-symbolic observations; incident
  and symbolic patterns must not pretend to parse them.
- **Clone normalization is justified and bounded.** `GraphBuilder::clone`
  preserves module scope/tokens, detaches pending contents and mints a root
  `definition_id` (`authoring.rs:802-819`). Contributions clone again
  (`authoring.rs:1864-1869`). Ignore pending wrapper allocation and the root
  definition ID under unchanged root scope/id, not declaration IDs, input IDs,
  dependency identities, raw/pending contents or pending-entry presence. Empty
  callbacks and repeated clone assignment must remain empty-footprint edits.
  Scope/name replacement still requires Graph; equality of compiled manifests
  cannot authorize it. Global token-counter history is not candidate storage.
- **Opaque-pointer comparison is the right boundary.** `NodeTemplate` stores a
  factory Arc (`lib.rs:2225-2236`); ports store policy and optional argument-builder
  Arcs (`lib.rs:1929-1935`). Compare these allocations without evaluating them.
  New equivalent allocations count as precise owner writes; the same allocation
  is not evidence of equal internal behavior. No action/codec equality protocol,
  factory execution, policy call, layout recomputation or compiler invocation
  belongs in the observer.
- **Candidate-owned rollback scope is approved for this repair.** The outer draft
  and final assignment already provide the correct commit boundary
  (`patching.rs:734,848`). Candidate pending contents are detached. A captured
  original-store `NodeHandle` can still upgrade its Weak pointer and mutate that
  other store (`authoring.rs:652-674`), just as a captured Arc can mutate shared
  state. Those effects are outside this transaction; unconditional arbitrary-Rust
  rollback is not a reasonable requirement here. Document the boundary without
  banning custom callbacks, precise custom edits, shared policies or retained
  handles. The design's host warning is a condition for relying on transactional
  guarantees, not a new API restriction enforced by this patch.
- **Alias ownership is necessary, not observer embellishment.** Compare dependency
  payload/identity/provenance differences under every before/after direct alias,
  with cycle-safe traversal. `refresh_dependency_identities` changes reverse
  ancestors too (`authoring.rs:1578-1604`); after-only claim traversal in
  `interfaces.rs:21-50` is insufficient. Do not charge an unrelated alias merely
  because it exists, or excuse shared changes because one instance was added.
  Owned imports and nested claims must retain Instance precision for custom and
  convenience edits alike. Changed orphan residuals can require explicit Graph.

### Cross-Kind Exceptions

The revised exceptions are acceptable as exact field ownership rules, not as
whole-observation exemptions. `substitution.rs:109-216` changes edge endpoints,
input targets and output sources; module-to-node replacement also appends
prefixed root requests (`substitution.rs:84-96`). The current planner's boolean
`requests_lifted`/empty-name skip (`patching.rs:775-811`) must disappear from the
enforcement path.

For actual same-name cross-kind transitions, derive the endpoint substitutions
from effective node interfaces and module interfaces. Exempt only those matching
target/source cells; compare all remaining export fields, token maps, target
multiplicity and order normally. Independently claimed export edits may coexist
with replacement. Require exact unchanged request prefix and exact appended
per-transition lift blocks, each used once, including duplicates. A legitimate
lift cannot excuse unrelated root requests, root claims, limits or any other
Graph-owned residual. The comparator correction above applies here too.

Do not gate these rules on `PatchBuilder::replace` provenance: an equivalent
`PatchBuilder::custom` calling `g.replace` with the same declared writes must
pass. The existing builder adds operation plus incident claims
(`patching/builder.rs:104-119`); those remain sufficient for ordinary symbolic
replacement, including projected fanout and output. Numeric root endpoints are
not produced by the checked typed/symbolic connection APIs
(`authoring.rs:1343-1384`); private numeric fixtures must not be described as
public custom mutation APIs. If an implementation makes such endpoints publicly
reachable, do not silently grant incident authority over an unparseable old key:
account for its resolved ownership and planner conflict coverage together.

Retain `operation_substitution` and `contract_dependency_identity` tests in the
required suite, in addition to the design's listed suites. The former already
tests opposite-end commuting replacements, disjoint rewires and request-lift
isolation (`operation_substitution.rs:430-649`). They are central evidence for
these exceptions, not optional adjacent coverage.

### Exact Minimal Implementation

1. Add the private directional coverage predicate over concrete requirements.
   Preserve resolution, conflict/priority/dependency rules, splice composition,
   public callback signatures and the one-shot execution/commit sequence.
2. Add one authoritative private before/after comparison at the candidate boundary.
   Capture detached pending contents before invoking the callback. Traverse
   borrowed structures directly, with fieldwise leaf comparators and exhaustive
   field destructuring. No general snapshot IR, mutation journal, public equality
   traits or capability wrapper is required. Return concrete requirements plus
   the events needed by the chosen revision policy; never reconstruct authority
   from revision diagnostics. Remove superseded partial enforcement scans.
3. Attribute all node, instance, topology, export and trigger differences to their
   precise owners, including removals, tokens, opaque-pointer replacement, raw
   and effective pending state, and all dependency aliases. Validate coupled
   indexes/cursors, preserved record IDs and survivor order. Use the design's
   checked cross-kind rules. Any unaccounted candidate-owned residual requires
   explicit Graph; do not ignore the rest of an object after finding one precise
   difference. Keep final-state rather than attempted-operation semantics.
4. Validate requirements against the footprint already used for resolution. Reject
   underdeclaration without claim promotion or callback replay. Keep the existing
   outer draft and candidate discard behavior. Graph authority permits edits, not
   bypass of authoring/compiler validity checks.
5. Independently extract the nonmutating projection-leaf preflight shared by
   manual and matching exposure (`operation.rs:928-1079`). Check existing export
   metadata and token association before mutation. Bulk exposure validates every
   leaf and aggregate capacity before committing anything; ID 65535 remains valid.

**Smaller alternative accepted:** enforcement need not expand the public
`AssemblyChange` enum. A private requirements result can coexist with the current
public, explicitly partial revision vocabulary, with existing event ordering
preserved. That is the smallest API-neutral correctness repair, provided the
revision documentation does not claim a complete write log. Complete successful
revision reporting is useful but separable: the design's six new variants are
also acceptable with explicit API-owner approval of exhaustive-match breakage.
Neither choice may use old diagnostics as the enforcement oracle. If complete
revision reporting is retained as a delivery requirement, keep the proposed enum
extension rather than silently dropping that requirement.

**Smaller alternatives rejected:** only special-case Graph in `overlaps`; compare
manifests or factories alone; instrument a few mutator methods; trust callback
self-reporting; or require Graph for every custom callback. Whole-builder
assignment bypasses method instrumentation, while blanket Graph claims destroy
precise custom conflict planning. A normalized residual comparison with precise
owner attribution is acceptable, but is the same complete accounting obligation
under another implementation shape, not a way to avoid the field inventory.

### Acceptance Gates

After the contract-comparator correction, there is no remaining architectural
blocker to this bounded repair. This review approves neither implementation
correctness nor broader callback isolation. Require the design's adversarial
coverage, clone, opaque allocation, pending, alias, order/cursor, cross-kind and
projection tests, plus the contract regression and substitution suites above.
In particular, test a legitimate lift plus a limits edit against a nonempty
narrow footprint, and equivalent precise custom/convenience replacements.

Explicit Graph fallback for genuinely graph-owned or unexplained residual state
is accepted conservatism, not permission to demote the named precise edit classes
to Graph for implementation convenience. No new expressiveness restriction is
approved. Final acceptance still requires field-by-field diff review and the
project's exact-head formatting, test and CI gates.
