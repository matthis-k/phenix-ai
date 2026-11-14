# H4 targeted wake and EOS — revision protocol

## Phase 0: problem and constraints

Baseline: exact `b4c8c55cefa52f4fc0489db6fb2c6102d540d201` (#1286), verified full six-job CI and 4 performance workflows. The hybrid scheduler visits only dirty ordinary processes plus always-polled special processes. **But** `WakeHandle::wake()` shares the same global `WakeSignal` notification as `SessionNotifier::notify()`, so any targeted process wake still requests an O(V) full scan. A process `Finished` closes its outgoing writers/own readers but also forces full scan. Other finish-on-EOS paths retry full inventory until no process finishes, even if only a short successor chain is affected.

Constraints: same ready FIFO and deterministic ascending process evaluation; no speculative event admission; external wake and cancellation races must remain safe; finish closes writers, readers and invalidates prior wake handles; full-scan fallback for generic notifier, failure, recovery and graph state of uncertain provenance; no public graph, plugin or storage API changes.

## Phase 1: options and scope

| Option | Pros | Cons |
| --- | --- | --- |
| No change | Simple, proven correctness | O(V) per ordinary wake or EOS |
| Separate lock-free wake queue | Lightweight writer | Requires new wake queue/ring, ABA and race proof; duplicative wake state |
| Extend existing `WakeSignal` mutex with target indices | Same serialization as wake and listener; no new scheduling abstraction | BTreeSet insertion per wake; signal mutex contention; process index per handle |
| Universal event-indexed scheduler | Minimal visited nodes | Too much complexity for current H4 checkpoint |

Choose **extend existing wake signal**: its lock already serializes validity and listener epochs. Maintain an aggregate notification epoch for `WakeListener`, a separate global (unaddressed) notification epoch, and a bounded set of process indices woken by `WakeHandle`. Make the process wake's pending-bit mutation, targeted-set insert and notification-epoch bump atomic under the same lock. At start of every `refresh()`, drain target IDs under this lock **before** evaluating nodes; any wake arriving during the scan remains queued for the next refresh. Unaddressed `SessionNotifier` still drives full scan. A full-scan fallback is allowed to clear pending target indices because the whole graph is inspected.

For EOS, `finish(index)` already has exact outgoing and incoming queue IDs and calls transport close writer/reader. Collect those endpoints and dirty producer/consumer counterparts through the existing `dirty_queue` method before advancing the scheduler. A local fixed point runs further newly-dirtied processes after `Finished`, without rescanning unrelated processes. For a non-queued loop predecessor, preserve the prior repeat-only-on-finish semantics and only process the newly dirtied subset until no more terminal nodes finish. If the finished process was already done, no new invalidation needed. Do not optimize catastrophic failure or request cancellation/restart; they still force full scan. Any tricky intra-scan ordering, closure or all-special case falls back to original full pass. Initial release can implement targeted wake first and EOS second after paired evidence.

## Safety argument

- A targeted `WakeHandle` owns its original process index and `Arc<WakeSlot>`. If a request recovery replaces the `WakeSlot`, its old handle is invalidated; the new process still maps to the same compiled index. Invalidated handles cannot insert a target after `slot.active` is false (checked while holding the signal lock), nor after signal invalidation.
- A generic `SessionNotifier` does not say *which* node changed and MUST preserve the full scan.
- A producer may be in `WaitingWake`, `Idle`, or already have `Ready` work. The existing `WakeSlot.pending` bit and `refresh_process()` still decide progress. Target indexing never changes state or grants admission by itself.
- A wake after snapshot/drain remains in the bounded set and is observed on the next driver entry; it cannot be lost by an epoch check later in the scan. `WakeListener` continues seeing every notify (global or targeted) through the aggregate epoch and condvar.
- All EOS writer closure and reader disposal are performed by `finish`, not speculatively by the dirty index. Marking both endpoints is necessary for pending-publication retries and legal EOS reservations. Ascending dirty order preserves the existing first-pass ordering, and further passes are allowed **only** after actual finish.
- Unaddressed notifier, panic containment and cancellation safety are not weakened by optimization.

## Phase 3 regression and measurement

- Two distinct external handles target two waiting processes with 64+ unrelated ordinary nodes, verifying one is not spuriously resumed or forgotten.
- Generic `SessionNotifier` retains full fallback, while `WakeListener` receives notifications from both sources (no lost wake between subscribe/check/wait).
- Old wake handle after request recovery/finish/cancel returns false; new one identifies the replacement index.
- EOS chain, cyclic full bounded queue, pending publication and lossless capture remain correct.
- Release paired benchmark against the exact #1286 base, three interleaved repetitions on one runner, identical benchmark source, 1/16/64/256 processes; report wake target, unscoped ingress and optional-chain controls separately.
- Exact-head six-job Flake CI and code review before accepting each stage.

## Accepted limitations

WakeLock mutex remains; BTreeSet wake indices are bounded by compiled process count, not by number of wake calls; generic notifier and all-special compositions still scan O(V). EOS may require repeated dirty passes across long chains. `state(&self)` and request status retain their independent O(V) observations. These are scoped limitations, not completion of H4/M7.
