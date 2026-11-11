# Adversarial critique — FBP admission and startup purity

Status: Phase 2 independent challenge of [the follow-up design](fbp-followup-design-20261111.md); written before implementation.

1. **Is lazy readiness a semantic regression?** Moving refresh away from injection can delay invalid custom readiness failures and runnable-state changes. This is acceptable only if every public driver, state query and completion path still refreshes; an accepted packet must not require a second injection to make progress. Add a direct test that drives once after injection and observes the expected action. Keep explicit producer notifications.
2. **Is an enqueue-only path actually side-effect free?** `ConnectionSet::prepare` calls `EdgeAdapter` where adapters exist. Session ingress currently uses identity targets only, but the implementation must not claim universal transport preparation purity or apply this conclusion to arbitrary edge-adapter callbacks. The constrained invariant is **no user readiness/process callbacks in ingress admission**.
3. **Do scoped requests need immediate refresh?** `inject_request` shares admission. `close_request` and `state` still refresh; request counters must update on successful admission. No admitted packet may be lost or marked settled prematurely.
4. **Does validation actually run before factory code?** Checking options after `RuntimeProcess` construction does not pass. Preflight must use compiled interface metadata, validate paths and optional/threshold policies, and populate initial failure *before* the factory-spawning iterator starts. Do not run application-defined `ReadinessPolicy::select` during preflight.
5. **Are multiple faults deterministic?** Preserve one stable first diagnostic, and avoid invoking factories even when there are several invalid option entries. `SessionState::Failed` remains the public failure mechanism unless a separately approved API change occurs.
6. **Reentrancy is not universally solved.** `Producer::poll` remains under its mutex and `NetworkSession::refresh` runs under its session mutex. The patch eliminates only the demonstrated producer-lock → readiness callback path. Do not claim arbitrary callbacks are safe to reenter the host or session.
7. **Remaining core FBP blockers:** Strict FIFO correlation head blocking and private nested edits remain primary use-case gaps. This patch cannot classify them as accepted absent explicit use-case evidence. Their solutions must retain bounded transport, exact grants and pure plugin parity.

Phase 2 verdict: **conditionally accepted** under the stated acceptance tests and diff review. Do not change producer lifecycle synchronization or graph authority in this iteration; those would require a new Phase 1 design.


## Phase 2 revision — after actual execution evidence

The first implementation failed three pre-existing graph behavioral tests because
the loss of eager refresh changed queue credit and active cancellation, even though
transport packets were still accepted. A passing new test alone was insufficient.
The previous conditional acceptance is withdrawn for global lazy injection.

The revised design separates one **shared admission implementation** from two
explicitly documented scheduling choices. This adds a small public method, but
does not introduce a second transport authority, buffering implementation,
scheduler or runtime. Challenge it against these counterexamples:

- Eager `inject` must restore all three historical tests without adjusting
  expectations.
- `inject_request` must retain eager admission/active-request accounting;
  scoped work may not incorrectly settle while queued.
- Kernel producer admission must use only transport-only `enqueue`; neither
  readiness callbacks nor process steps may execute under the producer lock.
- The same `Full` ownership and atomicity properties must apply to both APIs.
- Calling `state` or running a driver after enqueue must activate the
  already accepted packet without a second injection.
- Any application invoking the explicitly deferred operation assumes
  responsibility for driving the session; the kernel's driver already does so.
  The internal producer `poll` callback reentrancy problem is still open.

**Verdict:** revised proposal accepted for Phase 3, conditional on unchanged
historical semantics and the exact-head regression matrix. Do not substitute
test rewriting for preserved behavior.
