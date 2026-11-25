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
