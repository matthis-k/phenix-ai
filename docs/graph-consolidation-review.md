# Graph Consolidation Review

## Findings

No blocking correctness, regression, or public semantic-change finding in the
current Batch A/B production diff. No existing test assertion was removed or
weakened in a way that loses coverage of a retained production invariant.

### R1: Pending Cohorts Not Compared (Low, Nonblocking)

Locations: `packages/phenix-graphs/src/tests.rs:9289-9308`, using the capture at
`packages/phenix-graphs/src/tests.rs:9090-9100`.

The new targeted-versus-conservative differential test compares remaining input
addresses and ordered payloads, but the capture drops each delivery's cohort.
Two queues with identical payloads assigned to different cohorts therefore compare
equal. The independent activity oracle also cannot catch that discrepancy: pending
indexes describe occupied slots, not buffered cohort membership. Published cohorts
and incomplete-group cohorts are compared by the existing report normalizers, but
that does not establish equality of every retained delivery's cohort.

Action: retain and compare `(cohort, payload)` per buffered delivery, in queue
order, and include equal payloads in distinct cohorts in a leftover-input case.
This is a limitation of the newly added differential coverage, not an observed
scheduler defect or a weakening of the preexisting capture tests. It does not
block this state-deletion refactor.

Resolution (2026-11-10): `CapturedWave.pending_inputs` now retains ordered
`(cohort, payload)` deliveries, and the differential normalizer compares both.
The added two-`skip` fixture asserts equal leftover payloads in distinct cohorts
40 and 41; existing key/emptiness capture assertions are preserved.

## Acceptance

| Batch | Independent Phase 4 Decision | Basis |
|---|---|---|
| A: action lowering | ACCEPT current implementation diff | Shared sequencing and transport lowering genuinely replace the old executors; ownership, timing, factory lifetime, source and return semantics are preserved. |
| B: scheduling state | ACCEPT current implementation diff, with nonblocking R1 | Deletes duplicate stored facts without changing scheduler transitions; independent index/work checks replace the lost aggregate cross-checks. |

These are implementation-review decisions, not named-maintainer acceptance or
release/CI approval. No reduced expressiveness is approved. Batch C, projection
and patch-observer changes are outside this review.

## Review Basis

Reviewed 2026-11-10 against HEAD
`d758291c1db77abcff60ac3ac0e167c0f59261bd`. Inspected the actual working-tree diff
for `packages/phenix-graphs/src/{authoring,lib,scheduler,tests}.rs` and the full new,
untracked `packages/phenix-graphs/tests/action_lowering.rs`. Read the design,
independent critique, actions implementation record and scheduling-state record.
Unchanged surrounding source/tests were inspected only to resolve the changed
code's contracts. All source references below use current working-tree lines.

This review does not treat the implementation agents' self-reviews as independent
approval. The only file created by this reviewer is this document; no source or
test file was modified.

## Action Lowering

The facade challenge is satisfied, with a deliberately narrow claim.
`lib.rs:2137-2146` owns the mutable-context preparation, error short-circuit,
subsequent shared-context invocation and single zero-or-one vector boundary.
The six old private executors disappear, and `typed` delegates to `with_layout`
at `lib.rs:2508-2518`. Constructor closures still perform the genuinely different
extraction and return conversions; they no longer each implement context
sequencing or construct their own transport vector. There is no replacement
public trait, erased argument box, runtime enum or per-old-executor helper layer.
This is real internal consolidation, not a new scheduling or execution model.

- `lib.rs:2140-2145` adds no Clone/Sync bound on the extracted argument and does not store it in the returned action. The mutable borrow ends before invocation. The Send-only `Cell<i64>` probe checks context/grants after extraction and rejected decoding with no handler calls or publication (`tests/action_lowering.rs:14-99`).
- Encoded extraction remains `take_prepared`, not decoding moved after admission (`lib.rs:2379-2381,2476-2478,2498-2501`). The port-owned builder and reservation transaction are untouched.
- Local structural preparation still takes reserved raw selections and uses slots captured once at declaration (`lib.rs:2415-2429`). Move-only ownership and the preexisting lack of raw-input rollback on local preparation failure are unchanged.
- Multiport keeps physical-port dispatch and its original errors before the contextual handler (`lib.rs:2338-2358`). That heterogeneous adapter is necessary, not a duplicate action executor.
- Factory invocation remains inside the physical action factory (`lib.rs:2533-2543`). The new test checks no eager creation, four handlers for two nodes in two engines, and independent repeated counters (`tests/action_lowering.rs:114-162`).
- Both source paths retain empty port lists and now use the same helper (`authoring.rs:137-174`). Plain sources lower via the blanket plain-value LocalReturn implementation; directed sources use the existing Output conversion. No synthetic input or fabricated prepared unit is introduced.
- Plain Option/Result payloads are not interpreted as suppression. LocalReturn still distinguishes no emission from one value selecting no destinations (`lib.rs:2149-2174`). Raw actions and external Vec completions are intentionally not forced through the private Option boundary.

The helper reduces repeated code and transport-construction sites. It is not
evidence of improved allocation totals, binary size, compile time or throughput;
the actions record appropriately makes no such measured-performance claim.

## Scheduling State

The only production scheduler changes delete diagnostic aggregates and their
maintenance (`scheduler.rs:813-888`). Pending indexes, admitted producers,
dispatched tickets, reservations, rejected cohorts and settled colors remain
distinct. Admission, buffer consumption/retirement, decoding, publication and
completion ordering are not consolidated or moved.

The sole retained outgoing adjacency is `by_output`. Build validation still
requires canonical output zero and validates each edge before insertion
(`lib.rs:3618-3644`). Therefore iterating its keys has the same source-node order
as the removed node-keyed map, and each source's edge-vector order is unchanged.
Partition and reverse indexes reuse this map (`lib.rs:4522,4578`); topological
ordering still seeds all declared nodes and deduplicates successors
(`lib.rs:5254-5289`). Sparse IDs and nodes without outgoing entries remain valid.

### Oracle Independence

- Diagnostics scan nonempty actual queues and admitted producers, with producers taking precedence for Executing (`tests.rs:5-47`). Active partitions count distinct active nodes, not invocations. Empty retained queues and reservation history do not count as activity.
- `assert_activity_consistent` reconstructs pending-port occupied-slot counts and pending partition membership directly from buffers (`tests.rs:50-67`), not from either pending accelerator. A stale accelerator is detectable.
- Expected producer membership comes from ready work, dispatched tickets and explicitly supplied manual work, including IDs, nodes and cohorts (`tests.rs:69-114`). It is not derived from the producer map being checked. Duplicate ownership and mismatched producer map keys are asserted.
- The manual capture driver supplies the popped invocation before and after publication and removes that allowance only after finishing it (`tests.rs:9040-9086`). It does not pretend to exercise production ticket issuance.
- Separate real-session coverage checks queued versus dispatched work, two invocations of one node, independent waves, foreign/duplicate completion, pending inputs after completion, cancellation and invalidated tickets (`tests.rs:10277-10357`). The retained aggregate-style assertions are thus not merely circular replacements by new diagnostic getters.
- The new differential driver compares targeted and conservative completion reevaluation with the oracle on both paths (`tests.rs:9081-9089,9261-9311`). Existing expected-output, staged-reservation, rejected-cohort, malformed-selection, routing and exact reachability tests remain in place. R1 bounds what its leftover-input comparison establishes.

This is a useful independent consistency oracle, not proof of every policy or
delivery semantic. In particular, ready/ticket agreement cannot independently
prove correctness of a cohort that was already wrong in both work owners; the
existing behavioral expectations remain necessary.

## Cycles And Invalid Outputs

Cycles remain accepted, not converted into a DAG restriction. Stable cyclic
leftovers are still appended by node order (`lib.rs:5284-5288`), without promising
liveness. The new sparse fixture checks stable order, duplicate successors,
edge order/IDs, no-outdegree nodes and cycle reachability
(`tests.rs:10361-10412`). The randomized 24-graph cyclic/multiport test still
compares reverse reachability with a forward traversal
(`tests.rs:11150-11244`); changing that forward traversal to the retained adjacency
does not make it the same algorithm as the reverse cache.

Invalid output addresses do not alias slot zero: lookup explicitly rejects them
(`lib.rs:5349-5354`), and adapter registration uses the full OutputAddr key
(`scheduler.rs:625-638`). The new test checks both boundaries
(`tests.rs:10405-10412`).

Invalid runtime emissions are a separate contract from invalid graph addresses.
The unchanged whole-completion validator rejects multiple emissions, a nonzero
slot or wrong value identity before any routing/publication, recording a failed
settled outcome (`scheduler.rs:1410-1430`). Completion then closes the producer
and reevaluates waiting work (`scheduler.rs:445-461`); it does not leave an invalid
cyclic producer open, silently treat invalid output as successful suppression,
or route a valid sibling before discovering an invalid one. Prior successful
publications are not rolled back. Existing external-completion tests retain
zero-publication/zero-delivery assertions (`tests.rs:6435-6462`).

Coverage boundary: the new adjacency test's invalid address belongs to node 7,
not its cyclic component, and it does not submit a malformed runtime completion.
Runtime invalid-completion coverage is in a separate, acyclic fixture. There is
no dedicated combined cyclic-invalid-completion oracle here. This is not a new
behavior change or blocker: validation precedes adjacency traversal and the
relevant completion/settlement paths are unchanged. If that combined guarantee
is advertised as directly tested, add a feedback-session case asserting failed
settlement, no new deliveries/publications, producer closure and no spurious
feedback admission after malformed completion.

## Verification And Limits

Ran `git diff --check`: passed. No test execution was needed to resolve a suspected
production regression; the full suite was not duplicated. Reported test evidence
is attributed, not independently rerun: the actions record reports 610 passing
tests with 3 ignored at its snapshot and two no-default-feature lowering probes;
the later state record reports 198 passing library tests after the differential
test was added. These are different shared-worktree snapshots, not identical
exact-final-revision evidence.

Final integrated-revision checks, project-required remote CI and named maintainer
acceptance remain delivery-owner gates. Neither this static review nor a prior
snapshot's passing suite substitutes for them. The state record's inline-size
measurements and deleted edge-copy accounting are appropriately bounded; no
allocator-total or runtime-speedup acceptance follows from them.

## Batch C: Independent Phase 4

### Findings

#### C1: Bulk Projection Omits Public Revision Validation (Medium, Blocking)

Locations: `packages/phenix-graphs/src/authoring/operation.rs:1080-1100`
and `1109-1115`; compare manual validation at `936-940`. The shared leaf
preflight at `978-1016` validates the destination, not the retained public
interface's scope/definition token. `Module` exposes mutable access to its
GraphBuilder at `852-861`, so replacing or cloning that builder is reachable
without private field mutation.

Concrete public counterexample, starting with an otherwise ordinary atomic module:

```rust
let mut module = Module::new(
    "root",
    InputInterface::single(Layout::<i64>::atomic()).unwrap(),
).unwrap();
let child = module.node("child", |x: i64| x).unwrap();
*module = GraphBuilder::clone(&*module);
assert!(module.expose(module.inlet(), child.inlet()).is_err());
// Current control flow returns Ok and creates the export here:
module.expose_matching(child.operation().inlets()).unwrap();
```

GraphBuilder cloning preserves child scope/declaration tokens but mints a new
root definition token (`authoring.rs:802-819`). Consequently every bulk child
check passes while the retained public AddressBook is stale. Replacing the
underlying builder with `GraphBuilder::new(...)`, then declaring a new matching
child, also passes bulk validation despite a foreign retained public scope.
Manual exposure correctly rejects both cases. This is not an existing-export
token panic; it is a missing selected-side validity gate, required by patching
design lines 349-353 and the revised critique lines 428-431. The omission predates
this extraction, but remains an unfulfilled repair requirement in the actual
final implementation.

Required change: validate the Module's retained public book against the current
builder scope and definition token before bulk planning/commit, preserving the
manual check. Add both clone/divergent-token and whole-builder/foreign-scope
public tests. Assert Err and unchanged export vectors/tokens/IDs; include an
already-exported leaf so rejection cannot append fanout. Do not normalize the
public selector token merely because the patch observer normalizes clone-only
root definition IDs: these are different contracts.

#### C2: Cross-Kind Residual Tests Are Incomplete (Low, Nonblocking)

Locations: `packages/phenix-graphs/src/authoring/observer.rs:399-575`,
`packages/phenix-graphs/src/authoring/observer/tests.rs:506-540,597-663`, and
`packages/phenix-graphs/tests/operation_substitution.rs:291-331,558-602`.

The implementation compares the complete expected export after retargeting,
rather than skipping an export merely because a substitution exists. Static
inspection found no overbroad exception. However, the new exact-lift tests do
not independently exercise a legitimate cross-kind substitution combined with
an extra export target, target reorder, export metadata change, or input-token
change. The standalone export matrix at observer/tests.rs:805-862 does not enter
the substitution exception at all. The existing substitution tests cover ordinary
retargeting, not this negative ownership matrix. Add private observer cases
asserting the additional PublicInput/PublicOutput requirement while keeping the
operation transition precise; retain a valid multi-target fanout control and an
equivalent precise custom callback. This is a regression-coverage gap, not an
observed authority bypass, and is separate from C1.

Resolution (2026-11-10): private observer coverage now checks both cross-kind
directions with a valid ordered two-target fanout control and equivalent precise
custom callbacks. Extra targets, target reorder, input/output metadata changes
and input-token changes add exactly the appropriate PublicInput/PublicOutput
requirement, and narrowly declared callbacks reject those residual writes.
Verification for R1/C2: `nix develop --command cargo test -p phenix-graph --lib
--locked` passed all 220 tests (0 failed, 0 ignored). No production changes were
needed for these two coverage findings; C1 and integration gates remain separate.

#### C3: Full Accounting Has Nontrivial Authoring Cost (Low, Nonblocking)

Locations: `packages/phenix-graphs/src/authoring/observer.rs:146-150,153-187,
218-272,598-638` and `packages/phenix-graphs/src/authoring.rs:1868-1874`.

The roughly 730-line observer replaces partial enforcement scans with materially
more correctness logic. It does not simplify runtime execution or reduce stored
runtime state. Survivor comparison uses repeated slice membership checks, and
registration/index validation and cursor replay contain repeated linear scans;
these have quadratic worst cases in declaration/edge counts. Dependency closures
are traversed per direct alias in both states, and observation allocates ordered
maps/sets and clones the pending contents before callbacks. Those costs occur
even for a no-op contribution. The legacy diagnostic scan also remains after
the full enforcement traversal (`observer.rs:713`).

This is justified as a bounded correctness expansion under unrestricted builder
replacement, not by the number of scans deleted or a performance claim. The
implementation record appropriately disclaims measured speed/allocation wins.
No hot scheduler path was added. Benchmark representative large authoring graphs
and heavily shared aliases if throughput matters; do not rewrite the observer
or add cached ownership state solely to make a simplification narrative true.

### Accounting Review

No additional concrete missing field/owner or coupled-state defect was found in
the inspected observer. In particular:

- `patching.rs:137-166,770-782` separates closed directional coverage from planner overlap. Graph requires Graph, same-name Node/Instance authority is intentional, patterns check every specified component against parsed exact keys, numeric keys require exact/Connections authority, and Splice grants no ordinary connection authority. Resolution is not rerun and footprints are not promoted after callback execution.
- `observer.rs:38-135,196-397` accounts for raw and pending payloads, opaque allocation replacement, declaration/input tokens, limits, trigger maps, dependencies, identities and provenance. Before/after alias closures are unioned, cycle-safe, and charge every owner rather than the first or every unrelated alias. Orphan changed imports/origins and malformed associations conservatively require Graph. Reverse-ancestor identity refresh is included by comparing the complete identity map, not just the edited definition.
- `interfaces.rs:5-12` combines semantic shape/key/unnamed-identity comparison with exact stored named input/output values. This covers the current InterfaceShape inventory, including local schema text, without changing public semantic equality. Claims, request prefixes and lift blocks all use it. The privacy-boundary limitation on exhaustive field checking is accurately disclosed in the implementation record; new private shape fields require comparator review.
- `observer.rs:399-537` derives actual same-name cross-kind mappings and matches an unchanged request prefix plus complete ordered lift blocks, once each. Root claims and limits are checked independently. `539-575` preserves export metadata, target order/multiplicity and input tokens outside the exact mapping. Connection changes still emit old/new exact requirements. No boolean request-lift or empty-name diagnostic grants authority.
- `observer.rs:598-689` checks survivor order, node allocation replay, retained edge IDs, sequential new edge IDs and append position. A connect/disconnect allocation residual is not incorrectly treated as a clone no-op. Conservative Graph fallback for unexplained history follows the accepted design rather than silently enlarging a narrow footprint.
- `authoring.rs:1868-1895` snapshots pending contents before the callback and observes separately from revision rendering. `patching.rs:756-807` retains the outer draft and one final assignment, including splice lowering. Candidate error, coverage failure and panic cannot commit earlier selected changes. Captured original-store handles/interior state remain outside this guarantee, explicitly documented and tested (`observer/tests.rs:445-504,665-690`).
- Clone-only root definition IDs and pending wrapper allocations are normalized, while real pending-entry presence and stored content remain writes (`observer.rs:309-328`). The populated pending/dependency no-op regression includes repeated cloning, restored origin scopes and caught rolled-back additions (`observer/tests.rs:43-68`). No factory, policy, argument builder, codec, layout evaluator or compiler is invoked by observation.
- The accepted API-neutral alternative is implemented: private requirements enforce writes; public revisions are explicitly partial (`authoring.rs:1698-1704`). `assembly_changes` remains only a diagnostic renderer, not a second enforcement authority. No public AssemblyChange extension or exhaustive-match breakage is smuggled into this repair.

### Projection Atomicity

Apart from C1, the extraction preserves the intended preflight/commit separation.
`operation.rs:999-1014` checks existing export port, slot, identity, declaration
token presence and duplicate target before mutation. The indexed token lookup at
`1062` is therefore guarded for existing exports; new exports install their token
at `1048-1051`. No remaining existing-export token-index panic was found along
these paths. There is no user callback between preflight and commit that can
remove the checked token. Private missing-token/metadata cases are covered at
`1302-1325`; publicly introduced local/encoded incompatible exports are covered
in `tests/matching_projection.rs:153-227`.

Bulk exposure plans every leaf, then checks aggregate capacity before any append
or token allocation (`operation.rs:1098-1129`). Manual exposure also checks
capacity before commit. The inclusive ID 65535 boundary is preserved. Late-leaf
failure tests include both a planned addition and planned fanout, with duplicate
and incompatible final leaves (`tests/matching_projection.rs:276-330`); capacity
overflow has a private synthetic fixture (`operation.rs:1258-1298`). These checks
are useful atomicity evidence, not evidence that the missing public-book validity
check in C1 is performed.

### Acceptance And Basis

**Decision: REQUEST CHANGES for Batch C as a whole because of C1.** The observer
and directional-enforcement implementation have no independently identified
correctness blocker and are acceptable on this static review, subject to normal
integration gates. Projection acceptance is withheld until both selected-side
scope/revision cases are rejected atomically. C2 and C3 are nonblocking residual
coverage/performance risks, not claims that a failing exploit was executed.

Reviewed the actual current working-tree files on 2026-11-10 against the revised
patching design and critique, not only the implementation record. Source basis
remains HEAD `d758291c1db77abcff60ac3ac0e167c0f59261bd` plus the uncommitted observer,
interfaces, contribution, patching and projection changes. References are current
working-tree line numbers at inspection. Read observer/tests.rs, matching_projection
and surrounding substitution/interface/registration code to check ownership and
reachability. The counterexample above is a static control-flow reproduction,
not an independently executed new test. Ran `git diff --check`: passed. No source
or test edits were made; only this review document was appended. The full suite
was not duplicated.

The parent-owned stale `operation_substitution` assertion expecting Instance
instead of Graph is a known planned integration correction, not a newly discovered
blocker and not evidence of a production regression. The implementation record's
647-pass run explicitly skips that assertion; this review does not relabel it as
an unfiltered exact-final run. After C1 and the parent correction, require focused
projection regressions, the relevant final-revision checks and project-required
CI. Named maintainer/host acceptance of conservative Graph fallback, partial
revision diagnostics and the candidate-only rollback boundary remains separate
from independent implementation review.

## Final Independent Recheck

Reviewed the current shared working tree on 2026-11-10 after the C1 fix and R1/C2
test additions. This section supersedes the earlier Batch C request-changes
decision and the earlier open R1/C2 status; those findings and their original
line references are retained as review history. No source or test files were
edited by this recheck.

### Finding Resolutions

- **C1 closed.** `packages/phenix-graphs/src/authoring/operation.rs:1081-1085` now rejects a retained public book with either a foreign scope or divergent definition token, before frontier construction, leaf planning, capacity checks or commit. The guard matches manual exposure's selected-side check and does not weaken destination checks or existing-export token preflight. `tests/matching_projection.rs:277-305` exercises both cloning and whole-builder replacement, seeds an existing export in the current builder, rejects bulk fanout and manual exposure, compares exports before/after, uses the original export token in a trigger and asserts the next public input ID is 1. These cases reach the old failure path with a valid current child; they do not merely fail on a stale child. The static counterexample in C1 can no longer commit.
- **R1 closed.** `packages/phenix-graphs/src/tests.rs:9016-9020,9090-9103,9296-9324` retains and compares ordered `(CohortId, payload)` deliveries per pending address. The added equal-payload `skip` inputs in cohorts 40 and 41 are not a vacuous fixture: a final assertion requires adjacent equal pending payloads with those distinct cohort IDs. The independent activity checks and existing report comparisons remain intact.
- **C2 closed.** `packages/phenix-graphs/src/authoring/observer/tests.rs:693-806` adds six cases in each cross-kind direction: one valid two-target fanout control and five negative residual cases (extra target, target reversal, input schema text, input token and output schema text). Thus there are 12 matrix cases, including two controls, not 12 negative residual cases or 12 new test functions. Each negative case asserts the exact additional PublicInput/PublicOutput owner and rejection of the equivalent narrowly declared custom callback, with no committed revision. Controls retain operation-only precision and compile successfully. The low-level residual mutations are private fixtures, not newly exposed public mutation APIs.
- **Known assertion migration completed.** `packages/phenix-graphs/tests/operation_substitution.rs:640-648` now expects `UndeclaredWrite` targeting Graph and retains the rollback/revision/node-count assertions. The negative test has not been disabled or weakened to accept the operation-only write.
- **C3 remains nonblocking.** Full observation is still a correctness expansion with temporary allocations, per-alias traversal and some quadratic authoring scans. No benchmark or runtime simplification acceptance is implied by resolving C1/R1/C2.

### Report Findings

#### D1: Final-Validation Claim Is Premature Or Overbroad (Medium, Reporting)

`docs/graph-consolidation-report.md:40-43` says, "No test is disabled or skipped
in final validation." The report supplies no completed final command/SHA/result
for that assertion and describes exact-final validation as a future gate at
`116-122`. The previously reported package run also retains three ignored tests;
"no test ... skipped" needs to distinguish no newly disabled tests from the
existing ignored cases and the former command-line filter. The corrected source
assertion establishes that the known negative test remains enabled, not that an
unfiltered final-revision run has completed.

Use prospective wording until delivery evidence exists: the final run must omit
the temporary filter, with no newly disabled tests, and must report the existing
ignored tests explicitly. Once executed, cite the actual command, revision and
pass/ignore/filter counts. This is a reporting correction, not a new source
correctness blocker or a request to duplicate the full suite in this review.

#### D2: Linked Measurements Predate The Reviewed Fixes (Low, Reporting)

`docs/graph-consolidation-report.md:17-19` points to measurement tables without
identifying them as pre-resolution measurements. Those tables still describe the
current working files but count `operation.rs` as 1326 lines rather than the
inspected 1331 (`graph-consolidation-measurements.md:50`), 18 observer test
functions rather than the now-added nineteenth (`166`), and nine matching
projection tests rather than ten (`169`). Their production/test totals therefore
are not the current final inventory. More importantly, measurement lines 221-225
still say the substitution assertion currently expects Instance and is awaiting
correction, contradicting the actual source and report lines 40-41.

Refresh the snapshot-specific measurements and stale validation paragraph, or
explicitly label the linked inventory as historical/pre-C1/R1/C2 and state that
final counts differ. Do not silently extrapolate the old production exclusion
line ranges after inserting the new five-line guard. This does not invalidate
the bounded claims that total code grew, four redundant collections disappeared,
or no comparative performance benchmark was run.

No other misleading implementation-acceptance claim was identified in the
report. In particular, it separates intermediate local checks from exact-head
remote verification, acknowledges observer complexity and callback trust, and
does not claim a runtime speedup or overall LOC reduction. "Exhaustive stored-field
accounting" at report line 87 should continue to be read with the documented
GraphContract/InterfaceShape privacy-boundary caveat, not as a compiler-enforced
guarantee for every future private shape field.

### Final Decision

**ACCEPT the current Batch C observer, directional enforcement and projection
implementation on independent implementation review.** C1 is resolved, and no
remaining source correctness blocker was identified in this focused recheck.
A/B acceptance stands, with R1 now resolved; C2 is also resolved. D1/D2 are report
accuracy issues to correct before treating the report as final delivery evidence.
C3 remains an explicitly bounded, nonblocking authoring-performance risk.

The delivery owner's reported 28 targeted passing tests and 220 passing library
tests are attributed evidence, not tests independently rerun by this reviewer.
Inspected the actual guard, regression assertions, cohort capture/comparison,
cross-kind matrix and corrected substitution assertion rather than relying on
the appended resolution statements. `git diff --check` passed during this
recheck. No full suite was rerun. Exact-final-commit formatting/tests, project CI
and named maintainer/host acceptance remain delivery gates; this decision is not
remote CI approval or acceptance of broader callback isolation.
