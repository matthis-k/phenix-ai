mod support;

use phenix_graph::{
    ComponentDef, Interface, PortDef,
    network::NetworkBuilder,
    process::{
        ActivationJob, CompletedStep, DispatchResult, Process, ProcessContext, ProcessDispatcher,
        Step,
    },
    runtime::{RequestError, RequestQueuePolicy, SessionState},
};
use phenix_storage_graph::derived::{Error, GraphDeriveExt};
use std::{
    error::Error as _,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use support::*;

struct InvalidCompletion {
    foreign: Mutex<Option<CompletedStep>>,
    retained: Mutex<Option<ActivationJob>>,
    delay: AtomicBool,
}
impl ProcessDispatcher for InvalidCompletion {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        if job.metadata().is_some_and(|m| m.path == "sum")
            && !self.delay.swap(false, Ordering::SeqCst)
        {
            *self.retained.lock().unwrap() = Some(job);
            DispatchResult::Completed(self.foreign.lock().unwrap().take().unwrap())
        } else {
            DispatchResult::Completed(job.run())
        }
    }
}

#[derive(Clone)]
struct PanicCapture;
impl Drop for PanicCapture {
    fn drop(&mut self) {
        panic!("retained capture destructor fixture");
    }
}

#[test]
fn global_query_failure_with_retained_capture_returns_wave_not_release_not_settled() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for cleanup_error in [false, true] {
            let mut foreign = NetworkBuilder::new();
            let sink = foreign
                .add("sink", Arc::new(ComponentDef::sink(|_: &i64| {})))
                .unwrap();
            foreign
                .expose_input("input", sink.input::<i64>("input").unwrap(), capacity())
                .unwrap();
            let mut foreign = foreign.build().unwrap().compile().unwrap().start();
            foreign.inject_local("input", 0_i64).unwrap();
            let (_, job) = foreign.next_job().unwrap();
            let dispatcher = Arc::new(InvalidCompletion {
                foreign: Mutex::new(Some(job.run())),
                retained: Mutex::new(None),
                delay: AtomicBool::new(cleanup_error),
            });
            let f = Fixture::new(false);
            f.put(&f.a, 3);
            f.put(&f.b, 5);
            let mut graph = NetworkBuilder::new();
            let group = graph.sources("values", (&f.a, &f.b)).unwrap();
            let sum = graph
                .derive_from("sum", &group, |a: Number, b: Number| a.0 + b.0)
                .unwrap();
            let spec = graph.query_spec("query", &sum).unwrap();
            if cleanup_error {
                let capture = graph
                    .derive_from("a.capture", &group, |_: Number, _: Number| PanicCapture)
                    .unwrap();
                graph.expose_output("cleanup", capture.output()).unwrap();
            }
            let plugin = authored(graph, spec.clone(), None);
            let mut config = options(&plugin);
            config.execution.request_queue_policy = queue;
            let lease = Arc::new(());
            let weak = Arc::downgrade(&lease);
            config.execution.lease = Some(lease);
            let mut runtime = configured(plugin, dispatcher.clone(), config);
            runtime.start().unwrap();
            let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
            let job = dispatcher.retained.lock().unwrap().take().unwrap();
            let request = job.metadata().unwrap().request.unwrap();
            let escaped = match error {
                Error::RequestInterrupted {
                    cause,
                    cancelled_jobs,
                    disposal,
                } => {
                    let cause = if cleanup_error {
                        let Error::Cleanup {
                            cause,
                            request: stored,
                            termination,
                            termination_failure,
                            release,
                            inventory,
                        } = *cause
                        else {
                            panic!("lost original or typed capture cleanup error: {cause:?}");
                        };
                        assert_eq!(stored, request);
                        assert_eq!(termination, Some(RequestError::PacketDestructorPanicked));
                        assert!(termination_failure.is_none());
                        assert_eq!(release, None);
                        assert!(inventory.is_empty());
                        cause
                    } else {
                        cause
                    };
                    let Error::Execution(failure) = cause.as_ref() else {
                        panic!("lost execution failure: {cause:?}");
                    };
                    assert!(failure.summary().contains("invalid completion"));
                    assert_eq!(failure.context().request, Some(request));
                    if cleanup_error {
                        assert!(failure.cleanup().iter().any(|secondary| {
                            secondary.source().unwrap().downcast_ref::<RequestError>()
                                == Some(&RequestError::PacketDestructorPanicked)
                        }));
                    }
                    assert_eq!(cancelled_jobs, [job.ticket()]);
                    assert_eq!(disposal.handed_jobs, 1);
                    assert_eq!(
                        disposal.queued_packets,
                        if cleanup_error { 2 } else { 1 },
                        "dispose retained captures exactly once"
                    );
                    failure.clone()
                }
                other => panic!("lost global failure or inventory: {other:?}"),
            };
            runtime
                .kernel()
                .with_session(|session| {
                    assert_eq!(session.state(), SessionState::Failed);
                    assert_eq!(session.request_status(request), Err(RequestError::Stale));
                    assert!(
                        session
                            .failure()
                            .unwrap()
                            .summary()
                            .contains("invalid completion")
                    );
                    assert!(
                        !session
                            .failure()
                            .unwrap()
                            .summary()
                            .contains("destructor panicked")
                    );
                    assert!(session.complete_job(job.run()).is_err());
                })
                .unwrap();
            drop(runtime);
            assert!(weak.upgrade().is_some());
            let secondary = escaped.cleanup().last().cloned();
            drop(escaped);
            if cleanup_error {
                let secondary = secondary.expect("lost enriched cleanup after release");
                assert_eq!(
                    secondary.source().unwrap().downcast_ref::<RequestError>(),
                    Some(&RequestError::PacketDestructorPanicked)
                );
                assert!(weak.upgrade().is_some());
                drop(secondary);
            }
            assert!(weak.upgrade().is_none());
        }
    }
}

struct HoldPeer {
    job: Mutex<Option<ActivationJob>>,
    path: &'static str,
}
impl ProcessDispatcher for HoldPeer {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        if job.metadata().is_some_and(|m| m.path == self.path) {
            *self.job.lock().unwrap() = Some(job);
            DispatchResult::Deferred
        } else {
            DispatchResult::Completed(job.run())
        }
    }
}

struct CleanupProcess {
    ran: bool,
    panic_on_drop: bool,
}
impl Process for CleanupProcess {
    fn step(&mut self, ctx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        ctx.take("run");
        self.ran = true;
        Ok(Step::Yield)
    }
}
impl Drop for CleanupProcess {
    fn drop(&mut self) {
        assert!(
            !self.ran || !self.panic_on_drop,
            "cleanup destructor fixture"
        );
    }
}

#[test]
fn query_cleanup_preserves_original_typed_failure_peer_ticket_and_code_lease() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for (recovery_panic, hold_cleanup) in [(false, false), (true, false), (true, true)] {
            let f = Fixture::new(false);
            f.put(&f.a, 3);
            let mut graph = NetworkBuilder::new();
            let group = graph.sources("values", (&f.a,)).unwrap();
            let held = graph
                .derive_from("a.held", &group, |a: Number| a.0)
                .unwrap();
            let spec = graph.query_spec("query", &held).unwrap();
            let spawns = Arc::new(AtomicUsize::new(0));
            let count = spawns.clone();
            let cleanup = graph
                .node(
                    "z.cleanup",
                    Arc::new(Interface::new([PortDef::local::<(Number,)>("run")], []).unwrap()),
                    Arc::new(move || {
                        let spawn = count.fetch_add(1, Ordering::SeqCst);
                        assert!(!recovery_panic || spawn == 0, "recovery factory fixture");
                        CleanupProcess {
                            ran: false,
                            panic_on_drop: !recovery_panic,
                        }
                    }),
                )
                .unwrap();
            graph
                .connect(group.output(), cleanup.input("run").unwrap(), capacity())
                .unwrap();
            let plugin = authored(graph, spec.clone(), None);
            let dispatcher = Arc::new(HoldPeer {
                job: Mutex::new(None),
                path: if hold_cleanup { "z.cleanup" } else { "a.held" },
            });
            let mut config = options(&plugin);
            config.execution.request_queue_policy = queue;
            let lease = Arc::new(());
            let weak = Arc::downgrade(&lease);
            config.execution.lease = Some(lease);
            let mut runtime = configured(plugin, dispatcher.clone(), config);
            runtime.start().unwrap();
            let error = runtime
                .bind_query(&spec)
                .unwrap()
                .query_with_budget(16)
                .unwrap_err();
            let job = dispatcher.job.lock().unwrap().take().unwrap();
            let ticket = job.ticket();
            let process = job.metadata().unwrap().process;
            let request = job.metadata().unwrap().request.unwrap();
            let Error::RequestInterrupted {
                cause,
                cancelled_jobs,
                disposal,
            } = error
            else {
                panic!("lost peer inventory: {error:?}");
            };
            assert_eq!(cancelled_jobs, [job.ticket()]);
            assert_eq!(disposal.handed_jobs, 1);
            assert!(matches!(
                cause.source().unwrap().downcast_ref::<Error>(),
                Some(Error::Unsettled)
            ));
            let Error::Cleanup {
                cause,
                request: stored,
                termination,
                termination_failure,
                release,
                inventory,
            } = *cause
            else {
                panic!("lost typed cleanup failure: {cause:?}");
            };
            assert!(matches!(*cause, Error::Unsettled));
            assert_eq!(stored, request);
            assert_eq!(
                termination,
                Some(if recovery_panic {
                    RequestError::RecoveryFailed
                } else {
                    RequestError::ProcessDestructorPanicked
                })
            );
            assert_eq!(release, None);
            assert!(inventory.is_empty());
            let completion = runtime
                .kernel()
                .with_session(|session| {
                    assert_eq!(session.request_status(request), Err(RequestError::Stale));
                    session.complete_job(job.run()).unwrap_err().1
                })
                .unwrap();
            assert_eq!(spawns.load(Ordering::SeqCst), 2);
            drop(runtime);
            assert!(weak.upgrade().is_some());
            drop(completion);
            if recovery_panic {
                let failure = termination_failure.as_ref().expect("lost recovery failure");
                let panic = failure
                    .source()
                    .unwrap()
                    .downcast_ref::<phenix_graph::process::CallbackPanicked>()
                    .unwrap();
                assert_eq!(panic.phase, phenix_graph::CallbackPhase::RecoveryConstruct);
                assert_eq!(failure.context().request, Some(request));
                assert_eq!(failure.context().session, Some(request.session()));
                assert_eq!(failure.context().generation, Some(request.generation()));
                assert_eq!(failure.context().path.as_deref(), Some("z.cleanup"));
                if hold_cleanup {
                    assert_eq!(failure.context().process, Some(process));
                    assert_eq!(failure.context().ticket, Some(ticket));
                } else {
                    assert!(failure.context().process.is_some());
                    assert_eq!(failure.context().ticket, None);
                }
                assert_eq!(failure.context().phase, Some(panic.phase));
                assert!(weak.upgrade().is_some(), "escaped failure lost its lease");
            } else {
                assert!(termination_failure.is_none());
            }
            drop(termination_failure);
            assert!(weak.upgrade().is_none());
        }
    }
}

struct ExpireOnDrop;
impl Drop for ExpireOnDrop {
    fn drop(&mut self) {
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
}

#[test]
fn already_terminal_unrelated_failure_does_not_replace_the_query_error() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let f = Fixture::new(false);
        f.put(&f.a, 3);
        let mut graph = NetworkBuilder::new();
        let group = graph.sources("values", (&f.a,)).unwrap();
        let held = graph
            .derive_from("a.held", &group, |a: Number| a.0)
            .unwrap();
        let spec = graph.query_spec("query", &held).unwrap();
        let old = graph
            .add("old", Arc::new(ComponentDef::map(|_: ()| ExpireOnDrop)))
            .unwrap();
        graph
            .expose_input("old", old.input::<()>("input").unwrap(), capacity())
            .unwrap();
        // Drain this old unscoped capture first. Its destruction expires the
        // held query job between the driver's report and its next scoped read.
        graph
            .expose_output("a.old", old.output::<ExpireOnDrop>("output").unwrap())
            .unwrap();
        let plugin = authored(graph, spec.clone(), None);
        let dispatcher = Arc::new(HoldPeer {
            job: Mutex::new(None),
            path: "a.held",
        });
        let mut config = options(&plugin);
        config.execution.request_queue_policy = queue;
        config.execution.deferred_timeout = Some(std::time::Duration::from_secs(1));
        let mut runtime = configured(plugin, dispatcher.clone(), config);
        runtime.start().unwrap();
        runtime
            .kernel()
            .with_session(|session| {
                session.inject_local("old", ()).unwrap();
                session.run_until_idle(8);
            })
            .unwrap();
        let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
        let job = dispatcher.job.lock().unwrap().take().unwrap();
        let request = job.metadata().unwrap().request.unwrap();
        let Error::RequestInterrupted {
            cause,
            cancelled_jobs,
            disposal,
        } = &error
        else {
            panic!("lost timeout job inventory: {error:?}");
        };
        assert_eq!(*cancelled_jobs, [job.ticket()]);
        assert_eq!(disposal.handed_jobs, 1);
        let Error::Execution(failure) = cause.as_ref() else {
            panic!("an unchanged unrelated failure is not new cleanup: {cause:?}");
        };
        assert_eq!(failure.context().request, Some(request));
        assert!(failure.cleanup().is_empty());
        assert!(matches!(
            failure.source().unwrap().downcast_ref::<Error>(),
            Some(Error::Request(RequestError::Stale))
        ));
        runtime
            .kernel()
            .with_session(|session| {
                assert_eq!(session.request_status(request), Err(RequestError::Stale));
                assert!(session.complete_job(job.run()).is_err());
            })
            .unwrap();
        drop(runtime);
        assert!(matches!(
            failure.source().unwrap().downcast_ref::<Error>(),
            Some(Error::Request(RequestError::Stale))
        ));
    }
}
