//! Wake-driven published generations reuse the existing producer and wake protocol.
use phenix_graph::{LocalExecutor, Value};
use phenix_kernel::host::{Candidate, KernelHost, LifecycleError};
use phenix_kernel::{BuildOptions, Contribution, DriverError, Key, Notice, Producer, WakeHandle};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::thread;
use std::time::Duration;

#[derive(Default)]
struct Queue {
    notices: VecDeque<Notice>,
    wake: Option<WakeHandle>,
}

#[derive(Clone)]
struct Channel {
    queue: Arc<Mutex<Queue>>,
    polls: Arc<AtomicUsize>,
    registrations: Arc<AtomicUsize>,
    accepts_wake: bool,
    eager_wake: bool,
    drop_gate: Option<Arc<(Barrier, Barrier)>>,
}

impl Channel {
    fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Queue::default())),
            polls: Arc::new(AtomicUsize::new(0)),
            registrations: Arc::new(AtomicUsize::new(0)),
            accepts_wake: true,
            eager_wake: false,
            drop_gate: None,
        }
    }

    fn push(&self, value: i64) {
        let wake = {
            let mut queue = self.queue.lock().unwrap();
            queue.notices.push_back(Notice {
                trigger: "run".into(),
                values: vec![Value::local(value)],
            });
            queue.wake.clone()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

struct Source(Channel);

impl Producer for Source {
    fn poll(&mut self) -> Result<Option<Notice>, phenix_kernel::CallbackError> {
        self.0.polls.fetch_add(1, Ordering::SeqCst);
        Ok(self.0.queue.lock().unwrap().notices.pop_front())
    }

    fn set_waker(&mut self, wake: WakeHandle) -> bool {
        if !self.0.accepts_wake {
            return false;
        }
        let ready = {
            let mut queue = self.0.queue.lock().unwrap();
            queue.wake = Some(wake.clone());
            !queue.notices.is_empty()
        };
        self.0.registrations.fetch_add(1, Ordering::SeqCst);
        if ready || self.0.eager_wake {
            wake.wake();
        }
        true
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if let Some(gate) = &self.0.drop_gate {
            gate.0.wait();
            gate.1.wait();
        }
    }
}

struct Service(Channel);

impl PluginInstance for Service {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let source = self.0.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec!["run".into()],
            compose: Box::new(move |cx| {
                let node = cx.graph.node("value", |x: i64| x + 1)?;
                let input = cx.graph.expose_input("input", node.inlet())?;
                cx.graph.trigger("run", [input])?;
                let source = source.clone();
                cx.activate(move || Ok(Box::new(Source(source.clone()))))?;
                Ok(())
            }),
        })
    }
}

fn prepare(host: &KernelHost, stages: Vec<StagedPlugin>) -> Result<Candidate, LifecycleError> {
    host.prepare(
        stages,
        BuildOptions::default(),
        Arc::new(LocalExecutor),
        "wake-generation",
        |_| Ok(()),
    )
}

#[test]
fn wake_token_does_not_lose_prior_wake_and_shutdown_is_sticky() {
    let wake = WakeHandle::default();
    let old = wake.version();
    wake.wake();
    assert!(wake.wait_since(old)); // A wake before wait must not be lost.

    let token = wake.version();
    let waiter = wake.clone();
    let (tx, rx) = mpsc::channel();
    let thread = thread::spawn(move || tx.send(waiter.wait_since(token)).unwrap());
    assert!(rx.recv_timeout(Duration::from_millis(40)).is_err()); // Does not spin.
    wake.shutdown();
    assert!(!rx.recv_timeout(Duration::from_secs(2)).unwrap());
    thread.join().unwrap();
    wake.wake();
    assert!(!wake.wait_since(token)); // Shutdown wins even after a further wake.
}

#[test]
fn candidate_registers_before_publication_but_admits_only_afterward() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("service", |c: Channel| Ok(Service(c)))
        .unwrap();
    let wake = WakeHandle::default();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
    let channel = Channel::new();
    channel.push(41); // Already ready before registration.
    let before = wake.version();
    let stage = host.plugins().prepare_load(&key, channel.clone()).unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    assert_eq!(channel.registrations.load(Ordering::SeqCst), 1);
    assert_eq!(channel.polls.load(Ordering::SeqCst), 0);
    assert!(wake.wait_since(before)); // set_waker observed pending notice.

    let token = wake.version();
    let active = host.publish(candidate).unwrap();
    assert!(wake.wait_since(token)); // Publication is independently observable.
    assert_eq!(channel.polls.load(Ordering::SeqCst), 0);
    let events = active.drain(8);
    assert_eq!(events.len(), 1);
    assert!(events[0].result.as_ref().unwrap().success());
    assert!(
        active
            .fire("run", vec![Value::local(1_i64)])
            .unwrap()
            .success()
    );
}

#[test]
fn failed_wake_registration_keeps_old_generation_active() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("service", |c: Channel| Ok(Service(c)))
        .unwrap();
    let wake = WakeHandle::default();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
    let original = Channel::new();
    let stage = host.plugins().prepare_load(&key, original.clone()).unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let old = host.publish(candidate).unwrap();

    let mut unsupported = Channel::new();
    unsupported.accepts_wake = false;
    let stage = host
        .plugins()
        .prepare_reload(&key, unsupported.clone())
        .unwrap();
    match prepare(&host, vec![stage]) {
        Err(LifecycleError::Driver(DriverError::UnsupportedWake { producer })) => {
            assert_eq!(producer, Key::new("service", "main"));
        }
        _ => panic!("expected typed unsupported-wake error"),
    }
    assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
    assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);

    original.push(7);
    assert_eq!(old.drain(8).len(), 1);

    let shutdown = WakeHandle::default();
    shutdown.shutdown();
    let other = KernelHost::with_wake(catalog.instance(), shutdown);
    let stage = other.plugins().prepare_load(&key, Channel::new()).unwrap();
    assert!(matches!(
        prepare(&other, vec![stage]),
        Err(LifecycleError::Driver(DriverError::WakeShutdown))
    ));
    assert!(other.active().is_none());
}

#[test]
fn reload_wakes_a_driver_that_previously_observed_the_old_generation() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("service", |c: Channel| Ok(Service(c)))
        .unwrap();
    let wake = WakeHandle::default();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
    let original = Channel::new();
    let stage = host.plugins().prepare_load(&key, original.clone()).unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let old = host.publish(candidate).unwrap();
    let token = wake.version();
    let observed = host.active().unwrap();

    let replacement = Channel::new();
    let stage = host
        .plugins()
        .prepare_reload(&key, replacement.clone())
        .unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let new = host.publish(candidate).unwrap();

    assert!(wake.wait_since(token)); // Cannot sleep on a stale active pointer.
    assert!(observed.drain(8).is_empty());
    replacement.push(9);
    let events = host.active().unwrap().drain(8);
    assert_eq!(events.len(), 1);
    assert!(events[0].result.as_ref().unwrap().success());
    assert!(Arc::ptr_eq(&new, &host.active().unwrap()));
    assert!(
        old.fire("run", vec![Value::local(1_i64)])
            .unwrap()
            .success()
    );
}

#[test]
fn publication_signal_precedes_stalled_old_producer_destruction() {
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("service", |c: Channel| Ok(Service(c)))
        .unwrap();
    let wake = WakeHandle::default();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());

    let gate = Arc::new((Barrier::new(2), Barrier::new(2)));
    let mut old_channel = Channel::new();
    old_channel.drop_gate = Some(gate.clone());
    let stage = host.plugins().prepare_load(&key, old_channel).unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let _old = host.publish(candidate).unwrap();
    let token = wake.version();

    let host = Arc::new(Mutex::new(host));
    let publishing = host.clone();
    let new_key = key.clone();
    let worker = thread::spawn(move || {
        let mut host = publishing.lock().unwrap();
        let stage = host
            .plugins()
            .prepare_reload(&new_key, Channel::new())
            .unwrap();
        let candidate = prepare(&host, vec![stage]).unwrap();
        host.publish(candidate).unwrap()
    });

    gate.0.wait(); // Publish entered the old source's destructor.
    assert_ne!(wake.version(), token); // It must have signalled the new generation.
    gate.1.wait();
    let new = worker.join().unwrap();
    assert!(Arc::ptr_eq(&new, &host.lock().unwrap().active().unwrap()));
}
