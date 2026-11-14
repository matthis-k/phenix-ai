# H4 — revision: hybrid packet-dirty / always-polled scheduler

## Phase 0 — lessons from packet-only trial

The packet-only dirty index in [original design](fbp-h4-packet-dirty-design-20261113.md) measured ~1.8× scoped and ~3.5× unscoped speedup at 256 independent processes, but it disabled itself for an entire session if **one** source, Custom readiness, Optional input, or staged input existed. This nullifies its primary benefit for realistic mixed agent loops. It also imposed ~5–10% synthetic fan-in overhead on fallback from the shared scheduler helper. A bounded improvement only on pure packet graphs is not sufficient M7 evidence.

## Phase 1 — baseline/extend/proposed

| Option | Benefit | Cost |
| --- | --- | --- |
| Baseline #1285 | No extra runtime state or fallback regression | O(V) every refresh |
| Extend packet-only candidate | Improves simple graphs | One special node disables optimization globally; unacceptable for mixed loops |
| **Hybrid static/dynamic always-poll set plus dirty packets** | Every special node retains legacy readiness polling; independent ordinary nodes are skipped | Small per-session poll set; union/sort of special+dirty indices; still O(S+D) and global scan on EOS/terminal |

Select hybrid. There is still one `refresh_process` body and a complete legacy-mode scan for every-node special graphs; no public API, cache, cross-session state, or additional authority.

## Invalidation/eligibility

A process is **always polled** when it is a compiled source, has no physical input, uses `Optional` or `Custom` readiness, or has ever returned `WaitInput` or `WaitInputs`. Dynamic staged membership persists for session lifetime; no removal/revalidation complexity. Other processes are considered when their physical queue endpoints are touched, their handoff completes/rejects or they become affected by cancellation/recovery/EOS.

Any global wake signal (WakeHandle or SessionNotifier) still triggers full scan on epoch change. `Finished`, cancellation/recovery and initial session setup force full legacy scan. If every process is in always-poll, use legacy full scan and avoid queue-dirty bookkeeping.

**Sealing an authenticated request:** only its registered `RequestScope.reachable` can change scoped readiness/closure status. Mark these node indices dirty; always-polled nodes are checked regardless. No full-process scan needed for unrelated nodes; multiple roots are already included in reachability and cross-request isolation is preserved. Request IDs are not guessed from packet contents.

For mixed sessions, each `refresh()` visits the ascending-order deduplicated union of dirty indices and always-polled indices. Existing ready FIFO is preserved, and a completion that changes global EOS still falls back to full repeat-until-finished scan. Each call may leave newly dirtied earlier nodes for the next refresh, matching the former non-finish, one-pass ordering.

## Acceptance / measurements

Run exact-head six-job CI, dedicated ingress/capture/backpressure/wake regressions, multi-root, all-optional and staged/custom request tests, and paired same-runner H4 sparse + H5 fan-in and long-chain comparisons against exact #1285. Test a **mixed graph** with a custom or all-optional branch plus many idle ordinary sinks; verify only the relevant request scopes execute. Reject if correctness or significant mixed-workload regressions appear.

No claim of full M7 performance, allocation budget, universal liveness or static cycle analysis; worst-case all-special or all-reachable still O(V), and passive status scans remain O(V).
