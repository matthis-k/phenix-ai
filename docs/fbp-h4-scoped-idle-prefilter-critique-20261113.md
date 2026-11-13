# H4 scoped idle readiness prefilter — independent Phase 2 adversarial critique

Date: 2026-11-13. Review of `fbp-h4-scoped-idle-prefilter-design-20261113.md`.

1. **Scope is authoritative only for request-local work.** The set is expanded by every successful scoped ingress/source start. An unrelated idle process cannot receive that request without a future explicit root admission, and physical input absence means no current unscoped work. Do not infer permission or trust from the set.
2. **Unsealed requests:** Membership must consider all scopes, including unsealed, because a request may add new ingress roots before sealing. The prefilter is re-evaluated on every refresh after successful admission, so a new root becomes eligible immediately.
3. **Multiple roots and scopes:** Skip only if *none* of the active scopes reaches the process. Never use a single `next_request` or one arbitrarily chosen scope.
4. **Staged waiting:** The existing no-scopes optimization permits some globally waiting inputs to skip when no packets/EOS can occur. Do not extend the new scoped optimization to `WaitingInput` or an existing `p.request` owner; staging policies may require errors/closure checks without queued data.
5. **Custom policies:** Even empty buffers can be reevaluated on explicit driver passes. Continue excluding built-in `Readiness::Custom` and a current `waiting_policy = Custom`.
6. **Sources and wakes:** A source can run without an input and a wake may change readiness. Keep `p.source` excluded; never skip `WaitingWake`, `Ready` or a pending publication.
7. **EOS and trapped packets:** All queues must be empty **and writer-open**, with at least one queue per port. A closed empty queue needs EOS propagation. Any packet, even foreign/unscoped, is enough to retain the full path. Original failure handling must execute normally when relevant.
8. **Global and cross-request semantics:** A live request on an unrelated branch must not suppress a global unscoped packet admitted to this process (queue not empty) or a later newly admitted request (root set updated). No implicit cancellation or temporary discard.
9. **Complexity limits:** The loop still scans O(V) and performs up to O(R log V) scope reachability checks per untouched idle node. The big win is avoiding `selection` and its per-process allocations. Record both sparse request gain and fully reachable-chain regression controls; do not call this event-indexed scheduling.
10. **No new abstraction:** One predicate branch within the existing `idle_inputs_cannot_progress` function; no second source of scheduling truth, no watcher, no stateful dirty bit, no compatibility shim. A future event-driven redesign requires a separate formal protocol.

Disposition: proceed only with the conservative extra rule. Phase 4 must verify exact diff, all relevant request/EOS/staged/custom regressions and paired source-verified CI. Leave H4 and M7 incomplete.

## Phase 2 re-review of full-scope short circuit

1. **Proof of coverage:** Each reachable set contains only compiled process IDs. `scope.reachable.len() == self.processes.len()` implies all compiled process IDs are present; the predicate is not a heuristic.
2. **Correct timing:** Compute at `refresh` entry. Roots cannot be added mid-refresh; public ingress/source-start operations record roots before invoking refresh. Finishing or reserving a process changes work but not the registered set.
3. **Multiple scopes:** If one scope alone covers all nodes, no process is unreachable by all scopes; bypassing scoped prefilter is safe. If their union (but no single scope) covers all nodes, extra membership checks may remain but the result is still safe. No global union cache is justified.
4. **Unscoped path:** `self.requests.is_empty()` retains its existing short circuit independent of the new boolean; no impact on the no-request prefilter or custom/EOS safeguards.
5. **Performance acceptance:** Initial 128-chain regression is a defect for this improvement's primary use cases, not an accepted tradeoff. Verify final-head paired chain/fan-in/sparse/unscoped evidence; if the regression persists, reconsider or reject. Do not claim faster entire graph scanning.
