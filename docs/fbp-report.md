# Native FBP Worktree Report

## Current Campaign

The sections below are the historical native-engine cutover report, not acceptance
evidence for the 2026-11-14 lifecycle/request-lane candidate. That campaign started
at `16e77f6c6` with uncommitted work and now uses branch
`fix/fbp-lifecycle-request-lanes-20261114`. It is separate from the still-open draft
PR #1300 stack at `516e0883875d5ec483e3595d50b77b3d5e8979c4`.

Current implementation, policy decisions and verification gaps are recorded in
[the campaign record](fbp-hardening-campaign-20261114.md). The complete one-decimal
[33-issue tracker](fbp-issue-progress-estimates-20261113.md) remains authoritative.
No entire issue, M7 or M8 closure is inferred from local passing suites. Production
P1/P2 agent integration, OS plugin isolation, strict retired-generation revocation,
full static liveness, and representative paired performance/memory acceptance are
not delivered by this batch.

## Historical Report

## Identity And Status

- Exact baseline/head: `32bbe60e6cd2ebd34a705486b4ef96e85245ee42`.
- Branch: `impl/graph-consolidation-20261110`; initial worktree was clean.
- PR #1260 was rechecked at `671956269a2655084ae90eb0b1e09e506177d71c`, not assumed
  to be the checkout baseline. The checkout was six commits ahead.
- Implementation is in the current uncommitted worktree. No commit, push or PR
  mutation was requested. Consequently there is no new final commit or remote CI
  run to report. Baseline CI run 42773370525 passed, but does not certify changes.

## Delivered Model

One `ComponentDef` contains one `Interface` and a primitive factory or composite
network body. Builder `node`, `module` and `add` return the same `ComponentRef`.
Typed refs/keys enforce payload and direction; erased lookup checks actual scope,
revision and identity. There is no claimed generic owner-record proof. Replacement
checks exact interfaces and inherited semantic obligations, preserves symbolic
connections, and revokes old handles. Tests swap both body directions and cover
nested/repeated projections, independent mutable instances and stale refs.

Compilation validates names, complete types, claims, requirements, projections,
recursion and expansion limits, then flattens to physical primitive processes.
Factories are not spawned during compilation. Modules have no runtime executor.
The crate retains its existing package name and schema dependency.

## Persistent Execution

`NetworkSession` persists across admissions. Bounded FIFO ingress/connections and
bounded output captures own packets; a step consumes a reservation and returns at
most one emission. Sources/sinks need no fake ports or mandatory return value.
`Complete` settles an activation, while `Finished` ends a process. Function
adapters, stateful processes, delayed owned jobs and local dispatch use this path.

One emission's selected fanout is atomic. Full suspends publication with retained
prepared values, not an error or a rerun of action effects. Across emissions,
earlier commits survive later block/failure. Selection can only narrow existing
connections; invalid/duplicate targets fail before delivery. Explicit discard is
separate from forgotten wiring. Captures participate in backpressure and draining.

EOS is connection state, not an ordinary payload. Completion requires closed
ingress and drained work/captures. Idle open sessions remain quiescent. Required
stranded groups fail explicitly, optional/staged groups use actual upstream work
evidence, and unrelated closure cannot satisfy required readiness. Cycles are
legal, with full-queue SCC diagnostics rather than progress guarantees.

Cancellation invalidates work authority and accounts disposal. Scoped recovery
restarts only affected processes under an explicit policy; no external effects
are rolled back. Native payloads, typed wrappers, detached compiled factories,
jobs and callback errors retain exact code/resource leases through destruction.

## Composition And Integrations

`NetworkPatch` owns declarative edits. Host registrations separately supply exact
grants, provenance, dependency order and explicit priorities. Conflicts reject or
resolve explicitly; defeated prerequisites deactivate dependents. Dependency-
ordered splices retain stable anchors and require exact generated-edge grants.
Whole-candidate replay and compile commits once; revoke recomposes from base.
Replacement writes the component, not all neighboring edges.

Kernel retains provider selection, exact origin/contract checking, candidate
artifacts, retryable activation, wake/shutdown and generation retirement. Producers
retain at most one unadmitted event on Full and do not repoll it. StorageGraph
retains authorized atomic snapshots, subscribe-before-bootstrap, overflow origins,
fair observation, request-scoped demand and worker-thread query guards. Storage
and Schema implementation authorities remain unchanged. There is no core
dependency on Kernel, Plugin or Storage and no adapter-owned scheduler.

## Removal And Measurements

The old authoring tree, wave scheduler, deployment/output wrappers, module compiler
variants, mutation observer and obsolete derives are deleted. No v1/v2 forwarding
surface remains. `lib.rs` is 44 lines. `Ports` supplies typed descriptors plus
renamed/nested selectable input reconstruction; unique move-only payloads move and
explicit Arc leaves share without losing their code leases.

Graph-plus-derive non-test physical LOC: **15,688 -> 7,645**. Test/support LOC:
**31,857 -> 12,845**. Lexical public types: **165 -> 89**; traits: **35 -> 8**;
explicit public function lines: **429 -> 286**. These are physical/lexical counts,
not executable SLOC, semantic method counts or performance measurements.
The full [measurement record](fbp-measurements.md) includes module and downstream
counts, exclusions, reproduction and limitations. The [regression ledger](fbp-regression-ledger.md)
maps retired suites to named native tests and records deliberate syntax changes.

## Verification

Verification is local against the working sources, not new-head remote CI.
The Cargo matrix includes default workspace tests, workspace no-default tests,
graph-only no-default tests, strict all-target Clippy and rustfmt. Rustdoc embeds
the current standalone API examples. The required native examples and migrated
Kernel/StorageGraph examples run through the same production implementation.

| Final command | Result |
|---|---|
| `cargo test --workspace --locked --quiet` | Passed |
| `cargo test --workspace --no-default-features --locked --quiet` | Passed |
| `cargo test -p phenix-graph --no-default-features --locked --quiet` | Passed, including Rustdoc |
| `nix develop --command cargo fmt --all --check` | Passed |
| `nix develop --command cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| Native `basic_fbp`, `streaming_fbp`, `plugin_composition`, `owned_dispatch` | All executed successfully |
| Kernel `basic`, `storage`, `scoped_storage`; StorageGraph `derived` | All executed successfully |

`nix flake check --keep-going` passed against a filtered immutable source snapshot
including untracked files: eight changed derivations were built, with unchanged
checks reused. Snapshot construction uses `builtins.path` excluding `.git`,
`target` and result links, avoiding both index mutation and omission of new modules.
Final source-only formatting/Rustdoc checks are also recorded by the execution log.

Final checked immutable source:
`/nix/store/bp2c3y025jvmk2vsxsjcwdyfbz9mfjg4-phenix-fbp-verification-source`.
Only documentation outside the packaged Rust sources changed after that check.

Independent reviews found concrete payload-authentication, lease, EOS, required-
readiness, moved-builder and foreign-dispatch defects. Those were fixed with
regressions. Reference models exercise generated DAGs and bounded priority,
replacement/revoke/rewire/splice sequences. No arbitrary 9/10 score, formal
verification or universal behavioral-equivalence claim is made.

## Accepted Limits

- FIFO correlation can head-block; conservative cyclic absence can remain pending.
  No arbitrary arrival-skew or deadlock-recovery guarantee is synthesized.
- Cancellation cannot preempt nonyielding code. Explicit process restart does not
  undo side effects or migrate state; external runners still own handed jobs.
- Plugin objects are Rust-native, not a general dynamic-library ABI or sandbox.
  Complete schemas and codecs do not prove semantic honesty.
- Output records and every historical layout/macro spelling are not retained;
  native input records and routing preserve the relevant processing behaviors.
- Diagnostic state is bounded; historical full-payload reports, wildcard tracing
  and exact wave-color counters are not preserved as a compatibility engine.
- No DFIR backend spike or native performance baseline was performed. The chosen
  backend is the small native runtime; no performance gain is claimed.
- New-head remote CI and a clean committed PR remain unavailable until explicitly
  requested. The existing baseline SHA is not mislabeled as a final implementation.
