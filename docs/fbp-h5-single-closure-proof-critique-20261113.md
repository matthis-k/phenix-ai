# H5 single-readiness-closure proof — Phase 2 independent critique

Date: 2026-11-13. Subject: `fbp-h5-single-closure-proof-design-20261113.md`.

1. **Snapshot identity:** `ReadinessSnapshot.heads` must include every process input exactly once, including multiple input queues and rotated fairness cursor. Match by port name, not by positional assumption.
2. **Custom readiness:** `ReadinessPolicy::select` receives an immutable snapshot and no mutable session reference. It might update external state, but the selected-port closure predicate depends solely on the session, which is unchanged during `selection`. Retain the original error for unknown names.
3. **Staged `WaitInput`:** It has a different single-port path; do not alter or conflate it with the normal snapshot path. Preserve its closed-and-observed validation.
4. **Fanout/correlation:** `packet_for` returning the actual queue/offset remains intact and is intentionally repeated as a fresh pre-reservation search. The prior `.closed` is only substituted for a duplicate boolean test.
5. **Stale/foreign requests:** Request admission and scope validation occur elsewhere. No request identity is created or authorized by the snapshot. Never use the snapshot for packet ownership.
6. **Failure precedence:** Duplicate selected input and unknown selected input continue to be rejected before any queue pop. An open empty input still yields invalid selection. Closure freshness is checked as before.
7. **Panic and reentrancy:** Custom readiness panic containment remains unchanged. The second closure traversal had no side effects; removing it cannot suppress required callbacks because closure predicates are pure.
8. **Performance claims:** A wholly reachable optional chain may still spend substantial time in initial snapshot closure, refresh's O(V) scheduling, process selection and post-step reevaluation. Expect only a bounded constant-factor reduction; benchmark results must determine magnitude and any regression.
9. **Accepted limitations:** O(port count) snapshot-name lookup replaces O(upstream ancestry) duplicate closure check; for shallow ports this could be neutral or slightly worse. The extra scan is negligible for deep closure chains but should be evaluated for many-port custom-policy workloads separately.

**Decision:** proceed with only the local substitution. Phase 4 must compare exact diff, complete six-job CI, paired fan-in/chain/sparse benchmarks, and preserve existing readied/invalid policy outcomes.
