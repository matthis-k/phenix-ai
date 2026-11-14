# Proposed M8 — end-to-end FBP agent validation

Status: Phase 0/1 design, 2026-11-11. This is a **new proposed milestone**,
not an M8 entry in `phenix_fbp_implementation_blueprint.md` (which ends at M7).
Baseline: `4b6bc10b0ec02db93be603ea084c697a44b09261`, with
[exact-head six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43006354681)
passing. This does not change the canonical M0–M7 acceptance definitions.

## Phase 0 — requirements, constraints, baseline

M7 established one `ComponentDef` primitive/composite model, persistent FBP
`NetworkSession`, exact host-approved declarative `NetworkPatch` composition,
Kernel/StorageGraph integration and legacy removal. Current agent example
`fbp_agent_composition.rs` is a *linear* deterministic transformation; it
cannot demonstrate a tool-feedback/model-reasoning loop. Crossed request
streams on one FIFO shared-session join can still head-block.

Goal: show a plugin-authored **iterative cycle**, not privileged kernel agent
behavior, and demonstrate a bounded, composable way to execute independent
correlated requests without weakening existing FIFO semantics.

Non-negotiable invariants:
- Kernel has no special model, tool, memory, compaction or agent type.
- No new scheduler, unchecked patch callbacks or implicit component authority.
- One packet per emission, atomic fanout, bounded queue/capture, fair
  process continuation and state scoped to one process instance/session.
- Exact symbolic path grants and compositional revocation remain unchanged.
- No silent skipping of a connection's FIFO head. Pre-existing
  `all_join_keeps_requests_separate_and_never_skips_a_fifo_head` stays green.
- Existing whole-drain retirement and code lease rules remain untouched.

## Phase 1 — alternatives for correlated request execution

| Option | Benefits | Costs | Decision |
|---|---|---|---|
| Keep shared session, require aligned arrival orders | Zero code, shared process state | Common crossed joins can permanently wait | Insufficient for multi-request agent paths |
| Scan past FIFO heads for matching request | Could unblock crossed joins | Changes native per-connection FIFO, read reservation and fairness; risks unbounded staging and ambiguous side-effect order | Reject without a separate redesign |
| New per-request router/queues in `NetworkSession` | Shared process instances, per-request fairness | Adds queue authority, admission policy, memory bounds, cancellation accounting and new graph concepts | Defer pending real shared-state use case |
| **One existing `NetworkSession` per externally admitted request** | Strict FIFO unchanged; process state naturally isolated; existing cancellation/session identity/lease rules | More factories/state per request; host must cap and fairly drive sessions; global shared state requires an explicitly granted store | **Select for M8 proof** |

M8 test will use an existing compiled graph, two *independently started*
sessions with `Readiness::All` on a two-input join, and opposite cross-session
arrival orders. Both requests must settle with one correct captured output.
A foreign `RequestId` remains rejected across sessions. The host controls
max active sessions; this test adds **no new public graph runtime API**.

This **does not fix** shared-session correlated-head blocking; mark that
separately unresolved for any use case requiring shared mutable process state
within one session.

## Phase 1 — ordinary-plugin iterative agent acceptance

A pure FBP network with:
```text
  host prompt -> memory -> [optional compaction] -> model
                    ^                           |     |
                    |                      tool decision final answer
                    |                           v     |
                    +--- tool feedback <- tool    captured reply
```

The model process emits *one of two named output ports* per activation. The
first model step requests a tool, the ordinary tool process emits feedback,
the memory process incorporates the feedback, and the next model activation
emits the final response. This must execute at least two model activations
and at least one tool effect, with exact-once effects on backpressure retry.

A `preset` registration authored with existing `NetworkPatch::new` adds all
four stages and root projections into an initially empty `Composer`.
Independent `provider` can replace `model` (same interface), and
`compaction` can splice a transformation into the existing memory->model
connection. The host grants only their exact component/connection footprints.
No special plugin-origin privilege.

Test executable outcomes before/after replacement and compaction, remove
compaction and verify reversion, test two sessions for isolated per-session
memory state, and cancel an in-flight loop to prove stale work is not resumed.

## Phase 4 acceptance evidence

The existing tests including strict FIFO crossed-head-block fixture remain
unchanged and passing. New regressions must exercise *actual compiled*
`NetworkSession` execution, not only plan metadata or a mocked scheduler.
No benchmark or actual LLM/network/tool effects are implied by this
deterministic proof. Verify `cargo fmt`, strict Clippy, graph + kernel +
storage full workspace tests, no-default feature tests, the exact Nix CI
head, and a final adversarial diff review.

## Deferred capabilities, not waived

This is not P1/production readiness: no real model provider streaming,
API authentication, durable memory/journal, actual tool permissioning,
session persistence, crash recovery, native plugin loading, or frontend
interoperability is supplied by this test. The design makes each capability
an ordinary plugin extension point; later milestones must independently
verify the implementation.
