use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Default)]
pub(super) struct WakeSignal {
    state: Mutex<(u64, bool)>,
    changed: Condvar,
}
impl WakeSignal {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((0, true)),
            changed: Condvar::new(),
        })
    }
    // An epoch read before scheduling ensures that a concurrent external wake
    // cannot be silently acknowledged after its process was already scanned.
    pub(super) fn epoch(&self) -> u64 {
        self.state.lock().unwrap().0
    }

    fn notify(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.1 {
            return false;
        }
        state.0 = state.0.wrapping_add(1);
        self.changed.notify_all();
        true
    }
    pub(super) fn invalidate(&self) {
        let mut state = self.state.lock().unwrap();
        state.1 = false;
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
}
impl WakeHandle {
    /// False means the registration was invalidated by finish/cancellation/drop.
    pub fn wake(&self) -> bool {
        let state = self.signal.state.lock().unwrap();
        if !state.1 || !self.slot.active.load(Ordering::Acquire) {
            return false;
        }
        self.slot.pending.store(true, Ordering::Release);
        drop(state);
        self.signal.notify()
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
        let observed = signal.state.lock().unwrap().0;
        Self { signal, observed }
    }
    pub fn wait_timeout(&mut self, timeout: Duration) -> bool {
        let state = self.signal.state.lock().unwrap();
        let (state, _) = self
            .signal
            .changed
            .wait_timeout_while(state, timeout, |s| s.1 && s.0 == self.observed)
            .unwrap();
        let notified = !state.1 || state.0 != self.observed;
        self.observed = state.0;
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
