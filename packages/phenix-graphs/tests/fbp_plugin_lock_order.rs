//! Q5/H3b: native plugin callbacks must never run while holding the
//! session's wake-signal mutex. A plugin may take its own mutex, while
//! another thread holds that mutex and notifies the session.
//!
//! Each scenario forces the callback to request the externally held lock
//! *before* the notifier acquires WakeSignal. If callback execution held
//! WakeSignal, the two threads would deadlock. Timeouts bound failures;
//! production runtime and callback interfaces stay unchanged.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{ActivationJob, DispatchResult, Process, ProcessContext, ProcessDispatcher, Step},
    runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, RequestFailurePolicy, RequestStatus,
        SessionNotifier, SessionOptions, SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(5);
const ROUNDS: usize = 12;

#[derive(Clone)]
struct PluginLockProbe {
    external: Arc<Mutex<()>>,
    armed: Arc<AtomicBool>,
    entered: Sender<()>,
}
impl PluginLockProbe {
    fn enter(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.send(()).expect("probe observer dropped");
            let _plugin_guard = self.external.lock().expect("plugin mutex poisoned");
        }
    }
}
fn probe() -> (PluginLockProbe, Receiver<()>) {
    let (entered, observed) = mpsc::channel();
    (
        PluginLockProbe {
            external: Arc::new(Mutex::new(())),
            armed: Arc::new(AtomicBool::new(false)),
            entered,
        },
        observed,
    )
}

/// Deterministically construct the lock-order challenge:
///
/// notifier: external lock -> session WakeSignal lock
/// driver:   plugin callback -> attempts same external lock
///
/// A driver holding WakeSignal across the callback would create a cycle.
/// Neither external mutex nor notifier is held when the main thread waits.
fn verify_external_lock_order(
    probe: &PluginLockProbe,
    observed: Receiver<()>,
    notifier: SessionNotifier,
    drive: impl FnOnce() + Send + 'static,
) {
    let (locked_tx, locked_rx) = mpsc::channel();
    let (fire_tx, fire_rx) = mpsc::channel();
    let external = probe.external.clone();
    let holder = thread::spawn(move || {
        let _plugin_guard = external.lock().unwrap();
        locked_tx.send(()).unwrap();
        fire_rx.recv_timeout(DEADLINE).unwrap();
        assert!(notifier.notify(), "live session notification refused");
    });
    locked_rx
        .recv_timeout(DEADLINE)
        .expect("notifier never acquired external lock");
    probe.armed.store(true, Ordering::SeqCst);
    let (done_tx, done_rx) = mpsc::channel();
    let driver = thread::spawn(move || {
        drive();
        done_tx.send(()).unwrap();
    });
    observed
        .recv_timeout(DEADLINE)
        .expect("plugin callback not reached while external lock was held");
    fire_tx.send(()).unwrap();
    done_rx
        .recv_timeout(DEADLINE)
        .expect("possible external-plugin/WakeSignal lock cycle");
    holder.join().unwrap();
    driver.join().unwrap();
}

struct CompleteInput;
impl Process for CompleteInput {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        context.take("in").ok_or("missing input")?.get::<u64>()?;
        Ok(Step::Complete)
    }
}

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

fn simple_network(
    factory: impl phenix_graph::process::ProcessFactory,
) -> phenix_graph::network::CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let worker = builder
        .node(
            "worker",
            Arc::new(Interface::new([PortDef::wire::<u64>("in").unwrap()], []).unwrap()),
            Arc::new(factory),
        )
        .unwrap();
    builder
        .expose_input("input", worker.wire_input::<u64>("in").unwrap(), one())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

struct LockingReadiness {
    probe: PluginLockProbe,
}
impl ReadinessPolicy for LockingReadiness {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.probe.enter();
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

#[test]
fn readiness_callback_does_not_hold_wake_mutex_while_entering_plugin_lock() {
    for _ in 0..ROUNDS {
        let (probe, observed) = probe();
        let mut options = SessionOptions::default();
        options.readiness.insert(
            "worker".into(),
            Readiness::Custom(Arc::new(LockingReadiness {
                probe: probe.clone(),
            })),
        );
        let mut session = simple_network(|| CompleteInput).start_with_options(options);
        assert!(session.failure().is_none());
        let notifier = session.notifier();
        verify_external_lock_order(&probe, observed, notifier, move || {
            session.inject("input", Value::of(11_u64)).unwrap();
            assert_eq!(session.run_until_idle(16).steps, 1);
            assert_eq!(session.state(), SessionState::Quiescent);
            assert!(session.failure().is_none());
        });
    }
}

struct LockingDispatcher {
    probe: PluginLockProbe,
}
impl ProcessDispatcher for LockingDispatcher {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        self.probe.enter();
        DispatchResult::Completed(job.run())
    }
}

#[test]
fn dispatcher_callback_does_not_hold_wake_mutex_while_entering_plugin_lock() {
    for _ in 0..ROUNDS {
        let (probe, observed) = probe();
        let options = SessionOptions {
            dispatcher: Arc::new(LockingDispatcher {
                probe: probe.clone(),
            }),
            ..SessionOptions::default()
        };
        let mut session = simple_network(|| CompleteInput).start_with_options(options);
        session.inject("input", Value::of(17_u64)).unwrap();
        let notifier = session.notifier();
        verify_external_lock_order(&probe, observed, notifier, move || {
            assert_eq!(session.run_until_idle(16).steps, 1);
            assert_eq!(session.state(), SessionState::Quiescent);
            assert!(session.failure().is_none());
        });
    }
}

struct RequestWaitWake {
    waiting: bool,
}
impl Process for RequestWaitWake {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if self.waiting {
            self.waiting = false;
            return Ok(Step::Complete);
        }
        context.take("in").ok_or("missing input")?.get::<u64>()?;
        self.waiting = true;
        Ok(Step::WaitWake)
    }
}

#[test]
fn recovery_factory_does_not_hold_wake_mutex_while_entering_plugin_lock() {
    for _ in 0..ROUNDS {
        let (probe, observed) = probe();
        let spawned = Arc::new(AtomicUsize::new(0));
        let factory_probe = probe.clone();
        let spawns = spawned.clone();
        let compiled = simple_network(move || {
            factory_probe.enter();
            spawns.fetch_add(1, Ordering::SeqCst);
            RequestWaitWake { waiting: false }
        });
        let mut session = compiled.start_with_options(SessionOptions {
            request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
            ..SessionOptions::default()
        });
        assert_eq!(spawned.load(Ordering::SeqCst), 1);
        let cancelled = session.begin_request(None).unwrap();
        session
            .inject_request(cancelled, "input", Value::of(21_u64))
            .unwrap();
        session.close_request(cancelled).unwrap();
        assert_eq!(session.run_until_idle(16).steps, 1);
        let old_wake = session.wake_handle("worker").unwrap();
        let notifier = session.notifier();

        verify_external_lock_order(&probe, observed, notifier, move || {
            assert!(session.cancel_request(cancelled).unwrap().is_empty());
            assert_eq!(
                session.request_status(cancelled).unwrap(),
                RequestStatus::Cancelled
            );
            assert!(!old_wake.wake());
            assert_eq!(spawned.load(Ordering::SeqCst), 2);
            let fresh = session.begin_request(None).unwrap();
            session
                .inject_request(fresh, "input", Value::of(22_u64))
                .unwrap();
            session.close_request(fresh).unwrap();
            assert_eq!(session.run_until_idle(16).steps, 1);
            assert!(session.wake_handle("worker").unwrap().wake());
            assert_eq!(session.run_until_idle(16).steps, 1);
            assert_eq!(
                session.request_status(fresh).unwrap(),
                RequestStatus::Settled { captures: 0 }
            );
            session.release_request(cancelled).unwrap();
            session.release_request(fresh).unwrap();
            assert!(session.failure().is_none());
        });
    }
}
