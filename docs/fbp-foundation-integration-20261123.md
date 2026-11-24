# Foundation Integration

## Constraints

Assemble existing draft lanes without rebasing the stack or merging to main.
Preserve accepted H1, A1 and C7 TrustedNativeScope. Keep compiler reuse,
composition authority, runtime mechanisms and contributed capabilities separate.
Green component heads do not verify their combined source or complete an issue.

## Design And Critique

Leaving separate lanes avoids conflicts but provides no executable combined
candidate. Cherry-picking source without history obscures provenance. Merge the
reviewed heads into a new draft integration branch instead, then test the exact
combined source. No new production architecture is introduced by the assembly.

| Lane | Integrated Source | Component Evidence |
| --- | --- | --- |
| H1 | `ab7f5e93de460e6c472f847f7b7dbc62319907e7` | Accepted draft #1386, eleven workflows green |
| H4 | `3bdccbe9b2b3f3f3b30a6abe8a9a0fef66a1a822` | #1387, [Flake 45248986266](https://github.com/matthis-k/phenix-ai/actions/runs/45248986266) and eleven specialized workflows green |
| A1 | `4f59a52cdc6aad1d10938d5de640e8ffa1379c4b` | #1388 acceptance head, [Flake 45259711758](https://github.com/matthis-k/phenix-ai/actions/runs/45259711758), wake Loom and session Loom/native green |
| A2 | `e880c2ad875faa5cd6e086fc99f9409661bd68d0` | #1391, all six [Flake 45260953053](https://github.com/matthis-k/phenix-ai/actions/runs/45260953053) jobs green |
| Q3 coverage | `d6637349048e5dfd0a237ae66a5a54cf02b8d72f` | #1389, [Flake 45249309337](https://github.com/matthis-k/phenix-ai/actions/runs/45249309337) and [three hosted benchmark repetitions 45249308643](https://github.com/matthis-k/phenix-ai/actions/runs/45249308643) green |
| H5 | `733a70a3a59601d9eae13608fd8812984f8bed5b` | #1390, all six [Flake 45263147721](https://github.com/matthis-k/phenix-ai/actions/runs/45263147721) jobs and eleven specialized workflows green |
| Q2 sibling | `d23fa932f0dcc625a912ac82e3976c18894d3e4a` | #1371, all six [Flake 45039455620](https://github.com/matthis-k/phenix-ai/actions/runs/45039455620) jobs green |

Only scorecard/work-map conflicts required resolution: retain accepted A1 and
the H4 reassessment together, without restoring stale open/design-only labels.
Compiler/runtime source merged without conflicts. An independent integration
review found no actionable source defect but requested combined nested-reindex
coverage before relying on the separate compiler and composition tests.

## Combined Regression

`network/compiler/integrated_reindex_tests.rs` freezes a completed A1/A2 nested
composition before applying Q2 root edits. Replaying nested COW edits changes
definition keys and must not be falsely declared reuse-eligible. The tests assert
actual incremental eligibility, compare against canonical full compilation,
and exercise insertions before/between/after nested groups, single removals and
every eligible bounded batch subset.

They compare nested projections, inspection/physical indexes and generation
authority; reject stale nested endpoints through the new checked export helpers;
rebind fresh endpoints; resolve indexed wake/notifier paths; and execute both
queue policies. Staged wait/resume, cancellation cleanup, retired handles, lane
reuse and retained old sessions run on the recomposed topology. Default and
isolated no-default targeted tests, formatting and strict all-target Clippy pass.

A separate single-target regression avoids masking a relocated wake bug by
broadcasting: two nested notification-driven Custom peers shift physical IDs
through eligible Q2 insertion/removal. With both external readiness flags enabled,
one addressed notifier selects only its intended peer. Notification cannot resume
WaitWake; waking that peer resumes only it and leaves the other waiting without
a capture. Both queue policies verify capture value, process, request, session and
generation identity. Scopes remain open, so sealed-scope polling cannot rescue
misrouting. All three combined tests pass in both isolated feature configurations.

The combined public Graph/Kernel targeted run also passes H1, A1, A2, lane
closure, request lanes, prepared exports and Q2 compiler unit/oracle tests.
One ignored manual EOS profile is not an unexecuted correctness gate.

## Acceptance And Remaining Decisions

Only A1 receives whole-finding closure: 18 -> 100, with exact-source independent
whole-scope review and hosted verification. Nine findings remain. H1 stays 100
and C7 remains excluded at its unchanged historical 12, not sandbox credit.

| Finding | Implemented Boundary | Remaining Acceptance Or Decision |
| --- | --- | --- |
| H3b | Fail-fast session leases, exclusive producer/driver ownership, worker identities and bounded actual-source lifetime verification | Define remaining kernel-owned schedule obligations separately from arbitrary plugin-owned locks/threads; a trusted callback can create a lock/join cycle beyond kernel control |
| H4 | Dirty/targeted scheduling plus finite staged poll membership and safe mandatory-set shrink | Required source/Custom/global scans and passive report inventory scans remain; agree representative scheduling budgets |
| H5 | Adaptive visited membership, authenticated reachability and own-lane closure DFS | Repeated upstream proofs and full global-EOS checks remain; agree closure scaling budgets |
| H6 | Paired latency/throughput, allocator-event harnesses, RSS and workload controls | Local thermal/noise controls are not non-inferiority proof; portable product latency, live-memory and concurrency budgets remain unaccepted |
| H7 | Authenticated FIFO/lane queues, explicit credit/cancellation and passive witnessed single-flight blockers | Choose independent sessions versus opt-in per-request process instances versus a cooperative shared-state contract; stalled process ownership cannot be bypassed without defining state/effect continuity |
| A2 | Exact public facade, explicit schema conversions, canonical replacement diagnostics and executed versioned provider feedback | Decide whether explicit adaptation suffices or direct opt-in width satisfaction is required; never weaken existing exact claims or silently supply missing inputs/discard outputs |
| Q2 | Fresh generation authority with bounded root-delta reuse and canonical fallback/oracles | General nested/dependency-aware invalidation, persistent host reload reuse and whole-recomposition scaling remain; do not add another unmeasured ad hoc fast path |
| Q3 | Exact unique selection, indexed planning, chain/diamond/skew/early-conflict evidence | Dense successful priority graphs, total planning work and recomposition budgets remain; selector allowance does not bound all planner work |
| Q4 | Static wiring SCCs and declared-root reachability facts | Arbitrary callback termination/output/wake behavior cannot be proved from wiring; a restricted declared process model with explicit environment assumptions is needed for sound progress/reject/unknown results |

These decisions are not waived by the integration. Explicit adaptation only,
shared-state cooperative progress, or structural reachability alone must not be
silently relabeled as the broader A2/H7/Q4 guarantee. Independent unblocked work
has continued while those contracts remain open.

## Publishing And Verification

No workflow modifications are included. The current OAuth credential lacks
`workflow` scope. Prepared broader Q3 triggers/paired reference and the new H4
paired workflow remain in `/tmp/opencode/fbp-foundation-20261123`, with the H4
workflow also preserved in local commit `2c379943`. Existing Q3's benchmark-file
trigger covers #1389, but planner-only changes still need manual dispatch until
the trigger fix is authorized. No hosted execution of unpublished workflows is
claimed.

The first H5 source exposed a real historical-harness compile failure in run
45260722335: the old runtime lacked lane APIs. The correction separates the
current lane executable, reuses workload constructors, and successfully compiles
the historical standalone example without extending compatibility code. Fresh
exact-head H5 CI is green; the earlier failed source is not accepted.

The assembled integration head still requires its own full exact-head suite.
Component run links above are provenance, not substitution for that result.
