# Q3 Whole Acceptance Design

Date: 2026-11-24. Status: production design/implementation approved by independent
review as reported by main; both P2 acceptance followups are now implemented and
locally verified below. Fix review and hosted/exact-commit acceptance remain. Base source:
`1589ad0d2828fecea99f784ac8e64fe952e821bc`.

## Decision And Scope

Implement one closure package: a finite host-owned **composition planning**
allowance, two targeted planner optimizations, complete adversarial correctness
coverage, and paired Composer/Kernel measurements. Do not publish another
coverage-only slice as Q3 closure. Close Q3 only when every gate below passes on
the exact combined implementation source.

The original finding at `cf62add61`, `docs/fbp-issue-ledger-20261111.md`, is
"Dependency/conflict selection scaling": stress diamond, skew, and adversarial
priority graphs in composition. This proposal completes that finding, including
the actual host prefix/preview path. It does not redefine Q3 as general Q2
incremental compilation or a universal product latency/memory SLO.

The enforceable contract is a deterministic bound on specified logical planning
operations and storage construction. It is NOT a wall-clock, total build-work,
allocator-byte, or arbitrary native-code execution bound. End-to-end measurements
must include the work outside that contract rather than silently omit it.

## Source Findings

| Location At The Examined Source | Existing Behavior And Gap |
| --- | --- |
| `packages/phenix-graphs/src/compose/planning.rs:40-105` | Indexed dependency ordering preserves historical lexical sweeps and stores direct parents. Keep both semantics. |
| `compose/planning.rs:24-33,113,127-137,176` | Lazy closures still allocate a full N-cell Boolean mask for each queried registration, including independent registrations with no ancestors. Pending ancestry edges and cached rows have no planning allowance. |
| `compose/planning.rs:170-226` | Conflict candidates are indexed one earlier owner at a time, but successful distinct-priority collisions can append N(N-1)/2 blockers before calling selection. Equal-priority early rejection does not measure this case. |
| `compose/planning.rs:335-407` | Posting queries iterate the entire owner set and filter out earlier owners instead of starting at the suffix. Candidate order is a BTreeSet order and must stay unchanged. |
| `compose/mod.rs:283-322,329-355` | Registry cloning, revoke closure construction, and candidate collection precede the guarded planner and are not bounded by `SelectionLimits`. |
| `compose/mod.rs:381-418,482-487` | Full validation precedes unchanged-writing-set reuse. Selected-ID/footprint materialization is outside the selector allowance. Preserve fresh successful generations and failed-update identity. |
| `compose/edit.rs:191-257,324-377` | Payload validation walks paths and component origins. Origin traversal has per-edit default compiler instance/depth checks, but its pushes/path construction are not cumulatively charged across edits or attempts. |
| `compose/selection.rs:11-32,35-50,74-106` | Exact selection already has one global allowance for each complete uniqueness proof, with charges before solver growth. Keep it, not a replacement solver. |
| `packages/phenix-kernel/src/build.rs:390-477` | Each contribution sees a validated prefix, optionally a static self-import preview, then supplies its final patch. Main registry planning repeats for each prefix; preview has a different Composer. Each currently gets a fresh selector allowance. |
| `packages/phenix-graphs/benches/native_fbp.rs:328-488` | Real-write chains, diamonds, skew and dense equal-priority rejection exist. Successful dense priorities, defeat/recovery timings, ordered splices, hierarchical collision timings, and real Kernel preparation are missing. |

Existing correctness evidence in `fbp_selection_limits.rs` includes small
successful dense graphs and repeated transient-defeat motifs. That is useful
solver evidence, not a measurement of the missing whole planning/host workloads.

## Options Compared

| Option | Assessment |
| --- | --- |
| Expand `SelectionLimits.max_work` to cover all planning | Reject. It changes a shipped selector-specific allowance, its exact boundaries and error meaning. It still resets at each host attempt unless separately owned. |
| Reuse `CompileLimits` as the planning meter | Reject. Compiler limits measure recursive expansion/work, with `u64` work and instance counts and a depth dimension. They do not naturally count registration blockers, cached ancestry, or repeated host prefixes. Keep compiler enforcement unchanged. |
| Separate work, live storage, peak storage, byte and time meters | Reject for Q3. Requires release accounting, policies and substantially more API than this finding needs. A single cumulative logical allowance can charge both work and storage construction without refunds. |
| Public mutable `PlanningBudget` shared by main and preview | Viable ordinary Rust, but exports a meter lifecycle and methods solely to share two integer values across synchronous calls. No thread sharing is needed. |
| Move a remaining allowance into/out of Composer | Reject. Every error must return the remainder, and preview ownership transfers complicate existing `Result` and borrowing conventions. |
| Public used counter, private borrowed meter | Choose. Add one limits value, one setter and one cumulative registration method. Kernel owns an ordinary local counter. The borrowed meter and all charge helpers stay private to graph composition. |
| Batch all Kernel contributions or cache plans across prefixes | Reject. Preparation and self-imports observe intermediate compiled generations. A later contribution cannot repair an invalid prefix; a final-only plan changes callback, authority, error and generation semantics. No speculative cache identity/invalidation project. |
| General SAT solver or alternate selection representation | Reject. Existing exact signed-conjunction selection is verified; Q3 needs bounded input planning and evidence, not a new solver/framework. |

## Proposed API

Implemented public surface (declaration excerpts):

```rust
// phenix_graph::compose; follow SelectionLimits' usize convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanningLimits {
    pub max_units: usize,
}

// New ComposeError variant, scalar metadata only:
PlanningBudgetExceeded { limit: usize }

impl Composer {
    pub fn set_planning_limits(&mut self, limits: PlanningLimits);

    pub fn register_with_planning_usage(
        &mut self,
        registrations: impl IntoIterator<Item = Registration>,
        used_units: &mut usize,
    ) -> Result<&Composition, ComposeError>;
}

// New field on phenix_kernel::BuildOptions:
pub planning_limits: PlanningLimits,
```

`PlanningLimits::default().max_units = 1_000_000`. This adopts the existing
finite logical-allowance scale as a DELIBERATE NEW HOST RESOURCE POLICY, not
backward compatibility for previously unbounded valid inputs. Matching the
numeric values of other budgets is not evidence that this default is sufficient.
Keep it only after measuring an explicit existing Graph/Kernel real-write,
prefix/preview success envelope. Pin 2,048 independent 512-byte IDs as a default
rejection/larger-finite-limit success case: copying those IDs alone exceeds one
million bytes. No invented millisecond/memory SLO or silent default increase.

`Composer` stores `planning_limits`. Existing `new`, `register`, `revoke`, and
`set_selection_limits` signatures remain. Normal `register` creates a local zero
counter and delegates to the cumulative path. `revoke` creates one local zero
counter for reverse-dependency traversal, candidate construction, and the entire
remaining planning attempt; do not reset it after computing removed IDs. No
cumulative revoke method is needed by the current host, so do not add one.

For an empty current registry, empty admission or revoke performs zero metered
planning work. Otherwise even an absent-ID revoke pays for its lookup and full
candidate planning, and retains the existing successful fresh-generation behavior.

`register_with_planning_usage` consumes units through the mutable counter on
success AND failure. It does not retain the borrow in Composer or Composition,
and does not return a second composition/usage wrapper. Its maximum is that
Composer's configured `planning_limits`. A counter already above that limit is
rejected on entry. A trusted caller can choose a new scope/reset; Kernel must not
do so between prefixes/previews. Changing either limits setter does not publish,
validate, refresh a generation, or reset an externally owned counter.

Internally use a small private `PlanningBudget<'a>` containing `limit: usize`
and `used: &'a mut usize`, passed by `&mut` through candidate construction and
planning. Its only reservation primitive checks `used.checked_add(units)`
against the limit before updating the counter or performing the operation.
Checked size multiplication/addition overflow returns the same typed planning
error before allocation. A failed reservation leaves the counter at its previous
value; previously consumed units are never refunded, including after errors,
panics, scratch drops, and duplicate insertions. Do not use `Arc`, atomics,
interior mutability, a public meter, a generic budget trait, or lifetime parameters
on Composer.

The request identifies shipped consumers of `with_selection_limits`. The examined
1589 source exposes `set_selection_limits`, and a repository-wide search of the
integrated worktree found no `with_selection_limits`. Preserve the named shipped
API if present in the implementation/consumer source; do not rename/remove it or
invent a redundant alias here. Reconcile that source discrepancy before changing
any selection configuration surface. No such change is needed for this design.

## Accounting Boundary

The allowance starts inside each Composer update, before copying its current
registry or computing revoke dependents. Incoming registrations/patches/base
definitions are already owned inputs; their construction and iterator callbacks
are not retrospectively charged. An arbitrary iterator, destructor, allocator,
plugin callback, process factory, or native callback cannot be time-bounded by a
logical planning meter.

Charge only the composition domain: registry metadata copying, dependency/order
planning, structural payload/origin validation, resolved footprint/grant checks,
collision indexing/querying/predicates, ancestry, blocker construction and
selected-manifest/unchanged-write-set planning. Compiler reuse checks, frozen
base/network cloning, replay, normalization, compilation, and executable payload
internals remain outside this allowance. Their current compiler limits still
apply where implemented; they are not a cumulative build-work limit.

Kernel catalog collection, import/contract matching, ready-contribution ordering,
snapshot/artifact/lease bookkeeping, registration construction before admission,
and final offer validation also remain outside this *composition* planning
allowance. This is deliberate rather than an implicit assertion that those
operations are bounded by it. Measure them in real Kernel build/rebuild timings.
Do not add a second cross-crate charging framework for them as part of Q3.

### Deterministic Units

Use one counter for logical work plus storage reservations. Each item below is
charged before its visit, insertion/push, initialization, or copy, not after a
large `collect`, `extend`, `clone`, `vec!`, or Cartesian loop has run:

| Operation | Charge |
| --- | --- |
| Inspect a registration, edit, dependency, grant/footprint, order/frontier item, origin node, ancestor worklist item, posting owner, candidate pair, or predicate item | One work unit per visit, including repeat visits and unsuccessful/deduplicated work. |
| Explicit dependency/ID/index/authority lookup or insertion attempt | One work unit; map internals are not individually metered tree comparisons. |
| New logical collection slot/entry | One storage unit per scalar slot or map/set entry; charge prospective inserts even if deduplicated. Container implementation capacity rounding is not a logical slot. |
| Blocker `(source, target)` | One work unit to record plus two storage units for its index cells, BEFORE `push`. Pair discovery/predicate work is charged separately. |
| Cached ancestry row | N Boolean storage units BEFORE allocating/zeroing an N-cell `Vec<bool>` mask. Each outer cache slot is also reserved. Charge each visited parent and each pending index push separately. |
| Bulk fixed-length initialization/copy | Reserve all known logical slots before capacity allocation/initialization. Charge each subsequent nontrivial item traversal separately. No unchecked N-by-N size arithmetic. |
| Scan or copy owned symbolic text in the metered domain | Reserve one unit per byte before that scan/copy; for a copy this covers its logical byte storage as well as copying work. Charge each separately generated/copied key/path; prefix scans and copied prefixes must not hide unbounded repeated work. |
| Nested collision predicates | Charge each examined edit/target/footprint comparison, including inner scans and both nested-export directions, preserving short-circuit order. Do not charge only one unit for an unbounded pair predicate. |

An owner-posting lookup includes its fixed suffix-range setup in the logical
query operation; returned owners are charged separately. Explicit endpoint
equality operands and the following `contains('/')` scan are separate text
reservations, even when short-circuiting makes a reservation conservative.

This bounds logical constructed storage cumulatively, not just live storage:
freed scratch does not replenish it. Library allocation overhead, capacity growth,
opaque executable/interface/provenance payload internals and allocator failure are
not a byte-accurate memory bound. Do not advertise one. Store no payload or huge
diagnostic in an exhaustion error. Existing authority/type validation still runs;
budget checks do not convert unchecked payloads into trusted ones.

### Placement Rules

1. Replace whole-registry `clone`/candidate `collect` with a bounded traversal
   that reserves copied metadata entries and symbolic text before each clone.
   Arc-backed patch/component payloads stay shared. Meter reverse-index creation,
   every revoke push/pop and removed-ID insertion before candidate reconstruction.
2. In `dependency_order`, reserve order/cache/parent slots before capacity or
   initialization, and charge dependencies, lookups, ready-set operations and
   dependent traversal. Preserve missing-dependency and lexical-sweep behavior.
3. Add a private metered footprint-construction path; do not call public
   `NetworkPatch::footprints()` then count its result. Keep the public context-free
   helper unchanged. Charge origin work before pending growth/path formatting,
   including across many losing Add/Replace edits; retain current per-edit
   instance/depth guards. Charge path length before split/scan work.
4. Thread the same private borrow through `ConflictIndex::new`, `after`, ancestry
   and all collision predicates. Make them fallible where necessary. Reserve
   posting/candidate entries before insertion and each ancestry edge before
   extending a worklist. Never materialize the global pair matrix.
5. Charge blocker storage before append. A selector limit of zero does not waive
   validation or allow unbounded blocker preparation: the independent planning
   allowance stops it first when exhausted. Do not inspect selector internals or
   infer a planning cap from its remaining allowance.
6. After selection, charge selected IDs, selected footprint manifest construction,
   and before/after writing-set comparison/filtering. Resolve this metered work
   before replay or taking the unchanged-topology return. Reserve any required
   final manifest traversal in advance, or move it before replay, so late
   materialization cannot bypass the allowance. Replay/compiler internals remain
   outside the new contract.
7. Extend the existing guarded update boundary to encompass fallible private
   candidate construction/revoke work as needed. A budget reservation remains
   consumed if the guarded attempt panics. Do not attempt to roll back arbitrary
   effects in iterator callbacks or destructors.

## Two Targeted Optimizations

**Owner-posting suffixes.** In every owner posting lookup in `ConflictIndex::after`,
replace `owners.iter().copied().filter(|owner| *owner > i)` with
`owners.range((Excluded(i), Unbounded)).copied()`. Do the same in `from_path`.
Charge only yielded suffix owners, plus lookup/range/query work. Keep the final
candidate BTreeSet so pair order and deduplication remain exact. No custom merge
iterator or posting cache. Verify exact, component, connection and nested-export
directions against the exhaustive predicate oracle.

**No empty-ancestor rows.** At ancestry query sites, if the queried registration
has no direct parents, its ancestor relation is false without allocating an
N-cell mask. For an independent splice, skip both empty closure construction and
the predecessor scan. Keep the existing lazy cache for registrations with
parents; charge its rows/worklists before construction. Do not replace it with
an eager matrix, general reachability index or per-pair DFS. Empty cache slots are
O(N), but independent successful dense graphs allocate ZERO ancestor rows.

Neither optimization makes dense blockers linear. With B actual blocker edges,
the exact selector and blocker inventory still require O(N + D + B) logical
storage, and B can be quadratic. The new allowance makes that resource choice
finite and retryable rather than hiding it behind a selector-only limit.
Dependent splice/overlap graphs may still require quadratic cached ancestry;
that path must now reject before exceeding the logical allowance.

## Kernel Ownership

At the start of `Kernel::build_with_options` create one local
`let mut planning_used = 0;`. Configure main and every preview Composer with
`options.planning_limits`, and call `register_with_planning_usage` with that SAME
counter for every admission. A fresh self-preview must not receive a fresh
counter. `Composer::new` plans an empty registry; its base compilation/network
copy is explicitly outside the allowance. There is no constructor budget reset
to exploit for nonempty planning.

Retain `options.selection_limits` per attempt for main and preview. A build can
therefore perform several individually bounded selector proofs; their aggregate
selector work is not charged to `planning_used`. The cumulative new counter
limits repeated *planning*, not every operation in the build.

A failed preview returns before that contribution's preparation callback. A
failed main admission may occur after preparation; release candidate artifacts
and exact code leases as today, without activating producers. A build aborts on
its first failure; do not reset its counter and retry a contribution internally.
Host retry means a new explicit build/prepare scope, optionally with a larger
finite limit. `KernelHost::prepare` failure must leave the published generation,
epoch and plugin inventory unchanged. It cannot undo external callback effects.

Keep current prefix semantics: callbacks observe current selected/recomposed
prefixes, self-imports bind real static preview units, and final patches are
validated independently. No reordering, speculative callback execution, final-only
validation, or reuse of a self-preview as a replacement for main admission.

## Errors And Atomicity

Implement explicit prepare/commit/dispose transactions. Catch `into_iter`,
`next`, and iterator destruction before publication. Hold candidate registry,
pending rejected registration and iterator outside the unwind closure so normal
caught failures cannot unwind their user-owned payloads while unwinding another
panic. Dispose them in separate caught boundaries; preserve an already conclusive
semantic/budget error if cleanup panics. A cleanup panic on an otherwise prepared
candidate is a precommit `Panicked` failure. Commit uses only `mem::replace` of
registry and current, with no destructor or callback between replacements. Then
dispose retired state under a caught boundary. A postcommit cleanup panic is
suppressed: successfully published state stays success. No typed cleanup channel
is needed for this policy. Native abort, double panic and nontermination remain
outside the boundary; ordinary catchable Drop panics are not excluded.

Text reservations precede every library query using its full query-key bytes,
and each explicit predicate using both operands. Internal BTree comparison
multiplicity is excluded; no custom comparator/collection. Reserve generated
prefix/format bytes before allocation. Reserve possible diagnostic copies BEFORE
the semantic predicate that could make that error conclusive; those reservations
can exhaust before a semantic decision, but later cleanup cannot replace it.

Preserve current semantic first-error precedence when planning allowance is
sufficient: duplicate admission, dependency validation/order, payload/grant
validation, ordered pair/splice/nested-export conflict checks, exact selection,
then replay/compile. Candidate preparation may consume units before discovering
a duplicate, but do not move semantic validation ahead of its historical phase.

Under exhaustion, **the first failed reservation wins**. Do not finish scanning
to search for a preferred semantic error. Thus planning exhaustion may precede a
duplicate, missing dependency, forged origin, unauthorized edit or conflict that
has not yet been reached. With enough planning allowance, those errors still
precede a selector limit of zero. Do not add a selector-zero early return.

If a semantic error is established after all reservations required to detect it
have succeeded, return it immediately without demanding cleanup units. Within
selection, existing exact exhaustion/no-model/multiple-model precedence is
unchanged. Post-selection planning may still exhaust while materializing the
manifest, before replay/compilation. Network/compile errors are reached only
after that planning succeeds. Cleanup is not metered or fallibly blocked.

Any failed register/revoke retains registry, active IDs, exact footprints,
compiled topology and generation identity. Bindings from the previous successful
publication must still resolve. The used counter is accounting state, NOT
publication state; it is intentionally not rolled back. Successful unchanged
topology still issues fresh generation authority. No factories run inside
Composer; no external side-effect rollback is promised.

## Correctness Acceptance

Extend existing test families rather than adding a model framework. Keep the
19,683 signed-equation oracle and all selector exact-boundary tests unchanged.
Use private/test-only counters at relevant sites where a resource assertion
cannot be observed through public results; do not export phase instrumentation.

| Required Test Family | Decisive Assertions |
| --- | --- |
| Empty/zero planning | Truly empty registry planning costs zero; any nonempty admission with zero planning units fails before registry/index/ancestry/blocker growth. Register-empty on an existing registry is not free. Empty-registry revoke costs zero; absent-ID revoke on a nonempty registry still pays and refreshes generation only on success. |
| Exact boundaries | For small fixtures determine conclusive cost C; C succeeds and C-1 fails with the exact new error. Exercise candidate clone, revoke closure, dependency slots, origin worklist, footprints/postings, candidate query, ancestor row/push, blocker append and selected manifest. Assert denied growth did not occur using narrow private test hooks. |
| Arithmetic/usage | Checked reservation and dimension overflow fail before allocation; exact-limit reservations succeed; `used > limit` rejects on entry; zero charge at an exhausted valid counter succeeds; failed reservation does not erase previously consumed units. |
| Competing errors | Sufficient planning plus zero selector preserves missing/cycle/origin/grant/equal-priority/splice conflict errors. Low planning returns planning exhaustion at the pinned earlier reservation. Include duplicate and post-selection exhaustion; never assert one universal error independent of allowance. |
| Atomicity/retry | Test both register and revoke failures against an active sentinel and bound endpoint. Compare exact state and generation, prove rejected IDs were not admitted, then retry with a larger planning limit and obtain the canonical fresh result. Changing limits alone leaves bindings valid. |
| Successful dense priorities | Independent replacements of one primitive, distinct priorities in lexical, reverse and shuffled order. Assert the exact winner, exact footprint and executable result; revoke winner and observe next winner. No ancestor masks; exactly N(N-1)/2 blockers for the all-overlap fixture. |
| Defeat/recovery | Repeat the existing `{P,R,p}` motif, with staged admission that first suppresses p/P via Q, then R defeats q and re-enables p/P. Check intermediate and final exact sets, publication generations, revocation/re-admission and fresh compilation equivalence. Include mixed influence feedback with unique, zero and multiple models; exhaustion is not a successful solve. |
| Diamond/skew | Keep real writes, reverse lexical dependencies, redundant ancestors, hub/fan-in/tail and partial/root revokes. Verify exact descendants, unrelated survivors and compiled presence, not cardinality alone. |
| Ordered splice | Deep and wide dependencies sharing anchors, exact resolved predecessor-edge grants and replay connections; unrelated same-anchor splices reject first ordered pair even at different priorities. Pin unauthorized generated edge and duplicate-anchor diagnostics. |
| Hierarchical collisions | Independent parent/descendant replacement, sibling noncollision, nested endpoint equality, root/nested export separation, declaration/export dependencies in both directions and multi-edit competing overlaps. Compare indexed candidates and actual first errors to exhaustive predicates. |
| Cumulative public API | Two admissions each fit a fresh limit but their combined usage exceeds it. Failure preserves the second update's prior state; consumed usage persists. Independent Composers can share one counter synchronously without borrowing or generation leaks. |
| Kernel prefixes/previews | Several distinct contributions and multiple real self-import previews each fit alone, but the cumulative build fails at an exact contribution before an expected callback/admission. Assert typed source, callback counts and artifact/lease drops. Use exact-total and total-minus-one limits, larger-limit full retry and selector-only exhaustion separately. |
| Kernel recomposition | Preparation callbacks assert the prefix topology and authenticated imports, including defeat/recovery and a valid final patch differing from its self-preview. Failed `KernelHost::prepare` leaves active generation/epoch/inventory usable; larger-limit retry and publish produce correct outputs and stale old bindings only after publication as specified. |

For a dense request with `SelectionLimits { max_work: 0 }`, explicitly prove that
small planning allowance stops blocker growth BEFORE selection and that a larger
planning allowance reaches the unchanged selector-zero error. This is the
regression for the current pre-selector quadratic allocation gap.

## Measurement Acceptance

Extend `native_fbp.rs` for Composer cases and add one dependency-free
`packages/phenix-kernel/benches/composition_planning.rs` with `harness = false`.
Reuse established seven raw samples/two warmups and small fixture functions,
not a new benchmark framework. Keep original controls and assertions. Add an
optional simple case-name filter if needed to keep local paired runs bounded.

| Workload | Initial Fixed Sizes And Measurement |
| --- | --- |
| Existing chain, real-write diamond/skew, dense equal reject | Retain current sizes/samples and exact result assertions as controls. |
| Successful dense distinct priorities | N = 32, 64, 128, 256; register, winner revoke/recovery, and teardown. Include ordinary and adversarial priority-to-ID order. Report B and success separately from exhaustion. |
| Repeated defeat/recovery | 4, 16, 32 motifs; batch and staged register/revoke/re-admit sequences, checking actual recovered writers. |
| Ordered splices | 16, 64, 128 splices; actual resolved grants, replay and compilation, plus independent-anchor rejection controls. |
| Hierarchical conflicts | 16, 64, 128 shallow nested edits; successful ordered replacements and sibling writes, plus independent parent/child and export/declaration rejection. Stay within compiler instance/depth limits. |
| Real Kernel prefixes | 16, 32, 64 contributions with real patches; no-preview and multiple valid self-import-preview variants. Call production `Kernel::build_with_options`, bind/assert imports in prepare, verify compiled outputs. Include a dense-priority no-offer subgroup and a distinct self-import subgroup so defeated offers do not invalidate a supposedly successful fixture. |
| Actual host rebuild | Repeated `KernelHost::prepare`/`publish` of staged revisions at the same bounded sizes, checking final topology/output. Label restart/rebuild, not persistent Q2 cache reuse. Include a failing limited prepare followed by larger-limit successful retry. |

Report default-limit runs and explicitly configured larger finite runs separately.
Before measuring, compute/test necessary fixture costs so "success" does not
mean fast budget rejection. The old source has no new planning API: paired timing
fixtures must compile on both using common existing APIs with candidate defaults
where they fit. For larger cases, use a minimal candidate-only limits adapter
outside the byte-identical workload body, record/hash it, and prove it changes
only the candidate allowance, not the workload or correctness assertions. New
resource/error tests run only on the candidate. No production compatibility API
or speculative backport to the baseline.

Collect timing and allocation evidence in separate runs. Reuse the repository's
existing allocator-event approach in a dedicated process, not a global allocator
inside production code: allocations/reallocations, allocated bytes and peak live
requested bytes, with measurement boundaries stated. Also record private test
counts for ancestor rows/cells, posting owners visited, candidate pairs, blockers
and planning units. These distinguish improvements from mere early failure.
Actual allocator numbers include rounding/payload costs excluded by logical units.
Do not conflate allocated bytes, live bytes or process RSS.

Paired protocol: baseline integrated 1589 versus the exact candidate, same fixture
bodies/assertions and locks/toolchain, prebuilt executables, alternating AB/BA
order for at least three repetitions, retaining every raw sample and environment
record. Include CPU/OS/compiler, flags, source and harness hashes, effective
limits, workload sizes, callback/preview counts and measurement boundaries.
Record end-to-end construction, admission/recomposition, validation, replay,
compile, callback/assertion and teardown inclusion explicitly. Add an operation
window excluding fixture construction only as an additional labeled measurement,
never a replacement for end-to-end data.

Structural acceptance is not statistical: suffix queries visit no earlier owners,
independent dense graphs create zero ancestry rows, blocker construction stops
at the reserved logical bound, exact outputs remain canonical, and cumulative
host planning cannot reset across previews. For measured optimizations require
paired evidence explaining the targeted work/allocation reduction on successful
cases; publish regressions on controls and separate accounting overhead from
optimization benefit. Measure a bounded-accounting-only source snapshot before
the two optimizations with the same harness as an additional attribution control;
record its exact source/diff hash without adding runtime feature switches. If
timings are too noisy to support a speedup, say so and
repeat controlled runs rather than claim one. No invented latency ratio/SLO.
Material unexplained regressions or fixtures that only reject do not pass review.

## Proposed File Changes

| File | Implementation Responsibility |
| --- | --- |
| `packages/phenix-graphs/src/compose/mod.rs` | Limits/error, stored setting, private borrowed meter, cumulative public admission, bounded register/revoke construction, guard and selected-manifest accounting. Keep meter here unless its actual size warrants a small private module. |
| `packages/phenix-graphs/src/compose/planning.rs` | Fallible charged dependency/index/ancestry/conflict pipeline and both measured optimizations; preserve ordering and existing oracles. |
| `packages/phenix-graphs/src/compose/edit.rs` | Private metered footprint/payload/origin traversal. Leave public context-free footprint helper semantics intact. |
| `packages/phenix-graphs/src/compose/selection.rs` | No solver or selector-unit changes expected. |
| `packages/phenix-kernel/src/lib.rs`, `src/build.rs` | `BuildOptions.planning_limits`; one build-local consumed counter passed to all preview/main admissions; existing typed error mapping. |
| Graph planning/selection/composition tests, new `fbp_planning_limits.rs` | Narrow boundary/atomicity/storage tests plus extensions to existing stress/oracle families. |
| Kernel `composition_selection_limits.rs`, new `composition_planning_limits.rs` | Keep selector coverage, add cumulative multi-contribution/preview/lifecycle regression using existing support helpers. |
| Graph native bench, new Kernel composition bench and Kernel manifest | Successful stress workloads and actual host path; one bench entry, no dependency. |
| Graph API examples, Kernel README, Q3 acceptance/scorecard docs | Explain allowance ownership, exclusions, error precedence/default rationale and attach exact-source evidence; update closure status only after acceptance. |

Later CI should cover default/no-default targeted Graph/Kernel suites, complete
existing selector and planning oracles, formatting, strict affected all-target
Clippy, and exact-head integration suites. Paired CI triggers must include graph
compose/definitions/compiler, Kernel build, plugin/schema dependencies, benches,
manifests/locks/toolchain and workflow inputs. The integrated Q3 workflow currently
triggers only on its bench/workflow files; do not claim planner changes are
automatically covered. Reconcile the prepared workflow lane/credential restriction
documented in `fbp-foundation-integration-20261123.md`, or explicitly dispatch and
record exact-head runs. No unexecuted workflow patch counts as evidence.

## Review Gates

The design choices proposed for approval are the single cumulative unit counter,
private borrowed meter/public consuming-counter method, one-million-unit default,
composition-only enforceable boundary, first-failed-reservation precedence, and
unchanged per-attempt selection. These are concrete decisions, not a request to
design a general resource architecture during implementation.

Resolve these implementation unknowns with evidence, not silent scope changes:

1. Reconcile the named shipped `with_selection_limits` API with integrated 1589;
   retain external compatibility without introducing a duplicate API.
2. Audit every bulk clone/collect/extend and nested predicate in the stated domain
   against the charge table, including origin pushes and late manifest filtering.
   Exact fixture costs are not measured yet; pin them after this audit, not by
   guessing constants in advance.
3. Confirm default-fit sizes and whether the largest Kernel prefixes need an
   explicitly larger allowance. Aggregate planning can be quadratic for disjoint
   prefixes and cubic for repeated dense-prefix pair enumeration. This proposal
   bounds/reports that behavior; it does not claim to eliminate it.
4. Confirm the common-API paired fixtures compile on integrated 1589, keep any
   candidate limits adapter outside identical workload bodies, and record all
   allocation/timing boundaries before interpreting results.
5. Independent review must decide whether the complete evidence supports whole
   original Q3 closure with the stated honest exclusions. Do not substitute a
   "partial unit accepted, Q3 open" deliverable for the agreed package. If a
   required gate fails, fix/review the package before changing the ledger.

The initial turn was design-only. Implementation was subsequently authorized
with three blocking review requirements: explicit transaction disposal, exact
text/diagnostic preflight, and an honest new default-resource policy. The results
below supersede the earlier implementation/evidence unknowns, without declaring
an unreviewed ledger closure. No commit or push has been made.

## Implementation Outcome

The public API above is implemented. `budget.rs` contains the one private borrowed
meter and concrete metadata reservation helpers; no public meter, atomics,
generic collection/budget architecture, cache or new solver was added. Normal
updates have a fresh counter; Kernel has one local counter across every main
prefix and preview. There is no cumulative revoke API. `selection.rs` is unchanged.
The two intentional planner optimizations are posting suffix ranges and skipping
empty-ancestor masks/predecessor scans. Iterator-based predicates/collection
construction were rewritten only where fallible preflight is required.

Prepare/commit/dispose is explicit in Composer. Tests exercise `into_iter`, `next`,
iterator Drop, rejected registration/candidate/base Drop, semantic/budget errors
followed by cleanup panic, and retired factory Drop after successful revoke. No published
field changes before successful preparation and iterator disposal. Commit consists
of two nondropping replacements. Retired-state disposal catches normal unwind
panics and leaves successful publication successful, without an extra diagnostic
channel. The existing panic-payload disposer is reused. Abort/double panic and
nontermination are not represented as ordinary caught failures.

Queries reserve full key bytes; explicit text predicates reserve both variable
operands (not internal BTree comparison multiplicity). Copies, generated prefixes,
origin worklist paths and possible diagnostic copies are charged before the
semantic decision/allocation. Reservations can conservatively cover short-circuited
operations and prospective/deduplicated inserts; this is intentional, not an
allocator-instruction count. Long common-prefix 512-byte IDs/paths and exact
semantic-error boundaries pass. Revoke has one exact allowance spanning its
closure, candidate metadata copying and remaining plan, not separate phase caps.

### Default Policy Evidence

The one-million-unit default is retained as a deliberate finite host admission
policy, NOT because it numerically matches other limits and NOT as compatibility
for all previously unbounded valid inputs. Pinned successful default fixtures:

- Existing real-write chain, diamond and skew native workloads through 128
  registrations, with exact partial/root revocation assertions.
- Dense real replacements with both lexical priority directions through 32,
  shuffled dense priorities through 64, and ordered splices/hierarchical edits
  through 32. Repeated staged defeat/recovery runs with four motifs.
- Kernel real-write prefixes, self-import previews (whose final patch differs
  from its preview) and independent dense priorities at 16 and 32 contributions.
  Three real host rebuilds with 16 preview contributions also succeed.

Kernel32 prefix+preview composition consumes exactly **401,388 units**. Kernel4
consumes **6,783** without previews, **9,687** with previews; one preview contribution
costs **1,371**. Exact total and total-minus-one checks pass, including prepared
artifact/factory disposal and zero producer activation on planning failure.
The 64-contribution preview fixture rejects at the default and succeeds at an
explicit 50,000,000-unit limit. The benchmark labels/configures that larger finite
policy rather than passing off rejection as a successful latency sample.

The required 2,048 independent 512-byte-ID fixture rejects at the default, succeeds
with 50,000,000 units, and consumes **25,241,600 units** for initial admission.
Replanning that already admitted registry also rejects at the default without
publication, then succeeds after increasing the limit. Candidate ID copying alone
exceeds 1,048,576 bytes. No implicit shortening of IDs or default increase is used.

### Correctness And Tooling

Verified locally with Nix rustc **1.96.1**, commit
`31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`, LLVM **21.1.8**:

- Full final-source `cargo test --locked -p phenix-graph -p phenix-kernel`,
  including doctests, passed after the last code change.
- Final default Graph/Kernel libraries and composition/planning/selection/model
  targeted suites pass. Graph library: 111 passed, one existing ignored manual
  EOS profile. Kernel library: 37 passed. The 19,683 signed-system solver oracle,
  generated subset/DAG models, mixed influence zero/unique/multiple models,
  canonical replay/execution and indexed collision oracles remain green.
- Isolated Graph `--no-default-features` library plus composition/model/selection,
  planning/stress and nested-export suites pass. Kernel no-default targeted
  planning/selection, activation artifacts and generations pass; Kernel still
  enables its graph dependency's default features, so this is not substituted
  for the isolated no-default Graph run.
- Strict affected Graph/Kernel `cargo clippy --all-targets -- -D warnings` passes.
  Formatting, whitespace and paired workflow actionlint checks pass.
- New Kernel regression proves a later contribution cannot repair an invalid
  source-start prefix even when the equivalent single batch compiles. Preparation
  stops after the first callback. Failed host preparation preserves active Arc,
  epoch and plugin revision, followed by a successful explicit retry/publication.
- Private storage tests assert zero ancestor cells, 496 blockers and 1,488 yielded
  posting owners for the dense32 plan (43,787 planning units). An exhausted
  pre-selector attempt cannot finish collecting blockers even with selector zero.
  Exact mask reservation, pending growth, long-text query/predicate and arithmetic
  overflow boundaries pass. The small revoke boundary is 368 units.
- A literal 10,377-unit nested-connection query boundary independently checks
  full query-key bytes, comparison operands and separate endpoint `contains`
  scans, including a 512-byte common path prefix. The final audit added the
  missing separate scan reservations before the final measurement run.

Only the existing `/tmp/opencode/fbp-foundation-20261123/target` was used for debug
builds, with `CARGO_BUILD_JOBS=2`. No new full-workspace debug target or deletion of
preexisting artifacts was performed. Scoped release-only baseline and control
targets were used for paired evidence, not full-workspace builds.

## Local Paired Evidence

Baseline worktree `/tmp/opencode/fbp-q3-whole-baseline-20261124` is detached at
1589. Production baseline code was not patched. Workload bodies, assertions,
allocator instrumentation, Kernel bench manifest, Cargo/Nix locks and toolchain
are identical by `cmp`. Only the two tiny policy adapters differ: baseline no-op
versus candidate `PlanningLimits { max_units: 50_000_000 }`. No API is backported
and no production compatibility alias was introduced. Existing native controls
retain their default policy; all new stress fixtures successfully complete.

Both sources were prebuilt with `CARGO_PROFILE_BENCH_DEBUG=0`,
`CARGO_PROFILE_RELEASE_DEBUG=0`, two jobs and the same Nix environment. Timing
executables have NO counting allocator. Each case has two warmups/seven unsorted
microsecond samples. Three complete pairs ran in baseline/candidate,
candidate/baseline, baseline/candidate order. Runs include setup, actual planning,
replay/compiler fallback, callbacks/activation where present, assertions (including
fresh canonical compilation/execution in Graph fixtures), and teardown. Host
rebuild cases include three prepare/publish revisions and old-state retirement;
they are restart/rebuild measurements, not persistent Q2 cache reuse.

Host: Linux 6.18.38, Intel i7-8550U, four cores/eight logical CPUs. Frequency and
affinity were not pinned; other host work/thermal noise was not controlled. The
raw untouched runtime controls fluctuate substantially. These are descriptive
bounded workload measurements, not confidence intervals or a product SLO.

### Timing Summary

Numbers are the median of each source's three seven-sample medians, in us, not
ratios averaged across unrelated workloads. All underlying arrays are retained.

| End-To-End Case | Integrated 1589 | Candidate |
| --- | ---: | ---: |
| Successful dense32 | 938 | 776 |
| Successful dense64 | 3,130 | 2,363 |
| Successful dense128 | 13,865 | 9,439 |
| Successful dense256 | 61,936 | 41,000 |
| Reverse-priority dense128 | 13,702 | 9,320 |
| Staged defeat/recovery, 32 motifs | 121,344 | 139,962 |
| Ordered splices128 | 9,725 | 11,662 |
| Hierarchical128 | 4,877 | 5,151 |
| Real Kernel dense64 prefixes | 45,628 | 39,497 |
| Real Kernel preview64 build | 117,432 | 135,476 |
| Real Kernel preview64 three-revision rebuild | 369,686 | 402,363 |

Dense64/128/256 and reverse128 improve in all three source pairs. Do not
extrapolate that to all shapes: repeated defeat/recovery, splices and actual
preview/rebuild medians regress in this complete run. Metering registry/text,
ancestry and manifest work on every update is a deliberate boundedness cost;
these dependency-rich workloads gain little from the two targeted optimizations.
This is an explicit policy/performance tradeoff for final review, not a blanket
speedup or portable non-inferiority claim. The control below separates that cost
from the optimizations and shows substantial host/run-order variability.

### Separate Allocations

Dedicated System-wrapper executables, NOT the timed binaries, measured seven
whole-fixture samples after two warmups. Counts are identical across all seven
samples for each shown case; residual requested bytes are zero in all measured
fixtures. Peak live requested bytes are not RSS or allocator metadata/capacity.

| Case/Source | Allocations | Reallocations | Allocated Requested Bytes | Peak Live Requested Bytes |
| --- | ---: | ---: | ---: | ---: |
| Dense256 baseline | 26,954 | 2,911 | 11,028,046 | 3,103,987 |
| Dense256 accounting-only | 26,486 | 2,905 | 10,744,030 | 3,128,483 |
| Dense256 full candidate | 25,977 | 2,905 | 10,613,980 | 3,063,713 |
| Kernel dense64 baseline | 88,999 | 6,590 | 22,917,873 | 1,083,094 |
| Kernel dense64 accounting-only | 86,900 | 6,590 | 22,123,353 | 1,085,942 |
| Kernel dense64 full candidate | 84,884 | 6,590 | 22,035,993 | 1,081,910 |
| Kernel prefix64 baseline | 893,876 | 8,291 | 72,793,223 | 1,784,592 |
| Kernel prefix64 accounting-only/full | 891,808 | 8,095 | 71,936,727 | 1,788,560 |
| Kernel preview64 baseline | 1,267,083 | 21,022 | 112,338,879 | 2,207,543 |
| Kernel preview64 accounting-only/full | 1,264,823 | 20,826 | 111,450,127 | 2,211,511 |

Do not hide peak regressions: prefix/preview bookkeeping has a measured +3,968
requested-byte peak versus baseline, identical with/without the optimizations.
Accounting-only dense256 also increases peak versus baseline; the full candidate
reduces it. Collection construction/transaction lifetimes are not allocation-free
or promised byte-monotone. The bounded logical policy and correctness invariants
are the contract; the allocator records expose the implementation tradeoff.

### Accounting-Only Control

Detached worktree `/tmp/opencode/fbp-q3-accounting-control-20261124` has identical
candidate production/harness files except `compose/planning.rs`: restore full
posting-set visitation (charge each owner before filtering), and restore lazy
N-cell rows even for empty parent lists. No runtime switch or new production
planner is introduced. It uses the same finite adapter/selector settings and
successful workload assertions. Three interleaved control/candidate pairs ran
dense cases, an unchanged no-write chain, and Kernel prefix/preview/dense64.

The dense256 optimization-only comparison removes exactly **509 allocations and
130,050 requested bytes**, with peak down **64,770 bytes**. That matches the empty
rows in the initial 256-node plan and the 255-node winner-revocation plan.
Kernel dense64 removes exactly **2,016 allocations and 87,360 requested bytes**
across its prefixes, with peak down 4,032 bytes. Ordinary distinct prefixes and
previews have identical allocator records with/without the optimizations, as
expected. Private posting tests establish suffix-only visitation deterministically.

Timing-only attribution is weaker: dense256 three-trial median-of-medians is
37,734 us accounting-only versus 37,479 us full candidate; smaller dense sizes
vary in both directions. No statistically established isolated timing speedup
is claimed. The baseline-to-candidate timing difference must not be attributed
entirely to the two optimizations: fallible loop/materialization rewrites also
change generated code. Allocation/visitation wins are established independently.

A separate rotating three-source run focused on the apparent dependency-rich
regressions. Its median-of-medians (us) was:

| Case | Baseline | Accounting-Only | Full Candidate |
| --- | ---: | ---: | ---: |
| Splices128 | 11,498 | 12,109 | 10,416 |
| Hierarchical128 | 4,946 | 5,703 | 4,923 |
| No-write reverse-lexical chain128 | 525 | 524 | 514 |

These do not establish that bookkeeping is free or that the regressions in the
complete run can be discarded; they show why a universal timing conclusion is
unsupported. Accepting the finite resource policy, its added charged work and
these published timing/peak-memory tradeoffs is part of independent whole-finding
review. No ad hoc cache or third planner optimization was added to conceal cost.

Final measurements use three separate release locations: the existing shared
foundation target for candidate, the scoped baseline target, and the scoped
accounting-control target. A preliminary experiment reused the baseline target
for control builds. Final control binaries were instead rebuilt in a fresh
scoped release target to avoid shared-target fingerprint ambiguity; the final
hashes distinguish all three sources. Baseline source/target remain baseline;
control source/target remain control. No preexisting artifacts were deleted.

### Raw Records

Final complete timing records, each with original arrays:
`/tmp/opencode/fbp-q3-whole-v3-{baseline,candidate}-{graph,kernel}-{1,2,3}.txt`.
Separate allocator records:
`/tmp/opencode/fbp-q3-whole-v3-{baseline,candidate}-{graph,kernel}-allocations.txt`.
Control timing records:
`/tmp/opencode/fbp-q3-whole-v3-control-{control,candidate}-{1,2,3}.txt`;
allocator record `/tmp/opencode/fbp-q3-whole-v3-control-allocations.txt`.
Rotating attribution records:
`/tmp/opencode/fbp-q3-whole-v3-attribution-{baseline,control,candidate}-{1,2,3}.txt`.
Earlier filenames without `v3` are preliminary runs and must NOT substitute for
these records. The final source includes constructor rejection disposal and the
separate endpoint-scan text charges. All four full allocator logs and the control
allocator log are byte-identical to their earlier snapshots (the fixes add no
successful-path allocations).

Representative final dense256 raw samples, in us:

```text
baseline_1=[51625,52885,50810,51193,53302,49689,53038]
candidate_1=[33973,34140,40397,43478,33876,34569,33814]
baseline_2=[60966,61936,65692,60743,64453,63689,61067]
candidate_2=[41774,41298,38887,40826,43175,43232,45062]
baseline_3=[65324,64271,61816,69485,67273,69760,69265]
candidate_3=[37823,51325,41000,39939,42799,41519,37356]
```

## Reproducibility And Acceptance

All three worktrees start at integrated 1589; source changes remain uncommitted.
Candidate production SHA-256 at local verification/measurement:

```text
2408a421c56d50081965e87b7aa29e76a502d73f503e1b4079e99acb6bb05368 compose/budget.rs
0939aafad3600387d73a69d38dd2c16e963d0df67783996ade35022bed7660ca compose/edit.rs
1f875bc182153afe319761561a8147d677293ec20d0950984a3fe1f9df4ee2b4 compose/mod.rs
70abb741eeecfc1a6c5e890fc927c21314c271243c132690099155de8ec39707 compose/planning.rs
47167e50f699a4ed5a3bcfdb669f005a509f4e36bfc8a8dcf4532597c30ee8c3 compose/selection.rs (unchanged)
d69ed2465f6f78a46eefb9b718c4b43a5494eb56b5fbfb36c24beb0b675ce5d2 kernel/src/build.rs
69c7fd41de9b487b143dbdfd2317229be1f907cc8a38d6dfb82d90428113197c kernel/src/lib.rs
1904cc915b4a419e5e021120e533de344cac08aeb4a8e5eaf5d05648862e678c graph/API-EXAMPLES.md
```

V3 workload/instrumentation SHA-256, verified identical on baseline at that run
(the recovery followup below extends the Kernel harness):

```text
9483dc887944d082cdb78d9a18ae3fffa20d713e024d380c2839cface245496d graph/benches/native_fbp.rs
31667bd50a8f28b902b0a219fc329eeea6ee6a99250f09b3c6b82f0ccdcb8b67 graph/benches/support/composition_cases.rs
5bf88f4d447c0f6c438d837dc34ce39a130780f25ec604c250270f2a62344d4b graph/benches/support/allocation.rs
f607b10a1c9be940259ef8cca5e82aefc58ebb8e3b99653b49fe633b5a462bc7 kernel/benches/support/composition_cases.rs
062ece0710b2770aef70b773b6eed8b476dc001850af9d434617f27d52c195fd kernel/benches/composition_planning.rs
986e33d995a40d5e2f34703e8479ef3aff62c22addf8a938eb9a281da33f5a8b Cargo.lock
217f54c715f93488d4b0f2adfea3527c1d78c9e56900559650d0574f69881d1d flake.lock
263d5b239cdd01a4d4089308d3269cf99aa1b0e6ac7512dea6bd7ebf49e8c4f6 accounting-control compose/planning.rs
```

The intentionally different policy adapters are also pinned:

```text
3edba0522579fb1dbd58a88efea40b06c6f6d8b8f13462861f9229a00ccf9222 candidate graph planning_policy.rs
1492588bf9ac8a5ca423f2d8686275529beb0862d10be0d04955c4ff7bbd897c candidate kernel planning_policy.rs
39430bc56b8e37168590204a97e1228df6d34ef89a9e62ed6e479ab2cc63f0ab baseline graph no-op planning_policy.rs
d502931bdcc8d17c6d1c07a82239de3da1b8da02f1b53e957c2ecb8f45e21d72 baseline kernel no-op planning_policy.rs
```

V3 measured timing executable SHA-256, before the acceptance followup:

```text
3432f9852aa88c5ecca1cae65179510f7f2e09c8d7ca948f12aee31f4de0a375 baseline native_fbp
79c46273a92f78c6b20c0aa54669afd990d7649ac8e64adbc7536d00dead6e16 baseline composition_planning
618d8005a79a39f98ece7631487d240e0e70606363dab652ff049f9f46ccb6e9 candidate native_fbp
5e0ef9bcd9aa6a0245f8c2f5d8b94878008b46643dc8830e85ca6949d8dc373b candidate composition_planning
06b3c046a95b8ebbc0f580d461c9b83d8c26472b66789c9e2812db1c4f8b89b4 accounting-only native_fbp
bca1e9afe4a3352abb9b273e15c198104cdd679adb6aa7db359ef57da0517dbf accounting-only composition_planning
```

The Q3 workflow is updated locally with broader dependency triggers, exact pinned
1589 reference, identical-harness verification, two-source prebuild, three paired
Composer/Kernel repetitions and separate allocator artifacts. Actionlint passes.
This patch has NOT been pushed or hosted-verified; historical credential/workflow
restrictions are not declared resolved. Existing main scorecards were not edited.

Independent final review subsequently reported no production defect/P0/P1 and
approved the design/implementation, but identified two P2 acceptance gaps. The
followup below fixes those gaps rather than treating the preceding broad
correctness/ownership claims as sufficient. The original Q3 scope and explicit
composition-only logical-accounting exclusions are unchanged.

## Independent Review Followup

No production code or lease API changed for this followup. The production hashes
above were rechecked unchanged. New shared fixture
`packages/phenix-kernel/benches/support/recovery_case.rs` supplies real snapshots,
preparation callbacks, execution and lifetime witnesses to the Kernel tests,
timing bench and separate allocator executable. There is no new planner or
generic test framework.

### Actual Kernel Recovery

Each bounded group admits six distinct plugin revisions in lexical contribution
order, with snapshots supplied in reverse order. Each has one `main` contribution:

| Admission | Writes And Dependencies | Prefix Observed By Its Callback |
| --- | --- | --- |
| `g000-0p` | `a000`, priority 0 | Empty group |
| `g000-1q` | `b000`, priority 0 | `a000:p` |
| `g000-2Q` | `a000`, priority 5, requires q | `a000:p, b000:q` |
| `g000-3P` | `x000`, priority 10, requires p | `a000:Q, b000:q` |
| `g000-4R` | `b000`, priority 3 | `a000:Q, b000:q`, with P still absent after P's main admission |
| `g000-5s` | Stable witness `s000` | Recovered `a000:p, b000:R, x000:P`; additionally `s000:s` when self-preview is enabled |

Every callback asserts the exact physical declaration set/count and exact public
input/output names, not cardinality alone. Each declaration's explicit origin
equals its flat-process/effective origin, is a distinct descendant of the expected
actual snapshot's origin and belongs to no other revision. First-observed exact
contribution tokens are retained as metadata only: p must recover with its SAME
origin token after Q suppressed it. Q's token is checked across the suppressed
prefixes, and the self-preview witness token must equal its eventual main token.

Only s advertises/imports a contract. It is always selected and never competes
with p/q/Q/P/R, so no defeated offer is left to invalidate final contract checking.
Its static preview and prepared executable have different arithmetic; no static
preview handler may execute. Both preview and no-preview recovery run at the
default policy for one and four groups (six and 24 main admissions).

Final artifacts exist exactly for p/P/R/s; q/Q artifacts and prepared executable
payloads are disposed. Producer activation is exactly the selected owner set.
Actual Kernel injection/execution produces the expected per-owner arithmetic
and exactly one effect per selected owner, with no q/Q effects. After Kernel drop,
an independently retained compiled network releases q/Q revisions/code but keeps
only p/P/R/s executable ownership. A second actual session executes the retained
network, then a detached factory/process proves ownership survives session and
factory release. Dropping the last process releases all remaining revision/code
owners, with exact effect multiplicities and per-owner disposal ordering checked.

### Pinned Failure Phases

`later_preview_and_post_prepare_main_denials_preserve_exact_cleanup_order` uses
that same actual six-revision recovery sequence, with these literal whole-build
boundaries (not an aggregate callback-count search in the final test):

| Limit | Exact Failure Contribution/Phase | Exact Preparation Callback Sequence |
| ---: | --- | --- |
| 8,713 | `g000-5s/main`, late self-preview denial | p, q, Q, P, R |
| 8,714 | `g000-5s/main`, preview completes; main admission denies after preparation | p, q, Q, P, R, s |
| 12,022 | `g000-5s/main`, later main-planning denial, total minus one | p, q, Q, P, R, s |
| 12,023 | Exact-total success | p, q, Q, P, R, s |
| 13,023 | Explicit larger-finite-limit retry succeeds | p, q, Q, P, R, s |

Every denial preserves the exact `PlanningBudgetExceeded { limit }` source and
contribution key, and has zero producer activations or executable/static effects.
The existing exact-total/total-minus-one and larger-build retry tests remain.

These witnesses are NOT the old `factory_dropped` activation-closure counter.
They separately identify the actual prepared map-handler payload (`executable`),
static callable payload, exported artifact, activation closure and producer.
Every callable/artifact Drop asserts that its own revision and code are alive.
The plugin implementation owns the revision sentinel; the actual catalog factory
captures the code sentinel. Catalog/set temporaries are released after creating
each real `PluginSnapshot`, leaving normal snapshot/executable ownership as the
only lease mechanism. Trace flags and graph-origin tokens own no snapshot or
code lease and cannot artificially prolong the revision.

While each returned scalar planning error is STILL HELD, the test checks all six
revision/code sentinels have released exactly once. For EACH owning revision,
every expected executable/artifact/activation/static disposal precedes its
revision release, which precedes its code release. The preview-denied s has no
prepared payload/artifact/activation closure; the post-preparation-denied s has
all three and must dispose them before its lease releases. Assertions are per
owner and exact count/order, not aggregate totals or an unrelated final marker.

### Recovery Measurements

The existing Kernel harness now includes `kernel_recovery_preview{1,4,8}` end to
end: six, 24 and 48 real main admissions, plus one stable self-preview per group.
The allocator executable includes the same bodies, and the local paired workflow
copy/hash list includes the new fixture. Workload bodies and assertions are
byte-identical by `cmp` on the integrated-1589 baseline; the previously documented
no-op versus finite-limit adapters remain the only policy difference.

Both sources were prebuilt in their EXISTING release targets, using two jobs,
the same Nix/rustc 1.96.1 environment and release/bench debug=0 settings. Three
baseline/candidate, candidate/baseline, baseline/candidate pairs ran with two
warmups and seven unsorted samples each. Timings include snapshot/plugin
construction, prefix/origin checks, callbacks, compilation, final execution,
detached execution and teardown/ownership assertions. No pure-planner timing or
portable speedup/non-inferiority/SLO claim is made.

| Recovery Groups | Baseline Median Of Three Medians (us) | Candidate (us) |
| ---: | ---: | ---: |
| 1 | 755 | 773 |
| 4 | 7,253 | 6,470 |
| 8 | 26,600 | 27,060 |

Separate, untimed allocator evidence has identical counts across all seven
samples per case and zero residual requested bytes:

| Groups/Source | Allocations | Reallocations | Allocated Requested Bytes | Peak Live Requested Bytes |
| --- | ---: | ---: | ---: | ---: |
| 1 baseline | 5,421 | 155 | 728,255 | 164,503 |
| 1 candidate | 5,383 | 155 | 718,764 | 162,319 |
| 4 baseline | 56,760 | 1,571 | 6,766,625 | 606,682 |
| 4 candidate | 56,361 | 1,541 | 6,634,895 | 597,398 |
| 8 baseline | 245,983 | 5,402 | 26,217,405 | 1,146,254 |
| 8 candidate | 244,574 | 5,300 | 25,725,601 | 1,130,546 |

Complete raw records are retained at
`/tmp/opencode/fbp-q3-review-recovery-{baseline,candidate}-{1,2,3}.txt` and
`/tmp/opencode/fbp-q3-review-recovery-{baseline,candidate}-allocations.txt`.
The prior V3 records and fingerprints remain historical evidence for their
original workload bodies; the Kernel executable has since been extended, not
silently relabeled as a rerun of those old measurements.

Current common-harness/test SHA-256:

```text
f78eab63e48f8aa2d7eb0ef253149a929845d8269c5472cfa1e460e202222fa5 kernel/benches/support/recovery_case.rs
8c5c1b0d0a6e31d04709c32c8f0c9986a8070104d649edcab0b98f9b6633f1fa kernel/benches/composition_planning.rs
9af6332363b3c1d3d67e759d3641f4e93bf5d6c62643e1b79224221ac883986b kernel/examples/fbp_kernel_composition_allocations.rs
02ca3b766ba1db093e4a091b7ad126a168dcd65aed72872c5ebd009970837aca kernel/tests/composition_planning_limits.rs
```

Measured executable SHA-256 for this followup:

```text
4d35527a0ddf18bcb9d17d7bf1c8f7ea5b95ee065950665be11d9c345948c6aa baseline composition_planning
067ff53c3323cbc991f6b4eddf84d3d3267718c14dde6630ae00116929d6f7cd candidate composition_planning
c134eb840ec7d12874300f578df2fd4e1b4603cc70bcd4c56200eddd9945a228 baseline fbp_kernel_composition_allocations
3774ee3c5f93cc99ef066ae157c377e0c5bbf7b81630a1b961bacc3937c041ef candidate fbp_kernel_composition_allocations
```

### Followup Verification

Default and `--no-default-features` targeted Kernel runs both pass all 16 tests:
`composition_planning_limits` (now six), `composition_selection_limits` (three),
`code_leases` (five), and `activation_artifacts` (two). Kernel still enables its
graph dependency's default features; no new claim of isolated no-default Graph
verification is inferred from this. Graph production/tests are unchanged from
the earlier isolated verification. Strict Kernel all-target Clippy, formatting,
whitespace and local workflow actionlint pass. A full workspace run was not
repeated for these test/harness-only corrections.

Both P2 acceptance gaps are implemented and locally verified, not deferred.
Remaining acceptance is main's review of these fixes, source-only publication
through existing PR #1392, and exact published-source checks. The paired workflow
patch remains local/blocked and has not been published or hosted-verified. No
files were staged, committed or pushed; no preexisting artifacts were deleted
and no new target directory was created for this followup.
