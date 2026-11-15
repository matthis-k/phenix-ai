mod support;

use phenix_graph::{
    Value,
    compose::{Edit, Footprint, NetworkPatch},
    process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CallbackError, CandidateArtifacts, Contribution, DriverError, InputEvent, Key,
    Producer, WakeHandle,
    host::{Candidate, Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(3);

#[test]
fn version_closes_inspection_wait_race_and_shutdown_is_sticky() {
    let wake = WakeHandle::default();
    let observed = wake.version();
    let ready = Arc::new(Barrier::new(5));
    let (tx, rx) = mpsc::channel();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (wake, ready, tx) = (wake.clone(), ready.clone(), tx.clone());
            std::thread::spawn(move || {
                ready.wait();
                tx.send(wake.wait_since(observed)).unwrap();
            })
        })
        .collect();
    ready.wait();
    wake.wake();
    for _ in 0..4 {
        assert_ne!(rx.recv_timeout(TIMEOUT).unwrap(), observed);
    }
    for thread in threads {
        thread.join().unwrap();
    }
    // The notification happened before wait acquired its mutex.
    assert_ne!(wake.wait_since(observed), observed);
    assert!(!wake.is_shutdown());
    wake.shutdown();
    let shutdown = wake.version();
    assert!(wake.is_shutdown());
    assert_eq!(wake.wait_since(shutdown), shutdown);
    wake.wake();
    assert!(wake.is_shutdown());
    assert_eq!(wake.wait_since(wake.version()), wake.version());
}

#[test]
fn shutdown_releases_all_waiters_and_remains_visible_to_late_waiters() {
    let wake = WakeHandle::default();
    let observed = wake.version();
    let ready = Arc::new(Barrier::new(5));
    let (tx, rx) = mpsc::channel();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (wake, ready, tx) = (wake.clone(), ready.clone(), tx.clone());
            std::thread::spawn(move || {
                ready.wait();
                wake.wait_since(observed);
                tx.send(wake.is_shutdown()).unwrap();
            })
        })
        .collect();
    ready.wait();
    wake.shutdown();
    for _ in 0..4 {
        assert!(rx.recv_timeout(TIMEOUT).unwrap());
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(wake.wait_since(wake.version()), wake.version());
}

#[derive(Clone, Copy)]
enum Registration {
    Supported,
    Unsupported,
    Panic,
    Shutdown,
}

#[derive(Clone)]
struct Source {
    registration: Registration,
    registrations: Arc<AtomicUsize>,
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    values: Arc<Mutex<VecDeque<i64>>>,
    wake: Arc<Mutex<Option<WakeHandle>>>,
    on_poll: Option<Arc<dyn Fn() + Send + Sync>>,
    on_drop: Option<Arc<dyn Fn() + Send + Sync>>,
    continuous: bool,
    active: bool,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            registration: Registration::Supported,
            registrations: Arc::default(),
            polls: Arc::default(),
            drops: Arc::default(),
            values: Arc::default(),
            wake: Arc::default(),
            on_poll: None,
            on_drop: None,
            continuous: false,
            active: false,
        }
    }
}

impl Producer for Source {
    fn poll(&mut self) -> Result<Option<InputEvent>, CallbackError> {
        if let Some(on_poll) = &self.on_poll {
            on_poll();
        }
        let tick = self.polls.fetch_add(1, Ordering::SeqCst);
        let value = if self.continuous {
            Some(tick as i64)
        } else {
            self.values.lock().unwrap().pop_front()
        };
        Ok(value.map(|value| InputEvent {
            input: "in".into(),
            value: Value::local(value),
        }))
    }

    fn set_waker(&mut self, wake: WakeHandle) -> bool {
        self.registrations.fetch_add(1, Ordering::SeqCst);
        match self.registration {
            Registration::Supported => {
                *self.wake.lock().unwrap() = Some(wake);
                true
            }
            Registration::Unsupported => {
                wake.wake(); // A failed candidate is allowed to signal hints.
                false
            }
            Registration::Panic => panic!("registration failed"),
            Registration::Shutdown => {
                wake.shutdown();
                true
            }
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Some(on_drop) = &self.on_drop {
            on_drop();
        }
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Plugin {
    sources: Vec<Source>,
    processed: Arc<Mutex<Vec<i64>>>,
}

impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        for (index, source) in self.sources.iter().enumerate() {
            let patch = if index == 0 {
                let processed = self.processed.clone();
                NetworkPatch::new([
                    Edit::Add {
                        path: "sink".into(),
                        definition: support::sink(move |value| {
                            processed.lock().unwrap().push(value);
                        }),
                    },
                    Edit::ExposeInput {
                        name: "in".into(),
                        targets: vec![support::path("sink", "run")],
                        capacity: support::capacity(),
                    },
                ])
            } else {
                NetworkPatch::new([])
            };
            let source = source.clone();
            Contribution::new(format!("source{index}"), patch)
                .route(["in".into()])
                .prepare(move |_| {
                    let source = source.clone();
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.activate(move || {
                        let mut source = source.clone();
                        source.active = true;
                        Ok(Box::new(source))
                    })?;
                    Ok(artifacts)
                })
                .register(into)?;
        }
        Ok(())
    }
}

fn prepare(host: &KernelHost, stages: Vec<StagedPlugin>) -> Result<Candidate, LifecycleError> {
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("test", "source0"),
        [
            Footprint::Component("sink".into()),
            Footprint::InputExport("in".into()),
        ],
    );
    host.prepare(
        stages,
        options,
        Arc::new(LocalProcessDispatcher),
        "wake-host",
        |_| Ok(()),
    )
}

#[test]
fn private_candidate_registers_once_without_polling_or_processing_and_manual_stays_manual() {
    for opted_in in [false, true] {
        let source = Source::default();
        let registrations = source.registrations.clone();
        let polls = source.polls.clone();
        let processed = Arc::<Mutex<Vec<i64>>>::default();
        let mut catalog = PluginCatalog::new();
        let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
        let wake = WakeHandle::default();
        let mut host = if opted_in {
            KernelHost::with_wake(catalog.instance(), wake.clone())
        } else {
            KernelHost::new(catalog.instance())
        };
        let stage = host
            .plugins()
            .prepare_load(
                &key,
                Plugin {
                    sources: vec![source],
                    processed: processed.clone(),
                },
            )
            .unwrap();
        let version = wake.version();
        let candidate = prepare(&host, vec![stage]).unwrap();
        assert_eq!(registrations.load(Ordering::SeqCst), usize::from(opted_in));
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert!(processed.lock().unwrap().is_empty());
        assert!(host.active().is_none());
        assert_eq!(wake.version(), version);
        let old = host.publish(candidate).unwrap();
        assert_eq!(wake.version() != version, opted_in);
        let version = wake.version();
        let candidate = prepare(&host, vec![]).unwrap();
        let new = host.publish(candidate).unwrap();
        assert_eq!(wake.version() != version, opted_in);
        assert_eq!(
            registrations.load(Ordering::SeqCst),
            2 * usize::from(opted_in)
        );
        assert!(old.drain(10).unwrap().is_empty());
        old.inject("in", Value::local(7_i64)).unwrap();
        old.run_steps(32).unwrap();
        assert_eq!(*processed.lock().unwrap(), [7]);
        assert_eq!(new.epoch(), 2);
        assert!(Arc::ptr_eq(&new, &host.active().unwrap()));
    }
}

#[test]
fn failed_wake_candidates_preserve_inventory_active_epoch_and_release_owned_producers() {
    for registration in [
        Registration::Unsupported,
        Registration::Panic,
        Registration::Shutdown,
    ] {
        let wake = WakeHandle::default();
        let mut catalog = PluginCatalog::new();
        let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
        let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
        let processed = Arc::default();
        let initial = host
            .plugins()
            .prepare_load(
                &key,
                Plugin {
                    sources: vec![Source::default()],
                    processed: Arc::clone(&processed),
                },
            )
            .unwrap();
        let initial = prepare(&host, vec![initial]).unwrap();
        let active = host.publish(initial).unwrap();
        let mut failed = Source::default();
        failed.registration = registration;
        let drops = failed.drops.clone();
        let registrations = failed.registrations.clone();
        let stage = host
            .plugins()
            .prepare_reload(
                &key,
                Plugin {
                    sources: vec![failed],
                    processed,
                },
            )
            .unwrap();
        let error = match prepare(&host, vec![stage]) {
            Err(error) => error,
            Ok(_) => panic!("candidate unexpectedly accepted"),
        };
        assert!(match registration {
            Registration::Unsupported => matches!(
                error,
                LifecycleError::Driver(DriverError::UnsupportedWake { .. })
            ),
            Registration::Panic => matches!(
                error,
                LifecycleError::Driver(DriverError::WakeRegistrationPanicked { .. })
            ),
            Registration::Shutdown =>
                matches!(error, LifecycleError::Driver(DriverError::WakeShutdown)),
            Registration::Supported => unreachable!(),
        });
        assert_eq!(registrations.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&active, &host.active().unwrap()));
        assert_eq!(active.epoch(), 1);
        assert_eq!(host.plugins().snapshots().count(), 1);
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);
        assert!(active.drain(1).unwrap().is_empty());
        if !wake.is_shutdown() {
            let next = prepare(&host, vec![]).unwrap();
            assert_eq!(host.publish(next).unwrap().epoch(), 2);
        }
    }
}

#[test]
fn shutdown_before_registration_rejects_but_does_not_revoke_already_prepared_publication() {
    let wake = WakeHandle::default();
    let mut host = KernelHost::with_wake(PluginCatalog::new().instance(), wake.clone());
    let candidate = prepare(&host, vec![]).unwrap();
    wake.shutdown();
    assert!(matches!(
        prepare(&host, vec![]),
        Err(LifecycleError::Driver(DriverError::WakeShutdown))
    ));
    assert_eq!(host.publish(candidate).unwrap().epoch(), 1);

    let mut catalog = PluginCatalog::new();
    let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
    let host = KernelHost::with_wake(catalog.instance(), wake);
    let source = Source::default();
    let registrations = source.registrations.clone();
    let drops = source.drops.clone();
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Plugin {
                sources: vec![source],
                processed: Arc::default(),
            },
        )
        .unwrap();
    assert!(matches!(
        prepare(&host, vec![stage]),
        Err(LifecycleError::Driver(DriverError::WakeShutdown))
    ));
    assert_eq!(registrations.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(host.active().is_none());
    assert_eq!(host.plugins().snapshots().count(), 0);
}

#[test]
fn wake_host_retirement_still_notifies_native_driver_and_keeps_explicit_old_session_usable() {
    let wake = WakeHandle::default();
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
    let source = Source::default();
    source.values.lock().unwrap().push_back(1);
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Plugin {
                sources: vec![source],
                processed: Arc::default(),
            },
        )
        .unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let old = host.publish(candidate).unwrap();
    let worker_generation = old.clone();
    let worker_wake = wake.clone();
    let (event, admitted) = mpsc::channel();
    let (result, outcome) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        // Same handle only: replacing this with a distinct handle is not a
        // supported handover back to the host reactor.
        result
            .send(worker_generation.with_kernel(|kernel| {
                kernel.run_until(&worker_wake, |_| {
                    event.send(()).unwrap();
                })
            }))
            .unwrap();
    });
    admitted.recv_timeout(TIMEOUT).unwrap();
    let candidate = prepare(&host, vec![]).unwrap();
    host.publish(candidate).unwrap();
    let result = outcome.recv_timeout(TIMEOUT);
    if result.is_err() {
        wake.shutdown();
    }
    worker.join().unwrap();
    assert_eq!(result.unwrap(), Err(DriverError::Retired));
    assert!(!wake.is_shutdown());
    old.run_steps(32).unwrap();
    old.inject("in", Value::local(9_i64)).unwrap();
    old.run_steps(32).unwrap();
    assert!(old.drain(1).unwrap().is_empty());
}

#[test]
fn publication_wakes_before_stalled_old_destructor_and_after_old_drain_gate_release() {
    let wake = WakeHandle::default();
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
    let (entered, entering) = mpsc::channel();
    let release = Arc::new(Barrier::new(2));
    let old_slot = Arc::<Mutex<Option<Arc<Generation>>>>::default();
    let slot = old_slot.clone();
    let gate = release.clone();
    let mut source = Source::default();
    source.on_drop = Some(Arc::new(move || {
        if let Some(old) = slot.lock().unwrap().as_ref() {
            // Same-thread reentry would return ReentrantDrain if the gate
            // scope were still held, even if the mutex itself was released.
            assert!(old.drain(0).unwrap().is_empty());
            entered.send(()).unwrap();
            gate.wait();
        }
    }));
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Plugin {
                sources: vec![source],
                processed: Arc::default(),
            },
        )
        .unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let old = host.publish(candidate).unwrap();
    *old_slot.lock().unwrap() = Some(old.clone());
    // Keep the plugin/factory inventory alive; only the activated producer drops.
    let candidate = prepare(&host, vec![]).unwrap();
    let before = wake.version();
    let publisher = std::thread::spawn(move || {
        let new = host.publish(candidate).unwrap();
        (host, new)
    });
    entering.recv_timeout(TIMEOUT).unwrap();
    assert_ne!(wake.version(), before);
    assert!(!publisher.is_finished());
    assert!(old.drain(1).unwrap().is_empty());
    release.wait();
    let (host, new) = publisher.join().unwrap();
    *old_slot.lock().unwrap() = None;
    assert!(Arc::ptr_eq(&new, &host.active().unwrap()));
}

#[test]
fn partial_sweep_is_not_idle_and_backpressure_retries_without_repolling() {
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::with_wake(catalog.instance(), WakeHandle::default());
    let idle = Source::default();
    let busy = Source::default();
    busy.values.lock().unwrap().extend([11, 22, 33]);
    let polls = busy.polls.clone();
    let processed = Arc::<Mutex<Vec<i64>>>::default();
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Plugin {
                sources: vec![idle, busy],
                processed: processed.clone(),
            },
        )
        .unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    let generation = host.publish(candidate).unwrap();
    assert!(generation.drain(1).unwrap().is_empty());
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(generation.drain(1).unwrap().len(), 1);
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    // Fill native admission until an event is retained; queue capacities include
    // native ingress staging, not merely the declared edge capacity.
    let mut retained = false;
    for _ in 0..16 {
        let before = polls.load(Ordering::SeqCst);
        generation.drain(2).unwrap();
        if polls.load(Ordering::SeqCst) == before {
            retained = true;
            break;
        }
    }
    assert!(retained);
    let before = polls.load(Ordering::SeqCst);
    assert!(generation.drain(2).unwrap().is_empty());
    assert_eq!(polls.load(Ordering::SeqCst), before);
    for _ in 0..8 {
        generation.run_steps(32).unwrap();
        generation.drain(2).unwrap();
    }
    generation.run_steps(32).unwrap();
    assert_eq!(*processed.lock().unwrap(), [11, 22, 33]);
}

#[test]
fn application_reactor_checks_shutdown_even_while_events_are_continuous() {
    let wake = WakeHandle::default();
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("test", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::with_wake(catalog.instance(), wake.clone());
    let mut source = Source::default();
    source.continuous = true;
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Plugin {
                sources: vec![source],
                processed: Arc::default(),
            },
        )
        .unwrap();
    let candidate = prepare(&host, vec![stage]).unwrap();
    host.publish(candidate).unwrap();
    let host = Arc::new(Mutex::new(host));
    let reactor_host = host.clone();
    let reactor_wake = wake.clone();
    let (busy, running) = mpsc::channel();
    let reactor = std::thread::spawn(move || {
        let mut sweeps = 0;
        while !reactor_wake.is_shutdown() {
            let observed = reactor_wake.version();
            let generation = reactor_host.lock().unwrap().active().unwrap();
            // This deployment has a known bound of one producer. No host mutex
            // is held for producer callbacks, execution, or waiting.
            let events = generation.drain(1).unwrap();
            let report = generation.run_steps(32).unwrap();
            sweeps += 1;
            if sweeps == 2 {
                busy.send(()).unwrap();
            }
            if events.is_empty()
                && report.steps == 0
                && report.publications == 0
                && !report.budget_exhausted
                && report.state != phenix_graph::runtime::SessionState::Runnable
            {
                reactor_wake.wait_since(observed);
            }
        }
        sweeps
    });
    running.recv_timeout(TIMEOUT).unwrap();
    wake.shutdown();
    assert!(reactor.join().unwrap() >= 2);
}

#[test]
fn idle_reactor_reloads_on_publication_then_processes_source_wake_and_idle_shutdown() {
    use phenix_graph::runtime::SessionState;

    struct CapturedPlugin {
        sources: [Source; 2],
        check_unlocked: Arc<dyn Fn() + Send + Sync>,
    }
    impl PluginInstance for CapturedPlugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            for (index, source) in self.sources.iter().enumerate() {
                let patch = if index == 0 {
                    let check = self.check_unlocked.clone();
                    NetworkPatch::new([
                        Edit::Add {
                            path: "capture".into(),
                            definition: support::function(move |value| {
                                check();
                                value
                            }),
                        },
                        Edit::ExposeInput {
                            name: "in".into(),
                            targets: vec![support::path("capture", "run")],
                            capacity: support::capacity(),
                        },
                        Edit::ExposeOutput {
                            name: "out".into(),
                            from: support::path("capture", "value"),
                        },
                    ])
                } else {
                    NetworkPatch::new([])
                };
                let source = source.clone();
                Contribution::new(format!("source{index}"), patch)
                    .route(["in".into()])
                    .prepare(move |_| {
                        let source = source.clone();
                        let mut artifacts = CandidateArtifacts::default();
                        artifacts.activate(move || {
                            let mut source = source.clone();
                            source.active = true;
                            Ok(Box::new(source))
                        })?;
                        Ok(artifacts)
                    })
                    .register(into)?;
            }
            Ok(())
        }
    }
    #[derive(Debug, PartialEq, Eq)]
    enum Notice {
        Output(u64, i64),
        Idle(u64, u64),
        Waiting(u64),
        Stopped,
    }
    struct ShutdownOnDrop(WakeHandle);
    impl Drop for ShutdownOnDrop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }

    for publish_before_wait in [false, true] {
        let wake = WakeHandle::default();
        let _cleanup = ShutdownOnDrop(wake.clone());
        let mut catalog = PluginCatalog::new();
        let key = catalog.register("test", |p: CapturedPlugin| Ok(p)).unwrap();
        let host = Arc::new(Mutex::new(KernelHost::with_wake(
            catalog.instance(),
            wake.clone(),
        )));
        let weak_host = Arc::downgrade(&host);
        let check_unlocked: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            assert!(weak_host.upgrade().unwrap().try_lock().is_ok());
        });
        let sources = |value| {
            let mut sources = [Source::default(), Source::default()];
            for source in &mut sources {
                source.on_poll = Some(check_unlocked.clone());
            }
            // The first producer is idle. A partial sweep of one must not sleep
            // and strand the second producer's already queued value.
            sources[1].values.lock().unwrap().push_back(value);
            sources
        };
        let prepare_capture = |host: &KernelHost, stages: Vec<StagedPlugin>| {
            let mut options = BuildOptions::default();
            support::grant(
                &mut options,
                Key::new("test", "source0"),
                [
                    Footprint::Component("capture".into()),
                    Footprint::InputExport("in".into()),
                    Footprint::OutputExport("out".into()),
                ],
            );
            host.prepare(
                stages,
                options,
                Arc::new(LocalProcessDispatcher),
                "reactor-capture",
                |_| Ok(()),
            )
            .unwrap()
        };
        let b_sources = sources(20);
        let b_values = b_sources[1].values.clone();
        let b_waker = b_sources[1].wake.clone();
        let b_polls = b_sources.each_ref().map(|source| source.polls.clone());
        let b_registrations = b_sources
            .each_ref()
            .map(|source| source.registrations.clone());
        let replacement = {
            let mut host = host.lock().unwrap();
            let stage = host
                .plugins()
                .prepare_load(
                    &key,
                    CapturedPlugin {
                        sources: sources(10),
                        check_unlocked: check_unlocked.clone(),
                    },
                )
                .unwrap();
            let candidate = prepare_capture(&host, vec![stage]);
            assert_eq!(host.publish(candidate).unwrap().epoch(), 1);
            let before = wake.version();
            let stage = host
                .plugins()
                .prepare_reload(
                    &key,
                    CapturedPlugin {
                        sources: b_sources,
                        check_unlocked: check_unlocked.clone(),
                    },
                )
                .unwrap();
            let candidate = prepare_capture(&host, vec![stage]);
            assert_eq!(
                wake.version(),
                before,
                "registration must not wake the reactor"
            );
            assert!(
                b_registrations
                    .iter()
                    .all(|count| count.load(Ordering::SeqCst) == 1)
            );
            assert!(
                b_polls
                    .iter()
                    .all(|count| count.load(Ordering::SeqCst) == 0)
            );
            candidate
        };

        let (notice_tx, notices) = mpsc::channel();
        let (permit_wait, wait_permit) = mpsc::channel();
        let reactor_host = host.clone();
        let reactor_wake = wake.clone();
        let reactor = std::thread::spawn(move || {
            while !reactor_wake.is_shutdown() {
                let observed = reactor_wake.version();
                let generation = reactor_host.lock().unwrap().active().unwrap();
                // Restricted deployment: exactly two producers, one session
                // owner, synchronous steps, no independent wakes or deadlines.
                let events = generation.drain(2).unwrap();
                assert!(events.iter().all(|event| event.result.is_ok()));
                let report = generation.run_steps(32).unwrap();
                let mut captured = false;
                generation
                    .with_session(|session| {
                        assert!(reactor_host.try_lock().is_ok());
                        assert!(session.failure().is_none());
                        while let Some(packet) = session.take_output("out").unwrap() {
                            captured = true;
                            notice_tx
                                .send(Notice::Output(
                                    generation.epoch(),
                                    *packet.value().borrow::<i64>().unwrap(),
                                ))
                                .unwrap();
                        }
                    })
                    .unwrap();
                assert!(report.next_deferred_timeout.is_none());
                if events.is_empty()
                    && report.steps == 0
                    && report.publications == 0
                    && !report.budget_exhausted
                    && report.state != SessionState::Runnable
                    && !captured
                {
                    notice_tx
                        .send(Notice::Idle(generation.epoch(), observed))
                        .unwrap();
                    // Test-only scheduling gate forces the observation/wait
                    // race without changing the production wake API.
                    wait_permit.recv_timeout(TIMEOUT).unwrap();
                    assert!(reactor_host.try_lock().is_ok());
                    notice_tx.send(Notice::Waiting(generation.epoch())).unwrap();
                    reactor_wake.wait_since(observed);
                }
            }
            notice_tx.send(Notice::Stopped).unwrap();
        });
        assert_eq!(
            notices.recv_timeout(TIMEOUT).unwrap(),
            Notice::Output(1, 10)
        );
        let observed_a = match notices.recv_timeout(TIMEOUT).unwrap() {
            Notice::Idle(1, observed) => observed,
            other => panic!("expected idle generation A, got {other:?}"),
        };
        assert_eq!(wake.version(), observed_a);
        if !publish_before_wait {
            permit_wait.send(()).unwrap();
            assert_eq!(notices.recv_timeout(TIMEOUT).unwrap(), Notice::Waiting(1));
            // A bounded non-return assertion tests blocking, not a sleep used
            // to order publication. The other schedule explicitly gates wait.
            assert!(matches!(
                notices.recv_timeout(Duration::from_millis(25)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
        }
        {
            let mut host = host.try_lock().expect("reactor held host mutex while idle");
            assert_eq!(host.publish(replacement).unwrap().epoch(), 2);
        }
        assert_ne!(wake.version(), observed_a);
        if publish_before_wait {
            permit_wait.send(()).unwrap();
            assert_eq!(notices.recv_timeout(TIMEOUT).unwrap(), Notice::Waiting(1));
        }
        assert_eq!(
            notices.recv_timeout(TIMEOUT).unwrap(),
            Notice::Output(2, 20)
        );
        let observed_b = match notices.recv_timeout(TIMEOUT).unwrap() {
            Notice::Idle(2, observed) => observed,
            other => panic!("expected idle generation B, got {other:?}"),
        };
        assert!(
            b_polls
                .iter()
                .all(|count| count.load(Ordering::SeqCst) >= 2)
        );
        permit_wait.send(()).unwrap();
        assert_eq!(notices.recv_timeout(TIMEOUT).unwrap(), Notice::Waiting(2));
        assert!(matches!(
            notices.recv_timeout(Duration::from_millis(25)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        b_values.lock().unwrap().push_back(21);
        let producer_wake = b_waker.lock().unwrap().as_ref().unwrap().clone();
        producer_wake.wake();
        assert_ne!(wake.version(), observed_b);
        assert_eq!(
            notices.recv_timeout(TIMEOUT).unwrap(),
            Notice::Output(2, 21)
        );
        assert!(matches!(
            notices.recv_timeout(TIMEOUT).unwrap(),
            Notice::Idle(2, _)
        ));
        permit_wait.send(()).unwrap();
        assert_eq!(notices.recv_timeout(TIMEOUT).unwrap(), Notice::Waiting(2));
        assert!(matches!(
            notices.recv_timeout(Duration::from_millis(25)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(host.try_lock().is_ok());
        wake.shutdown();
        assert_eq!(notices.recv_timeout(TIMEOUT).unwrap(), Notice::Stopped);
        reactor.join().unwrap();
    }
}
