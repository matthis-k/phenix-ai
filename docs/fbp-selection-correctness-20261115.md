# Dependency And Priority Selection Correctness

Status: implemented, independently reviewed and exact-head verified at
`006b2e9ccda6b06739571e5bef3b6d94a74fb220`. Full locked workspace/default/no-default,
formatting and strict Clippy checks also pass locally.
Based on verified `bd397a757`.

## Defect And Contract

The former monotone `deactivated` set permanently removed a dependent when its
prerequisite was absent from an intermediate greedy selection. Removing another
ineligible winner could restore that prerequisite without reconsidering its
dependent. The isolated Q3 reproduction expected `{P,R,p}` but published `{R,p}`.

The existing independently authored oracle already specifies a biconditional:
registration r is active exactly when all its direct dependencies are active and
no strictly higher-priority, independent overlapping registration is active.
Independence still uses transitive ancestry; overlap retains hierarchical component
paths and resolved splice writes. Direct prerequisites suffice for the equations
because every selected prerequisite must satisfy its own equation too.

Dependency cycles/missing keys, payload/origin and exact-grant validation, and
global equal-priority overlap/independent same-anchor splice rejection remain before
selection. Actual anchor/seam/replacement validity remains in replay. Selection
ambiguity rejects even if only one hypothetical topology would compile; compilation
must not become a selection policy. Replay remains dependency-ordered and candidate
registry/topology publication remains atomic.

## Alternatives And Critique

- Permanent exclusion is the demonstrated bug. Repeatedly clearing exclusions has
  no general termination or uniqueness argument.
- Rejecting all unresolved cycles rejects an existing correctly selected unique
  cyclic fixture. Dependency DAGs alone do not prevent signed influence feedback.
- Unbounded production subset enumeration is not an acceptable scaling repair.
- SCC-local uniqueness is insufficient: a downstream inconsistent component can
  eliminate one locally consistent upstream choice, making the complete model unique.
- Selected: one private exact selector with forward propagation and globally bounded
  iterative search. No SCC subsystem, recursive search, SAT dependency, second
  composer, clause database or public test inspector is introduced.

Independent preimplementation critique required full outgoing counter updates before
logical rollback, checking assigned left-hand sides against known right-hand sides,
accounting for search bookkeeping, and preserving validation precedence. Final
production and assembled public-limit/Kernel integration reviews found no actionable
defects. Public scaling tests are an additional integrating-owner acceptance slice.

## Solver And Work Allowance

Dense equations contain unknown/false signed-literal counters and outgoing
incidences. Empty conjunctions seed a deterministic queue. Assignments update every
outgoing counter before queued equations can report contradiction. Acyclic influence
graphs resolve without decisions. Unknown feedback branches false then true in
deterministic order, using explicit frames and a reversible assignment trail.
Rollback discards the abandoned queue and restores all counters to a quiescent
checkpoint. There is no recursive call-stack growth or per-branch graph clone.

A complete assignment counts only after propagation drains. Keep the first model
while searching all remaining alternatives; a second complete model establishes
ambiguity. Exhausting all branches proves either no model or a unique model.
Finding one model is not sufficient if the remaining proof exceeds the allowance.

Public `SelectionLimits { max_work: usize }` defaults to 1,000,000. The same checked
counter covers the entire attempt, including all feedback components and work after
the first model. Each initialized equation, ID-index entry, dependency lookup,
constructed edge, seed inspection, queue push/pop/discard, assignment, traversed
forward/reverse edge, undone trail entry, inspected search slot, search-frame
creation/visit and captured model entry costs one unit. A conclusive result at the
exact limit needs no unnecessary private-state cleanup. Empty selection costs zero;
one dependency-free public registration costs ten units.

`NoConsistentSelection`, `AmbiguousSelection`, and `SelectionBudgetExceeded { limit }`
are distinct typed errors. Exhaustion is not a partial selection or evidence of
ambiguity. The finite default can reject a uniquely solvable feedback problem whose
uniqueness proof requires more work; the trusted host can increase its allowance.
The setter changes future attempts only, without recomposing current bindings.

The allowance excludes registry cloning, ancestor planning, footprint/conflict
validation and blocker collection, selected-ID materialization, replay, compilation
and allocation failure. Index construction uses ordered maps; a charged map lookup
is not a constant-time execution guarantee. Propagation is linear in its influence
graph; arbitrary feedback can require exponential search up to the allowance.
Live solver storage is O(V+E), but dense blocker lists and existing ancestry can be
quadratic. Neither linear whole composition nor a total memory/time bound is claimed.

Kernel `BuildOptions::selection_limits` configures both the main composer and
self-import preview immediately after construction, before registration. Each
incremental prefix must independently resolve; a later contribution cannot rescue
an already rejected prefix. Per-update/per-preview allowance is not a total Kernel
build budget and does not preempt preparation callbacks.

## Acceptance Evidence And Limits

- The independent composition oracle now distinguishes conflict, zero, one and
  multiple models, retaining exhaustive enumeration only for at most twelve inputs.
- Public model fixtures cover the original recovery, existing unique cyclic case,
  toggle ambiguity, inconsistent odd ring and locally ambiguous/globally unique
  feedback. Batch reversal, ID-only renaming, revocation and old binding/topology/
  footprint/origin preservation are checked, plus 256 generated DAG/conflict cases.
- Private selector tests exhaustively compare all 19,683 three-variable signed
  systems with independent truth tables, exercise rollback after queued contradiction,
  mixed literals/multiple blockers, exact limit results, exhaustion during uniqueness
  proof, and a 20,000-variable chain without recursion.
- Public limit tests find minimum conclusive allowances rather than asserting wall
  clock thresholds. Independent registrations and repeated Q3 motifs show linear
  selector work; chain/diamond and dense-blocker families fit edge-based allowances.
  Multiple unique feedback components share one allowance. Failed updates/revokes
  and limit changes preserve registry/bindings; increasing the limit permits retry.
- Real Kernel fixtures check main-path typed errors and artifact cleanup without
  activation, and zero-budget self-import preview rejection before preparation.

These establish the selection-correctness and bounded-search slice. They are not
whole Q3 scaling acceptance: existing dependency/ancestor/footprint planning and
end-to-end recomposition costs remain outside the allowance and require separate
measurement/acceptance. Do not manufacture a whole-finding closure or percentage
increase from this correction. Existing runtime, request, lifecycle and lease
contracts are unchanged and still require assembled exact-head checks.

## Exact-Head Acceptance

Implementation `006b2e9ccda6b06739571e5bef3b6d94a74fb220` passed all fifteen
hosted workflows, including:

- [Full six-job Flake matrix 43770741835](https://github.com/matthis-k/phenix-ai/actions/runs/43770741835).
- [Actual-source wake Loom 43770741345](https://github.com/matthis-k/phenix-ai/actions/runs/43770741345).
- [Session ownership Loom 43770741355](https://github.com/matthis-k/phenix-ai/actions/runs/43770741355).
- [Eight-repeat native notifier/plugin-lock gate 43770749070](https://github.com/matthis-k/phenix-ai/actions/runs/43770749070).
- [Twelve-repeat in-flight worker recovery 43770749243](https://github.com/matthis-k/phenix-ai/actions/runs/43770749243).

Independent preimplementation, production and assembled public-limit/Kernel reviews
found no unresolved actionable defects. The budget-failure public regression also
attempts removal of a present registration, preserving that registry and binding
before a larger allowance permits the retry.

The demonstrated selection defect is resolved. Q3 remains open with its existing
score because complete planning/recomposition scaling acceptance is not delivered.
Counts remain twelve scoped verified / seven partial / fourteen open. Benchmark
workflow success is not blanket performance acceptance. This evidence-only
follow-up requires its own committed-head checks, separate from the source above.
