# H4 scoped optional readiness watch — design protocol

## Phase 0: problem and invariants

Exact verified baseline: EOS PR #1288, commit `aa3225110d29939a2f842a102d80ef74e7a367e2`, six CI jobs and paired H4/H5/EOS/wake benchmarks green. Hybrid scheduler's `always_poll` includes **every** `Readiness::Optional` process for the lifetime of every session, even while no request is sealed and every optional input queue is writer-open/empty. A large FBP graph containing optional tool outputs therefore polls hundreds of irrelevant optional policies on **every** unrelated ingress, wake, and process completion.

Correctness:
- Global EOS requires actual writer closure and is already signaled by `dirty_queue()`.
- Packet availability requires a queued input event and is signaled by ingress, publication and drain.
- Request-local input absence can arise **without** a packet or global EOS when *a sealed, authenticated request's upstream owner completes*, and must keep affected optional readers eligible until proof/activation. This also applies to **required+optional** readiness with already buffered required data, not only `required=[]`.
- Custom policies may change decision on external state without a queue event; keep them unconditionally polled. Staged `WaitInput`/`WaitInputs` permanently join unconditional polling.
- Avoid new public API, compatibility or disabled constraints. Retain deterministic sorted evaluation, authenticated request reachability and legacy full-scan fallback.

## Phase 1 alternatives

| Approach | Cost | Correctness |
| --- | --- | --- |
| No change | Poll every optional policy on every drive | Proven, expensive in mixed graphs |
| Skip optional whenever queues empty | None | **Incorrect** for sealed zero-packet obligations |
| Poll optional when any sealed request exists | Simple dynamic global flag | Still scans unrelated optional nodes; defeats sparse branches |
| **Index optional nodes reachable from currently sealed scopes** | Sparse per-scope count, updated on lifecycle (seal, release, cancellation) | Safe if sealed scope reachability stays immutable; queue events independently wake non-scoped optional |
| Add explicit per-port closure subscriptions | Potential narrower polling | New runtime abstraction and complex upstream completion invalidation |

Choose scoped optional watchers.

## Proposed implementation

Maintain `always_poll: BTreeSet<usize>` for only sources, zero-input nodes, Custom policies and staged waits. Add `scoped_optional_poll: BTreeMap<usize, usize>` as sealed-scope reference counts, keyed by compiled process index. `close_request()` registers **every** `Optional` node in `scope.reachable` exactly once (repeated seal is idempotent), before reevaluating the existing reachable dirty set. `release_request()` decrements registered indices after settlement. `terminate_request()` decrements registered indices **before** clearing `reachable` and marking terminal. Unsealed cancelled scopes are never counted.

On every incremental `refresh()`, evaluate the sorted union of `dirty_processes`, unconditional `always_poll` and `scoped_optional_poll.keys()`. `prefer_full_refresh` may conservatively overestimate coverage by adding their sizes; this can only choose the proven full-scan fallback more often, never skip work. No per-refresh traversal of all request scopes or per-port closure DFS is introduced.

In the absence of sealed scopes, `Optional` behaves like ordinary packet-driven readiness: any packet arrival, writer close, capture drain, process handoff or EOS dirty event marks it. A sealed optional node with an in-flight request is polled even if it is currently blocked, preserving closure progress after upstream `Step::Complete` without output.

## Phase 3 evidence and benchmark

- Existing multi-root, request-local optional absence, required+optional with staged waits, retained data, repeated scopes, cycles, request cancellation and EOS tests must pass.
- New overlapping scope and terminal lifecycle regression with one registered optional consumer and one unrelated optional node; verify repeated `close_request` does not duplicate an obligation, cancellation removes its watcher while preserving other scope.
- New paired release benchmark: 1 active packet sink + 0/16/64/256 unrelated, writer-open all-optional processes; no sealed scopes. Warmup and 3 interleaved samples, same logical CPU, exact identical source copied to baseline; include per-iteration timings and RSS. Also retain previous H4 sparse and H5 chain/fan-in controls, which should not materially regress.
- Reject on a lost optional closure, changed request status, incorrect cancellation/release, or a measured regression without significant mixed optional benefit.

## Accepted limits

A sealed request reaching all optional nodes still requires broad polling; custom readiness remains always-polled until an explicit invalidation contract exists. The fast path cannot remove O(V) status scans or generic-notifier full scans. The new reference-count map is bounded by compiled optional nodes and updated by request lifecycle, not every event. No claim of M7 acceptance or universal speedup.
