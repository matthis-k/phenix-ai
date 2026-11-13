# H5 delivered-closure pruning — independent Phase 2 adversarial review

Reviewed [design](fbp-h5-delivered-closure-design-20261113.md) separately from implementation.

## Attacks and resolutions

1. **Observed packet != absent-input closure.** `observed_inputs` records both. Reusing it would falsely prune ancestors after a data event while another same-request packet could still be pending. **Required:** a separate set populated exclusively for a selected `None` queue when `selected.request=Some(id)` and `!global_eos`. Never infer proof from existing `observed_inputs`.
2. **Other ports of the same producer remain live.** Skipping all predecessors after one proven-closed port is unsound. **Required:** skip exactly the recorded `(producer, input_port_index)`, and still check the producer's `p.request == Some(id)`, `closure_obligation`, all nonclosed input queues and their predecessors.
3. **Multiple roots admitted before seal.** A second root can feed the same downstream scope and must be visible during the original proof. **Required:** do not record any request-local closure until sealed; retain `request_input_closed` rooted on full registered reachability, and existing multi-root regression unchanged.
4. **A later different request traverses same process.** Must not wake or "reopen" the closed first request, but must make full progress for the new ID. **Required:** per-request evidence, remove with `release_request`, and test interleaved/repeated request IDs on a persistent session.
5. **Cycle and deferred worker.** A request-owned outstanding activation anywhere in the transitive upstream (including cycles) must prevent first closure. **Required:** retain the current exhaustive proof before recording; check owner before skipping cached port; no new synthetic short-circuit on arbitrary transient negative state.
6. **Global EOS.** A globally closed port is already handled by `self.closed`, and its lifecycle is distinct from a request-local absence. **Required:** do not record it in delivered-request evidence; preserve global `eos_delivered`.
7. **Custom policy and staged WaitInput.** A policy may select a closed input without a packet, but cannot make that closure reappear. **Required:** place evidence at the reservation boundary after validated `selection`, not in policy code, and preserve `closed_in_activation` behavior.
8. **Memory / API.** A new set adds sparse memory proportional to actually delivered request-local closure events, bounded by the compiled per-process input count and max live request scopes. **Required:** no speculative DFS cache or persistent `RefCell`, no public API, and no wrapper/compatibility shim.
9. **Soundness of monotonic closure.** Relies on authentic request IDs, a sealed request refusing new ingress/source roots, and process output request provenance being stamped from the owning activation. If those invariants change, this optimization must be revisited. No external plugin is allowed to synthesize a stamped request packet.

## Assessment

**Conditional acceptance for Phase 3:** the implementation must alter only the request lifecycle bookkeeping, reservation bookkeeping, and closure DFS; add regression evidence for multiple request IDs; preserve all existing tests. Phase 4 must measure exact-head changes. This is an optimization of accepted per-request closure proof, not permission to weaken correctness to gain throughput.

**Accepted limitations under protocol default:** still O(V) refresh inventory, still DFS for first/uncached closure, no fixed allocator/RSS budget from this patch, and no general static liveness guarantee. They do not contradict this scoped optimization's primary semantics. No unreviewed semantic limitation is accepted.
