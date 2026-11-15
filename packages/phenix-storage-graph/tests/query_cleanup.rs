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
            let mut runtime = configured(plugin, dispatcher.clone(), config);
            runtime.start().unwrap();
            let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
            let job = dispatcher.retained.lock().unwrap().take().unwrap();
            let request = job.metadata().unwrap().request.unwrap();
            match error {
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
                            release,
                        } = *cause
                        else {
                            panic!("lost original or typed capture cleanup error: {cause:?}");
                        };
                        assert_eq!(stored, request);
                        assert_eq!(termination, Some(RequestError::PacketDestructorPanicked));
                        assert_eq!(release, None);
                        cause
                    } else {
                        cause
                    };
                    assert!(
                        matches!(*cause, Error::Wave(ref e) if e.join(" ").contains("invalid completion"))
                    );
                    assert_eq!(cancelled_jobs, [job.ticket()]);
                    assert_eq!(disposal.handed_jobs, 1);
                    assert_eq!(
                        disposal.queued_packets,
                        if cleanup_error { 2 } else { 1 },
                        "dispose retained captures exactly once"
                    );
                }
                other => panic!("lost global failure or inventory: {other:?}"),
            }
            runtime
                .kernel()
                .with_session(|session| {
                    assert_eq!(session.state(), SessionState::Failed);
                    assert_eq!(session.request_status(request), Err(RequestError::Stale));
                    assert!(session.failure().unwrap().contains("invalid completion"));
                    assert!(!session.failure().unwrap().contains("destructor panicked"));
                    assert!(session.complete_job(job.run()).is_err());
                })
                .unwrap();
        }
    }
}

struct HoldPeer(Mutex<Option<ActivationJob>>);
impl ProcessDispatcher for HoldPeer {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        if job.metadata().is_some_and(|m| m.path == "a.held") {
            *self.0.lock().unwrap() = Some(job);
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
    fn step(&mut self, ctx: &mut ProcessContext) -> Result<Step, String> {
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
        for recovery_panic in [false, true] {
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
            let dispatcher = Arc::new(HoldPeer(Mutex::new(None)));
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
            let job = dispatcher.0.lock().unwrap().take().unwrap();
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
                release,
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
            assert!(weak.upgrade().is_none());
        }
    }
}
