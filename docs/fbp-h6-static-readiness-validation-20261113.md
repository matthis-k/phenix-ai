# H6: validate static readiness only at startup

2026-11-13. Base: H6 allocation measurement [#1294](https://github.com/matthis-k/phenix-ai/pull/1294), exact head `89e48b896e9687ffbb8bb92abde621d7bbd74d97`, six-job Flake CI [#43406458212](https://github.com/matthis-k/phenix-ai/actions/runs/43406458212), H6 allocation workflow [#43406457667](https://github.com/matthis-k/phenix-ai/actions/runs/43406457667), mixed-agent timing [#43406457743](https://github.com/matthis-k/phenix-ai/actions/runs/43406457743), all successful. This is another stacked draft, not merged into main.

## Phase 0 — source-level hot-path hypothesis

`NetworkSession::new()` validates all explicitly configured static policies against the compiled input interfaces before calling any plugin process factory. `Readiness::Any` (the default) needs no declaration validation. Nevertheless `NetworkSession::selection()` currently calls `validate_policy(index, p.waiting_policy.as_ref().unwrap_or(&p.policy))` on **every** readiness attempt. `validate_policy_ports` collects a fresh `BTreeSet<&str>` of input names before checking even `Any` or `Custom`; a nonempty port list therefore produces allocator traffic in every attempt. H6 measurements on the exact baseline showed ~530 / 496 allocation calls per notified local/remote warmed agent turn, with execution/readiness dominating. This is source-level attribution to a redundant allocation *path*, not a full allocation flamegraph.

The dynamic `Step::WaitInputs(Readiness)` path has a different lifetime: the process can set a policy after session creation and the kernel must continue to validate its declarations and return typed failure on illegal names/counts. Any optimization that skips this check is rejected.

## Phase 1 — alternatives and scoped change

| Approach | Benefit | Risk/cost | Decision |
| --- | --- | --- | --- |
| No change | Minimal diff | Continues allocating during readiness on prevalidated static configuration | Baseline |
| Memoize validation outcomes | Cache can cover dynamic policies | State and invalidation/identity complexity | Reject |
| Precompute all readiness reservations | Potentially fewer snapshot allocations | Invalidates closure, request and policy side-effect semantics | Reject |
| **Skip repeated static validation when `waiting_policy` is `None`** | Zero new fields; preserves startup preflight and full dynamic checking | Requires proof no post-start mutation of static policy | Select |

Concrete change in `selection()` only:
```rust
if let Some(policy) = &p.waiting_policy {
    self.validate_policy(index, policy)?;
}
```
`NetworkSession::new()` already validates `options.readiness`, but the default policy and optional overrides need inspection in code review. RuntimeProcess.policy is only initialized in `new`; code paths for `Step::WaitInputs` only assign `waiting_policy`. If these assumptions fail, return to Phase 1 instead of adding a shim.

## Phase 2 — adversarial critique / acceptance boundaries

- Reject if any dynamic WaitInputs missing port, duplicate Optional port, empty Optional declaration, or AtLeast overflow is no longer rejected.
- Reject if factory startup or any `NetworkSession::new()` path accepts previously invalid configured policy; all explicit entries must continue through the existing preflight.
- `Custom::select` remains invoked in every *eligible* selection; no purity, caching or notification promise is inferred. The hot-path change removes only an independent structural declaration validation.
- Never change `RuntimeProcess` or `NetworkSession` shape, request closure, EOS, request scope, dynamic policy, exception containment, or readiness snapshot contents. Proven absence of new state is a design advantage.
- Benchmarks must compile byte-identical agent/timing, EOS/chain/closure workloads against the exact baseline and candidate, use 7 interleaved single-CPU repetitions, record build/source SHAs and RSS; additionally compare allocation calls by route and phase. Avoid claiming total allocator cost from `System` requested-byte counts.
- Accept only if exact-head six-job CI and allocation/timing integrity tests pass and no sustained ~3–5%+ regression appears in notified, polled, EOS or closure primary cases. The primary intended result is less **warmed per-turn allocation**, not speculative wall-clock speedup.

## Phase 3 — implementation

Change `readiness.rs` one guard in `selection()`. Preserve `validate_policy_ports` and `validate_policy` for explicit static startup and dynamically staged policies. Add regression tests for the dynamic failure boundary, if existing coverage is incomplete. Add a new pinned paired A/B benchmark workflow reusing identical examples on both exact commits. No unrelated cleanup or backward-compatibility layers.

## Phase 4 — pending evidence

Record the exact candidate SHA, CI, per-route allocation deltas, paired latency medians and any regressions. Reject with evidence if acceptance fails. Passing tests alone do not establish M7 acceptance.
