# Graph Consolidation Report

## Outcome

Stack baseline: PR #1261 at `d758291c1db77abcff60ac3ac0e167c0f59261bd`,
fetched from GitHub rather than assumed. Its CI failed rustfmt; other shards
passed. This branch fixes that formatting as part of the affected projection code.
Final commit and remote verification evidence belong in the draft PR; intermediate
working-tree results below are not exact-head evidence.

The public authoring vocabulary stays unchanged. Six private action executors
become one generic composition function using the existing closure NodeAction
implementation. Four redundant stored collections disappear. Manual and matching
projections share preflight, capacity and commit. Patch conflict accounting stops
using partial diagnostic events as write authority.

This is not an overall LOC reduction. Complete patch observation costs more code
than the action/state consolidation removes. See the separately measured tables
in [graph-consolidation-measurements.md](graph-consolidation-measurements.md).
The correctness expansion closes reachable underdeclared-write failures rather
than adding another facade or graph engine.

## Protocol And Iterations

1. Phase 0/1: `graph-consolidation-design.md` records problems, current footprint,
   ownership, examples, removal scope, rejected alternatives and test obligations.
2. Independent Phase 2: `graph-consolidation-critique.md` conditionally accepted
   A/B and rejected the original patch proposal. Directional coverage, full field
   accounting and stored schema-text equality were added before implementation.
   The revised patch design is `graph-consolidation-patching-design.md`.
3. Phase 3: action, state, projection and patch implementation records describe
   actual changes, not proposals. Tests cover Send-only prepared arguments,
   factories, policy/cohort behavior, projection atomicity and mutation accounting.
4. Independent Phase 4: `graph-consolidation-review.md` accepted action/state,
   found missing public-book validation in bulk projection, and requested cohort
   and cross-kind residual tests. The bulk gate was added before planning; cloned
   and foreign builder regressions assert unchanged exports and usable old tokens.
   Buffered cohorts and cross-kind export residuals now have direct tests.

The formerly expected `Instance("")` error for unrelated root requests is migrated
to `Graph`, the accepted directional write target. The negative assertion remains:
an operation-only claim still rejects the request. No new test is disabled or
ignored. Final validation must use unfiltered commands; the existing manual-timing
and Store-Graph example ignores remain and must be reported. An intermediate
worker's filtered run is historical only.

## Workstream Decisions

| Workstream | Implemented Or Retained | Evidence And Stopping Reason |
|---|---|---|
| A: actions/output | Six structs deleted; common prepare/invoke/lower sequencing | Option/Result remain data; raw/external vectors remain untrusted. Whole-completion validation precedes routing. No new public return trait is needed. |
| B: declaration ownership | Retained fluent pending ownership after comparing alternatives | Pre-registration `Node::with_layout` is simpler for one-shot configuration, and supports move-only fields. Removing registered retargeting would delete existing behavior. `L` cannot simply be `D::Layout`: a retargeted handle has a new layout while retaining its original handler definition. |
| C: readiness | Retained select/retire/incomplete | Selection, alternative disposal and reservation-only terminal obligations are independent. Wait/Invoke/Fail alone loses staged semantics. No scheduler responsibility moves into policy. |
| D: wave state | Deleted three eager activity summaries and duplicate outgoing adjacency | Independent test oracle reconstructs buffers and ready/dispatched work. Producer/ticket separation and targeted pending indexes are necessary; see per-field state audit. |
| E: composition | Shared projection checks; added selected-side stale-book rejection | Existing Operation already represents typed common boundaries. GraphUnit represents authoritative inspection, including roots without output. A Unit trait would not remove an owner. Nested/sequential/parallel projections still flatten with no wrapper/barrier. |
| F: construction/patching | Complete candidate-owned observation plus directional coverage | Contributor construction remains unrestricted Rust; kernel admission remains separate. Precise node/instance/connection/route edits remain precise. Whole-builder/configuration residuals require an honest Graph conflict claim. |
| G: types/erasure | Generic composition before NodeAction erasure; no new public representation | Local identity and canonical encoded identity are distinct relations, not duplicate booleans to collapse. PortInterface/NodeInputs describe cardinality and admission, not a second runtime. |
| H: completeness | Corrected provenance/metadata/alias write observation and strengthened invariant tests | Existing manifests are deterministic declaration/topology descriptions, not identities of captured behavior. Portable boundaries, replacement and diagnostics keep existing authorities. Behavioral hashing or a kernel migration would be speculative. |

## API Evaluation

All ten required before/after cases are compiled/executed in
`tests/consolidation_api.rs`, with matched excerpts in
[graph-consolidation-api.md](graph-consolidation-api.md). Before equals after:
this cleanup does not invent new syntax to claim ergonomic progress. Cases cover
ordinary functions, explicit sources, structured inputs, independent encoded Rust
types, custom readiness, selected destinations, nested F(x,y)=A(x,B(y)), cross-kind
replacement, multiple contributors and repeated feedback activation.

Existing module/conformance suites additionally cover diamond fanout/fanin,
multi-producer joins, nested/repeated instances and independent child readiness.
The new checks complement rather than replace local/encoded mismatch, stale and
foreign token, one-output, cancellation/external completion, malformed selection,
cohort isolation and deterministic cyclic reachability regressions.

## Quality Assessment

These are evidence-backed engineering judgments, not objective benchmark scores
or a claim of 10/10. Baseline judgments refer to this audit's source reading.

| Dimension | Baseline | After | Reason And Remaining Cost |
|---|---:|---:|---|
| Public API ergonomics | 8/10 | 8/10 | Common syntax intentionally unchanged; dynamic frontier checks and raw compiler surface remain. |
| Conceptual minimality | 7/10 | 8/10 | Fewer execution implementations and stored facts; construction staging still substantial. |
| Rust type usage | 8/10 | 8.5/10 | Generic composition preserves Send-only/move-only arguments before existing erasure; layout/mode markers remain necessary. |
| Implementation simplicity | 6.5/10 | 7/10 | Execution/state get smaller; complete observer is a real authoring complexity cost. |
| Extensibility | 8.5/10 | 8.5/10 | Low-level actions, external executors, arbitrary policies and custom layouts stay available. |
| Runtime correctness | 8/10 | 8.5/10 | Less redundant state, independent consistency/differential tests; not a formal proof. |
| Maintainability | 7/10 | 8/10 | Shared lowering/projection transitions, exhaustive stored-field accounting; coupled substitution exceptions require care. |
| Kernel/plugin boundary | 9/10 | 9/10 | No second compatibility authority or execution policy added. |

## Remaining Boundaries

No new expressiveness limitation is accepted on a human's behalf. These retained
or corrected boundaries preserve the stated requirements and primary use cases:

| Affected Cases | Rationale | Consequence | Authority |
|---|---|---|---|
| Custom low-level/external completions | Runtime collections are heterogeneous and input is untrusted | Vec remains transport; Graph checks zero/one, slot and identity before publication | Existing Graph completion contract, preserved by this implementation |
| Fluent registered layout edits | Mutable construction and stale-token revocation are useful behavior | Pending Rc/Weak ownership, frozen books and revision checks remain | Existing authoring contract; no removal authorized |
| Custom staged/cohort-aware policies and cycles | Live producers, terminal history and future obligations differ | Reservations/colors/tickets/indexes cannot collapse to a single readiness bit | Existing runtime contract; no mandatory DAG or new budget introduced |
| Captured Rust side effects or original-store handles in callbacks | Candidate cloning cannot isolate arbitrary external memory or I/O | Rollback covers candidate-owned declarations, not captured external effects; callbacks are not a sandbox | Existing trusted Rust callback boundary, documented rather than expanded |
| Whole-builder/configuration/orphan metadata edits | Precise ownership cannot honestly describe a global residual | Graph conflict footprint may serialize these edits; precise normal edits still work | Required accurate conflict reporting, not kernel permission policy |
| Large/shared authoring graphs | Complete observation scans owned declarations and aliases | Additional temporary allocations and some quadratic scans; no authoring speedup claimed | Correctness repair within existing representation; performance remains measurable follow-up |
| Closure behavior and dishonest codecs | Equal structural declarations cannot certify semantics | Manifests/schema equality do not imply behavioral equivalence or codec honesty | Existing Graph/schema boundary; no new behavioral guarantee claimed |

Further worthwhile runtime deletion would require changing distinctions currently
used by custom readiness or ownership. Removing fluent staging without equivalent
mutation support, fusing inspection with addressing, introducing Unit, behavioral
hashing or caching the observer would be speculative or reduce guarantees. Small
independent cosmetic changes (for example Deployment's always-present optional
retention setting) are not architectural work and do not justify another batch.
No controlled before/after performance benchmark was run; inline storage evidence
and incremental command timings are explicitly diagnostic only.

## Delivery Evidence

The first committed-head Nix run caught the API excerpt test importing a root
document excluded by package-scoped source filtering. The canonical examples now
live in `packages/phenix-graphs/API-EXAMPLES.md`, with a root documentation link.
The drift test remains enabled and reads the packaged document. This is a build
input correction, not an architectural or acceptance change. All exact-head gates
must be rerun after the correction; the earlier Nix failures are not passes.

The final commit must be tested without further source changes using all requested
commands through `nix develop --command` where needed. New files must be tracked
before Nix evaluation. The draft PR is based on
`impl/operation-projection-followup-20261110`, not the default branch. Its body and
follow-up verification comment record the exact SHA, local outcomes, example
commands and exact-head GitHub check URLs. Baseline CI and intermediate filtered
runs do not satisfy that gate.
