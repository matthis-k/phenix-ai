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
| Skip repeated static validation when `waiting_policy` is `None` | Zero new fields | May change hot-path layout; one EOS sparse/256 workload regressed +8.3% in paired sample | **Rejected variant 1** |
| **Avoid input-name sets for policy shapes that do not need them** | Preserves original selection() call/validation semantics; AtLeast only counts, Optional still gets a name set | `Optional` retains per-check set allocation | **Select revised variant 2** |

Revised implementation: keep the original `selection()` validation call, but move building `BTreeSet<&str>` inside the `Readiness::Optional` arm of `validate_policy_ports()`. `Readiness::AtLeast` checks `ports.count()` without an allocated set; `Any`, `All`, `CorrelatedAll` and `Custom` need neither set nor count. `WaitInputs` receives the **same** full structural checks. The only semantic change should be removing unnecessary input-name materialization, not skipping a policy validation invocation.

## Phase 2 — adversarial critique / acceptance boundaries

- Reject if any dynamic WaitInputs missing port, duplicate Optional port, empty Optional declaration, or AtLeast overflow is no longer rejected.
- Reject if factory startup or any `NetworkSession::new()` path accepts previously invalid configured policy; all explicit entries must continue through the existing preflight.
- `Custom::select` remains invoked in every *eligible* selection; no purity, caching or notification promise is inferred. The hot-path change removes only an independent structural declaration validation.
- Never change `RuntimeProcess` or `NetworkSession` shape, request closure, EOS, request scope, dynamic policy, exception containment, or readiness snapshot contents. The revised variant also retains the `selection()` call graph. Proven absence of new state is a design advantage.
- Benchmarks must compile byte-identical agent/timing, EOS/chain/closure workloads against the exact baseline and candidate, use 7 interleaved single-CPU repetitions, record build/source SHAs and RSS; additionally compare allocation calls by route and phase. Avoid claiming total allocator cost from `System` requested-byte counts.
- Accept only if exact-head six-job CI and allocation/timing integrity tests pass and no sustained ~3–5%+ regression appears in notified, polled, EOS or closure primary cases. The primary intended result is less **warmed per-turn allocation**, not speculative wall-clock speedup.

## Phase 3 — implementation

**Revised Phase 3:** Move the `BTreeSet` allocation inside `Readiness::Optional` validation and count `AtLeast` inputs without allocating; preserve `selection()`'s existing validator call. Add regression tests for the dynamic `WaitInputs` failure boundary, and a pinned paired A/B benchmark workflow reusing identical examples on both exact commits. No unrelated cleanup or backward-compatibility layers.

## Phase 4 — initial candidate rejected and new variant pending

**Variant 1:** candidate `c1eeb946ea7500e40773e72c30b15def35f9f599`, [paired workflow #43408452642](https://github.com/matthis-k/phenix-ai/actions/runs/43408452642), seven interleaved single-CPU repetitions. Agent latency improved 7.0–25.3% across all polled/notified sample sizes. Allocation counts improved 16–19% on notified turns and ~60% on large legacy-polled turns. However **sparse EOS with 256 unrelated processes regressed on all 7 paired comparisons**, median +8.3%; +7.0% comparing separate medians. Under the predeclared threshold this is **rejected as-is**, despite H5 chain improvements.

**Variant 2:** retain the original callsite validation; only avoid allocation inside `validate_policy_ports` for Any/All/Custom/CorrelatedAll/AtLeast. Requires fresh exact-head six-job CI and pinned A/B (same baseline) to be accepted. A source-code rewrite does not inherit variant 1's performance evidence.

The documented benchmark suite also preserves raw RSS and other cases. Passing tests alone does not establish M7 acceptance.
