mod support;
use phenix_graph::{
    Interface, Value,
    component::ComponentDef,
    compose::{Edit, NetworkPatch},
    process::{
        ActivationJob, DispatchResult, LocalProcessDispatcher, Process, ProcessContext,
        ProcessDispatcher, ProcessExecutionGuard, Step,
    },
};
use phenix_kernel::{CandidateArtifacts, Contribution, InputEvent, Kernel, Producer, WakeHandle};
use phenix_plugin::{Failure, PluginInstance, Registrar};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct CountingSource(Arc<AtomicUsize>);
impl Producer for CountingSource {
    fn poll(&mut self) -> Result<Option<InputEvent>, phenix_kernel::CallbackError> {
        let next = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(InputEvent {
            input: "input".into(),
            value: Value::local(next as i64),
        }))
    }
}
struct Fixture {
    polls: Arc<AtomicUsize>,
    calls: Arc<Mutex<Vec<i64>>>,
    drops: Arc<AtomicUsize>,
    capture: bool,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl PluginInstance for Fixture {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let calls = self.calls.clone();
        let definition = if self.capture {
            support::function(move |value| {
                calls.lock().unwrap().push(value);
                value
            })
        } else {
            support::sink(move |value| {
                calls.lock().unwrap().push(value);
            })
        };
        let mut edits = vec![
            Edit::Add {
                path: "worker".into(),
                definition,
            },
            Edit::ExposeInput {
                name: "input".into(),
                targets: vec![support::path("worker", "run")],
                capacity: support::capacity(),
            },
        ];
        if self.capture {
            edits.push(Edit::ExposeOutput {
                name: "output".into(),
                from: support::path("worker", "value"),
            });
        }
        let polls = self.polls.clone();
        into.add(Contribution {
            id: "main".into(),
            requires: vec![],
            routes: vec!["input".into()],
            patch: NetworkPatch::new(edits),
            prepare: Some(Box::new(move |_| {
                let mut artifacts = CandidateArtifacts::default();
                let polls = polls.clone();
                artifacts.activate(move || Ok(Box::new(CountingSource(polls.clone()))))?;
                Ok(artifacts)
            })),
        })
    }
}
type BuiltFixture = (
    Kernel,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<i64>>>,
    Arc<AtomicUsize>,
);
fn fixture(capture: bool) -> BuiltFixture {
    let polls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Mutex::new(vec![]));
    let drops = Arc::new(AtomicUsize::new(0));
    let snapshot = support::snapshot(
        "fixture",
        Fixture {
            polls: polls.clone(),
            calls: calls.clone(),
            drops: drops.clone(),
            capture,
        },
    );
    let options = support::options(std::slice::from_ref(&snapshot));
    let kernel = Kernel::build_with_options(
        "bounded",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    (kernel, polls, calls, drops)
}
#[test]
fn full_retains_exactly_one_event_and_does_not_repoll_or_execute() {
    let (mut kernel, polls, calls, _) = fixture(false);
    assert!(kernel.with_session(|_| ()).is_err());
    kernel.start().unwrap();
    assert_eq!(kernel.drain(10).len(), 2); // One reserved input plus one queued input.
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    assert!(calls.lock().unwrap().is_empty());
    assert!(kernel.drain(100).is_empty());
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    kernel.run_steps(100).unwrap();
    assert_eq!(*calls.lock().unwrap(), [1, 2]);
    assert_eq!(kernel.drain(1).len(), 1);
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    kernel.run_steps(100).unwrap();
    assert_eq!(*calls.lock().unwrap(), [1, 2, 3]);
}

#[test]
fn producer_admission_readiness_callbacks_run_outside_producer_lock_and_reentry_fails_fast() {
    struct ReentrantReadiness {
        kernel: Arc<Mutex<Option<std::sync::Weak<Kernel>>>>,
        failures: Arc<Mutex<Vec<String>>>,
    }
    impl phenix_graph::runtime::ReadinessPolicy for ReentrantReadiness {
        fn select(&self, snapshot: &phenix_graph::runtime::ReadinessSnapshot) -> Vec<String> {
            let kernel = self
                .kernel
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap();
            assert!(kernel.drain(0).is_empty());
            self.failures
                .lock()
                .unwrap()
                .push(kernel.run_steps(0).unwrap_err());
            snapshot
                .heads
                .iter()
                .find(|head| head.port == "run" && head.available)
                .map(|head| vec![head.port.clone()])
                .unwrap_or_default()
        }
    }

    let polls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Mutex::new(vec![]));
    let drops = Arc::new(AtomicUsize::new(0));
    let snapshot = support::snapshot(
        "fixture",
        Fixture {
            polls,
            calls,
            drops,
            capture: false,
        },
    );
    let kernel_ref = Arc::new(Mutex::new(None));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options.execution.readiness.insert(
        "worker".into(),
        phenix_graph::runtime::Readiness::Custom(Arc::new(ReentrantReadiness {
            kernel: kernel_ref.clone(),
            failures: failures.clone(),
        })),
    );
    let mut built = Kernel::build_with_options(
        "admission-callback",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    built.start().unwrap();
    let kernel = Arc::new(built);
    *kernel_ref.lock().unwrap() = Some(Arc::downgrade(&kernel));

    let driver = kernel.clone();
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || sent.send(driver.drain(1)).unwrap());
    let events = received
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("readiness callback deadlocked on kernel ownership");
    assert_eq!(events.len(), 1);
    assert_eq!(
        *failures.lock().unwrap(),
        ["reentrant access to the same kernel session"]
    );
}

#[test]
fn owned_jobs_and_captured_packets_retain_snapshot_code_after_kernel_drop() {
    let (mut kernel, _, _, drops) = fixture(false);
    kernel.start().unwrap();
    kernel.inject("input", Value::local(1_i64)).unwrap();
    let (_, job) = kernel
        .with_session(|session| session.next_job().unwrap())
        .unwrap();
    drop(kernel);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let completion = job.run();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(completion);
    assert_eq!(drops.load(Ordering::SeqCst), 1);

    let (mut kernel, _, _, drops) = fixture(true);
    kernel.start().unwrap();
    support::run(&kernel, "input", 2);
    let packet = kernel
        .with_session(|session| session.take_output("output").unwrap().unwrap())
        .unwrap();
    drop(kernel);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(packet.value().borrow::<i64>(), Some(&2));
    let value = packet.value().clone();
    let payload = value.leased_shared::<i64>().unwrap();
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(value);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(*payload, 2);
    drop(payload);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[test]
fn explicit_session_ownership_defers_producer_admission_without_blocking_retirement() {
    let (mut kernel, polls, _, _) = fixture(false);
    kernel.start().unwrap();
    let kernel = Arc::new(kernel);
    let owner = kernel.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        owner
            .with_session(|_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
            })
            .unwrap()
    });
    ready.recv().unwrap();
    assert!(kernel.drain(100).is_empty());
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    release.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(kernel.drain(1).len(), 1);
    assert_eq!(polls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_session_calls_wait_for_the_exclusive_session_lease() {
    let (mut kernel, _, _, _) = fixture(false);
    kernel.start().unwrap();
    let kernel = Arc::new(kernel);
    let owner = kernel.clone();
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        owner
            .with_session(|_| {
                entered.send(()).unwrap();
                released.recv().unwrap();
            })
            .unwrap()
    });
    ready.recv().unwrap();

    let reader = kernel.clone();
    let (completed, completion) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        completed.send(reader.run_steps(0).is_ok()).unwrap();
    });
    assert!(
        completion
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err()
    );
    release.send(()).unwrap();
    holder.join().unwrap();
    assert!(
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
    );
    waiter.join().unwrap();
}

#[test]
fn host_grants_are_required_and_cannot_be_supplied_by_the_patch() {
    let snapshot = support::snapshot(
        "fixture",
        Fixture {
            polls: Arc::default(),
            calls: Arc::default(),
            drops: Arc::default(),
            capture: false,
        },
    );
    let error = Kernel::build("no-grants", [snapshot], Arc::new(LocalProcessDispatcher))
        .err()
        .unwrap();
    let phenix_kernel::ResolutionError::Composition { cause, .. } = error else {
        panic!("unexpected error")
    };
    assert!(matches!(
        cause.downcast_ref::<phenix_graph::compose::ComposeError>(),
        Some(phenix_graph::compose::ComposeError::Unauthorized { .. })
    ));
}
#[test]
fn portless_sources_require_explicit_start_edits() {
    struct Source(Arc<AtomicUsize>);
    impl Process for Source {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Step::Finished)
        }
    }
    struct Plugin(Arc<AtomicUsize>, bool);
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            let calls = self.0.clone();
            let mut edits = vec![Edit::Add {
                path: "source".into(),
                definition: Arc::new(ComponentDef::primitive(
                    Arc::new(Interface::new([], []).unwrap()),
                    Arc::new(move || Source(calls.clone())),
                )),
            }];
            if self.1 {
                edits.push(Edit::AddSource("source".into()));
            }
            into.add(support::contribution("main", NetworkPatch::new(edits)))
        }
    }
    for explicit in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let snapshot = support::snapshot("fixture", Plugin(calls.clone(), explicit));
        let options = support::options(std::slice::from_ref(&snapshot));
        let mut kernel = Kernel::build_with_options(
            "source",
            [snapshot],
            Arc::new(LocalProcessDispatcher),
            options,
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        kernel.start().unwrap();
        kernel.run_steps(1).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(explicit));
    }
}
#[test]
fn dispatcher_jobs_enter_guards_on_the_executing_worker() {
    struct Dispatcher(Arc<Mutex<Vec<ActivationJob>>>);
    impl ProcessDispatcher for Dispatcher {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            self.0.lock().unwrap().push(job);
            DispatchResult::Deferred
        }
    }
    struct Guard(Arc<Mutex<Vec<std::thread::ThreadId>>>);
    impl ProcessExecutionGuard for Guard {
        fn enter(&self) -> Result<Box<dyn std::any::Any>, String> {
            self.0.lock().unwrap().push(std::thread::current().id());
            Ok(Box::new(()))
        }
    }
    let jobs = Arc::new(Mutex::new(vec![]));
    let entered = Arc::new(Mutex::new(vec![]));
    let snapshot = support::snapshot(
        "fixture",
        Fixture {
            polls: Arc::default(),
            calls: Arc::default(),
            drops: Arc::default(),
            capture: false,
        },
    );
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options
        .execution
        .execution_guards
        .push(Arc::new(Guard(entered.clone())));
    let mut kernel = Kernel::build_with_options(
        "worker",
        [snapshot],
        Arc::new(Dispatcher(jobs.clone())),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    kernel.inject("input", Value::local(1_i64)).unwrap();
    assert_eq!(kernel.run_steps(1).unwrap().steps, 1);
    assert!(entered.lock().unwrap().is_empty());
    let job = jobs.lock().unwrap().pop().unwrap();
    let (thread, completion) = std::thread::spawn(move || (std::thread::current().id(), job.run()))
        .join()
        .unwrap();
    assert_eq!(*entered.lock().unwrap(), [thread]);
    kernel
        .with_session(|session| session.complete_job(completion).unwrap())
        .unwrap();
}

#[test]
fn process_callback_reentry_fails_fast_without_holding_the_session_mutex() {
    struct WorkerDispatcher;
    impl ProcessDispatcher for WorkerDispatcher {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(job.run());
            });
            DispatchResult::Completed(
                receive
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .expect("kernel re-entry guard was not propagated to the worker"),
            )
        }
    }

    struct ReentrantProcess(
        Arc<Mutex<Option<std::sync::Weak<Kernel>>>>,
        Arc<Mutex<Vec<String>>>,
    );
    impl Process for ReentrantProcess {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            let kernel = self.0.lock().unwrap().as_ref().unwrap().upgrade().unwrap();
            self.1
                .lock()
                .unwrap()
                .push(kernel.with_session(|_| ()).unwrap_err());
            Ok(Step::Finished)
        }
    }

    struct Plugin(
        Arc<Mutex<Option<std::sync::Weak<Kernel>>>>,
        Arc<Mutex<Vec<String>>>,
    );
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            let access = self.0.clone();
            let failures = self.1.clone();
            let source = Arc::new(ComponentDef::primitive(
                Arc::new(Interface::new([], []).unwrap()),
                Arc::new(move || ReentrantProcess(access.clone(), failures.clone())),
            ));
            into.add(support::contribution(
                "source",
                NetworkPatch::new([
                    Edit::Add {
                        path: "source".into(),
                        definition: source,
                    },
                    Edit::AddSource("source".into()),
                ]),
            ))
        }
    }

    let access = Arc::new(Mutex::new(None));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let snapshot = support::snapshot("reentrant", Plugin(access.clone(), failures.clone()));
    let options = support::options(std::slice::from_ref(&snapshot));
    let mut kernel = Kernel::build_with_options(
        "reentrant-process",
        [snapshot],
        Arc::new(WorkerDispatcher),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    let kernel = Arc::new(kernel);
    *access.lock().unwrap() = Some(Arc::downgrade(&kernel));

    assert_eq!(kernel.run_steps(1).unwrap().steps, 1);
    assert_eq!(
        *failures.lock().unwrap(),
        ["reentrant access to the same kernel session"]
    );
}

#[test]
fn request_scopes_are_bounded_authenticated_and_reusable_after_release() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let snapshot = support::snapshot(
        "fixture",
        Fixture {
            polls: Arc::default(),
            calls: Arc::default(),
            drops: Arc::default(),
            capture: true,
        },
    );
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options.execution.max_requests = support::capacity();
    let mut kernel = Kernel::build_with_options(
        "requests",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    let request = kernel
        .with_session(|session| {
            let request = session.begin_request(Some("output")).unwrap();
            assert_eq!(
                session.begin_request(Some("output")),
                Err(RequestError::Full)
            );
            session
                .inject_request_local(request, "input", 9_i64)
                .unwrap();
            session.close_request(request).unwrap();
            request
        })
        .unwrap();
    kernel.run_steps(100).unwrap();
    kernel
        .with_session(|session| {
            let packet = session.take_output("output").unwrap().unwrap();
            assert_eq!(packet.metadata().unwrap().request(), Some(request));
            assert_eq!(packet.metadata().unwrap().capture(), Some("output"));
            assert_eq!(packet.value().borrow::<i64>(), Some(&9));
            assert_eq!(
                session.request_status(request),
                Ok(RequestStatus::Settled { captures: 1 })
            );
            session.release_request(request).unwrap();
            assert_eq!(session.request_status(request), Err(RequestError::Stale));
            assert!(session.begin_request(Some("output")).is_ok());
        })
        .unwrap();
}

#[test]
fn blocking_driver_observes_native_process_wakes_and_external_shutdown() {
    struct Source {
        calls: Arc<AtomicUsize>,
        done: std::sync::mpsc::Sender<()>,
    }
    impl Process for Source {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(Step::WaitWake);
            }
            self.done.send(()).unwrap();
            Ok(Step::Finished)
        }
    }
    struct Plugin(Arc<AtomicUsize>, std::sync::mpsc::Sender<()>);
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            let calls = self.0.clone();
            let done = self.1.clone();
            into.add(support::contribution(
                "main",
                NetworkPatch::new([
                    Edit::Add {
                        path: "source".into(),
                        definition: Arc::new(ComponentDef::primitive(
                            Arc::new(Interface::new([], []).unwrap()),
                            Arc::new(move || Source {
                                calls: calls.clone(),
                                done: done.clone(),
                            }),
                        )),
                    },
                    Edit::AddSource("source".into()),
                ]),
            ))
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let (done, completed) = std::sync::mpsc::channel();
    let snapshot = support::snapshot("fixture", Plugin(calls.clone(), done));
    let options = support::options(std::slice::from_ref(&snapshot));
    let mut kernel = Kernel::build_with_options(
        "wakes",
        [snapshot],
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    kernel.start().unwrap();
    let process_wake = kernel
        .with_session(|session| session.wake_handle("source").unwrap())
        .unwrap();
    let shutdown = phenix_kernel::WakeHandle::default();
    let driver_shutdown = shutdown.clone();
    let driver =
        std::thread::spawn(move || kernel.run_until(&driver_shutdown, |_| panic!("no producers")));
    // Native wakes are latched even when sent before the process waits.
    assert!(process_wake.wake());
    let result = completed.recv_timeout(std::time::Duration::from_secs(2));
    shutdown.shutdown();
    driver.join().unwrap().unwrap();
    result.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn blocking_driver_wakes_for_deferred_job_deadline_without_worker_notification() {
    struct EventSource(bool);
    impl Producer for EventSource {
        fn poll(&mut self) -> Result<Option<InputEvent>, phenix_kernel::CallbackError> {
            if std::mem::replace(&mut self.0, true) {
                Ok(None)
            } else {
                Ok(Some(InputEvent {
                    input: "input".into(),
                    value: Value::local(1_i64),
                }))
            }
        }

        fn set_waker(&mut self, _: WakeHandle) -> bool {
            true
        }
    }
    struct HoldJob(Arc<Mutex<Option<ActivationJob>>>);
    impl ProcessDispatcher for HoldJob {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            *self.0.lock().unwrap() = Some(job);
            DispatchResult::Deferred
        }
    }
    struct Plugin;
    impl PluginInstance for Plugin {
        fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
            into.add(Contribution {
                id: "main".into(),
                requires: vec![],
                routes: vec!["input".into()],
                patch: NetworkPatch::new([
                    Edit::Add {
                        path: "worker".into(),
                        definition: support::sink(|_| {}),
                    },
                    Edit::ExposeInput {
                        name: "input".into(),
                        targets: vec![support::path("worker", "run")],
                        capacity: support::capacity(),
                    },
                ]),
                prepare: Some(Box::new(move |_| {
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.activate(|| Ok(Box::new(EventSource(false))))?;
                    Ok(artifacts)
                })),
            })
        }
    }

    let held = Arc::new(Mutex::new(None));
    let snapshot = support::snapshot("timeout", Plugin);
    let mut options = support::options(std::slice::from_ref(&snapshot));
    options.execution.deferred_job_timeout = Some(std::time::Duration::from_millis(20));
    let mut kernel = Kernel::build_with_options(
        "deferred-timeout",
        [snapshot],
        Arc::new(HoldJob(held.clone())),
        options,
    )
    .unwrap();
    kernel.start().unwrap();

    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        send.send(kernel.run_until(&WakeHandle::default(), |_| {}))
            .unwrap();
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(2));
    assert!(
        result.is_ok(),
        "driver slept past the deferred-job deadline; job held={}",
        held.lock().unwrap().is_some()
    );
    let error = result.unwrap().unwrap_err();
    assert!(matches!(
        error,
        phenix_kernel::DriverError::SessionStopped {
            state: phenix_graph::runtime::SessionState::Failed,
            cause: Some(cause),
        } if cause.contains("deferred job timed out")
    ));
    assert!(held.lock().unwrap().is_some());
}

#[test]
fn prepared_binding_uses_declaration_keys_without_admission() {
    use phenix_kernel::host::KernelHost;
    use phenix_plugin::PluginCatalog;
    let polls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Mutex::new(vec![]));
    let mut catalog = PluginCatalog::new();
    let key = catalog
        .register("fixture", |plugin: Fixture| Ok(plugin))
        .unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let stage = host
        .plugins()
        .prepare_load(
            &key,
            Fixture {
                polls: polls.clone(),
                calls: calls.clone(),
                drops: Arc::default(),
                capture: true,
            },
        )
        .unwrap();
    let snapshot = stage.candidate().unwrap();
    let options = support::options(std::slice::from_ref(&snapshot));
    let contribution = snapshot.get::<Contribution>("main").unwrap();
    let Edit::Add { definition, .. } = &contribution.patch.edits()[0] else {
        unreachable!()
    };
    let declaration = definition.definition_key();
    let candidate = host
        .prepare(
            vec![stage],
            options,
            Arc::new(LocalProcessDispatcher),
            "inspect",
            |binding| {
                let physical = binding.lookup_definition(declaration)?;
                assert_eq!(physical.path, "worker");
                assert_eq!(binding.inspect("worker")?.definition_key, Some(declaration));
                assert_eq!(
                    binding.output_source("output"),
                    Some((physical.index, "value"))
                );
                assert_eq!(binding.output_source("missing"), None);
                assert_eq!(
                    binding.input_targets("input"),
                    Some([(physical.index, "run".into())].as_slice())
                );
                assert!(calls.lock().unwrap().is_empty());
                assert_eq!(polls.load(Ordering::SeqCst), 0);
                Ok(())
            },
        )
        .unwrap();
    assert!(host.active().is_none());
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    let generation = host.publish(candidate).unwrap();
    generation.inject("input", Value::local(5_i64)).unwrap();
    generation.run_steps(8).unwrap();
    assert_eq!(*calls.lock().unwrap(), [5]);
}

#[test]
fn cancelled_sessions_stop_the_driver_instead_of_spinning_on_invalid_wakes() {
    let mut kernel = Kernel::build("cancelled", [], Arc::new(LocalProcessDispatcher)).unwrap();
    kernel.start().unwrap();
    kernel.with_session(|session| session.cancel()).unwrap();
    assert_eq!(
        kernel
            .run_until(&phenix_kernel::WakeHandle::default(), |_| {})
            .unwrap_err(),
        phenix_kernel::DriverError::SessionStopped {
            state: phenix_graph::runtime::SessionState::Cancelled,
            cause: None
        }
    );
}
