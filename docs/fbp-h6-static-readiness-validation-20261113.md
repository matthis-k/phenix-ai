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
| Avoid input-name sets for all policy shapes that do not need them | Preserves original selection() call; AtLeast only counts, Optional still gets a name set | H5 fan-in and EOS regressions in measured variant 2 | **Rejected variant 2** |
| **Custom-only early return before name-set construction** | Eliminates legacy-Custom idle polling allocations; leaves default Any, Optional and AtLeast original code and validation | Does not optimize the preferred notified/default paths broadly | **Select revised variant 3, pending benchmarks** |

Revised implementation: keep the original `selection()` validation call **and restore the exact prior name-set path for all built-in readiness modes**. In `validate_policy_ports()`, return `Ok(())` immediately only for `Readiness::Custom(_)`, whose dynamically supplied selector function has no static port declarations to validate. This is still sound for staged `WaitInputs(Readiness::Custom(_))`: the selected ports remain checked against the real snapshot in `selection()` before reservation. `WaitInputs` with Optional/AtLeast retains the complete original structural preflight.

## Phase 2 — adversarial critique / acceptance boundaries

- Reject if any dynamic WaitInputs missing port, duplicate Optional port, empty Optional declaration, or AtLeast overflow is no longer rejected.
- Reject if factory startup or any `NetworkSession::new()` path accepts previously invalid configured policy; all explicit entries must continue through the existing preflight.
- `Custom::select` remains invoked in every *eligible* selection; no purity, caching or notification promise is inferred. The third hot-path change removes only the redundant port-name `BTreeSet` for a policy type that has **no structural declaration**. Other policy shapes preserve their original operations.
- Never change `RuntimeProcess` or `NetworkSession` shape, request closure, EOS, request scope, dynamic policy, exception containment, or readiness snapshot contents. The third variant leaves built-in `Any`/Optional/AtLeast declaration validation unchanged, including the `selection()` call graph. Proven absence of new state is a design advantage.
- Benchmarks must compile byte-identical agent/timing, EOS/chain/closure workloads against the exact baseline and candidate, use 7 interleaved single-CPU repetitions, record build/source SHAs and RSS; additionally compare allocation calls by route and phase. Avoid claiming total allocator cost from `System` requested-byte counts.
- Accept only if exact-head six-job CI and allocation/timing integrity tests pass and no sustained ~3–5%+ regression appears in notified, polled, EOS or closure primary cases. The primary intended result is less **warmed per-turn allocation**, not speculative wall-clock speedup.

## Phase 3 — implementation

**Revised Phase 3:** Return early for `Readiness::Custom` before collecting the names set; preserve `selection()`'s validator call and all built-in policy validation. Regression tests cover dynamic `WaitInputs` declaration failures. A pinned paired A/B benchmark workflow reuses identical examples on exact baseline and candidate commits. No unrelated cleanup or backward-compatibility layers.

## Phase 4 — initial candidate rejected and new variant pending

**Variant 1:** candidate `c1eeb946ea7500e40773e72c30b15def35f9f599`, [paired workflow #43408452642](https://github.com/matthis-k/phenix-ai/actions/runs/43408452642), seven interleaved single-CPU repetitions. Agent latency improved 7.0–25.3% across all polled/notified sample sizes. Allocation counts improved 16–19% on notified turns and ~60% on large legacy-polled turns. However **sparse EOS with 256 unrelated processes regressed on all 7 paired comparisons**, median +8.3%; +7.0% comparing separate medians. Under the predeclared threshold this is **rejected as-is**, despite H5 chain improvements.

**Variant 2:** `f2fef4b93afbf23d3e60edee2039f30dfc4e1b93`, [paired workflow #43409126845](https://github.com/matthis-k/phenix-ai/actions/runs/43409126845). Reduces notified allocation calls from 530/496 to 472/428 and large-polled from 6767 to 2739; 7-rep agent timings improved 1.0–17.5%. **Rejected:** H5 fan-in/64 `mean_ns_per_request` regressed ~10.8% comparing medians (10.9% median paired), and sparse EOS/256 regressed 5.1% comparing medians. The candidate preserved semantics, but failed the stated throughput guard.

**Variant 3:** Custom-only validation fast path, retains the source form of every built-in policy's validation. Requires fresh exact-head six-job CI and pinned A/B (same verified baseline). If no acceptable improvement remains, reject rather than weaken M7 performance criteria.

The benchmark suite also preserves raw RSS and all EOS/closure cases. Passing tests alone does not establish M7 acceptance.
