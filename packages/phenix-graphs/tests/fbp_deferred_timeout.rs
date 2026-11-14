use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{
        ActivationJob, DispatchResult, HandoffError, Process, ProcessContext, ProcessDispatcher,
        ProcessEmission, Step, StepTicket,
    },
    runtime::{RequestError, RequestFailurePolicy, RequestStatus, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Payload {
    id: i64,
    drops: Arc<AtomicUsize>,
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
fn payload(id: i64) -> (Value, Arc<AtomicUsize>) {
    let drops = Arc::new(AtomicUsize::new(0));
    (
        Value::local(Payload {
            id,
            drops: drops.clone(),
        }),
        drops,
    )
}

struct Echo {
    emitted: bool,
    seen: Arc<Mutex<Vec<i64>>>,
}
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.seen
            .lock()
            .unwrap()
            .push(value.borrow::<Payload>().unwrap().id);
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", value)))
    }
}
fn echo(seen: Arc<Mutex<Vec<i64>>>) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "echo",
            Arc::new(
                Interface::new(
                    [PortDef::local::<Payload>("in")],
                    [PortDef::local::<Payload>("out")],
                )
                .unwrap(),
            ),
            Arc::new(move || Echo {
                emitted: false,
                seen: seen.clone(),
            }),
        )
        .unwrap();
    builder
        .expose_input("input", node.input::<Payload>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_output("output", node.output::<Payload>("out").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn default_timeout_accepts_direct_job_and_preserves_payload_provenance() {
    let seen = Arc::new(Mutex::new(vec![]));
    let options = SessionOptions::default();
    assert_eq!(options.deferred_job_timeout, None);
    let mut session = echo(seen.clone()).start_with_options(options);
    let (value, drops) = payload(41);
    session.inject("input", value).unwrap();
    let (index, job) = session.next_job().unwrap();
    let ticket = job.ticket();
    let metadata = job.metadata().unwrap().clone();
    assert_eq!(metadata.path, "echo");
    let report = session.run_until_idle(0);
    assert_eq!(report.state, SessionState::Waiting);
    assert_eq!(report.next_deferred_timeout, None);
    session.supervise_deferred_jobs();
    session.complete(index, job.run()).unwrap();
    let (_, continuation) = session.next_job().unwrap();
    session.complete_job(continuation.run()).unwrap();
    let packet = session.take_output("output").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<Payload>().unwrap().id, 41);
    let source = packet.metadata().unwrap();
    assert_eq!(source.session(), metadata.session);
    assert_eq!(source.generation(), metadata.generation);
    assert_eq!(source.process(), Some(index));
    assert_eq!(source.activation(), ticket.activation());
    assert_eq!(source.request(), None);
    assert_eq!(source.output(), "out");
    assert_eq!(source.capture(), Some("output"));
    assert_eq!(*seen.lock().unwrap(), [41]);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

#[test]
fn zero_timeout_refuses_direct_reject_and_both_completion_apis_intact() {
    for api in ["reject", "complete", "complete_job"] {
        let seen = Arc::new(Mutex::new(vec![]));
        let mut session = echo(seen.clone()).start_with_options(SessionOptions {
            deferred_job_timeout: Some(Duration::ZERO),
            ..Default::default()
        });
        let (value, drops) = payload(73);
        session.inject("input", value).unwrap();
        let (index, job) = session.next_job().unwrap();
        let ticket = job.ticket();
        let metadata = job.metadata().unwrap().clone();
        // Do not supervise first: each return API must itself notice expiry.
        let (error, completion) = match api {
            "reject" => {
                let (error, returned) = session.reject(index, job).unwrap_err();
                assert_eq!(error, HandoffError::StaleCompletion);
                assert_eq!(returned.ticket(), ticket);
                assert_eq!(returned.metadata().unwrap().path, metadata.path);
                assert!(seen.lock().unwrap().is_empty());
                assert_eq!(drops.load(Ordering::SeqCst), 0);
                session.complete_job(returned.run()).unwrap_err()
            }
            "complete" => session.complete(index, job.run()).unwrap_err(),
            "complete_job" => session.complete_job(job.run()).unwrap_err(),
            _ => unreachable!(),
        };
        assert_eq!(error, HandoffError::StaleCompletion, "{api}");
        assert_eq!(completion.ticket(), ticket);
        let returned = completion.metadata().unwrap();
        assert_eq!(returned.session, metadata.session);
        assert_eq!(returned.generation, metadata.generation);
        assert_eq!(returned.process, index);
        assert_eq!(returned.path, "echo");
        assert_eq!(returned.activation, ticket.activation());
        assert_eq!(returned.step, ticket.sequence());
        assert_eq!(*seen.lock().unwrap(), [73]);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(session.state(), SessionState::Failed);
        assert!(
            session
                .failure()
                .unwrap()
                .contains("deferred job timed out")
        );
        assert!(session.take_output("output").unwrap().is_none());
        assert_eq!(session.run_until_idle(4).next_deferred_timeout, None);
        drop(session);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(completion);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn synchronous_completed_dispatch_is_not_timed_with_zero_timeout() {
    let seen = Arc::new(Mutex::new(vec![]));
    let mut session = echo(seen.clone()).start_with_options(SessionOptions {
        deferred_job_timeout: Some(Duration::ZERO),
        ..Default::default()
    });
    let (value, drops) = payload(19);
    session.inject("input", value).unwrap();
    let report = session.run_until_idle(8);
    assert_eq!(report.state, SessionState::Waiting);
    assert_eq!(report.steps, 2);
    assert_eq!(report.publications, 1);
    assert!(!report.budget_exhausted);
    assert_eq!(report.next_deferred_timeout, None);
    assert!(session.failure().is_none());
    assert!(session.take_dispatch_return().is_none());
    let packet = session.take_output("output").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<Payload>().unwrap().id, 19);
    assert_eq!(packet.metadata().unwrap().process(), Some(0));
    assert_eq!(packet.metadata().unwrap().output(), "out");
    assert_eq!(*seen.lock().unwrap(), [19]);
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

#[test]
fn synchronous_rejected_dispatch_is_not_timed_with_zero_timeout() {
    struct RejectOnce(AtomicUsize);
    impl ProcessDispatcher for RejectOnce {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                DispatchResult::Rejected(job)
            } else {
                DispatchResult::Completed(job.run())
            }
        }
    }
    let seen = Arc::new(Mutex::new(vec![]));
    let dispatcher = Arc::new(RejectOnce(AtomicUsize::new(0)));
    let mut session = echo(seen.clone()).start_with_options(SessionOptions {
        deferred_job_timeout: Some(Duration::ZERO),
        dispatcher: dispatcher.clone(),
        ..Default::default()
    });
    let (value, drops) = payload(23);
    session.inject("input", value).unwrap();
    let report = session.run_until_idle(8);
    assert_eq!(report.state, SessionState::Runnable);
    assert_eq!(report.steps, 1);
    assert_eq!(report.publications, 0);
    assert_eq!(report.next_deferred_timeout, None);
    assert!(session.take_dispatch_return().is_none());
    assert!(session.failure().is_none());
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    session.supervise_deferred_jobs();
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 2);
    assert_eq!(report.publications, 1);
    assert_eq!(report.next_deferred_timeout, None);
    assert_eq!(dispatcher.0.load(Ordering::SeqCst), 3);
    let packet = session.take_output("output").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<Payload>().unwrap().id, 23);
    assert_eq!(packet.metadata().unwrap().process(), Some(0));
    assert_eq!(packet.metadata().unwrap().activation(), 1);
    assert_eq!(*seen.lock().unwrap(), [23]);
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

#[test]
fn timely_direct_rejection_clears_timeout_and_redispatches_a_fresh_ticket() {
    let seen = Arc::new(Mutex::new(vec![]));
    let mut session = echo(seen.clone()).start_with_options(SessionOptions {
        deferred_job_timeout: Some(Duration::MAX),
        ..Default::default()
    });
    let (value, drops) = payload(31);
    session.inject("input", value).unwrap();
    let (index, job) = session.next_job().unwrap();
    let old_ticket = job.ticket();
    let metadata = job.metadata().unwrap().clone();
    assert!(session.run_until_idle(0).next_deferred_timeout.unwrap() > Duration::ZERO);
    session.reject(index, job).unwrap();
    let report = session.run_until_idle(0);
    assert_eq!(report.state, SessionState::Runnable);
    assert_eq!(report.next_deferred_timeout, None);
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let (new_index, job) = session.next_job().unwrap();
    let ticket = job.ticket();
    assert_eq!(new_index, index);
    assert_ne!(ticket, old_ticket);
    assert_eq!(ticket.activation(), old_ticket.activation());
    assert!(ticket.sequence() > old_ticket.sequence());
    assert_eq!(job.metadata().unwrap().session, metadata.session);
    assert_eq!(job.metadata().unwrap().path, metadata.path);
    assert!(session.run_until_idle(0).next_deferred_timeout.is_some());
    session.complete_job(job.run()).unwrap();
    assert_eq!(session.run_until_idle(0).next_deferred_timeout, None);
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 1);
    assert_eq!(report.next_deferred_timeout, None);
    let packet = session.take_output("output").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<Payload>().unwrap().id, 31);
    assert_eq!(packet.metadata().unwrap().process(), Some(index));
    assert_eq!(
        packet.metadata().unwrap().activation(),
        old_ticket.activation()
    );
    assert_eq!(*seen.lock().unwrap(), [31]);
    drop(packet);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn timeout_restart_retains_external_job_and_lease_after_request_credit_reuse() {
    struct Lease(Arc<AtomicUsize>);
    impl Drop for Lease {
        fn drop(&mut self) {
            assert_eq!(self.0.swap(0, Ordering::SeqCst), 1);
        }
    }
    struct NativePayload {
        source: usize,
        alive: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for NativePayload {
        fn drop(&mut self) {
            assert_eq!(
                self.alive.load(Ordering::SeqCst),
                1,
                "payload lost its code lease"
            );
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct NativeSource {
        source: usize,
        emitted: bool,
        alive: Arc<AtomicUsize>,
        process_drops: Arc<AtomicUsize>,
        payload_drops: Arc<AtomicUsize>,
        seen: Arc<Mutex<Vec<usize>>>,
    }
    impl Drop for NativeSource {
        fn drop(&mut self) {
            assert_eq!(
                self.alive.load(Ordering::SeqCst),
                1,
                "process lost its code lease"
            );
            self.process_drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Process for NativeSource {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            assert_eq!(self.alive.load(Ordering::SeqCst), 1);
            if self.emitted {
                return Ok(Step::Complete);
            }
            self.emitted = true;
            self.seen.lock().unwrap().push(self.source);
            Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::local(NativePayload {
                    source: self.source,
                    alive: self.alive.clone(),
                    drops: self.payload_drops.clone(),
                }),
            )))
        }
    }
    let alive = Arc::new(AtomicUsize::new(1));
    let spawned = Arc::new(AtomicUsize::new(0));
    let process_drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(vec![]));
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "native",
            Arc::new(Interface::new([], [PortDef::local::<NativePayload>("out")]).unwrap()),
            Arc::new({
                let (alive, spawned, process_drops, payload_drops, seen) = (
                    alive.clone(),
                    spawned.clone(),
                    process_drops.clone(),
                    payload_drops.clone(),
                    seen.clone(),
                );
                move || NativeSource {
                    source: spawned.fetch_add(1, Ordering::SeqCst),
                    emitted: false,
                    alive: alive.clone(),
                    process_drops: process_drops.clone(),
                    payload_drops: payload_drops.clone(),
                    seen: seen.clone(),
                }
            }),
        )
        .unwrap();
    builder.start(&node).unwrap();
    builder
        .expose_output("output", node.output::<NativePayload>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut session = compiled.start_with_options(SessionOptions {
        auto_start_sources: false,
        max_requests: one(),
        deferred_job_timeout: Some(Duration::ZERO),
        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
        lease: Some(Arc::new(Lease(alive.clone()))),
        ..Default::default()
    });
    let old = session.begin_request(Some("output")).unwrap();
    session.start_request_source(old, "native").unwrap();
    session.close_request(old).unwrap();
    let (_, job) = session.next_job().unwrap();
    let ticket = job.ticket();
    let metadata = job.metadata().unwrap().clone();
    session.supervise_deferred_jobs();
    assert_eq!(session.request_status(old).unwrap(), RequestStatus::Failed);
    assert!(
        session
            .request_failure(old)
            .unwrap()
            .unwrap()
            .contains("deferred job timed out")
    );
    assert_eq!(session.request_cancelled_jobs(old).unwrap(), &[ticket]);
    assert_eq!(session.request_disposal(old).unwrap().handed_jobs, 1);
    assert_eq!(spawned.load(Ordering::SeqCst), 2);
    assert_eq!(process_drops.load(Ordering::SeqCst), 0);
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(session.begin_request(None), Err(RequestError::Full));
    session.release_request(old).unwrap();
    assert_eq!(session.request_status(old), Err(RequestError::Stale));
    let next = session.begin_request(Some("output")).unwrap();
    assert!(next.sequence() > old.sequence());
    session.start_request_source(next, "native").unwrap();
    session.close_request(next).unwrap();
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 2);
    assert_eq!(report.next_deferred_timeout, None);
    let packet = session
        .take_request_output(next, "output")
        .unwrap()
        .unwrap();
    assert_eq!(packet.value().borrow::<NativePayload>().unwrap().source, 1);
    let source = packet.metadata().unwrap();
    assert_eq!(source.session(), metadata.session);
    assert_eq!(source.generation(), metadata.generation);
    assert_eq!(source.process(), Some(metadata.process));
    assert_eq!(source.request(), Some(next));
    assert_eq!(source.output(), "out");
    assert!(source.activation() > ticket.activation());
    assert_eq!(
        session.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    let (error, returned) = session.complete_job(job.run()).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(returned.ticket(), ticket);
    assert_eq!(returned.metadata().unwrap().request, Some(old));
    assert_eq!(returned.metadata().unwrap().path, "native");
    assert_eq!(*seen.lock().unwrap(), [1, 0]);
    assert_eq!(payload_drops.load(Ordering::SeqCst), 0);
    assert!(session.take_output("output").unwrap().is_none());
    assert_eq!(
        session.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    session.release_request(next).unwrap();
    drop(session);
    drop(compiled);
    drop(packet);
    assert_eq!(payload_drops.load(Ordering::SeqCst), 1);
    assert_eq!(process_drops.load(Ordering::SeqCst), 1);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(returned);
    assert_eq!(payload_drops.load(Ordering::SeqCst), 2);
    assert_eq!(process_drops.load(Ordering::SeqCst), 2);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}

#[test]
fn zero_timeout_does_not_expire_wait_wake() {
    struct WakeSource(usize);
    impl Process for WakeSource {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            self.0 += 1;
            Ok(match self.0 {
                1 => Step::WaitWake,
                2 => Step::Emit(ProcessEmission::new("out", Value::local(97_i64))),
                _ => Step::Finished,
            })
        }
    }
    let mut builder = NetworkBuilder::new();
    let node = builder
        .node(
            "wake",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| WakeSource(0)),
        )
        .unwrap();
    builder.start(&node).unwrap();
    builder
        .expose_output("output", node.output::<i64>("out").unwrap())
        .unwrap();
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            deferred_job_timeout: Some(Duration::ZERO),
            ..Default::default()
        });
    let wake = session.wake_handle("wake").unwrap();
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 1);
    assert_eq!(report.state, SessionState::Waiting);
    let activation = session.last_step().unwrap().activation;
    for _ in 0..3 {
        session.supervise_deferred_jobs();
        let report = session.run_until_idle(4);
        assert_eq!(report.steps, 0);
        assert_eq!(report.state, SessionState::Waiting);
        assert_eq!(report.next_deferred_timeout, None);
        assert!(session.failure().is_none());
    }
    assert!(wake.wake());
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 2);
    let packet = session.take_output("output").unwrap().unwrap();
    assert_eq!(packet.value().borrow::<i64>(), Some(&97));
    assert_eq!(packet.metadata().unwrap().process(), Some(0));
    assert_eq!(packet.metadata().unwrap().activation(), activation);
    assert_eq!(session.state(), SessionState::Completed);
}

#[test]
fn zero_timeout_does_not_expire_nondeferred_publication_backpressure() {
    let seen = Arc::new(Mutex::new(vec![]));
    let mut session = echo(seen.clone()).start_with_options(SessionOptions {
        capture_capacity: one(),
        deferred_job_timeout: Some(Duration::ZERO),
        ..Default::default()
    });
    let (first, first_drops) = payload(101);
    session.inject("input", first).unwrap();
    assert_eq!(session.run_until_idle(8).publications, 1);
    let (second, second_drops) = payload(103);
    session.inject("input", second).unwrap();
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 1);
    assert_eq!(report.publications, 0);
    assert_eq!(report.state, SessionState::Waiting);
    assert_eq!(session.blocked_queues().len(), 1);
    for _ in 0..3 {
        session.supervise_deferred_jobs();
        let report = session.run_until_idle(4);
        assert_eq!(report.steps, 0);
        assert_eq!(report.state, SessionState::Waiting);
        assert_eq!(report.next_deferred_timeout, None);
        assert!(session.failure().is_none());
    }
    let first = session.take_output("output").unwrap().unwrap();
    assert_eq!(first.value().borrow::<Payload>().unwrap().id, 101);
    let report = session.run_until_idle(8);
    assert_eq!(report.steps, 1);
    assert_eq!(report.publications, 1);
    let second = session.take_output("output").unwrap().unwrap();
    assert_eq!(second.value().borrow::<Payload>().unwrap().id, 103);
    assert_eq!(first.metadata().unwrap().process(), Some(0));
    assert_eq!(second.metadata().unwrap().process(), Some(0));
    assert!(second.metadata().unwrap().activation() > first.metadata().unwrap().activation());
    assert_eq!(*seen.lock().unwrap(), [101, 103]);
    drop((first, second));
    assert_eq!(first_drops.load(Ordering::SeqCst), 1);
    assert_eq!(second_drops.load(Ordering::SeqCst), 1);
    assert_eq!(session.state(), SessionState::Quiescent);
}

#[test]
fn sibling_expiry_during_dispatch_does_not_fail_replacement_request() {
    const TIMEOUT: Duration = Duration::from_millis(30);
    struct ExpireSibling {
        mode: &'static str,
        calls: AtomicUsize,
        held: Mutex<Option<(ActivationJob, Instant)>>,
        tickets: Mutex<Vec<StepTicket>>,
        window: Mutex<Option<(Instant, Instant)>>,
    }
    impl ProcessDispatcher for ExpireSibling {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            match self.calls.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    assert_eq!(job.metadata().unwrap().path, "a");
                    self.tickets.lock().unwrap().push(job.ticket());
                    *self.held.lock().unwrap() = Some((job, Instant::now()));
                    DispatchResult::Deferred
                }
                1 => {
                    let entered = Instant::now();
                    assert_eq!(job.metadata().unwrap().path, "b");
                    self.tickets.lock().unwrap().push(job.ticket());
                    // a's timer was armed after its dispatcher returned, before b entered.
                    let deadline = entered + TIMEOUT;
                    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                    *self.window.lock().unwrap() = Some((entered, Instant::now()));
                    match self.mode {
                        "completed" => DispatchResult::Completed(job.run()),
                        "rejected" => DispatchResult::Rejected(job),
                        "panic" => panic!("old request's dispatcher panicked after sibling expiry"),
                        _ => unreachable!(),
                    }
                }
                _ => DispatchResult::Completed(job.run()),
            }
        }
    }

    for mode in ["completed", "rejected", "panic"] {
        for queued in [false, true] {
            let seen = Arc::new(Mutex::new(vec![]));
            let dispatcher = Arc::new(ExpireSibling {
                mode,
                calls: AtomicUsize::new(0),
                held: Mutex::new(None),
                tickets: Mutex::new(vec![]),
                window: Mutex::new(None),
            });
            let mut builder = NetworkBuilder::new();
            for name in ["a", "b"] {
                let seen = seen.clone();
                let node = builder
                    .node(
                        name,
                        Arc::new(
                            Interface::new(
                                [PortDef::local::<Payload>("in")],
                                [PortDef::local::<Payload>("out")],
                            )
                            .unwrap(),
                        ),
                        Arc::new(move || Echo {
                            emitted: false,
                            seen: seen.clone(),
                        }),
                    )
                    .unwrap();
                builder
                    .expose_input(name, node.input::<Payload>("in").unwrap(), one())
                    .unwrap();
                builder
                    .expose_output(name, node.output::<Payload>("out").unwrap())
                    .unwrap();
            }
            let mut session = builder
                .build()
                .unwrap()
                .compile()
                .unwrap()
                .start_with_options(SessionOptions {
                    max_requests: NonZeroUsize::new(2).unwrap(),
                    deferred_job_timeout: Some(TIMEOUT),
                    request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
                    dispatcher: dispatcher.clone(),
                    ..Default::default()
                });
            let a = session.begin_request(None).unwrap();
            let (a_input, a_drops) = payload(11);
            let (b_input, b_drops) = payload(13);
            session.inject_request(a, "a", a_input).unwrap();
            session.inject_request(a, "b", b_input).unwrap();
            session.close_request(a).unwrap();
            let b = session.begin_request(Some("b")).unwrap();
            let (value, drops) = payload(17);
            let mut pending = Some(value);
            if queued {
                session
                    .inject_request(b, "b", pending.take().unwrap())
                    .unwrap();
                session.close_request(b).unwrap();
            }

            let report = session.run_until_idle(2);
            assert_eq!(report.steps, 2, "{mode}, queued={queued}");
            let (job, before_arming) = dispatcher.held.lock().unwrap().take().unwrap();
            let (entered, returned) = dispatcher.window.lock().unwrap().unwrap();
            // These bounds prove expiry occurred inside b, not before its dispatch.
            assert!(entered.duration_since(before_arming) < TIMEOUT);
            assert!(returned.duration_since(entered) >= TIMEOUT);
            assert_eq!(report.publications, 0);
            assert_eq!(report.next_deferred_timeout, None);
            assert_eq!(
                report.state,
                if queued {
                    SessionState::Runnable
                } else {
                    SessionState::Quiescent
                }
            );
            assert!(session.failure().is_none(), "{mode}, queued={queued}");
            assert!(session.take_dispatch_return().is_none());
            assert_eq!(session.request_status(a).unwrap(), RequestStatus::Failed);
            assert!(
                session
                    .request_failure(a)
                    .unwrap()
                    .unwrap()
                    .contains("deferred job timed out")
            );
            assert_eq!(
                session.request_cancelled_jobs(a).unwrap(),
                dispatcher.tickets.lock().unwrap().as_slice()
            );
            assert_eq!(session.request_disposal(a).unwrap().handed_jobs, 2);
            assert_eq!(
                session.request_status(b).unwrap(),
                if queued {
                    RequestStatus::Pending
                } else {
                    RequestStatus::Open
                }
            );
            assert_eq!(session.request_work(b).unwrap().ready, usize::from(queued));
            assert_eq!(a_drops.load(Ordering::SeqCst), 0);
            assert_eq!(b_drops.load(Ordering::SeqCst), 1);
            assert_eq!(session.begin_request(None), Err(RequestError::Full));
            session.release_request(a).unwrap();
            assert_eq!(session.request_status(a), Err(RequestError::Stale));
            let reused = session.begin_request(Some("b")).unwrap();
            assert!(reused.sequence() > b.sequence());

            let ticket = job.ticket();
            let (error, late) = session.complete_job(job.run()).unwrap_err();
            assert_eq!(error, HandoffError::ForeignCompletion);
            assert_eq!(late.ticket(), ticket);
            assert_eq!(late.metadata().unwrap().request, Some(a));
            drop(late);
            assert_eq!(a_drops.load(Ordering::SeqCst), 1);
            assert!(session.take_output("a").unwrap().is_none());
            assert!(session.take_output("b").unwrap().is_none());
            if !queued {
                session
                    .inject_request(b, "b", pending.take().unwrap())
                    .unwrap();
                session.close_request(b).unwrap();
            }
            assert_eq!(session.run_until_idle(8).steps, 2);
            let packet = session.take_request_output(b, "b").unwrap().unwrap();
            assert_eq!(packet.value().borrow::<Payload>().unwrap().id, 17);
            let source = packet.metadata().unwrap();
            assert_eq!(source.request(), Some(b));
            assert_eq!(source.process(), Some(1));
            assert_eq!(source.output(), "out");
            let activation = source.activation();
            assert_eq!(
                session.request_status(b).unwrap(),
                RequestStatus::Settled { captures: 1 }
            );
            session.release_request(b).unwrap();
            drop(packet);
            assert_eq!(drops.load(Ordering::SeqCst), 1);

            let (value, drops) = payload(19);
            session.inject_request(reused, "b", value).unwrap();
            session.close_request(reused).unwrap();
            assert_eq!(session.run_until_idle(8).steps, 2);
            let packet = session.take_request_output(reused, "b").unwrap().unwrap();
            assert_eq!(packet.value().borrow::<Payload>().unwrap().id, 19);
            assert_eq!(packet.metadata().unwrap().request(), Some(reused));
            assert!(packet.metadata().unwrap().activation() > activation);
            session.release_request(reused).unwrap();
            drop(packet);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                *seen.lock().unwrap(),
                if mode == "completed" {
                    vec![13, 11, 17, 19]
                } else {
                    vec![11, 17, 19]
                }
            );
            assert!(session.take_output("a").unwrap().is_none());
            assert!(session.take_output("b").unwrap().is_none());
            assert!(session.failure().is_none());
            assert_eq!(session.state(), SessionState::Quiescent);
        }
    }
}
