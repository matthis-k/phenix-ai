# C10 — remove misleading completion alias

Status: Phase 0/1 design, 2026-11-12. Baseline `b6c79d42f2f52fda322d299db99617e12247a522`.

## Phase 0 — actual problem

`NetworkSession::run_until_complete(budget)` delegates unconditionally to
`run_until_idle(budget)`. Both return after bounded runnable steps, even when
inputs are still open, a job is handed out, output capture is full or no progress
is possible. The alias therefore promises an effect it does not implement.
Because the runtime is cooperative, blocking until actual completion would
violate nonblocking backpressure, wake and request semantics, and could hang
indefinitely when the host has not closed external input.

No change would leave a misleading public API with two names for one operation.
This is an API cleanup, not a runtime algorithm change.

## Phase 1 — alternatives

| Choice | Advantage | Cost | Decision |
| --- | --- | --- | --- |
| Baseline | No migration | Misleading completion guarantee | Reject |
| Add a looping complete driver | Intuitive name | Invents waiting/budgets, races external work and can hang | Reject |
| Rename to `run_until_quiescent` | More precise description | Additional alias/renaming with no new behavior | Reject |
| **Remove alias; use `run_until_idle`** | One canonical bounded executor, no duplicated API or semantic change | Existing callers must migrate | Select |

Update native examples and tests to `run_until_idle`. Assertions that
want `SessionState::Completed` must still explicitly check `report.state`.
No `run_until_complete` shim is retained (latest-only policy).

## Phase 4 gates

Remove only the alias and its callers. All bounded-stepping, EOS, backpressure,
cancellation and capture semantics must remain identical. No new runtime
scheduler, block-wait loop or parameter introduced. Verify public Rustdoc and
examples, complete workspace and no-default-features tests, strict rustfmt and
Clippy, and exact-head CI.
