# Staged Poll Lifecycle

## Constraints

Build on draft #1386, `ab7f5e93de460e6c472f847f7b7dbc62319907e7`.
Preserve H1 authenticated staged selection, cooperative execution, ownership,
EOS, request-local closure and recovery. Unresolved staged input waits must
remain polled: upstream completion can establish closure without a queue event.
Sources, inputless processes and legacy Custom policies still require polling.
No new scheduler, public API, per-request process instances, cache, or C7 change.

## Design

Baseline: every process that ever stages an input wait remains in `always_poll`
for its lifetime. This avoids removal but makes unrelated persistent turns pay
for historical waits. On the unchanged baseline, three local pinned-CPU runs
measured median run-mean jobs-driver latency of 2.568/5.539/22.898 microseconds
at 16/64/256 completed staged processes, versus 1.410/1.535/1.402 for identical
never-staged controls. Setup is excluded. This is not a whole-product budget.

Incremental choice: synchronize existing poll-set membership with base policy,
current staged wait, and completion. Use the existing set and dirty/full-refresh
mechanisms, not another staged index or reference count. Remove membership only
after successful input resumption, request detachment, or process finish.

Alternative: eliminate polling even during staged waits using reverse closure
dependencies. That could reduce more work but would introduce new invalidation
state and a substantially larger closure-correctness obligation. Defer it.

## Adversarial Critique

- A staged wait can become closed without receiving a packet. Retain polling
  until reservation actually resumes it, including Custom/group waits.
- A base Custom/source policy must not lose its polling when a wait ends. Base
  classification uses the already-snapshotted notification promise, never another
  plugin callback.
- Full-scan mode suppresses dirty insertion. Removing mandatory polling can cross
  the density threshold after relevant dirty events were discarded. Force one
  full refresh on that transition; never assume an empty dirty set is complete.
- Removal inside a full scan must not erase that resynchronization request.
- Cancellation must detach old wait membership before fresh process replacement;
  finished processes need no polling even if their historical wait fields remain.
- Preserve generic wake full scans, sealed Optional/notified watches, outstanding
  jobs, request capture ownership, queue bounds and explicit failure policies.

Independent preimplementation review found the same full-to-sparse hazard on
normal request release: removing the final scoped readiness watchers can expose
previously suppressed dirty events. The implementation must latch a full refresh
around that removal too, not only around staged membership removal. This is a
correctness fix using the same density-transition invariant, not another scheduler.

## Implementation And Review

Implemented one pure polling predicate and synchronization of the existing set
after successful staged reservation, request detachment and finish. Ordinary
idle admissions do not incur membership maintenance. Both mandatory-set shrink
paths latch resynchronization. No additional persistent state was introduced.

Five deterministic tests cover exact/group/Custom waits, both queue and failure
policies, base polling, finish, cancellation/reuse and both density crossovers.
Negative controls on the unchanged baseline fail for historical membership;
removing either crossover latch separately fails its intended regression.
Local default/no-default targeted suites, strict Graph Clippy and formatting
pass. All eleven inherited public H1 tests and worker recovery pass. A separate
read-only final source review found no actionable correctness defects. Its noted
ordinary-admission maintenance overhead was removed before this candidate.

Paired hosted measurements and exact-head CI remain pending. Publishing the new
paired workflow is blocked because the current GitHub OAuth token lacks
`workflow` scope. The prepared workflow is preserved on local branch
`fix/fbp-staged-poll-lifecycle-20261123` at `2c379943`; this source-only branch
publishes no workflow changes and can use the existing verification suite.
This evidence
does not close H4/H6, accept wider concurrency schedules, or change percentages.

First source `1a6d702b9` passed all twelve triggered workflows, including all six
jobs of [Flake CI 45194928320](https://github.com/matthis-k/phenix-ai/actions/runs/45194928320).
The follow-up moves maintenance into the existing successful staged-resume branch,
removing the extra ordinary-admission comparison and redundant wait-field stores.
Initial fields, successful resume and request detachment are the only routes to
Idle; none can leave an unresolved wait there. Library/H1/lane tests still pass.
This changed source requires fresh exact-head CI, not the earlier head's results.

## Final Local Paired Measurement

Six alternating baseline/candidate pairs used identical prebuilt harnesses,
CPU 0 affinity, 30,000 measured turns per case, alternating history order and a
10-second cooldown before each process. Setup and 64 warmup turns are excluded.
Baseline is unchanged `ab7f5e93`; candidate scheduler SHA-256 is
`e7f132aa31753f9398927739ffb4d1ce3063941b8eac57df89238de79281588a`.
Numbers are medians of run means in microseconds, not pooled turn statistics.

| Driver | Idle pairs | Baseline staged | Candidate staged | Baseline fresh | Candidate fresh |
| --- | ---: | ---: | ---: | ---: | ---: |
| Jobs | 16 | 2.443 | 1.613 | 1.550 | 1.683 |
| Jobs | 64 | 5.338 | 1.490 | 1.621 | 1.531 |
| Jobs | 256 | 24.154 | 1.430 | 1.567 | 1.493 |
| Report | 16 | 2.597 | 1.616 | 1.805 | 1.734 |
| Report | 64 | 6.312 | 2.271 | 2.386 | 2.394 |
| Report | 256 | 28.456 | 6.308 | 6.450 | 6.415 |

Every staged scenario improved in all six pairs. At 256 pairs, reciprocal-mean
throughput improves about 16.9x for jobs and 4.5x for reports. Jobs staged 256/16
latency growth falls from 9.89x to 0.89x; reports retain the separate state scan.
Median peak RSS is 5,884/5,832 KiB baseline/candidate, not live-heap attribution.
No allocation-event measurements were made for this change.

Fresh jobs/16 is 8.6% slower in aggregate mean (paired median +7.2%, slower in
four of six pairs). Other mean differences range from -5.6% to +0.3%. This is
not blanket performance acceptance. Even with no observed compiler processes,
baseline sampled temperature reached 98 C versus 95 C for candidate; 314 thermal
throttle events occurred during baseline and none during candidate. These are
confounds, not grounds to discard results or declare small differences noise.
Earlier 3,000/100,000-turn experiments showed different adverse controls and are
retained rather than replaced by a favorable run. No post-hoc product budget is
introduced. The deterministic acceptance here is removal of historical poll
membership while preserving readiness/invalidation, not a universal timing SLO.

Raw emitted CSV summaries, resources, provenance, thermal/process snapshots and
aggregation are in `/tmp/opencode/fbp-staged-poll-final-paired-20261123/`.
Earlier runs remain in `fbp-staged-poll-paired-local-20261123-1500/` and
`fbp-staged-poll-paired-highsample-20261123/` under `/tmp/opencode`.

Baseline raw local evidence is in
`/tmp/opencode/fbp-h4-staged-baseline-20261123/artifacts/`; the paired workflow
pins the same source and is designed to preserve raw hosted artifacts, affinity,
RSS and source/harness hashes once publishing is authorized. RSS is whole-process
peak, not live-heap attribution.
