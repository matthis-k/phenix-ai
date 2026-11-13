# H5: reuse the readiness closure proof within one selection

Date: 2026-11-13. Base: verified PR #1278, head `4eb584473818d86014f92f04806a3e22ed78863e`, [exact-head six-job CI #43329332426](https://github.com/matthis-k/phenix-ai/actions/runs/43329332426), [paired closure benchmark #43329331554](https://github.com/matthis-k/phenix-ai/actions/runs/43329331554).

## Phase 0 — problem and invariants

`NetworkSession::selection` first builds a `ReadinessSnapshot` for the candidate request by evaluating `input_closed_for(index, port, request)` on **every** input port. It later validates every port returned by the readiness policy with a *second identical* `input_closed_for` call. For sealed request-local optional closures, this means a repeated upstream DFS, including the fully reachable chain whose 128-node end-to-end request benchmark costs ~8.33 ms/request.

No mutable session operations occur between snapshot construction and selected-port validation. `selection(&self)` and `ReadinessPolicy::select(&self, &snapshot)` never mutate the session's queues, process handoffs, scope reachability or observed inputs. An external policy may mutate its own state, but cannot change `NetworkSession` through the provided snapshot API. Closure, EOS, capture and ownership invariants must remain unchanged.

## Phase 1 — alternatives

| Option | Advantage | Defect / tradeoff | Decision |
| --- | --- | --- | --- |
| No change | Zero implementation risk | Recomputes an expensive already-proven predicate in the same immutable selection | Reject |
| Add per-request closure cache to session | Amortizes across all process selections | Invalidation on dispatch, reserve, cancellation, wake and queue mutations; new mutable state | Defer |
| Precompute global transitive closure per request | Can accelerate many reachability proofs | Extra per-request memory and dynamic state, may grow O(V²); requires architecture review | Defer |
| **Use snapshot head `.closed` when validating a selected port** | Small local change, no new state; identical result for the same snapshot | O(port count) lookup, already used in policy handling; gains only where a legal selection occurs | **Select** |

The selected name is validated against the declared input port names as before. `ReadinessSnapshot.heads` includes exactly one entry for every declared port (in fairness-rotated order). Extract the corresponding snapshot `.closed` rather than invoking `input_closed_for` again. Retain the existing fresh `packet_for` lookup when reserving exact physical positions, to preserve FIFO and correlation semantics. All invalid selection errors, duplicate-port rejection and staged group behavior remain unchanged.

## Acceptance and nonclaims

- New test demonstrates a custom readiness policy selecting one request-local closed input with a sealed request, and a second test covers rejecting an open, empty port.
- Existing optional closure, multi-root, staged custom policy, FIFO/correlation, and cancellation regressions pass.
- Use the existing [paired closure workflow](../.github/workflows/fbp-h5-closure-benchmark.yml) with exact baseline #1278 and this candidate, plus sparse scheduling and H5 request-index controls. Record paired raw samples, not imagined speedups.
- No new public API, mutating cache, scheduler concept, authority or compatibility shim.
- The upstream DFS is still required once per candidate/input; `refresh()` remains O(process inventory). H5 and M7 remain incomplete.
