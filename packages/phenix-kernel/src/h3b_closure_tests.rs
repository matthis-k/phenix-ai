//! Native compositions of the production boundaries, not a second executor.
use super::*;
use crate::{DriverError, ExclusiveSlot, InputEvent, Producer, SessionAccessError};
use phenix_graph::{ComponentDef, Value, network::NetworkBuilder, process::LocalProcessDispatcher};
use phenix_plugin::PluginCatalog;
use std::{
    cell::RefCell,
    sync::{
        Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const LIMIT: Duration = Duration::from_secs(5);

fn live_session<T: Send + Sync + 'static>() -> phenix_graph::runtime::NetworkSession {
    let mut builder = NetworkBuilder::new();
    let sink = builder
        .add("sink", Arc::new(ComponentDef::sink(|_: &T| {})))
        .unwrap();
    builder
        .expose_input(
            "in",
            sink.input::<T>("input").unwrap(),
            std::num::NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder.build().unwrap().compile().unwrap().start()
}

fn host_for(kernel: Kernel) -> (KernelHost, Arc<Generation>) {
    let generation = Arc::new(Generation { epoch: 1, kernel });
    let mut host = KernelHost::new(PluginCatalog::new().instance());
    host.epoch = 1;
    host.active = Some(generation.clone());
    (host, generation)
}

#[test]
fn queued_direct_drains_and_external_retirement_dispose_one_busy_or_full_event() {
    struct Payload(Arc<AtomicUsize>, Option<Weak<()>>);
    impl Drop for Payload {
        fn drop(&mut self) {
            if let Some(code) = &self.1 {
                assert!(
                    code.upgrade().is_some(),
                    "code lease released before event disposal"
                );
            }
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Source {
        kernel: Weak<Generation>,
        polls: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        payload: Arc<AtomicUsize>,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        code: Weak<()>,
    }
    impl Producer for Source {
        fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
            assert_eq!(
                self.polls.fetch_add(1, Ordering::SeqCst),
                0,
                "retained event was repolled"
            );
            self.entered.send(()).unwrap();
            self.release.recv_timeout(LIMIT).unwrap();
            Ok(Some(InputEvent {
                input: "in".into(),
                value: Value::local(Payload(self.payload.clone(), Some(self.code.clone()))),
            }))
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            let generation = self.kernel.upgrade().unwrap();
            assert!(!KernelLockScope::held(&generation.kernel.producer_drain));
            assert!(!crate::ActiveSessionCall::held(
                generation.kernel.session_identity
            ));
            assert!(
                self.code.upgrade().is_some(),
                "code lease released before producer"
            );
            // Other admitted threads may own the gate now. Acquiring inventory
            // here tests same-thread lock freedom, not global mutex availability.
            assert!(generation.kernel.producers.lock().unwrap().retired);
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    for full in [false, true] {
        for publish in [false, true] {
            let polls = Arc::new(AtomicUsize::new(0));
            let drops = Arc::new(AtomicUsize::new(0));
            let payload = Arc::new(AtomicUsize::new(0));
            let code = Arc::new(());
            let weak_code = Arc::downgrade(&code);
            let (entered, entry) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let generation = Arc::new_cyclic(|weak| {
                let mut kernel =
                    Kernel::build("queued-retirement", [], Arc::new(LocalProcessDispatcher))
                        .unwrap();
                kernel.start().unwrap();
                kernel.code_lease = code.clone();
                kernel.session = ExclusiveSlot::new(live_session::<Payload>());
                if full {
                    for _ in 0..2 {
                        kernel
                            .with_session(|session| {
                                session
                                    .inject("in", Value::local(Payload(Arc::default(), None)))
                                    .unwrap()
                            })
                            .unwrap();
                    }
                }
                let key = Key::new("test", "source");
                kernel
                    .routes
                    .insert(key.clone(), std::collections::BTreeSet::from(["in".into()]));
                kernel.producers.get_mut().unwrap().producers.push((
                    key,
                    Some(Box::new(Source {
                        kernel: weak.clone(),
                        polls: polls.clone(),
                        drops: drops.clone(),
                        payload: payload.clone(),
                        entered,
                        release: released,
                        code: weak_code.clone(),
                    })),
                    None,
                ));
                Generation { epoch: 1, kernel }
            });
            drop(code);
            let mut host = KernelHost::new(PluginCatalog::new().instance());
            host.epoch = 1;
            host.active = Some(generation.clone());
            let candidate = publish.then(|| {
                host.prepare(
                    vec![],
                    BuildOptions::default().with_dispatcher(Arc::new(LocalProcessDispatcher)),
                    "successor",
                    |_| Ok(()),
                )
                .unwrap()
            });
            // Busy must remain true for both drains; Full uses real ingress
            // backpressure instead. Neither contender may execute the session.
            let busy = (!full).then(|| generation.kernel.session.try_take().unwrap());
            let first = {
                let generation = generation.clone();
                // A second iteration must retry the retained event, even if
                // retirement wins the gate before the queued drain runs.
                std::thread::spawn(move || generation.drain(2).unwrap())
            };
            entry.recv_timeout(LIMIT).unwrap();
            let second = {
                let generation = generation.clone();
                std::thread::spawn(move || generation.drain(1).unwrap())
            };
            let deadline = Instant::now() + LIMIT;
            while generation.kernel.producer_ownership.load(Ordering::Acquire) != 2 {
                assert!(
                    Instant::now() < deadline,
                    "second direct drain was not admitted"
                );
                std::thread::yield_now();
            }
            assert!(generation.kernel.producer_drain.try_lock().is_err());
            let (attempt, attempted) = mpsc::channel();
            let retire = std::thread::spawn(move || {
                attempt.send(()).unwrap();
                if let Some(candidate) = candidate {
                    Some(host.publish(candidate).unwrap())
                } else {
                    drop(host);
                    None
                }
            });
            attempted.recv_timeout(LIMIT).unwrap();
            release.send(()).unwrap();
            // Mutex is not FIFO: retirement or the second drain may win next.
            assert!(first.join().unwrap().is_empty());
            assert!(second.join().unwrap().is_empty());
            let successor = retire.join().unwrap();
            drop(busy);
            assert_eq!(polls.load(Ordering::SeqCst), 1);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(payload.load(Ordering::SeqCst), 1);
            assert_eq!(
                generation.kernel.producer_ownership.load(Ordering::Acquire),
                0
            );
            assert!(generation.drain(10).unwrap().is_empty());
            if let Some(successor) = successor {
                assert_eq!(
                    successor.with_session(|s| s.state()).unwrap(),
                    phenix_graph::runtime::SessionState::Completed
                );
            }
            drop(generation);
            assert!(weak_code.upgrade().is_none());
        }
    }
}

#[test]
fn eager_readiness_self_retirement_finishes_permitted_admission_only() {
    use phenix_graph::runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions, SessionState,
    };
    thread_local! { static OWNER: RefCell<Option<KernelHost>> = const { RefCell::new(None) }; }
    struct Retire(Weak<Generation>, Arc<AtomicUsize>);
    impl ReadinessPolicy for Retire {
        fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
            let generation = self.0.upgrade().unwrap();
            assert!(generation.kernel.producers.try_lock().is_ok());
            assert!(generation.kernel.session.lock_is_available());
            assert!(matches!(
                generation.drain(0),
                Err(DriverError::ReentrantDrain)
            ));
            assert_eq!(
                generation.with_session(|_| ()),
                Err(SessionAccessError::Reentrant)
            );
            OWNER.with(|owner| drop(owner.borrow_mut().take()));
            assert!(generation.kernel.producers_retired());
            assert_eq!(self.1.load(Ordering::SeqCst), 0);
            snapshot
                .heads
                .iter()
                .filter(|h| h.available)
                .map(|h| h.port.clone())
                .collect()
        }
    }
    struct Source(Weak<Generation>, Arc<AtomicUsize>, Arc<AtomicUsize>);
    impl Producer for Source {
        fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
            self.1.fetch_add(1, Ordering::SeqCst);
            Ok(Some(InputEvent {
                input: "in".into(),
                value: Value::local(7_i64),
            }))
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            let generation = self.0.upgrade().unwrap();
            assert!(generation.kernel.producer_drain.try_lock().is_ok());
            assert!(generation.kernel.producers.try_lock().is_ok());
            assert!(!KernelLockScope::held(&generation.kernel.producer_drain));
            assert!(!crate::ActiveSessionCall::held(
                generation.kernel.session_identity
            ));
            self.2.fetch_add(1, Ordering::SeqCst);
        }
    }
    let polls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let generation = Arc::new_cyclic(|weak| {
        let mut kernel =
            Kernel::build("readiness-retirement", [], Arc::new(LocalProcessDispatcher)).unwrap();
        kernel.start().unwrap();
        let mut builder = NetworkBuilder::new();
        let sink = builder
            .add("sink", Arc::new(ComponentDef::sink(|_: &i64| {})))
            .unwrap();
        builder
            .expose_input(
                "in",
                sink.input::<i64>("input").unwrap(),
                std::num::NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        kernel.session = ExclusiveSlot::new(
            builder
                .build()
                .unwrap()
                .compile()
                .unwrap()
                .start_with_options(SessionOptions {
                    readiness: std::collections::BTreeMap::from([(
                        "sink".into(),
                        Readiness::new(Arc::new(Retire(weak.clone(), drops.clone()))),
                    )]),
                    ..Default::default()
                }),
        );
        for id in ["retire", "must-not-poll"] {
            let key = Key::new("test", id);
            kernel
                .routes
                .insert(key.clone(), std::collections::BTreeSet::from(["in".into()]));
            kernel.producers.get_mut().unwrap().producers.push((
                key,
                Some(Box::new(Source(weak.clone(), polls.clone(), drops.clone()))),
                None,
            ));
        }
        Generation { epoch: 1, kernel }
    });
    let mut host = KernelHost::new(PluginCatalog::new().instance());
    host.epoch = 1;
    host.active = Some(generation.clone());
    OWNER.with(|owner| *owner.borrow_mut() = Some(host));
    let events = generation.drain(8).unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].result.is_ok());
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert_eq!(
        generation.with_session(|s| s.state()).unwrap(),
        SessionState::Runnable
    );
    generation.run_steps(8).unwrap();
    assert!(generation.drain(8).unwrap().is_empty());
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[test]
fn session_release_notifies_after_restore_on_success_and_unwind() {
    let mut kernel = Kernel::build("session-notify", [], Arc::new(LocalProcessDispatcher)).unwrap();
    kernel.start().unwrap();
    kernel.session = ExclusiveSlot::new(live_session::<i64>());
    let mut listener = kernel.with_session(|s| s.wake_listener()).unwrap();
    assert!(listener.wait_timeout(Duration::ZERO));
    for unwind in [false, true] {
        assert!(!listener.wait_timeout(Duration::ZERO));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            kernel
                .with_session(|_| {
                    assert_eq!(
                        kernel.with_session(|_| ()),
                        Err(SessionAccessError::Reentrant)
                    );
                    if unwind {
                        panic!("session callback unwind");
                    }
                })
                .unwrap();
        }));
        assert_eq!(result.is_err(), unwind);
        assert!(listener.wait_timeout(Duration::ZERO));
        assert!(kernel.session.try_take().is_some());
        assert!(!crate::ActiveSessionCall::held(kernel.session_identity));
    }
}

#[test]
fn actual_driver_event_cutpoint_retires_without_gate_and_restores_error_paths() {
    struct Fails;
    impl Producer for Fails {
        fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
            Err(std::io::Error::other("terminal source event").into())
        }
        fn set_waker(&mut self, _: crate::WakeHandle) -> bool {
            true
        }
    }
    for panic in [false, true] {
        let mut kernel =
            Kernel::build("event-retirement", [], Arc::new(LocalProcessDispatcher)).unwrap();
        kernel.start().unwrap();
        kernel.session = ExclusiveSlot::new(live_session::<i64>());
        kernel.producers.get_mut().unwrap().producers.push((
            Key::new("test", "fails"),
            Some(Box::new(Fails)),
            None,
        ));
        let (host, generation) = host_for(kernel);
        let mut owner = Some(host);
        let mut callbacks = 0;
        let mut reached_retirement = false;
        let outcome = generation
            .kernel
            .run_until(&crate::WakeHandle::default(), |event| {
                callbacks += 1;
                assert!(event.result.is_err());
                assert!(generation.kernel.producer_drain.try_lock().is_ok());
                assert!(!KernelLockScope::held(&generation.kernel.producer_drain));
                generation.with_session(|_| ()).unwrap();
                drop(owner.take());
                assert!(generation.kernel.producers_retired());
                reached_retirement = true;
                if panic {
                    panic!("event callback after retirement");
                }
            });
        assert_eq!(callbacks, 1);
        assert!(
            reached_retirement,
            "caught panic hid an earlier callback assertion"
        );
        assert!(owner.is_none());
        if panic {
            assert!(matches!(
                outcome,
                Err(DriverError::EventCallbackPanicked { .. })
            ));
        } else {
            assert!(matches!(outcome, Err(DriverError::Retired)));
        }
        assert!(generation.kernel.driver_wake.lock().unwrap().is_none());
        assert_eq!(
            generation.kernel.producer_ownership.load(Ordering::Acquire),
            0
        );
        assert!(!crate::ActiveSessionCall::held(
            generation.kernel.session_identity
        ));
        assert!(generation.drain(1).unwrap().is_empty());
        generation.with_session(|_| ()).unwrap();
    }
}

#[test]
fn actual_driver_startup_retirement_and_busy_exit_restore_scopes() {
    struct Startup(mpsc::Sender<()>, mpsc::Receiver<()>);
    impl Producer for Startup {
        fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
            Ok(None)
        }
        fn set_waker(&mut self, _: crate::WakeHandle) -> bool {
            self.0.send(()).unwrap();
            self.1.recv_timeout(LIMIT).unwrap();
            true
        }
    }
    let mut kernel =
        Kernel::build("startup-cutpoint", [], Arc::new(LocalProcessDispatcher)).unwrap();
    kernel.start().unwrap();
    kernel.session = ExclusiveSlot::new(live_session::<i64>());
    let (host, generation) = host_for(kernel);
    let wake = crate::WakeHandle::default();
    let busy = generation.kernel.session.try_take().unwrap();
    assert!(matches!(
        generation.kernel.run_until(&wake, |_| {}),
        Err(DriverError::SessionBusy)
    ));
    assert_eq!(
        generation.kernel.producer_ownership.load(Ordering::Acquire),
        0
    );
    assert!(generation.kernel.driver_wake.lock().unwrap().is_none());
    drop(busy);
    let (entered, entry) = mpsc::channel();
    let (release, released) = mpsc::channel();
    generation.kernel.producers.lock().unwrap().producers.push((
        Key::new("test", "startup"),
        Some(Box::new(Startup(entered, released))),
        None,
    ));
    let (done, completion) = mpsc::channel();
    let driver = {
        let generation = generation.clone();
        let wake = wake.clone();
        std::thread::spawn(move || {
            done.send(generation.kernel.run_until(&wake, |_| {}))
                .unwrap()
        })
    };
    entry.recv_timeout(LIMIT).unwrap();
    assert!(generation.kernel.driver_wake.lock().unwrap().is_some());
    assert!(matches!(
        generation.drain(0),
        Err(DriverError::AlreadyRunning)
    ));
    let retire = std::thread::spawn(move || drop(host));
    release.send(()).unwrap();
    retire.join().unwrap();
    let result = completion.recv_timeout(LIMIT);
    if result.is_err() {
        wake.shutdown();
    }
    driver.join().unwrap();
    assert!(matches!(result.unwrap(), Err(DriverError::Retired)));
    assert_eq!(
        generation.kernel.producer_ownership.load(Ordering::Acquire),
        0
    );
    assert!(generation.kernel.driver_wake.lock().unwrap().is_none());
    assert!(generation.with_session(|_| ()).is_ok());
    assert!(matches!(
        generation
            .kernel
            .run_until(&crate::WakeHandle::default(), |_| {}),
        Err(DriverError::Retired)
    ));
}
