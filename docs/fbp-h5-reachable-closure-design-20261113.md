# H5 — request reachability as closure traversal bound

Date: 2026-11-13. Base: PR #1277 final head `629a1400dbee5f47f9c7707898ec5a1302470087`, exact-head six-job CI #43321584761 plus paired benchmark #43321583806 green.

## Phase 0: problem and invariants

`request_input_closed` currently traverses *all* upstream processes from a consumer to determine whether a sealed request could still deliver an input, even through branches that **cannot** belong to that request. `request_live`, `request_work.closure_pending`, and passive `state()` also evaluate request-local `closure_obligation` over the entire process inventory. There is already an authoritative, monotone, bounded `RequestScope.reachable` set, expanded from every successful `inject_request` ingress and `start_request_source`. Its closure over the compiled `downstream` graph includes every process that can propagate that request.

Preserve event ordering, explicit session/packet ownership, closure correctness, fanout, FIFO backpressure, typed errors, state purity and cancellation. There must be no new public API, additional authority, event cache or privileged agent-loop scheduler.

## Phase 1: compare alternatives

| Option | Strength | Weakness | Decision |
| --- | --- | --- | --- |
| Baseline: no change | No risk | Upstream closures scan unrelated live graph and passive snapshots walk all N processes per sealed request | Reject for demonstrated structural waste |
| Independent cached transitive ancestors for every node | O(1) indexed tests | O(V²) size, topology churn, duplicated dynamic state | Reject |
| Keep all traversals, use a faster visited structure | Minor constant-factor improvement | Still explores irrelevant graph | Defer |
| **Bound traversal by the existing request's reachable set** | No new concept or state; excludes unrelated upstream subgraphs and passive closure candidates | Still scans physical ingress fan-in and reachable paths; `reachable` BTreeSet membership costs O(log V) | **Select** |

### Structural invariant

A packet carrying `RequestId = R` can originate only from successful `inject_request(R, ingress)` or `start_request_source(R, path)`. Both register their root through `note_request_root`, which expands *all* compiled downstream edges, including cycles. Every downstream process and output queue that can carry R therefore has its producer in `R.reachable`. A request-typed packet in an input queue whose producer is not reachable is impossible under the retained kernel/transport provenance model. In `request_input_closed`, for any process-owned upstream queue whose producer is outside R.reachable, skip membership inspection and DFS expansion; direct host ingress queues retain their normal membership check. For reachable producers, retain exactly the prior activity, closure-obligation and queue checks.

`RequestScope.reachable` is monotonic until the request is released. Multiple source/ingress roots merge their downstream sets. There is no false closure from foreign-request or global packets: those never grant a request R source identity.

In passive `request_live`, iterate *all processes* for live `p.request == R` (conservative, keeps nonclosure ownership safety), then check closure obligations **only on reachable process indexes**. In `request_work` and passive `state()`, enumerate reachable processes for scoped closure obligations. Keep queue-count scans, active process diagnostics, wake checks and any global/no-request closure behavior unchanged.

### Performance study

Add an identical portable `fbp_closure_benchmark.rs` compiled against (a) verified #1277 head and (b) candidate on the same Ubuntu runner. Workloads:

- **Fan-in:** one scoped root that drops its event, one all-optional downstream sink that requires its upstream request closure; N other idle unscoped upstream processes also connect to the optional port. A sealed scoped request must activate exactly the root and the optional sink. The candidate should avoid traversing the N unrelated upstream producers.
- **Chain:** root followed by N all-optional descendants in a linear path, all request-reachable. Each must receive exactly one scoped absence notification. The candidate must not skip reachable closure obligations or regress this control workload.

Measure end-to-end request admission, seal, native drive, status, release on one persistent session across N and repetitions, plus compile/start times. Run both implementations with byte-identical benchmark source, compare median of three interleaved repetitions; test exact semantics independently of timing. Existing H4 sparse-ingress benchmark can be run as an additional control.

## Acceptance, limitations

- Regression: an unrelated open upstream branch must not delay a sealed request's optional closure, and a separately rooted **same-request** upstream branch must **not** be prematurely closed.
- Cycles, diamond reachability, retained bounded packets, multiple request roots and cancellation must keep existing semantics.
- Diff introduces no extra data structure beyond existing reachable set. `request_input_closed` does not assume all upstream queues are empty; it checks queues whose producers can belong to this request and all direct ingress queues.
- Six-job exact-head CI, paired benchmark and Phase 4 diff/limitation review required. Do not claim measured speedup without successful raw paired samples. H5 remains partial for O(V) refresh and linear/diamond reachable closure paths; H4 and M7 remain incomplete.
