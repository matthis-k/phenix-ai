# Graph Consolidation: Patching Implementation

## Scope And Decision

Batch C follows `graph-consolidation-patching-design.md`, including its Phase 2
correction, and the critique's Revised Batch C acceptance. No callback capability
wrapper, mutation journal, public equality change, or blanket Graph requirement
was introduced. No commit was made.

The accepted API-neutral alternative is used: private concrete requirements are
the enforcement authority. `AssemblyRevision::changes` retains the existing,
explicitly partial public diagnostic vocabulary and event ordering. The public
`AssemblyChange` enum was not expanded. An unrelated root request now reports
`UndeclaredWrite { target: Graph }`, not the old `Instance("")` diagnostic target.

Projection implementation and public API tests are owned by other concurrent
work; this document does not claim authorship or independent approval of them.

## Implementation

- `GraphAssembly::contribute` detaches pending contents before the callback,
  observes the candidate once, and returns requirements separately from events.
- `observer.rs` compares raw and pending nodes, opaque factory/policy/argument
  allocations, stored metadata, declarations, dependencies, provenance, routes,
  tokens, ordering, indexes, allocation cursors, and compile limits.
- Dependency payload, identity, and origin changes charge every owning direct
  alias from the union of cycle-safe before/after closures. Unowned historical
  imports and inconsistent bookkeeping require Graph.
- Exact cross-kind substitutions account only for matching endpoint cells.
  Other export metadata, target multiplicity/order, and input tokens remain
  independently owned. Request lifts require an unchanged prefix and every
  ordered per-transition block exactly once, including duplicate requests.
- `stored_contract_eq` compares contract ID, shape keys/unnamed identities,
  and the stored named input/output values, including local schema text. The
  same comparison is used for claims, requests, and exact request-lift matching.
- `PatchTarget::covers` is directional and closed over concrete observations.
  `overlaps` and planner resolution remain unchanged. Incident/pattern claims
  parse length-prefixed exact symbolic keys; neither covers numeric keys or
  Graph. Splice claims do not authorize ordinary callback topology changes.
- The original outer draft and final assignment remain the plan commit boundary.
  Underdeclaration rejects the selected plan without promoting claims or replaying
  callbacks. `apply` still does not compile.

The superseded partial `claim_changes` scan, event-to-authority mapping, and
planner-wide boolean request-lift bypass were removed. `assembly_changes` remains
only as the legacy diagnostic renderer, never as an enforcement input.

## Phase 4 Review

This is an implementation self-review, not the independent acceptance review or
remote CI required by the design. Field accounting was checked against the design:

| Storage | Accounting |
|---|---|
| Builder origin, root scope/name, limits | Graph for differences |
| Root definition token and pending wrapper allocation | Clone-only normalization |
| Raw/pending nodes and pending-entry presence | Node owner; malformed/orphan entries additionally Graph |
| Instances and declaration tokens | Instance owner; inconsistent indexes additionally Graph |
| Dependencies and identities | Every before/after alias; missing, mismatched, or orphan payload additionally Graph |
| Composition origins | Checked declaration-token ownership across root instances and dependency aliases; orphan Graph |
| Public inputs and token map | Old/new public input names, except exact cross-kind endpoint cells |
| Public output | Old/new public output names, except exact cross-kind source cell |
| Triggers and name-to-ID map | Old/new trigger names; malformed association additionally Graph |
| Root claims and requests | Graph, except exact appended cross-kind lift blocks |
| Node indexes and cursor | Validate indexes and replay new raw registrations; residual Graph |
| Edge records and cursor | Old/new exact keys, sequential minted IDs, retained IDs only for unchanged/checked retargeted records |
| Survivor order | Graph for node/instance/input/trigger/edge survivor reorder |

Review found and fixed one additional residual-accounting hole: newly minted edge
records could precede retained records while still passing the sequential cursor
check. The observer now requires new records to be appended after retained ones.
The private-interface compiler warning from returning an authoring-private
requirement through a crate-visible method was removed by making the internal
contribution method private to its owning module.

### Deviations And Boundaries

The accepted private-requirements/partial-events alternative replaces the original
proposal's public enum extension. This is an explicitly accepted alternative, not
an architectural redesign. No other architecture change was needed.

Builder, declaration, dependency, and leaf structs use exhaustive destructuring.
`GraphContract`/`InterfaceShape` have private fields in the separately owned
`interface.rs`; their stored comparison uses the complete current read-only
accessors plus `same_shape` for map keys and unnamed identities. This covers the
current inventory without changing public semantic equality, but adding a private
shape field will require reviewing this comparator: its field inventory is not
compiler-enforced across that privacy boundary. No ownership/privacy redesign was
attempted.

Rollback concerns candidate-owned state only. Captured original-store fluent
handles, shared interior state, I/O, and global token-counter history are not
restored. Tests cover both candidate discard and an explicit captured-handle
counterexample. Successful contribution clones retain the existing handle-store
detachment behavior; this change does not promise successful-commit handle
continuity.

## Verification And Metrics

Commands use `nix develop --command` and `--locked` for Cargo tests.

- Authoring unit tests: 27 passed, including 18 observer tests, one directional
  coverage test, six additions tests, and two concurrently owned projection tests.
- The targeted patch/projection/layout run passed 69 tests across
  `patch_planner`, `patch_lineage`, `patch_syntax`, `patch_output_routing`,
  `symbolic_pending`, `typed_operations`, `matching_projection`, and
  `module_projection_semantics`.
- `canonical_interfaces`: 27 passed. `contract_dependency_identity`: seven
  passed. Initial `operation_substitution`: 17 passed, one failed solely because
  its expected root-request target remains `Instance("")` instead of Graph.
- Full package run with `--no-fail-fast`: 644 passed, one failed, three ignored,
  including 216 library tests and all 24 documentation tests passing. This run
  preceded the final three additional observer matrix tests, which passed in the
  27-test authoring run above.
- `cargo fmt --all -- --check` and `git diff --check` passed before the final
  three test additions; final verification is recorded below.

The stale assertion is in
`packages/phenix-graphs/tests/operation_substitution.rs:640`, test
`operation_write_does_not_authorize_unrelated_root_requests`. It is outside this
worker's source/test ownership and was deliberately not modified. The new private
tests assert the accepted Graph diagnostic directly. Updating that assertion and
rerunning the unfiltered package suite remains an integration gate.

This is a correctness expansion, not a measured performance optimization. The
observer adds one detached pre-callback pending-content copy alongside the
existing candidate clones, plus temporary ordered owner/key/requirement maps and
sets. No factory, policy, codec, layout evaluator, or compiler is executed by
observation. No allocation-count, compile-size, or authoring-throughput improvement
is claimed; no comparative benchmark was run. Runtime scheduling storage and hot
paths are not changed by this batch.

### Final Verification

After the final test additions and formatting:

```text
nix develop --command cargo test -p phenix-graph --locked --no-fail-fast -- --skip operation_write_does_not_authorize_unrelated_root_requests
647 passed, 0 failed, 3 ignored, 1 explicitly filtered out
```

This includes 219 library tests, 17 remaining substitution tests, and 24
documentation tests. The skipped assertion is the known integration gate above,
not an uninvestigated runtime failure. Final `cargo fmt --all -- --check` and
`git diff --check` both passed. No remote CI, independent implementation review,
or comparative performance benchmark was run.
