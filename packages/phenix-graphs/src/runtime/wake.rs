use std::time::Duration;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
struct WakeState {
    // Every targeted and generic notification advances this epoch, preserving
    // WakeListener's observed-generation contract.
    epoch: u64,
    // Only an unaddressed notifier requires a graph-wide scheduler refresh.
    global_epoch: u64,
    active: bool,
    targets: BTreeSet<usize>,
}

#[derive(Default)]
pub(super) struct WakeSignal {
    state: Mutex<WakeState>,
    changed: Condvar,
}
impl WakeSignal {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(WakeState {
                active: true,
                ..WakeState::default()
            }),
            changed: Condvar::new(),
        })
    }

    // Drain before scanning. A targeted wake arriving after this lock is
    // released remains in targets for the next driver refresh.
    pub(super) fn take_scheduler_wakes(&self) -> (u64, BTreeSet<usize>) {
        let mut state = self.state.lock().unwrap();
        (state.global_epoch, std::mem::take(&mut state.targets))
    }

    // A targeted notification is a scheduler invalidation, not a process
    // activation. Only WakeHandle sets WakeSlot.pending; ProcessNotifier
    // reevaluates readiness without resuming a WaitingWake handoff.
    fn notify_target(&self, slot: &WakeSlot, process: usize, wake: bool) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.active || !slot.active.load(Ordering::Acquire) {
            return false;
        }
        if wake {
            slot.pending.store(true, Ordering::Release);
        }
        state.targets.insert(process);
        state.epoch = state.epoch.wrapping_add(1);
        self.changed.notify_all();
        true
    }

    // Retirement and notification share one lock: a stale handle either
    // notified before retirement (its target is discarded) or observes the
    // inactive slot afterward. Recovery reuses the compiled process index,
    // so clearing an old queued target prevents cross-generation delivery.
    pub(super) fn retire_target(&self, process: usize, slot: &WakeSlot) {
        let mut state = self.state.lock().unwrap();
        slot.active.store(false, Ordering::Release);
        slot.pending.store(false, Ordering::Release);
        state.targets.remove(&process);
    }

    // SessionNotifier has no process identity: retain full-scan semantics.
    fn notify(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.active {
            return false;
        }
        state.epoch = state.epoch.wrapping_add(1);
        state.global_epoch = state.global_epoch.wrapping_add(1);
        self.changed.notify_all();
        true
    }
    pub(super) fn invalidate(&self) {
        let mut state = self.state.lock().unwrap();
        state.active = false;
        state.targets.clear();
        self.changed.notify_all();
    }
}
pub(super) struct WakeSlot {
    pub pending: AtomicBool,
    pub active: AtomicBool,
}
impl Default for WakeSlot {
    fn default() -> Self {
        Self {
            pending: AtomicBool::new(false),
            active: AtomicBool::new(true),
        }
    }
}
#[derive(Clone)]
pub struct WakeHandle {
    pub(super) slot: Arc<WakeSlot>,
    pub(super) signal: Arc<WakeSignal>,
    pub(super) process: usize,
}
impl WakeHandle {
    /// False means the registration was invalidated by finish/cancellation/drop.
    pub fn wake(&self) -> bool {
        self.signal.notify_target(&self.slot, self.process, true)
    }
}

/// Invalidate one process's readiness without resuming its WaitWake handoff.
/// Obtain from NetworkSession::process_notifier for a physical process path.
/// Old handles are invalid after finish, request recovery or session terminal.
#[derive(Clone)]
pub struct ProcessNotifier {
    pub(super) slot: Arc<WakeSlot>,
    pub(super) signal: Arc<WakeSignal>,
    pub(super) process: usize,
}
impl ProcessNotifier {
    /// Returns false only if this process registration is no longer active.
    /// A true return does not claim that a process activation was scheduled.
    pub fn notify(&self) -> bool {
        self.signal.notify_target(&self.slot, self.process, false)
    }
}
/// Register before checking session state or a completion channel, then wait.
/// The epoch latch makes a notification between that check and wait observable.
pub struct WakeListener {
    signal: Arc<WakeSignal>,
    observed: u64,
}
impl WakeListener {
    pub(super) fn new(signal: Arc<WakeSignal>) -> Self {
        let observed = signal.state.lock().unwrap().epoch;
        Self { signal, observed }
    }
    pub fn wait_timeout(&mut self, timeout: Duration) -> bool {
        let state = self.signal.state.lock().unwrap();
        let (state, _) = self
            .signal
            .changed
            .wait_timeout_while(state, timeout, |s| s.active && s.epoch == self.observed)
            .unwrap();
        let notified = !state.active || state.epoch != self.observed;
        self.observed = state.epoch;
        notified
    }
}
#[derive(Clone)]
pub struct SessionNotifier(pub(super) Arc<WakeSignal>);
impl SessionNotifier {
    pub fn notify(&self) -> bool {
        self.0.notify()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Barrier, thread};

    #[test]
    fn targeted_notification_and_retirement_linearize_at_reused_process_index() {
        // Both operations take the *same* signal mutex. Across real concurrent
        // starts, either notification wins and is purged by retirement, or
        // retirement wins and the old registration refuses the notification.
        // Reusing the compiled index must not reuse the old WakeSlot.
        const PROCESS: usize = 3;
        for round in 0_usize..128 {
            let signal = WakeSignal::new();
            let old_slot = Arc::new(WakeSlot::default());
            let barrier = Arc::new(Barrier::new(3));
            let wants_wake = round.is_multiple_of(2);
            let notified = {
                let signal = signal.clone();
                let slot = old_slot.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    signal.notify_target(&slot, PROCESS, wants_wake)
                })
            };
            let retired = {
                let signal = signal.clone();
                let slot = old_slot.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    signal.retire_target(PROCESS, &slot);
                })
            };
            barrier.wait();
            let succeeded = notified.join().unwrap();
            retired.join().unwrap();
            assert!(!old_slot.active.load(Ordering::Acquire));
            assert!(!old_slot.pending.load(Ordering::Acquire));
            assert!(!signal.notify_target(&old_slot, PROCESS, true));
            let (epoch, pending) = signal.take_scheduler_wakes();
            assert_eq!(epoch, 0);
            assert!(pending.is_empty(), "round {round} retained an old target");
            assert_eq!(
                signal.state.lock().unwrap().epoch,
                u64::from(succeeded),
                "every successful target notification advances the listener epoch"
            );

            // A newly installed slot at the same compiled index has distinct
            // lifetime authority, regardless of which old operation won.
            let fresh = WakeSlot::default();
            assert!(signal.notify_target(&fresh, PROCESS, false));
            assert!(!signal.notify_target(&old_slot, PROCESS, true));
            let (_, targets) = signal.take_scheduler_wakes();
            assert_eq!(targets.into_iter().collect::<Vec<_>>(), vec![PROCESS]);
            assert!(!fresh.pending.load(Ordering::Acquire));
        }
    }

    #[test]
    fn listener_observes_notification_across_registration_to_wait_race() {
        // The barrier starts a waiter and notifier together. Notification
        // before the wait is caught by the epoch latch, while notification
        // during the condvar wait is caught by the condition variable.
        // A terminal invalidate wakes the listener without incrementing epoch.
        for mode in 0..4 {
            for _ in 0..24 {
                let signal = WakeSignal::new();
                let slot = Arc::new(WakeSlot::default());
                let mut listener = WakeListener::new(signal.clone());
                let barrier = Arc::new(Barrier::new(2));
                let sender = {
                    let signal = signal.clone();
                    let slot = slot.clone();
                    let barrier = barrier.clone();
                    thread::spawn(move || {
                        barrier.wait();
                        match mode {
                            0 => assert!(signal.notify_target(&slot, 1, false)),
                            1 => assert!(signal.notify_target(&slot, 1, true)),
                            2 => assert!(signal.notify()),
                            _ => signal.invalidate(),
                        }
                    })
                };
                barrier.wait();
                assert!(
                    listener.wait_timeout(Duration::from_secs(2)),
                    "listener missed mode {mode}"
                );
                sender.join().unwrap();
                if mode != 3 {
                    assert!(!listener.wait_timeout(Duration::ZERO));
                } else {
                    assert!(!signal.notify());
                }
            }
        }
    }
}
