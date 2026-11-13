# H4 scheduling optimization — evidence and acceptance ledger

**Revision base:** H4 candidate chain #1281 → #1285 → #1286 → #1287 → #1288 → #1289 → #1290. All are draft/unmerged stacked PRs. Each step used explicit parent SHA and exact-head CI/paired benchmark evidence. This record separates a verified optimization candidate from production/M7 agent acceptance.

## What has been optimized

| Mechanism | PR | Evidence and limitations |
| --- | --- | --- |
| Idle scoped readiness / obligation-aware prefilter | [#1281](https://github.com/matthis-k/phenix-ai/pull/1281), [#1285](https://github.com/matthis-k/phenix-ai/pull/1285) | [Six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43344454402), same-runner paired H4 [#43344453975](https://github.com/matthis-k/phenix-ai/actions/runs/43344453975). O(V) inventory remains |
| Dirty ordinary packet-process selection in mixed graphs | [#1286](https://github.com/matthis-k/phenix-ai/pull/1286) | [Six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43350266152), 256-node scoped sparse 19.41→8.18 µs, unscoped 10.43→2.96 µs. Small/all-optional regressions recorded |
| Addressed process wakes | [#1287](https://github.com/matthis-k/phenix-ai/pull/1287) | [Six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43372886541), [pinned paired wake](https://github.com/matthis-k/phenix-ai/actions/runs/43372886118), 256-process 2.72→0.89 µs. `WakeHandle` resumes `WaitWake` |
| Writer EOS and reader disposal endpoints | [#1288](https://github.com/matthis-k/phenix-ai/pull/1288) | [Six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43374067706), [EOS paired](https://github.com/matthis-k/phenix-ai/actions/runs/43374067227), 128-step chain 194.2→55.8 µs. Tiny-graph regression |
| Scoped Optional polling | [#1289](https://github.com/matthis-k/phenix-ai/pull/1289) | [Six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43378848894), [paired optional](https://github.com/matthis-k/phenix-ai/actions/runs/43378848418), 256 unrelated Optional branches 2.74× unscoped, 4.15× scoped. Reachable sealed Optional still polled |
| Explicitly notified Custom readiness and scoped non-wake notifier | [#1290](https://github.com/matthis-k/phenix-ai/pull/1290) | [Six-job CI](https://github.com/matthis-k/phenix-ai/actions/runs/43382163601), [same-runtime pinned mode benchmark](https://github.com/matthis-k/phenix-ai/actions/runs/43382163122), 256 blocked Custom 167.5→1.05 µs (pathological idle polling stress only). Existing Custom defaults to polling |

## Design correctness boundary

- **Ready work:** never manufactured by a notification. Actual `selection`/reservation and owner handoff still authorize an activation.
- **Request scope:** sealed scope membership uses authenticated topology reachability, with idempotent registration, reference-counted overlapping scopes, and cleanup on release/cancellation. An unselected route closes **for that request**; this is distinct from actual global writer EOS.
- **Explicit external invalidation:** `ProcessNotifier::notify()` dirties one compiled process but cannot wake a `WaitWake` continuation. `WakeHandle::wake()` does both. Existing `SessionNotifier::notify()` has no process identity and retains full-scan semantics.
- **Conservative policies:** `ReadinessPolicy::notification_driven()` returns false by default; an opt-in policy must notify every external change relevant to `select`. A dishonest/forgetful plugin can strand work. Custom code cannot be safely assumed pure or event-driven.
- **Fallbacks:** startup, global notifier, complex recovery, all-special graphs and ambiguous lifecycle changes may still scan all processes; this is a correctness feature, not unaddressed H4 technical debt.
- **Memory:** `dirty_processes`, `always_poll`, sealed-scope reference counts and targeted-wake set are bounded by compiled process count / reached scope count. Peak RSS on a shared runner is an indicative whole-process metric, not allocator-event proof.

## New integration acceptance: mixed agent loop

[Draft PR #1291](https://github.com/matthis-k/phenix-ai/pull/1291) introduces:
- Persistent graph: prompt forks into model/skills; model alternates local and remote tool calls; skills+tool converge at a required+optional join; persistence records one result per request.
- For **every** local tool turn, a custom readiness gate must remain blocked until a `ProcessNotifier` is sent. The remote-turn local branch must instead close request-locally, with no global EOS. Both modes must settle and release every authenticated request, with exactly 11 owned process steps and no missing/duplicate persistence.
- 0/16/64/128 unrelated writer-open service nodes, split between Custom and Optional; compare legacy-polled against explicitly notified Custom policies under the **same compiled release executable** and CPU affinity. The benchmark isolates the scheduler fraction rather than a real LLM/SSH network path.
- Concurrent targeted ProcessNotifier regression: 64 independent pending Custom inputs, eight notifying threads, repeated coalesced notification per target, concurrent session drive, final exactly-once consumption and stale-handle invalidation after session drop.

**First successful integration run:** [mixed-flow benchmark #43383811340](https://github.com/matthis-k/phenix-ai/actions/runs/43383811340) passed: 0 idle ~25.6 µs both modes; 16 idle 49.0→26.6 µs (1.84×); 64 idle 127.1→27.9 µs (4.55×); 128 idle 225.8→29.4 µs (7.68×). Three interleaved single-CPU repetitions. Whole-process peak RSS varied ~3.7–3.9 MiB with no obvious mode separation; this does not prove allocation parity. Updated exact-head CI and concurrency test evidence must be checked before finalizing.

## Remaining acceptance and explicit nonclaims

1. **Exact HEAD / tests:** final #1291 CI, process-notifier stress and mixed example tests must pass, no format exceptions.
2. **Real agent harness:** benchmark through plugin-composed tool switch/SSH, persisted memory and skills; target no full scan on routine addressed events. External model/SSH latency will dominate many real workloads, so synthetic speedups must not be interpreted as end-to-end wins.
3. **Concurrency adversariality:** current stress tests are interleavings with real threads, not systematic model checking; lost wake race and cancellation/replacement should still receive targeted model checking or deterministic barriers.
4. **H6 memory/allocations:** pinned CPU/RSS evidence exists, but allocation *counts per turn* and representative concurrency are missing.
5. **M7:** overall agent composition, backpressure, fairness, cancellation of work and plugin lifecycle remain separate issues; H4 scheduling optimizations do not satisfy M7 by themselves.

**Provisional disposition:** no further H4 architecture concepts recommended solely to remove deliberate full-scan fallbacks. Once final #1291 CI and regression evidence pass, H4 can be treated as a *scoped scheduling optimization candidate*, not complete production agent acceptance. Reject changes that remove polling of legacy/custom policies or infer closure without authenticated provenance.
