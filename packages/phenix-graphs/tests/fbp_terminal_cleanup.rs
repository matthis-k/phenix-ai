use phenix_graph::{
    Interface, PortDef,
    network::{CompiledNetwork, NetworkBuilder},
    process::{ActivationJob, DispatchResult, Process, ProcessContext, ProcessDispatcher, Step},
    runtime::{
        RequestError, RequestFailurePolicy, RequestQueuePolicy, RequestStatus, SessionOptions,
        SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Failure;
impl Process for Failure {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        Err(phenix_graph::Failure::application_message(
            "initiating global failure",
        ))
    }
}

fn network() -> CompiledNetwork {
    let mut graph = NetworkBuilder::new();
    let echo = graph
        .add(
            "echo",
            Arc::new(phenix_graph::ComponentDef::map(|v: i64| v)),
        )
        .unwrap();
    graph
        .expose_input("input", echo.input::<i64>("input").unwrap(), one())
        .unwrap();
    graph
        .expose_output("output", echo.output::<i64>("output").unwrap())
        .unwrap();
    let fail = graph
        .node(
            "fail",
            Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap()),
            Arc::new(|| Failure),
        )
        .unwrap();
    graph
        .expose_input("fail", fail.input::<()>("in").unwrap(), one())
        .unwrap();
    graph.build().unwrap().compile().unwrap()
}

fn options(queue: RequestQueuePolicy, failure: RequestFailurePolicy) -> SessionOptions {
    SessionOptions {
        max_requests: NonZeroUsize::new(4).unwrap(),
        capture_capacity: NonZeroUsize::new(4).unwrap(),
        request_queue_policy: queue,
        request_failure_policy: failure,
        ..Default::default()
    }
}

#[derive(Default)]
struct Hold {
    defer: AtomicBool,
    jobs: Mutex<Vec<ActivationJob>>,
}
impl ProcessDispatcher for Hold {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        if self.defer.load(Ordering::SeqCst) && job.metadata().is_some_and(|m| m.path == "echo") {
            self.jobs.lock().unwrap().push(job);
            DispatchResult::Deferred
        } else {
            DispatchResult::Completed(job.run())
        }
    }
}

#[test]
fn session_cancel_after_failure_does_not_dispose_retained_capture() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for failure in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let mut session = network().start_with_options(options(queue, failure));
            let id = session.begin_request(None).unwrap();
            session.inject_request_local(id, "input", 42_i64).unwrap();
            session.close_request(id).unwrap();
            session.run_until_idle(16);
            session.inject_local("fail", ()).unwrap();
            assert_eq!(session.run_until_idle(16).state, SessionState::Failed);
            let original = session
                .failure()
                .map(phenix_graph::Failure::summary)
                .unwrap()
                .to_owned();
            let disposal = session.request_disposal(id).unwrap();
            let disposed = session.disposed_packets();
            session.cancel();
            session.cancel();
            assert_eq!(session.state(), SessionState::Failed);
            assert_eq!(
                session.failure().map(phenix_graph::Failure::summary),
                Some(original.as_str())
            );
            assert_eq!(session.request_status(id).unwrap(), RequestStatus::Failed);
            assert_eq!(session.request_work(id).unwrap().queued, 1);
            assert_eq!(session.request_disposal(id).unwrap(), disposal);
            assert_eq!(session.disposed_packets(), disposed);
            let packet = if queue == RequestQueuePolicy::BoundedLanes {
                session.take_request_output(id, "output").unwrap().unwrap()
            } else {
                session.take_output("output").unwrap().unwrap()
            };
            assert_eq!(packet.metadata().unwrap().request(), Some(id));
            assert_eq!(packet.value().borrow::<i64>(), Some(&42));
            session.release_request(id).unwrap();
        }
    }
}

#[test]
fn explicit_disposal_after_global_failure_keeps_failed_cause_and_exact_inventory() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for failure in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let compiled = network();
            let runner = Arc::new(Hold::default());
            let mut config = options(queue, failure);
            config.dispatcher = runner.clone();
            let mut session = compiled.start_with_options(config);
            let a = session.begin_request(None).unwrap();
            let b = session.begin_request(None).unwrap();
            let local = session.begin_request(None).unwrap();
            let peer = session.begin_request(None).unwrap();
            session.cancel_request(local).unwrap();
            for (id, value) in [(a, 11_i64), (b, 22)] {
                session.inject_request_local(id, "input", value).unwrap();
                session.close_request(id).unwrap();
                session.run_until_idle(16);
                assert_eq!(session.request_work(id).unwrap().queued, 1);
            }
            runner.defer.store(true, Ordering::SeqCst);
            session.inject_request_local(peer, "input", 33_i64).unwrap();
            session.close_request(peer).unwrap();
            session.run_until_idle(1);
            let job = runner.jobs.lock().unwrap().pop().unwrap();
            session.inject_local("fail", ()).unwrap();
            assert_eq!(session.run_until_idle(16).state, SessionState::Failed);
            let original = session
                .failure()
                .map(phenix_graph::Failure::summary)
                .unwrap()
                .to_owned();
            assert!(original.contains("initiating global failure"));
            let peer_disposal = session.request_disposal(peer).unwrap();
            assert_eq!(peer_disposal.handed_jobs, 1);
            assert_eq!(
                session.request_cancelled_jobs(peer).unwrap(),
                [job.ticket()]
            );
            assert_eq!(
                session.request_status(local).unwrap(),
                RequestStatus::Cancelled
            );

            // Global cancel after failure retains both captures and the cause.
            let disposed = session.disposed_packets();
            session.cancel();
            assert_eq!(session.disposed_packets(), disposed);
            for id in [a, b] {
                assert_eq!(session.request_status(id).unwrap(), RequestStatus::Failed);
                assert_eq!(session.request_work(id).unwrap().queued, 1);
                if queue == RequestQueuePolicy::BoundedLanes {
                    assert_eq!(session.release_request(id), Err(RequestError::NotSettled));
                }
            }

            session.cancel_request(a).unwrap();
            assert_eq!(session.request_status(a).unwrap(), RequestStatus::Failed);
            assert_eq!(
                session
                    .request_failure(a)
                    .map(|failure| failure.map(phenix_graph::Failure::summary))
                    .unwrap(),
                Some(original.as_str())
            );
            assert_eq!(session.request_work(a).unwrap().queued, 0);
            assert_eq!(session.request_work(b).unwrap().queued, 1);
            assert_eq!(session.request_disposal(a).unwrap().queued_packets, 1);
            assert_eq!(session.disposed_packets(), disposed + 1);
            let disposal = session.request_disposal(a).unwrap();
            session
                .fail_request(
                    a,
                    phenix_graph::Failure::application_message("late diagnostic"),
                )
                .unwrap();
            session.cancel_request(a).unwrap();
            assert_eq!(session.request_disposal(a).unwrap(), disposal);
            assert_eq!(session.disposed_packets(), disposed + 1);

            session
                .fail_request(
                    b,
                    phenix_graph::Failure::application_message("another late diagnostic"),
                )
                .unwrap();
            assert_eq!(session.request_status(b).unwrap(), RequestStatus::Failed);
            assert_eq!(
                session
                    .request_failure(b)
                    .map(|failure| failure.map(phenix_graph::Failure::summary))
                    .unwrap(),
                Some(original.as_str())
            );
            assert_eq!(session.request_disposal(b).unwrap().queued_packets, 1);
            assert_eq!(session.disposed_packets(), disposed + 2);
            assert_eq!(session.cancel_request(peer).unwrap(), [job.ticket()]);
            assert_eq!(
                session
                    .fail_request(peer, phenix_graph::Failure::application_message("late"))
                    .unwrap(),
                [job.ticket()]
            );
            assert_eq!(session.request_disposal(peer).unwrap(), peer_disposal);
            assert_eq!(session.request_status(peer).unwrap(), RequestStatus::Failed);
            assert!(session.complete_job(job.run()).is_err());
            assert_eq!(
                session.failure().map(phenix_graph::Failure::summary),
                Some(original.as_str())
            );

            let mut foreign = compiled.start();
            let foreign = foreign.begin_request(None).unwrap();
            assert_eq!(session.cancel_request(foreign), Err(RequestError::Foreign));
            assert_eq!(
                session.fail_request(
                    foreign,
                    phenix_graph::Failure::application_message("foreign")
                ),
                Err(RequestError::Foreign)
            );
            for id in [a, b, local, peer] {
                session.release_request(id).unwrap();
                assert_eq!(session.cancel_request(id), Err(RequestError::Stale));
                assert_eq!(
                    session
                        .fail_request(id, phenix_graph::Failure::application_message("released")),
                    Err(RequestError::Stale)
                );
            }
        }
    }
}

#[test]
fn global_cancel_then_late_failure_keeps_cancelled_without_late_diagnostic() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for failure in [
            RequestFailurePolicy::FailSession,
            RequestFailurePolicy::RestartAffectedProcess,
        ] {
            let runner = Arc::new(Hold::default());
            runner.defer.store(true, Ordering::SeqCst);
            let mut config = options(queue, failure);
            config.dispatcher = runner.clone();
            let mut session = network().start_with_options(config);
            let id = session.begin_request(None).unwrap();
            let local = session.begin_request(None).unwrap();
            session
                .fail_request(
                    local,
                    phenix_graph::Failure::application_message("earlier local failure"),
                )
                .unwrap();
            session.inject_request_local(id, "input", 1_i64).unwrap();
            session.run_until_idle(1);
            let job = runner.jobs.lock().unwrap().pop().unwrap();
            session.cancel();
            let disposal = session.request_disposal(id).unwrap();
            assert_eq!(disposal.handed_jobs, 1);
            assert_eq!(
                session
                    .fail_request(id, phenix_graph::Failure::application_message("too late"))
                    .unwrap(),
                [job.ticket()]
            );
            assert_eq!(session.cancel_request(id).unwrap(), [job.ticket()]);
            assert_eq!(
                session.request_status(id).unwrap(),
                RequestStatus::Cancelled
            );
            assert_eq!(
                session
                    .request_failure(id)
                    .map(|failure| failure.map(phenix_graph::Failure::summary))
                    .unwrap(),
                None
            );
            assert_eq!(session.failure().map(phenix_graph::Failure::summary), None);
            assert_eq!(session.request_disposal(id).unwrap(), disposal);
            session.cancel_request(local).unwrap();
            assert_eq!(
                session.request_status(local).unwrap(),
                RequestStatus::Failed
            );
            assert_eq!(
                session
                    .request_failure(local)
                    .map(|failure| failure.map(phenix_graph::Failure::summary))
                    .unwrap(),
                Some("earlier local failure")
            );
            assert!(session.complete_job(job.run()).is_err());
            session.release_request(id).unwrap();
            session.release_request(local).unwrap();
        }
    }
}

#[test]
fn live_request_cancel_records_cancelled_before_fail_session_policy_runs() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session =
            network().start_with_options(options(queue, RequestFailurePolicy::FailSession));
        let id = session.begin_request(None).unwrap();
        session.inject_request_local(id, "input", 1_i64).unwrap();
        session.cancel_request(id).unwrap();
        assert_eq!(session.state(), SessionState::Failed);
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Cancelled
        );
        let failure = session
            .failure()
            .map(phenix_graph::Failure::summary)
            .unwrap()
            .to_owned();
        session
            .fail_request(
                id,
                phenix_graph::Failure::application_message("late failure"),
            )
            .unwrap();
        assert_eq!(
            session.request_status(id).unwrap(),
            RequestStatus::Cancelled
        );
        assert_eq!(
            session.failure().map(phenix_graph::Failure::summary),
            Some(failure.as_str())
        );
        session.release_request(id).unwrap();
    }
}

#[test]
fn recovery_panic_appends_context_without_replacing_initiating_failure_or_cancellation() {
    for queue in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for cancel in [false, true] {
            let spawns = Arc::new(AtomicUsize::new(0));
            let calls = spawns.clone();
            let mut graph = NetworkBuilder::new();
            let node = graph
                .node(
                    "panic-on-recovery",
                    Arc::new(Interface::new([PortDef::local::<()>("in")], []).unwrap()),
                    Arc::new(move || {
                        assert_eq!(calls.fetch_add(1, Ordering::SeqCst), 0, "recovery panic");
                        Failure
                    }),
                )
                .unwrap();
            graph
                .expose_input("input", node.input::<()>("in").unwrap(), one())
                .unwrap();
            let mut session = graph
                .build()
                .unwrap()
                .compile()
                .unwrap()
                .start_with_options(options(queue, RequestFailurePolicy::RestartAffectedProcess));
            let id = session.begin_request(None).unwrap();
            session.inject_request_local(id, "input", ()).unwrap();
            let result = if cancel {
                session.cancel_request(id)
            } else {
                session.fail_request(
                    id,
                    phenix_graph::Failure::application_message("original request failure"),
                )
            };
            assert_eq!(result, Err(RequestError::RecoveryFailed));
            assert_eq!(
                session.request_status(id).unwrap(),
                if cancel {
                    RequestStatus::Cancelled
                } else {
                    RequestStatus::Failed
                }
            );
            let failure = session.request_failure(id).unwrap().unwrap();
            let panic = if cancel {
                failure
            } else {
                &failure.cleanup()[0]
            };
            assert!(matches!(
                std::error::Error::source(panic)
                    .unwrap()
                    .downcast_ref::<phenix_graph::process::CallbackPanicked>(),
                Some(phenix_graph::process::CallbackPanicked {
                    phase: phenix_graph::CallbackPhase::RecoveryConstruct,
                    ..
                })
            ));
            let diagnostic = failure.summary().to_owned();
            if !cancel {
                assert_eq!(diagnostic, "original request failure");
            }
            assert_eq!(
                session.failure().map(phenix_graph::Failure::summary),
                Some(diagnostic.as_str())
            );
            session.cancel_request(id).unwrap();
            session
                .fail_request(id, phenix_graph::Failure::application_message("late"))
                .unwrap();
            assert_eq!(spawns.load(Ordering::SeqCst), 2);
            assert_eq!(
                session
                    .request_failure(id)
                    .map(|failure| failure.map(phenix_graph::Failure::summary))
                    .unwrap(),
                Some(diagnostic.as_str())
            );
            session.release_request(id).unwrap();
        }
    }
}
