# H6 allocation events in a persistent FBP agent workload

Date: 2026-11-13. Base: verified #1292 exact head `b584e22656418c6ef61210d42000b6f68cb349ed` ([six-job CI #43396627873](https://github.com/matthis-k/phenix-ai/actions/runs/43396627873)). This branch is a draft measurement-only experiment. No runtime or scheduling behavior is changed.

## Phase 0 — problem and baseline

H4's paired CPU-pinned microbenchmarks show reduced idle polling in a synthetic but genuinely compiled, persistent 11-step agent graph. H6 still lacks event-level allocator accounting. Peak RSS includes allocator reuse, process mappings and startup; it cannot identify per-turn allocations. The recently rejected sorted-vector refresh candidate #1293 demonstrated that fewer possible data-structure allocations are not sufficient grounds for an optimization: notified, EOS and closure paths regressed.

Requirements: use the same compiled graph, request admission, notification, optional join, persistence, request settlement and release as `fbp_agent_mixed_benchmark.rs`; report polled and opt-in notified Custom readiness at 0/16/64/128 unrelated open service processes; separate graph construction and session start from **warmed** per-turn measurements. Preserve runtime invariants, normal benchmark timing and exact-parent traceability. Never infer causality or precise heap lifetime from raw allocation counts.

## Phase 1 — approaches

| Approach | Strength | Cost | Decision |
| --- | --- | --- | --- |
| Keep RSS only | No code | No allocation-event or hot-phase information | Reject |
| Count allocators inside runtime | Can attribute every call site | Alters production code and needs new tracing semantics | Reject for baseline |
| External heap profiler | Call stacks and object lifetimes | Tooling, symbols and runner reproducibility; extra profiling overhead | Follow-up after hotspot isolation |
| **Separate `System`-delegating global allocator benchmark executable** | Allocation and requested-byte events by graph lifecycle and per-turn phase, no runtime edits | Instrumentation overhead; no call-stack attribution, retained/live-byte proof or fair cross-machine timing | Select |

The executable includes the existing benchmark's graph construction as a Rust module, with only `compiled()` exposed to the sibling benchmark. It drives the same request path with the same checks and warmup count. It does not replace or alter the timed executable, and it does **not** compare instrumented wall-clock times.

## Phase 2 — adversarial critique and acceptance

1. Global allocator instruments the entire process; any spawned background threads would contaminate counts. This benchmark is single-threaded during measurement. Repeated independent processes and deterministic per-turn phases expose accidental variation; do not extrapolate to concurrent agents.
2. `GlobalAlloc::alloc` and `alloc_zeroed` count successful calls; `realloc` is reported separately, and requested bytes use the **new** allocation size, not net heap growth. Deallocation, allocator pooling, peak live bytes, Rust stack storage and external allocator internals are not represented.
3. Warmup (12 requests) uses the same instrumented phase brackets, but its samples are discarded. Count graph compilation, process/session construction and warmed steady-state turn phases separately. Each sampled turn includes admission, initial drain, any gated tool continuation, and status/release. Alternating local/remote requests are aggregated independently, not averaged across different work.
4. In the recorded baseline, the original benchmark's 11 owned steps per turn, request-local closure, exactly-once persistence and `Settled { captures: 0 }` remain asserted. A measurement failure is a failed acceptance run, not a zero-allocation result.
5. Aggregate `total`, `admission`, `first_drain`, `tool_resume`, `release` per route; require total equal the sum of phases. Print raw CSV including setup and sample counts; GitHub workflow records source SHA, CPU topology, `cargo --locked` release build and three independent repetitions. Count differences alone do not justify a runtime refactor.
6. An optimization candidate requires a fresh Phase 1/2, exact-head six-job CI, and equal-workload A/B performance runs including notified, legacy polled, EOS and H5 cases. Prefer eliminating workload-specific allocation in a measured hot phase over another general scheduler rewrite.

## Phase 3 — implementation scope

Add `fbp_agent_allocation_benchmark.rs`, a dedicated actions workflow, and a `pub(crate)` visibility on the existing benchmark's `compiled()` graph fixture. No production crate changes. The new benchmark's allocator counter is compiled into **only the new example executable**. The existing timed example remains behaviorally and structurally unchanged.

## Phase 4 — measured results and remaining checks

**Verified allocation workload:** corrected-source SHA `809f8e4355538eb7f73583c903e3e1a274f094bb`, [H6 workflow #43406006940](https://github.com/matthis-k/phenix-ai/actions/runs/43406006940) green. The earlier source compiled and measured successfully [#43405545024](https://github.com/matthis-k/phenix-ai/actions/runs/43405545024) but failed Flake CI formatting/Clippy; corrections changed neither graph topology nor allocation counts. All **six independently executed CSV traces** on the corrected source were byte-identical to the earlier traces and identical across three repetitions per mode. The workflow verified all phase totals and every request's 11 owned steps/settlement.

### Warmed allocation calls per turn (not timings)

| Idle unrelated services | Polled local | Notified local | Polled remote | Notified remote |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 533 | 530 | 499 | 496 |
| 16 | 1,283 | 530 | 1,231 | 496 |
| 64 | 3,664 | 530 | 3,529 | 496 |
| 128 | 6,767 | 530 | 6,535 | 496 |

For **notified** paths, the allocation breakdown per warmed turn is independent of 0/16/64/128 idle services:

| Phase | Local allocations | Remote allocations |
| --- | ---: | ---: |
| Request admission | 38 | 38 |
| First `run_until_idle` | 279 | 457 |
| Local permission/notification + resumed `run_until_idle` | 212 | 0 |
| Status + release | 1 | 1 |
| Total | 530 | 496 |

Both routes perform 37 `realloc` calls per turn. Notified local totals request 38,444 bytes per turn, remote 36,680; these are cumulative allocation **requests**, not retained or peak heap. At 128 unrelated services, the polled local path requests 499,508 bytes/turn versus 38,444 notified, largely due to readiness work in admission/drain/resume. This is an event-count comparison **within the same runtime**; no cross-commit speedup is inferred.

The exact corrected-source H4 mixed-agent timing workflow [#43406007204](https://github.com/matthis-k/phenix-ai/actions/runs/43406007204) also passed. The final corrected-source [six-job Flake CI #43406007630](https://github.com/matthis-k/phenix-ai/actions/runs/43406007630) must pass before claiming exact-head acceptance. No production runtime code was changed.

**Phase 4 conclusion:** The H6 *event-count measurement gap* is filled for this bounded single-threaded synthetic-agent workload. H6 overall remains **Partial**: no call-site allocation attribution, heap live/high-water semantics, concurrent multi-request agent budget, or production-model/SSH throughput acceptance. The next justified work is allocation-callsite profiling **inside execution/readiness**, not another unproven scheduler data-structure rewrite. Any proposed removal of the baseline ~530/496 calls per turn requires its own source attribution and pinned paired latency regression suite.
