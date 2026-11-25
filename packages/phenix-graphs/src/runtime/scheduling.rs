//! Existing dirty-process scheduling, full-scan fallback and wake accessors.
use super::*;

impl NetworkSession {
    pub fn wake_handle(&self, path: &str) -> Option<WakeHandle> {
        let index = self.compiled.physical_process_index(path)?;
        (!self.processes[index].done && self.terminal.is_none()).then(|| WakeHandle {
            slot: self.processes[index].wake.clone(),
            signal: self.signal.clone(),
            process: index,
        })
    }
    /// Addressed readiness invalidation for one compiled process. Unlike
    /// wake_handle(), this does not resume a process suspended in WaitWake.
    /// External notification-driven Custom policies should retain this handle
    /// and call notify() after a change affecting their selection.
    pub fn process_notifier(&self, path: &str) -> Option<ProcessNotifier> {
        let index = self.compiled.physical_process_index(path)?;
        (!self.processes[index].done && self.terminal.is_none()).then(|| ProcessNotifier {
            slot: self.processes[index].wake.clone(),
            signal: self.signal.clone(),
            process: index,
        })
    }
    pub fn wake_listener(&self) -> WakeListener {
        WakeListener::new(self.signal.clone())
    }
    pub fn notifier(&self) -> SessionNotifier {
        SessionNotifier(self.signal.clone())
    }

    pub(super) fn closed(&self, index: usize, port: usize) -> bool {
        self.first_non_eos_queue(index, port).is_none()
    }
    pub(super) fn first_non_eos_queue(&self, index: usize, port: usize) -> Option<usize> {
        self.processes[index].inputs[port]
            .queues
            .iter()
            .copied()
            .find(|q| !self.transport.queues()[*q].is_eos())
    }
    pub(super) fn idle_completion_after_empty_selection(&self, index: usize) -> bool {
        let p = &self.processes[index];
        p.handoff.state() == HandoffState::Idle
            && !p.source
            && p.inputs.iter().all(|port| port.eos_delivered)
    }
    // An idle process with empty, writer-open inputs has no native packet or
    // global EOS to reserve. An all-optional scoped closure is the remaining
    // zero-packet readiness path, and must retain its full selection.
    pub(super) fn idle_inputs_cannot_progress(&self, index: usize) -> bool {
        let p = &self.processes[index];
        !p.source
            && !p.queued
            && p.pending.is_none()
            && !p.inputs.is_empty()
            && (self.requests.is_empty()
                || (p.handoff.state() == HandoffState::Idle
                    && p.request.is_none()
                    && p.waiting.is_none()
                    && p.waiting_policy.is_none()
                    && !p.wake.pending.load(Ordering::Acquire)
                    // Only unobserved all-optional request closures can
                    // activate an idle process without physical packet data.
                    // Other policies need a packet or global EOS; reachability
                    // alone does not make them runnable.
                    && (!matches!(
                        &p.policy,
                        Readiness::Optional { required, .. } if required.is_empty()
                    ) || self
                        .requests
                        .keys()
                        .all(|request| !self.closure_obligation(index, *request)))))
            && !matches!(&p.policy, Readiness::Custom(_))
            && !p
                .waiting_policy
                .as_ref()
                .is_some_and(|policy| matches!(policy, Readiness::Custom(_)))
            && p.inputs.iter().all(|port| {
                !port.queues.is_empty()
                    && port.queues.iter().all(|queue| {
                        let queue = &self.transport.queues()[*queue];
                        queue.is_empty() && !queue.writer_closed()
                    })
            })
    }

    // Each sealed scope registers Optional and notified Custom policies it
    // can reach. Physical packets/EOS and targeted notifications cover those
    // policies outside a sealed scope.
    pub(super) fn unwatch_scoped_readiness(&mut self, reachable: &[usize]) {
        let was_full = self.prefer_full_refresh();
        for &index in reachable {
            if matches!(&self.processes[index].policy, Readiness::Optional { .. })
                || self.processes[index].notification_driven
            {
                let count = self
                    .scoped_readiness_poll
                    .get_mut(&index)
                    .expect("sealed readiness scope must have a registered watcher");
                *count -= 1;
                if *count == 0 {
                    self.scoped_readiness_poll.remove(&index);
                }
            }
        }
        self.force_full_refresh |= was_full && !self.prefer_full_refresh();
    }

    pub(super) fn sync_poll(&mut self, index: usize) {
        if self.processes[index].requires_poll() {
            self.always_poll.insert(index);
        } else {
            let was_full = self.prefer_full_refresh();
            self.always_poll.remove(&index);
            // Full mode discards dirty events. A shrinking mandatory set must
            // resynchronize before sparse scheduling can trust that set again.
            self.force_full_refresh |= was_full && !self.prefer_full_refresh();
        }
    }

    // A conservative upper bound on mandatory checks: a staged optional node
    // may appear in both sets. Overcounting only chooses a safe full scan.
    fn prefer_full_refresh(&self) -> bool {
        self.always_poll
            .len()
            .saturating_add(self.scoped_readiness_poll.len())
            >= self.processes.len().saturating_sub(1)
    }

    pub(super) fn dirty_process(&mut self, index: usize) {
        if !self.prefer_full_refresh() {
            self.dirty_processes.insert(index);
        }
    }

    pub(super) fn dirty_queue(&mut self, queue: usize) {
        if self.prefer_full_refresh() {
            return;
        }
        let producer = match &self.endpoints[queue].0 {
            QueueEndpoint::Process { index, .. } => Some(*index),
            _ => None,
        };
        let consumer = match &self.endpoints[queue].1 {
            QueueEndpoint::Process { index, .. } => Some(*index),
            _ => None,
        };
        if let Some(index) = producer {
            self.dirty_process(index);
        }
        if let Some(index) = consumer {
            self.dirty_process(index);
        }
    }

    #[inline(always)]
    fn refresh_process(&mut self, index: usize) -> Option<bool> {
        if self.processes[index].done {
            return Some(false);
        }
        let mut state = self.processes[index].handoff.state();
        if state == HandoffState::WaitingWake
            && self.processes[index]
                .wake
                .pending
                .swap(false, Ordering::AcqRel)
        {
            self.processes[index].handoff.resume_wake().unwrap();
            state = HandoffState::Ready;
        }
        if matches!(state, HandoffState::Idle | HandoffState::WaitingInput) {
            if self.idle_inputs_cannot_progress(index) {
                return Some(false);
            }
            let selected = match self.selection(index) {
                Ok(selected) => selected,
                Err(error) => {
                    self.fail_process(index, error);
                    return None;
                }
            };
            let wake = state == HandoffState::Idle
                && self.processes[index].source
                && self.processes[index]
                    .wake
                    .pending
                    .swap(false, Ordering::AcqRel);
            if !selected.ports.is_empty() || wake {
                let context = self.reserve(index, selected);
                let result = if state == HandoffState::Idle {
                    self.processes[index].handoff.admit(context)
                } else {
                    let result = self.processes[index].handoff.resume_input(context);
                    if result.is_ok() {
                        self.processes[index].waiting = None;
                        self.processes[index].waiting_policy = None;
                        self.sync_poll(index);
                    }
                    result
                };
                if let Err((error, _)) = result {
                    self.fail_process(
                        index,
                        Failure::external("process reservation failed", error)
                            .with_kind(FailureKind::Framework),
                    );
                    return None;
                }
            } else if self.idle_completion_after_empty_selection(index) {
                self.finish(index);
                if self.terminal.is_some() {
                    return None;
                }
                return Some(true);
            } else if self.stranded_at_eos(index) {
                self.fail_input(index, InputFailureReason::Stranded);
                return None;
            }
        }
        let p = &self.processes[index];
        let runnable = p.handoff.state() == HandoffState::Ready
            || (p.pending.is_some()
                && (p.pending_targets.iter().any(|q| {
                    self.transport.queues()[*q].reader_closed()
                        || self.transport.queues()[*q].writer_closed()
                }) || p
                    .pending_targets
                    .iter()
                    .all(|q| !self.transport.queues()[*q].is_full())));
        if runnable && !p.queued {
            self.processes[index].queued = true;
            self.ready.push_back(index);
        }
        Some(false)
    }

    // The original scan ordering and repeat-on-finish behavior remains the
    // correctness fallback for all non-packet-driven session semantics.
    fn refresh_all_processes(&mut self) {
        loop {
            let mut finished = false;
            for index in 0..self.processes.len() {
                let Some(did_finish) = self.refresh_process(index) else {
                    return;
                };
                finished |= did_finish;
                if self.terminal.is_some() {
                    return;
                }
            }
            if !finished {
                break;
            }
        }
    }

    pub(super) fn refresh(&mut self) {
        if self.terminal.is_some() {
            return;
        }
        // Drain targeted wakes atomically before processing scheduler work.
        // A subsequent wake remains queued for the next drive. Generic
        // SessionNotifier notifications still force the legacy full scan.
        let (global_epoch, targets) = self.signal.take_scheduler_wakes();
        if self.prefer_full_refresh() {
            self.observed_global_wake_epoch = global_epoch;
            self.dirty_processes.clear();
            self.refresh_all_processes();
            return;
        }
        for index in targets {
            self.dirty_process(index);
        }
        let complete = std::mem::take(&mut self.force_full_refresh)
            || global_epoch != self.observed_global_wake_epoch;
        self.observed_global_wake_epoch = global_epoch;
        if complete {
            self.dirty_processes.clear();
            self.refresh_all_processes();
            return;
        }
        let mut dirty = std::mem::take(&mut self.dirty_processes);
        // Legacy Custom/source nodes and unresolved staged waits stay polled.
        // Optional and notified Custom nodes additionally poll for
        // registered sealed scopes reaching them.
        dirty.extend(self.always_poll.iter().copied());
        dirty.extend(self.scoped_readiness_poll.keys().copied());
        loop {
            let mut finished = false;
            for index in dirty {
                let Some(did_finish) = self.refresh_process(index) else {
                    return;
                };
                finished |= did_finish;
                if self.terminal.is_some() {
                    return;
                }
            }
            if !finished {
                break;
            }
            // Finishing closes exactly its input readers and output writers.
            // Revisit only their producers/consumers and always-polled nodes
            // when EOS propagation requires another pass. A packet dequeue
            // without a finish stays pending for the next driver call.
            dirty = std::mem::take(&mut self.dirty_processes);
            dirty.extend(self.always_poll.iter().copied());
            dirty.extend(self.scoped_readiness_poll.keys().copied());
            if dirty.is_empty() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod staged_poll_tests;

#[cfg(test)]
mod indexed_process_paths_tests {
    use crate::{ComponentDef, NetworkBuilder};
    use std::{num::NonZeroUsize, sync::Arc};

    #[test]
    fn addressed_handles_resolve_only_exact_physical_processes() {
        let mut builder = NetworkBuilder::new();
        let first = builder
            .add("first", Arc::new(ComponentDef::map(|n: i64| n + 1)))
            .unwrap();
        let last = builder
            .add("last", Arc::new(ComponentDef::map(|n: i64| n + 2)))
            .unwrap();
        let one = NonZeroUsize::new(1).unwrap();
        builder
            .connect(
                first.output::<i64>("output").unwrap(),
                last.input::<i64>("input").unwrap(),
                one,
            )
            .unwrap();
        builder
            .expose_input("input", first.input::<i64>("input").unwrap(), one)
            .unwrap();
        builder
            .expose_output("output", last.output::<i64>("output").unwrap())
            .unwrap();
        let compiled = builder.build().unwrap().compile().unwrap();
        let session = compiled.start();
        for (index, process) in compiled.processes().iter().enumerate() {
            assert_eq!(compiled.physical_process_index(process.path()), Some(index));
            assert!(session.wake_handle(process.path()).is_some());
            assert!(session.process_notifier(process.path()).is_some());
        }
        assert_eq!(compiled.physical_process_index(""), None);
        assert_eq!(compiled.physical_process_index("missing"), None);
        assert!(session.wake_handle("").is_none());
        assert!(session.process_notifier("missing").is_none());
    }
}
