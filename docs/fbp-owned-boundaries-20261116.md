# Q1 Canonical Ownership Boundaries

Status: Q1's original three-root responsibility scope is scoped verified at
`7c61191222a19cefccde9c38adedbc9ad290bf01`, after independent structural review,
full local gates and exact-head hosted acceptance.
Reference behavior is verified `89171a843`.

## Scope And Constraints

The original Q1 finding names `network.rs`, `runtime/mod.rs` and `compose/mod.rs`.
It asks for cohesive tested seams, not a line-count target, public API redesign or
another mutable state owner. Recent authority/equivalence, independent composition
models, selection-budget, lifecycle/lease and passive explanation regressions provide
the behavior oracles for this extraction.

Public module-qualified paths remain direct reexports of the same definitions.
There are no compatibility wrapper objects, replacement handles, parallel caches,
registry managers, scheduler services or copied state. Canonical fields and their
declaration/drop order remain unchanged. Private child modules borrow the existing
owners through inherent implementations; individually necessary helper visibility
is parent-scoped, not a blanket widening to public or crate-wide access.

## Network

- The root retains canonical frozen/compiled models, errors, lightweight accessors,
  lease propagation and public exports.
- `authoring.rs` keeps builder mutation, authoring capabilities, scope/revision and
  transaction lineage, endpoint refs, additions/replacement and builder inspection/
  binding together. Builder binding is not moved into immutable resolution because
  it participates in capability lineage.
- `validation.rs` contains borrowed name/type/topology/claim validation, declaration
  traversal and obligation normalization, without a retained registry or alternate
  validation policy.
- `compiler.rs` owns the ephemeral compiler/boundary expansion and its private tests.
  Charging still precedes declaration validation; physical order, capacities,
  projections, origins and frozen obligations are unchanged.
- `resolution.rs` reads the canonical compiled model for inspection, definition and
  contract lookup, bound endpoints and compiled-allocation authentication.

## Composition

The root `Composer` remains the sole committed registration/publication owner.
`edit.rs` owns immutable payloads and context-free writes; `planning.rs` produces
an ephemeral dependency-ordered selected plan, validating all registrations before
selection, including defeated ones. Global priority/splice conflicts and resolved
grants retain their existing precedence.

The existing exact `selection.rs` is byte-identical. `replay.rs` applies only the
selected dependency-ordered edits, with attempt-local splice tails, nested copy-on-
write replacement, grant rechecks and obligation checkpoints. Root orchestration
still contains the same panic boundary, normalization and canonical compilation.
Registry and current composition change only after the entire attempt succeeds.

## Runtime

`NetworkSession` remains the single owner of transport, processes, requests,
scheduling fields and leases. Its root retains model/options fields, startup and
the unchanged Drop implementation, including the final code-lease field.

- `admission.rs`: lane materialization, injection, input closure and capture reads.
- `scheduling.rs`: existing dirty propagation, watches, idle filter and refresh/full
  scan fallback. This is the existing scheduler, not an additional scheduler.
- `execution.rs`: job transfer/return, dispatch, supervision, routing, retained
  publication and bounded driving. Wall-clock supervision stays here.
- `observation.rs`: passive state, counters, origins, output closure, queue waits
  and caller-time overdue inspection.
- Existing `recovery.rs`: request recovery plus moved session finish/fail/stop/cancel
  machinery, retaining detach, wake-retirement, panic and lease teardown order.

Readiness, closure proofs, request behavior, diagnostics/explanations and `wake.rs`
remain byte-identical to the reference. No callback, cursor, deadline, cleanup or
handoff transition was moved relative to other operations inside its method.

## Alternatives And Review

Moving whole monoliths unchanged into differently named files would not establish
ownership boundaries. New controller objects or grouped state would risk duplicate
authority and destruction-order changes. Public child modules or wholesale field
widening would expose unnecessary implementation access. None is introduced.

Independent preimplementation critique required builder authority to remain together,
plan-versus-commit separation, exact validation/panic extent and recursive child CI
coverage. Independent final structural review inspected all three roots and children
and found no actionable defect. Mechanical comparisons additionally checked moved
method bodies and canonical fields; they are supporting evidence, not substitutes
for behavior tests.

The network cut is recorded in `ca684fbbb`; the remaining cuts are assembled with
it before Q1 acceptance. The runtime comparison covers all 54 original methods and
fourteen individually parent-visible helpers. Compose replay/planning blocks and
selector are mechanically preserved. No existing test oracle was changed to make
the extraction pass.

## Gates And Nonclaims

The [CI path audit](fbp-q1-ci-paths-20261116.md) preserves every former parent-file
match and covers child-only runtime changes in the same specialized workflows.
No new workflow, benchmark, external CI pin or Loom source include is introduced.

Final gates include full workspace/default/no-default and isolated graph no-default
tests, doctests, examples/downstream compilation, strict Clippy, formatting, actual
source Loom and native lifecycle checks on the assembled committed head. Baseline
acceptance is not relabeled as extraction acceptance.

This can close the three-file original Q1 scope, not every responsibility
concentration in the repository. Kernel `lib.rs` still has broader authoring,
resolution and producer/driver/lifecycle concentrations; that residual is explicit.
No Q2 incremental compilation, A2 compatibility, runtime speedup or universal
deadlock-freedom claim follows from private module decomposition.

## Exact-Head Acceptance

The assembled implementation passed all fifteen hosted workflows, including:

- [Full six-job Flake matrix 43811659284](https://github.com/matthis-k/phenix-ai/actions/runs/43811659284).
- [Actual-source wake Loom 43811658724](https://github.com/matthis-k/phenix-ai/actions/runs/43811658724).
- [Session ownership Loom 43811658772](https://github.com/matthis-k/phenix-ai/actions/runs/43811658772).
- [Eight-repeat native notifier/plugin-lock gate 43811961753](https://github.com/matthis-k/phenix-ai/actions/runs/43811961753).
- [Twelve-repeat in-flight worker recovery 43811961702](https://github.com/matthis-k/phenix-ai/actions/runs/43811961702).

Q1 moves 22.0% to 100.0%, +78.0 pp. Counts are eighteen scoped verified, six
partial, eight open and one approved exclusion; fourteen in-scope findings remain.
This closes the original three graph concentrations, not the broader Kernel root.
The evidence-only follow-up needs its own head's checks; no runtime performance,
incremental compilation or repository-wide modularity perfection is inferred.
