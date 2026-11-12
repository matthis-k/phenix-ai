# Graph Consolidation Actions

## Scope

Batch A only, implementing the Phase 2 revisions in
`graph-consolidation-design.md` and Batch A gates in
`graph-consolidation-critique.md`. No commits were created.

Owned changes:

- `packages/phenix-graphs/src/lib.rs`: private action block and NodeTemplate constructors.
- `packages/phenix-graphs/src/authoring.rs`: only the two source templates near the top.
- `packages/phenix-graphs/tests/action_lowering.rs`: new feature-independent regression probes.
- `docs/graph-consolidation-actions.md`: this implementation and review record.

Concurrent Batch B changes in lib.rs adjacency, scheduler.rs and tests.rs were
observed and left untouched. The design and critique documents were not edited.
The full test results below describe the shared working tree, not an isolated
Batch A checkout or a committed final revision.

## Implementation Ledger

Deleted all six private executor structs and their NodeAction implementations:
TypedNodeAction, FunctionNodeAction, LocalFunctionNodeAction,
LocalPreparedFunctionNodeAction, ContextFunctionNodeAction and MultiPortNodeAction.
Replaced them with one private generic function, `compose_action`.

Its preparation closure takes `&mut InvocationCtx` and returns `Result<A, String>`;
its invocation closure takes owned A and `&InvocationCtx`, returning
`Result<Option<Emission>, String>`. The returned closure uses the existing
NodeAction blanket implementation. Preparation completes before shared-context
invocation. Only the helper constructs the zero-or-one transport vector for these
constructor paths. No bound on A, public trait/type, erased intermediate, extra
allocation for preparation, or additional dynamic-dispatch boundary was added.

| Constructor | Admitted-context preparation | Return conversion |
|---|---|---|
| function | take_prepared A | Some(encoded emission) |
| flattened | take_prepared A | Some(encoded emission) |
| with_input_layout | take_prepared L::Value | Some(encoded emission) |
| with_layout | take_prepared L::Value | Some(NodeOutput emission) |
| typed | delegates to with_layout | same as with_layout |
| typed_factory | take_prepared A | Some(NodeOutput emission) |
| local_function | take local slot 0 | LocalReturn |
| local_with_layout | consume reserved raw selections with frozen slots | LocalReturn |
| multi_port | look up registered physical port dispatch and extract its event | Some(NodeOutput emission) |
| plain source | unit, without adding an input port | LocalReturn |
| directed source | unit, without adding an input port | LocalReturn |

Neither source path is excluded. Encoded decoding and validation remain in the
port-owned argument builder before admission. Local structural preparation still
occurs after reservation; L::slots is evaluated at declaration, not invocation.
The heterogeneous port dispatch map remains. Factory handler construction remains
inside the physical action factory, not at template declaration.

Public NodeAction, arbitrary external Vec completions, scheduler validation,
NodeOutput, LocalReturn, metadata, grants and constructor signatures are unchanged.
Raw/custom action constructors intentionally retain their unrestricted public
transport rather than being forced through the private Option boundary.

## Phase 4 Review

Postimplementation self-review inspected the actual source diff and new tests
against the design and critique, separately from the initial implementation pass.
No Batch A correctness issue was found. This is not independent reviewer approval.

- Mutable extraction ends before handler access to the same context. A is not captured as a stored value or constrained to Clone/Sync by the helper.
- Preparation errors short-circuit before handler invocation and vector construction. Encoded decoder ownership and scheduler admission ordering are unchanged.
- Local raw extraction, error text, frozen slot cloning and move-only preparation match the deleted implementation.
- Typed delegation preserves the same port layout, policy, output declaration and handler contract. typed_factory cannot delegate through an eagerly created handler without changing factory lifetime, so retains its factory boundary.
- LocalReturn continues to distinguish suppression from an emitted value with no destinations. Ordinary Option/Result values remain payloads.
- Multiport dispatch still selects by physical port before shared-context handler execution. External actions and whole-result validation are unaffected.
- Source templates retain an empty port list, not an artificial unit input. Both source return forms now share sequencing and vector conversion.
- Diff scope was checked: this batch did not change adjacency, scheduler implementation, other authoring areas, or the main design/critique documents.

## Regression Evidence

Completed command:

`nix develop --command cargo test -p phenix-graph --locked`

Result: **610 passed, 0 failed, 3 ignored**, across 61 result groups:
197 unit tests, 389 integration tests and 24 doctests (3 executable examples and
21 compile-fail probes). The three ignored tests are existing manual timing tests.
An initial attempt exceeded the tool's 120-second limit during portable-wire
tests; the complete retry used a longer limit and passed. The completed retry
reported 0.09 seconds for the warm Cargo build, 40.04 seconds for portable_wire,
and 2.22 seconds for all doctests. These are observations, not before/after
performance comparisons.

New tests, both passing:

- `send_only_prepared_argument_preserves_custom_context_and_grants`: custom decoded layout yielding Cell<i64> (Send, not Sync), context access after extraction, allowed writes/reads, denied grants, decoder failure with zero handler calls/publications, and subsequent successful waves.
- `typed_factory_is_fresh_for_two_physical_nodes_and_two_engines`: zero eager factory calls, exactly four physical handler creations, independent counters in both nodes, and independent repeated execution in both engines.

Feature check:

`nix develop --command cargo test -p phenix-graph --locked --no-default-features --test action_lowering`

Result: **2 passed, 0 failed**; build reported 16.62 seconds. Tests use manual
layout/schema implementations and require no derive feature. This was a targeted
no-default-features check, not a full no-default-features package run.

Existing passing coverage includes quality_10 move-only fields; frozen layouts
in iteration_eleven_layout; heterogeneous ports in layout_api; Option/Result and
suppression/routing in output_semantics/output_presence/output_wrapper;
source_operations portless starts and directed sources; manual_api and scheduler
atomic decode failures; typed factory tests; external dyn NodeAction and invalid
external completions; cancellation; codecs; layout/handler compile-fail probes.

Formatting verification passed with rustfmt edition 2024, skip_children=true,
--check on the three owned Rust files. `git diff --check` also passed.

## Measurements

Counts are static diff/source measurements against workspace HEAD, excluding the
other agent's adjacency hunks:

| Measure | Before | After |
|---|---:|---:|
| Private named action executors | 6 | 0 |
| Shared closure composition helpers | 0 | 1 |
| Library vector-construction sites in the six executors and two source templates | 8 | 1 |
| Duplicated typed/contextual constructor bodies | 2 | 1 (typed delegates) |
| Added public types/traits | 0 | 0 |

Owned production source delta: lib.rs +55/-130; authoring.rs +8/-2, totaling
**63 added, 132 removed, net 69 fewer lines**. The new test file adds 163 lines.
The combined lib.rs diff also contains unrelated Batch B adjacency edits and
must not be attributed entirely to this batch.

The helper stores only the original preparation/handler captures, with no new
boxed argument or runtime enum. Single-emission vectors still require their
existing transport storage; suppressed output still produces an empty vector.
No allocator instrumentation, binary-size comparison, clean compile comparison,
or runtime benchmark was performed. No memory, compile-time, code-size or
throughput improvement is claimed from these source measurements.

## Remaining Gates

No known Batch A functional issue remains from the local tests and self-review.
Independent Phase 4 diff review and named maintainer acceptance remain pending;
this report cannot satisfy those authority gates itself. Final integrated-revision
checks and required remote CI must still be run by the delivery owner after all
agents finish. No remote CI or commits were requested or performed here.

Unchanged limitations: local preparation failure does not restore consumed raw
inputs; external actions remain untrusted; heterogeneous dispatch remains erased;
arbitrary closure behavior is not certified by structural metadata. Further
allocation or dispatch optimization requires measurements and a separate design,
not additional abstraction in Batch A.
