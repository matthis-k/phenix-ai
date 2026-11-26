use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{
        ActivationJob, DispatchResult, HandoffState, Process, ProcessContext, ProcessDispatcher,
        ProcessEmission, Step,
    },
    runtime::*,
};
use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn n(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}
type Inputs = Arc<Mutex<Vec<Vec<(String, i64)>>>>;

#[derive(Clone)]
enum Action {
    Complete,
    Wake,
    Wait(&'static str),
    Group(Readiness),
    Emit,
    SelectedEmpty,
    Finished,
}
struct Script {
    actions: VecDeque<Action>,
    inputs: Inputs,
}
impl Process for Script {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        let names: Vec<_> = cx.inputs().map(|(name, _)| name.to_owned()).collect();
        let values = names
            .into_iter()
            .map(|name| {
                let value = cx.take(&name).unwrap().get::<i64>().unwrap();
                (name, value)
            })
            .collect();
        self.inputs.lock().unwrap().push(values);
        Ok(match self.actions.pop_front().unwrap_or(Action::Complete) {
            Action::Complete => Step::Complete,
            Action::Wake => Step::WaitWake,
            Action::Wait(name) => Step::WaitInput(name.into()),
            Action::Group(policy) => Step::WaitInputs(policy),
            Action::Emit => Step::Emit(ProcessEmission::new("out", Value::of(42_i64))),
            Action::SelectedEmpty => {
                Step::Emit(ProcessEmission::select("out", Value::of(42_i64), vec![]))
            }
            Action::Finished => Step::Finished,
        })
    }
}
fn pair(actions: Vec<Action>, inputs: Inputs) -> CompiledNetwork {
    let mut b = NetworkBuilder::new();
    let node = b
        .node(
            "p",
            Arc::new(
                Interface::new(
                    ["a", "b"].map(|name| PortDef::wire::<i64>(name).unwrap()),
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(move || Script {
                actions: actions.clone().into(),
                inputs: inputs.clone(),
            }),
        )
        .unwrap();
    for (name, port) in [("a", "a"), ("a2", "a"), ("b", "b")] {
        b.expose_input(name, node.wire_input::<i64>(port).unwrap(), n(4))
            .unwrap();
    }
    b.expose_output("out", node.wire_output::<i64>("out").unwrap())
        .unwrap();
    b.build().unwrap().compile().unwrap()
}
fn options(policy: Readiness, queue_policy: RequestQueuePolicy) -> SessionOptions {
    SessionOptions {
        readiness: [("p".into(), policy)].into(),
        request_queue_policy: queue_policy,
        max_requests: n(3),
        capture_capacity: n(1),
        ..Default::default()
    }
}
fn candidate(
    explanation: &ProcessExplanation,
    request: Option<RequestId>,
) -> &CandidateExplanation {
    explanation
        .candidates
        .iter()
        .find(|c| c.request == request)
        .unwrap()
}
fn port(explanation: &ProcessExplanation, name: &str) -> usize {
    explanation
        .inputs
        .iter()
        .find(|p| p.name == name)
        .unwrap()
        .port
}
fn stable(session: &NetworkSession, request: Option<RequestId>, now: Instant) {
    let process = format!("{:?}", session.explain_process(0, now));
    let scope = request.map(|id| format!("{:?}", session.explain_request(id, now)));
    for _ in 0..3 {
        assert_eq!(format!("{:?}", session.explain_process(0, now)), process);
        assert_eq!(
            request.map(|id| format!("{:?}", session.explain_request(id, now))),
            scope
        );
    }
}

#[test]
fn builtin_assessments_match_next_actual_reservation_and_count_distinct_ports() {
    let policies = [
        Readiness::ANY,
        Readiness::ALL,
        Readiness::CORRELATED_ALL,
        Readiness::at_least(n(2)),
        Readiness::optional(vec!["a".into()], vec!["b".into()]),
    ];
    for queues in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for policy in policies.clone() {
            for second_port in [false, true] {
                let inputs = Inputs::default();
                let mut session = pair(vec![], inputs.clone())
                    .start_with_options(options(policy.clone(), queues));
                let id = session.begin_request(None).unwrap();
                // Hold a previous activation so the desired inputs remain queued.
                session.inject_request(id, "a", Value::of(0_i64)).unwrap();
                session.inject_request(id, "b", Value::of(0_i64)).unwrap();
                let (index, held) = session.next_job().unwrap();
                // Any reserved only a; consume its remaining seed before the probe.
                if matches!(policy.declaration().rule, Some(SelectionRule::Any)) {
                    session.complete(index, held.run()).unwrap();
                    let (index, job) = session.next_job().unwrap();
                    session.complete(index, job.run()).unwrap();
                    session.inject_request(id, "a", Value::of(0_i64)).unwrap();
                } else {
                    // Keep the original job unexecuted until after observation.
                    session.inject_request(id, "a", Value::of(11_i64)).unwrap();
                    session.inject_request(id, "a2", Value::of(12_i64)).unwrap();
                    if second_port {
                        session.inject_request(id, "b", Value::of(13_i64)).unwrap();
                    }
                    let explanation = session.explain_process(0, Instant::now()).unwrap();
                    assert_eq!(explanation.handoff, HandoffState::Dispatched);
                    let available =
                        matches!(explanation.readiness, ReadyAssessment::Available { .. });
                    assert_eq!(available, second_port, "{policy:?}: {explanation:?}");
                    if matches!(policy.declaration().rule, Some(SelectionRule::AtLeast(_)))
                        && !second_port
                    {
                        assert!(matches!(&candidate(&explanation, Some(id)).assessment,
                            ReadyAssessment::Blocked { reasons } if reasons.contains(&ReadinessBlock::Threshold { available_ports: 1, required_ports: 2 })));
                    }
                    stable(&session, Some(id), Instant::now());
                    let expected = match explanation.readiness {
                        ReadyAssessment::Available { selected, .. } => Some(
                            selected
                                .into_iter()
                                .map(|s| {
                                    let input = &explanation.inputs[s.port];
                                    let (queue, offset) = s.packet.unwrap();
                                    assert_eq!(offset, 0);
                                    let queue =
                                        input.queues.iter().find(|q| q.queue == queue).unwrap();
                                    let value = match &queue.producer {
                                        QueueEndpoint::Input(name) if name == "a" => 11,
                                        QueueEndpoint::Input(name) if name == "a2" => 12,
                                        QueueEndpoint::Input(name) if name == "b" => 13,
                                        other => panic!("unexpected probe queue: {other:?}"),
                                    };
                                    (input.name.clone(), value)
                                })
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    };
                    session.complete(index, held.run()).unwrap();
                    if let Some(mut expected) = expected {
                        let (_, job) = session.next_job().unwrap();
                        session.complete_job(job.run()).unwrap();
                        expected.sort();
                        let actual = inputs.lock().unwrap().last().unwrap().clone();
                        assert_eq!(actual, expected);
                    } else {
                        assert!(session.next_job().is_none());
                    }
                    continue;
                }
                let (_, held) = session.next_job().unwrap();
                session.inject_request(id, "a2", Value::of(12_i64)).unwrap();
                let explanation = session.explain_process(0, Instant::now()).unwrap();
                assert!(
                    matches!(explanation.readiness, ReadyAssessment::Available { ref selected, .. } if selected.len() == 1)
                );
                session.complete_job(held.run()).unwrap();
                let (_, job) = session.next_job().unwrap();
                session.complete_job(job.run()).unwrap();
                assert_eq!(
                    inputs.lock().unwrap().last().unwrap(),
                    &vec![("a".into(), 12)]
                );
            }
        }
    }
}

struct Opaque {
    armed: AtomicBool,
    open: AtomicBool,
    calls: AtomicUsize,
    notified: bool,
}
impl Opaque {
    fn called(&self) {
        assert!(!self.armed.load(Ordering::SeqCst));
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
impl ReadinessPolicy for Opaque {
    fn closure_ports(&self, _: usize) -> Vec<String> {
        self.called();
        vec!["a".into()]
    }
    fn notification_driven(&self) -> bool {
        self.called();
        self.notified
    }
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.called();
        snapshot
            .heads
            .iter()
            .filter(|h| self.open.load(Ordering::SeqCst) && h.available)
            .map(|h| h.port.clone())
            .collect()
    }
}

#[test]
fn passive_inspection_retains_opaque_correlation_declaration() {
    struct CorrelatedOpaque(Arc<AtomicUsize>);
    impl ReadinessPolicy for CorrelatedOpaque {
        fn declaration(&self) -> PolicyDeclaration {
            PolicyDeclaration {
                rule: None,
                correlated: true,
            }
        }

        fn select(&self, _: &ReadinessSnapshot) -> Vec<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            vec![]
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Readiness::from_policy(CorrelatedOpaque(calls.clone()));
    let expected = policy.declaration().clone();
    let session = pair(vec![], Inputs::default())
        .start_with_options(options(policy, RequestQueuePolicy::SharedFifo));
    let before = calls.load(Ordering::SeqCst);
    let explanation = session.explain_process(0, Instant::now()).unwrap();
    assert_eq!(explanation.policy, expected);
    assert_eq!(explanation.readiness, ReadyAssessment::UnknownCustom);
    assert_eq!(calls.load(Ordering::SeqCst), before);
}

#[test]
fn custom_is_opaque_and_repeated_format_drop_queries_do_not_call_or_consume_notifications() {
    for notified in [false, true] {
        for queues in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            let policy = Arc::new(Opaque {
                armed: AtomicBool::new(false),
                open: AtomicBool::new(false),
                calls: AtomicUsize::new(0),
                notified,
            });
            let mut session = pair(vec![], Inputs::default())
                .start_with_options(options(Readiness::new(policy.clone()), queues));
            let id = session.begin_request(None).unwrap();
            session.inject_request(id, "a", Value::of(1_i64)).unwrap();
            session.inject_request(id, "b", Value::of(2_i64)).unwrap();
            session.close_request(id).unwrap();
            let notifier = session.process_notifier("p").unwrap();
            notifier.notify();
            let calls = policy.calls.load(Ordering::SeqCst);
            policy.armed.store(true, Ordering::SeqCst);
            let now = Instant::now();
            let explanation = session.explain_process(0, now).unwrap();
            assert_eq!(explanation.readiness, ReadyAssessment::UnknownCustom);
            assert!(!explanation.wake.pending);
            assert_eq!(explanation.notification_driven, notified);
            assert!(
                candidate(&explanation, Some(id))
                    .inputs
                    .iter()
                    .all(|h| h.selectable.is_some())
            );
            let request = session.explain_request(id, now).unwrap();
            assert_eq!(request.work.closure_pending, 1);
            assert_eq!(request.closure_obligations[0].unobserved_ports, [0]);
            assert_eq!(request.work.queued, 2);
            stable(&session, Some(id), now);
            assert_eq!(policy.calls.load(Ordering::SeqCst), calls);
            policy.armed.store(false, Ordering::SeqCst);
            session.run_until_idle(1);
            assert!(policy.calls.load(Ordering::SeqCst) > calls);
        }
    }
}

#[test]
fn fifo_foreign_heads_correlated_offsets_and_mixed_unscoped_evidence() {
    for queues in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for correlated in [false, true] {
            let policy = if correlated {
                Readiness::CORRELATED_ALL
            } else {
                Readiness::ALL
            };
            let mut session = pair(vec![Action::Wake], Inputs::default())
                .start_with_options(options(policy, queues));
            let a = session.begin_request(None).unwrap();
            let b = session.begin_request(None).unwrap();
            for port in ["a", "b"] {
                session.inject_request(a, port, Value::of(0_i64)).unwrap();
            }
            session.run_until_idle(1);
            session.inject_request(a, "a", Value::of(1_i64)).unwrap();
            session.inject_request(b, "a", Value::of(2_i64)).unwrap();
            session.inject_request(b, "b", Value::of(3_i64)).unwrap();
            assert!(
                session
                    .explain_process(0, Instant::now())
                    .unwrap()
                    .inputs
                    .iter()
                    .flat_map(|i| &i.queues)
                    .any(|q| q.head == QueueHeadExplanation::Empty)
            );
            session.inject("a2", Value::of(4_i64)).unwrap();
            let e = session.explain_process(0, Instant::now()).unwrap();
            assert_eq!(e.handoff, HandoffState::WaitingWake);
            assert!(!e.scheduled);
            let b_candidate = candidate(&e, Some(b));
            assert_eq!(
                matches!(b_candidate.assessment, ReadyAssessment::Available { .. }),
                correlated || queues == RequestQueuePolicy::BoundedLanes
            );
            let selected = b_candidate
                .inputs
                .iter()
                .find(|i| i.port == port(&e, "a"))
                .unwrap()
                .selectable;
            assert_eq!(
                selected.map(|(_, offset)| offset),
                if queues == RequestQueuePolicy::BoundedLanes {
                    Some(0)
                } else if correlated {
                    Some(1)
                } else {
                    None
                }
            );
            assert!(
                e.inputs
                    .iter()
                    .flat_map(|i| &i.queues)
                    .any(|q| q.head == QueueHeadExplanation::Unscoped)
            );
            let r = session.explain_request(b, Instant::now()).unwrap();
            assert!(r.owners.is_empty());
            assert_eq!(r.work.waiting_wake, 0);
            assert!(
                r.queues
                    .iter()
                    .all(|q| q.consumer_ownership == ConsumerOwnership::OtherRequest(a))
            );
            assert_eq!(
                r.queues.iter().map(|q| q.matching_packets).sum::<usize>(),
                r.work.queued
            );
            stable(&session, Some(b), Instant::now());
        }
    }
}

#[test]
fn wait_input_and_groups_link_owner_and_keep_closure_proof_separate_from_freshness() {
    for queues in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        for action in [
            Action::Wait("b"),
            Action::Group(Readiness::ALL),
            Action::Group(Readiness::at_least(n(2))),
        ] {
            let mut session = pair(vec![action.clone()], Inputs::default())
                .start_with_options(options(Readiness::ANY, queues));
            let id = session.begin_request(None).unwrap();
            session.inject_request(id, "a", Value::of(1_i64)).unwrap();
            session.run_until_idle(1);
            let now = Instant::now();
            let e = session.explain_process(0, now).unwrap();
            assert_eq!(e.handoff, HandoffState::WaitingInput);
            assert_eq!(e.owner, Some(id));
            assert_eq!(
                e.candidates
                    .iter()
                    .filter(|c| c.scheduler_candidate)
                    .count(),
                1
            );
            assert!(matches!(e.readiness, ReadyAssessment::Blocked { .. }));
            assert!(candidate(&e, Some(id)).inputs.iter().all(|h| matches!(h.closure,
                ClosureProof::NotProven(ClosureWitness::UnsealedRequest { request }) if request == id)));
            stable(&session, Some(id), now);
            session.close_request(id).unwrap();
            if matches!(action, Action::Wait(_)) {
                let e = session.explain_process(0, now).unwrap();
                assert_eq!(e.handoff, HandoffState::Ready);
                let b = candidate(&e, Some(id))
                    .inputs
                    .iter()
                    .find(|h| h.port == port(&e, "b"))
                    .unwrap();
                assert_eq!(b.closure, ClosureProof::ScopedRequestAbsent);
                assert!(!b.fresh);
            } else {
                assert_eq!(session.request_status(id), Ok(RequestStatus::Failed));
                assert!(
                    session
                        .explain_request(id, now)
                        .unwrap()
                        .failure
                        .unwrap()
                        .contains("staged")
                );
            }
        }
    }
}

#[test]
fn observed_wait_input_closure_and_staged_exhaustion_are_explained_without_driving() {
    let mut session = pair(
        vec![Action::Wait("b"), Action::Wait("b")],
        Inputs::default(),
    )
    .start();
    session.inject("a", Value::of(1_i64)).unwrap();
    session.close_input("b").unwrap();
    session.run_until_idle(1);
    let (index, job) = session.next_job().unwrap();
    // Completion immediately applies the existing observed-closure validation.
    session.complete(index, job.run()).unwrap();
    assert_eq!(session.state(), SessionState::Failed);
    assert!(
        session
            .explain_process(0, Instant::now())
            .unwrap()
            .session
            .failure
            .unwrap()
            .contains("already observed closure")
    );
}

#[test]
fn global_eos_wins_unsealed_scope_and_buffered_global_input_is_not_eos() {
    let mut session = pair(vec![Action::Wake], Inputs::default()).start();
    let id = session.begin_request(None).unwrap();
    session.inject_request(id, "a", Value::of(0_i64)).unwrap();
    session.run_until_idle(1);
    session.inject("a", Value::of(7_i64)).unwrap();
    for name in ["a", "a2", "b"] {
        session.close_input(name).unwrap();
    }
    let e = session.explain_process(0, Instant::now()).unwrap();
    assert_eq!(
        candidate(&e, Some(id))
            .inputs
            .iter()
            .find(|h| h.port == port(&e, "b"))
            .unwrap()
            .closure,
        ClosureProof::GlobalEos
    );
    assert!(matches!(
        candidate(&e, None)
            .inputs
            .iter()
            .find(|h| h.port == port(&e, "a"))
            .unwrap()
            .closure,
        ClosureProof::NotProven(ClosureWitness::BufferedGlobalInput { .. })
    ));
    stable(&session, Some(id), Instant::now());
}

#[test]
fn wake_bits_are_passive_notifier_does_not_resume_and_idle_sources_need_a_wake() {
    for source in [false, true] {
        let mut b = NetworkBuilder::new();
        let node = b
            .node(
                "p",
                Arc::new(Interface::default()),
                Arc::new(|| Script {
                    actions: vec![Action::Wake, Action::Complete].into(),
                    inputs: Inputs::default(),
                }),
            )
            .unwrap();
        b.start(&node).unwrap();
        let mut s = b
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .start_with_options(SessionOptions {
                auto_start_sources: !source,
                ..Default::default()
            });
        if !source {
            s.run_until_idle(1);
        }
        let wake = s.wake_handle("p").unwrap();
        let notifier = s.process_notifier("p").unwrap();
        let e = s.explain_process(0, Instant::now()).unwrap();
        assert!(e.wake.active && e.wake.resume_eligible && !e.wake.pending);
        assert_eq!(
            e.handoff,
            if source {
                HandoffState::Idle
            } else {
                HandoffState::WaitingWake
            }
        );
        notifier.notify();
        assert!(!s.explain_process(0, Instant::now()).unwrap().wake.pending);
        assert!(s.next_job().is_none());
        wake.wake();
        let now = Instant::now();
        stable(&s, None, now);
        assert!(s.explain_process(0, now).unwrap().wake.pending);
        let (_, job) = s.next_job().unwrap();
        assert!(!s.explain_process(0, now).unwrap().wake.pending);
        s.complete_job(job.run()).unwrap();
    }
}

#[derive(Default)]
struct Deferred(Mutex<Vec<ActivationJob>>);
impl ProcessDispatcher for Deferred {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        self.0.lock().unwrap().push(job);
        DispatchResult::Deferred
    }
}
#[test]
fn exact_outstanding_tickets_direct_deferred_unsupervised_and_future_clock_are_passive() {
    for deferred in [false, true] {
        for timeout in [None, Some(Duration::from_secs(60))] {
            let dispatcher = Arc::new(Deferred::default());
            let mut opts = options(Readiness::ANY, RequestQueuePolicy::SharedFifo);
            opts.deferred_timeout = timeout;
            opts.dispatcher = dispatcher.clone();
            let mut s = pair(vec![], Inputs::default()).start_with_options(opts);
            let id = s.begin_request(None).unwrap();
            s.inject_request(id, "a", Value::of(1_i64)).unwrap();
            let (index, job) = if deferred {
                s.run_until_idle(1);
                (0, dispatcher.0.lock().unwrap().pop().unwrap())
            } else {
                s.next_job().unwrap()
            };
            let now = Instant::now() + Duration::from_secs(120);
            let e = s.explain_process(index, now).unwrap();
            let observed = e.job.unwrap();
            assert_eq!(observed.ticket, job.ticket());
            assert_eq!(observed.dispatched_at.is_some(), timeout.is_some());
            assert_eq!(observed.overdue, timeout.is_some());
            assert_eq!(observed.overdue, !s.overdue_jobs(now).is_empty());
            assert_eq!(s.explain_request(id, now).unwrap().owners[0].job, e.job);
            stable(&s, Some(id), now);
            assert_eq!(s.request_status(id), Ok(RequestStatus::Open));
            s.complete(index, job.run()).unwrap();
            assert!(s.explain_process(index, now).unwrap().job.is_none());
        }
    }
}

#[test]
fn retained_capture_targets_cross_request_backpressure_and_empty_selection_captures() {
    for selected in [false, true] {
        let action = if selected {
            Action::SelectedEmpty
        } else {
            Action::Emit
        };
        let mut s = pair(
            vec![action.clone(), Action::Complete, action],
            Inputs::default(),
        )
        .start_with_options(options(Readiness::ANY, RequestQueuePolicy::SharedFifo));
        let a = s.begin_request(Some("out")).unwrap();
        let b = s.begin_request(Some("out")).unwrap();
        s.inject_request(a, "a", Value::of(1_i64)).unwrap();
        s.close_request(a).unwrap();
        s.run_until_idle(2);
        s.inject_request(b, "a", Value::of(2_i64)).unwrap();
        s.close_request(b).unwrap();
        s.run_until_idle(1);
        let now = Instant::now();
        let e = s.explain_process(0, now).unwrap();
        let publication = e.publication.unwrap();
        assert_eq!(publication.retained_packets, 1);
        assert_eq!(publication.assessment, PublicationAssessment::Full);
        assert_eq!(publication.targets.len(), 1);
        assert!(matches!(
            publication.targets[0].consumer,
            QueueEndpoint::Capture(_)
        ));
        assert_eq!(
            publication.targets[0].head,
            QueueHeadExplanation::Request {
                id: a,
                live_lane: Some(0)
            }
        );
        assert_eq!(
            s.explain_request(b, now).unwrap().work.capture_backpressure,
            1
        );
        stable(&s, Some(b), now);
        s.take_request_output(a, "out").unwrap().unwrap();
        s.run_until_idle(2);
        assert!(s.explain_process(0, now).unwrap().publication.is_none());
        assert_eq!(
            s.request_status(a),
            Ok(RequestStatus::Settled { captures: 1 })
        );
        s.take_request_output(b, "out").unwrap().unwrap();
        assert_eq!(
            s.request_status(b),
            Ok(RequestStatus::Settled { captures: 1 })
        );
    }
}

#[test]
fn request_status_counts_authentication_release_reuse_and_materialized_terminal_provenance() {
    let compiled = pair(vec![Action::Wake], Inputs::default());
    let mut s = compiled.start();
    let mut foreign = compiled.start();
    let foreign_id = foreign.begin_request(None).unwrap();
    assert_eq!(
        s.explain_request(foreign_id, Instant::now()).unwrap_err(),
        RequestError::Foreign
    );
    let local = s.begin_request(Some("out")).unwrap();
    let inherited = s.begin_request(None).unwrap();
    assert_eq!(
        s.explain_request(local, Instant::now()).unwrap().status,
        Ok(RequestStatus::Open)
    );
    s.inject_request(local, "a", Value::of(1_i64)).unwrap();
    s.close_request(local).unwrap();
    assert_eq!(
        s.explain_request(local, Instant::now()).unwrap().status,
        Ok(RequestStatus::Pending)
    );
    s.run_until_idle(1);
    s.fail_request(
        local,
        phenix_graph::Failure::application_message("initiating failure"),
    )
    .unwrap();
    let now = Instant::now();
    assert_eq!(
        s.explain_request(local, now).unwrap().terminal_origin,
        Some(TerminalOrigin::Local)
    );
    assert_eq!(
        s.explain_request(inherited, now).unwrap().terminal_origin,
        Some(TerminalOrigin::InheritedSession)
    );
    s.cancel_request(inherited).unwrap();
    let explanation = s.explain_request(inherited, now).unwrap();
    assert_eq!(explanation.status, Ok(RequestStatus::Failed));
    assert_eq!(
        explanation.terminal_origin,
        Some(TerminalOrigin::InheritedSession)
    );
    assert_eq!(explanation.failure.as_deref(), Some("initiating failure"));
    stable(&s, Some(inherited), now);
    s.release_request(inherited).unwrap();
    assert_eq!(
        s.explain_request(inherited, now).unwrap_err(),
        RequestError::Stale
    );

    let mut s = compiled.start();
    let id = s.begin_request(Some("out")).unwrap();
    s.close_request(id).unwrap();
    assert_eq!(
        s.explain_request(id, now).unwrap().status,
        Err(RequestError::CaptureCount(0))
    );
    s.release_request(id).unwrap();
    let reused = s.begin_request(None).unwrap();
    assert_eq!(s.explain_request(reused, now).unwrap().lane, 0);
    assert_ne!(id, reused);
    assert_eq!(s.explain_request(id, now).unwrap_err(), RequestError::Stale);
    s.close_request(reused).unwrap();
    assert_eq!(
        s.explain_request(reused, now).unwrap().status,
        Ok(RequestStatus::Settled { captures: 0 })
    );
}

#[test]
fn startup_failure_reports_actual_handoff_and_cause_not_a_fabricated_activation() {
    let mut opts = SessionOptions::default();
    opts.readiness
        .insert("not-a-process".into(), Readiness::ANY);
    let s = pair(vec![], Inputs::default()).start_with_options(opts);
    let e = s.explain_process(0, Instant::now()).unwrap();
    assert_eq!(e.session.terminal, Some(SessionState::Failed));
    assert!(e.session.failure.unwrap().contains("unknown process"));
    assert_eq!(e.handoff, HandoffState::Cancelled);
    assert!(!e.scheduled);
    assert!(e.job.is_none());
    assert!(s.explain_process(1, Instant::now()).is_none());
}

#[test]
fn iterative_large_upstream_proof_uses_owner_witness_then_scoped_absence_with_open_writer() {
    let mut b = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("a").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let mut nodes = vec![];
    for i in 0..2048 {
        nodes.push(
            b.node(
                format!("p{i:04}"),
                interface.clone(),
                Arc::new(|| Script {
                    actions: VecDeque::new(),
                    inputs: Inputs::default(),
                }),
            )
            .unwrap(),
        );
    }
    b.expose_input("a", nodes[0].wire_input::<i64>("a").unwrap(), n(1))
        .unwrap();
    for pair in nodes.windows(2) {
        b.connect(
            pair[0].wire_output::<i64>("out").unwrap(),
            pair[1].wire_input::<i64>("a").unwrap(),
            n(1),
        )
        .unwrap();
    }
    let compiled = b.build().unwrap().compile().unwrap();
    let mut s = compiled.start();
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "a", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    let (index, held) = s.next_job().unwrap();
    let now = Instant::now();
    let e = s.explain_process(2047, now).unwrap();
    assert_eq!(
        candidate(&e, Some(id)).inputs[0].closure,
        ClosureProof::NotProven(ClosureWitness::UpstreamOwner { process: index })
    );
    s.complete(index, held.run()).unwrap();
    let e = s.explain_process(2047, now).unwrap();
    assert_eq!(
        candidate(&e, Some(id)).inputs[0].closure,
        ClosureProof::ScopedRequestAbsent
    );
    assert!(matches!(
        candidate(&e, None).inputs[0].closure,
        ClosureProof::NotProven(ClosureWitness::OpenGlobalWriter { .. })
    ));
    assert!(candidate(&e, Some(id)).inputs[0].fresh);
    let request = s.explain_request(id, now).unwrap();
    assert!(
        request.owners.is_empty()
            && request.queues.is_empty()
            && request.closure_obligations.is_empty()
    );
    assert_eq!(request.status, Ok(RequestStatus::Settled { captures: 0 }));
    for _ in 0..3 {
        assert_eq!(
            format!("{:?}", s.explain_process(2047, now)),
            format!("{:?}", Some(e.clone()))
        );
    }
}

#[test]
fn cycle_closure_obligation_uses_cached_custom_names_and_is_not_mistaken_for_reachability() {
    let mut b = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("a").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let nodes: Vec<_> = ["root", "middle", "tail"]
        .into_iter()
        .map(|name| {
            b.node(
                name,
                interface.clone(),
                Arc::new(|| Script {
                    actions: VecDeque::new(),
                    inputs: Inputs::default(),
                }),
            )
            .unwrap()
        })
        .collect();
    b.expose_input("a", nodes[0].wire_input::<i64>("a").unwrap(), n(1))
        .unwrap();
    for (from, to) in [(0, 1), (1, 2), (2, 0)] {
        b.connect(
            nodes[from].wire_output::<i64>("out").unwrap(),
            nodes[to].wire_input::<i64>("a").unwrap(),
            n(1),
        )
        .unwrap();
    }
    let compiled = b.build().unwrap().compile().unwrap();
    let index = |name| {
        compiled
            .processes()
            .iter()
            .position(|p| p.path() == name)
            .unwrap()
    };
    let custom = Arc::new(Opaque {
        armed: AtomicBool::new(false),
        open: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        notified: true,
    });
    let mut s = compiled.start_with_options(SessionOptions {
        readiness: [("middle".into(), Readiness::new(custom.clone()))].into(),
        ..Default::default()
    });
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "a", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    s.run_until_idle(1);
    custom.armed.store(true, Ordering::SeqCst);
    let now = Instant::now();
    let e = s.explain_process(index("tail"), now).unwrap();
    assert_eq!(
        candidate(&e, Some(id)).inputs[0].closure,
        ClosureProof::NotProven(ClosureWitness::ClosureObligation {
            process: index("middle")
        })
    );
    let request = s.explain_request(id, now).unwrap();
    assert_eq!(request.closure_obligations.len(), 1);
    assert_eq!(request.closure_obligations[0].process, index("middle"));
    assert_eq!(request.closure_obligations[0].unobserved_ports, [0]);
    stable(&s, Some(id), now);
}

#[test]
fn required_optional_port_waits_for_proven_absence_not_just_sealing_and_packet_witness_is_indexed()
{
    let mut s = pair(vec![Action::Wake], Inputs::default()).start_with_options(options(
        Readiness::optional(vec!["a".into()], vec!["b".into()]),
        RequestQueuePolicy::SharedFifo,
    ));
    let id = s.begin_request(None).unwrap();
    for name in ["a", "b"] {
        s.inject_request(id, name, Value::of(0_i64)).unwrap();
    }
    s.run_until_idle(1);
    s.inject_request(id, "a", Value::of(1_i64)).unwrap();
    let now = Instant::now();
    let e = s.explain_process(0, now).unwrap();
    assert!(
        matches!(e.readiness, ReadyAssessment::Blocked { reasons } if reasons.contains(&ReadinessBlock::AwaitingOptionalInputOrClosure { port: 1 }))
    );
    s.close_request(id).unwrap();
    let e = s.explain_process(0, now).unwrap();
    assert!(
        matches!(e.readiness, ReadyAssessment::Available { ref selected, .. } if selected.len() == 2 && selected[1].packet.is_none())
    );
    assert!(matches!(
        candidate(&e, Some(id)).inputs[0].closure,
        ClosureProof::NotProven(ClosureWitness::RequestPacket { count: 1, .. })
    ));
    assert_eq!(
        candidate(&e, Some(id)).inputs[1].closure,
        ClosureProof::ScopedRequestAbsent
    );
    assert_eq!(e.handoff, HandoffState::WaitingWake);
    stable(&s, Some(id), now);
}

#[test]
fn publication_reports_all_selected_internal_and_capture_targets_and_closed_beats_full() {
    let mut b = NetworkBuilder::new();
    let producer = b
        .node(
            "producer",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("a").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Script {
                actions: vec![Action::Emit, Action::Emit].into(),
                inputs: Inputs::default(),
            }),
        )
        .unwrap();
    let consumer = b
        .node(
            "consumer",
            Arc::new(Interface::new([PortDef::wire::<i64>("a").unwrap()], []).unwrap()),
            Arc::new(|| Script {
                actions: vec![Action::Finished].into(),
                inputs: Inputs::default(),
            }),
        )
        .unwrap();
    b.expose_input("a", producer.wire_input::<i64>("a").unwrap(), n(1))
        .unwrap();
    b.expose_output("out", producer.wire_output::<i64>("out").unwrap())
        .unwrap();
    b.connect(
        producer.wire_output::<i64>("out").unwrap(),
        consumer.wire_input::<i64>("a").unwrap(),
        n(1),
    )
    .unwrap();
    let gate = Arc::new(Opaque {
        armed: AtomicBool::new(false),
        open: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        notified: true,
    });
    let compiled = b.build().unwrap().compile().unwrap();
    let p = compiled
        .processes()
        .iter()
        .position(|p| p.path() == "producer")
        .unwrap();
    let mut s = compiled.start_with_options(SessionOptions {
        capture_capacity: n(1),
        readiness: [("consumer".into(), Readiness::new(gate.clone()))].into(),
        ..Default::default()
    });
    s.inject("a", Value::of(1_i64)).unwrap();
    s.run_until_idle(2);
    let now = Instant::now();
    let e = s.explain_process(p, now).unwrap();
    let publication = e.publication.unwrap();
    assert_eq!(publication.retained_packets, 2);
    assert_eq!(publication.targets.len(), 2);
    assert_eq!(publication.assessment, PublicationAssessment::Full);
    gate.armed.store(true, Ordering::SeqCst);
    assert_eq!(
        format!("{:?}", s.explain_process(p, now).unwrap().publication),
        format!("{:?}", Some(publication))
    );
    gate.armed.store(false, Ordering::SeqCst);
    gate.open.store(true, Ordering::SeqCst);
    s.process_notifier("consumer").unwrap().notify();
    let consumer = compiled
        .processes()
        .iter()
        .position(|p| p.path() == "consumer")
        .unwrap();
    let (_, job) = s.next_job().unwrap();
    assert_eq!(job.metadata().unwrap().process, consumer);
    s.complete_job(job.run()).unwrap();
    let publication = s.explain_process(p, now).unwrap().publication.unwrap();
    assert!(matches!(
        publication.assessment,
        PublicationAssessment::ClosedTargetError {
            error: phenix_graph::connection::ConnectionError::ReaderClosed,
            ..
        }
    ));
    assert!(
        publication
            .targets
            .iter()
            .any(|q| q.occupancy == q.capacity && matches!(q.consumer, QueueEndpoint::Capture(_)))
    );
}

#[test]
fn terminal_retained_capture_head_can_have_released_id_without_lane_authority() {
    let mut opts = options(Readiness::ANY, RequestQueuePolicy::SharedFifo);
    opts.capture_capacity = n(2);
    let mut s = pair(
        vec![
            Action::Emit,
            Action::Complete,
            Action::Emit,
            Action::Complete,
            Action::Wake,
        ],
        Inputs::default(),
    )
    .start_with_options(opts);
    let a = s.begin_request(None).unwrap();
    let b = s.begin_request(None).unwrap();
    let failing = s.begin_request(None).unwrap();
    for id in [a, b] {
        s.inject_request(id, "a", Value::of(1_i64)).unwrap();
        s.close_request(id).unwrap();
        s.run_until_idle(2);
    }
    s.inject_request(failing, "a", Value::of(1_i64)).unwrap();
    s.run_until_idle(1);
    s.fail_request(failing, phenix_graph::Failure::application_message("stop"))
        .unwrap();
    let now = Instant::now();
    assert_eq!(
        s.explain_request(b, now).unwrap().queues[0].queue.head,
        QueueHeadExplanation::Request {
            id: a,
            live_lane: Some(0)
        }
    );
    s.release_request(a).unwrap();
    let e = s.explain_request(b, now).unwrap();
    assert_eq!(e.status, Ok(RequestStatus::Failed));
    assert_eq!(e.work.queued, 1);
    assert_eq!(e.queues[0].matching_packets, 1);
    assert_eq!(
        e.queues[0].queue.head,
        QueueHeadExplanation::Request {
            id: a,
            live_lane: None
        }
    );
    assert_eq!(e.queues[0].queue.occupancy, 2);
    stable(&s, Some(b), now);
    s.cancel_request(b).unwrap();
    let e = s.explain_request(b, now).unwrap();
    assert_eq!(e.terminal_origin, Some(TerminalOrigin::InheritedSession));
    assert!(e.queues.is_empty());
    assert_eq!(e.disposal.queued_packets, 1);
}

#[test]
fn invalid_capture_is_an_inner_status_error_and_observed_is_not_queued_count() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "p",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("a").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Script {
                actions: vec![Action::Emit].into(),
                inputs: Inputs::default(),
            }),
        )
        .unwrap();
    b.expose_input("a", p.wire_input::<i64>("a").unwrap(), n(1))
        .unwrap();
    for name in ["out", "alias"] {
        b.expose_output(name, p.wire_output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut s = b.build().unwrap().compile().unwrap().start();
    let id = s.begin_request(Some("out")).unwrap();
    s.inject_request(id, "a", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    s.run_until_idle(2);
    let now = Instant::now();
    assert_eq!(s.explain_request(id, now).unwrap().observed_captures, 0);
    assert_eq!(s.explain_request(id, now).unwrap().work.queued, 2);
    s.take_output("alias").unwrap().unwrap();
    let e = s.explain_request(id, now).unwrap();
    assert_eq!(e.status, Err(RequestError::InvalidCapture));
    assert!(e.invalid_capture);
    assert_eq!(e.observed_captures, 1);
    assert_eq!(e.expected_capture.as_deref(), Some("out"));
    stable(&s, Some(id), now);
}

#[test]
fn stranded_eos_failure_is_not_reported_as_merely_waiting_for_inputs() {
    let mut s = pair(vec![Action::Complete], Inputs::default())
        .start_with_options(options(Readiness::ALL, RequestQueuePolicy::SharedFifo));
    s.inject("a", Value::of(1_i64)).unwrap();
    for name in ["a", "a2", "b"] {
        s.close_input(name).unwrap();
    }
    // Deliver the fresh EOS notification for b; the remaining a cannot form All.
    s.run_until_idle(2);
    let e = s.explain_process(0, Instant::now()).unwrap();
    assert_eq!(e.session.terminal, Some(SessionState::Failed));
    assert!(e.session.failure.unwrap().contains("stranded input"));
    assert_eq!(e.handoff, HandoffState::Cancelled);
    assert!(e.stranded_after_empty_selection.is_none());
    assert!(!e.scheduled);
}

#[test]
fn successful_empty_selected_publication_is_commit_eligible_even_with_no_targets() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "p",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("a").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Script {
                actions: vec![Action::SelectedEmpty].into(),
                inputs: Inputs::default(),
            }),
        )
        .unwrap();
    b.expose_input("a", p.wire_input::<i64>("a").unwrap(), n(1))
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    s.inject("a", Value::of(1_i64)).unwrap();
    let (index, job) = s.next_job().unwrap();
    s.complete(index, job.run()).unwrap();
    let now = Instant::now();
    let e = s.explain_process(index, now).unwrap();
    let publication = e.publication.unwrap();
    assert_eq!(e.handoff, HandoffState::PublicationPending);
    assert!(e.scheduled);
    assert_eq!(publication.retained_packets, 0);
    assert!(publication.targets.is_empty());
    assert_eq!(
        publication.assessment,
        PublicationAssessment::CommitEligible
    );
    stable(&s, None, now);
    s.run_until_idle(1);
    assert_eq!(s.discarded_emissions(), 1);
    assert!(s.explain_process(index, now).unwrap().publication.is_none());
}

#[derive(Default)]
struct Callbacks {
    armed: AtomicBool,
    calls: AtomicUsize,
    drops: AtomicUsize,
}
impl Callbacks {
    fn call(&self) {
        assert!(
            !self.armed.load(Ordering::SeqCst),
            "application callback during explanation"
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
struct CallbackProcess(Arc<Callbacks>);
impl Process for CallbackProcess {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        self.0.call();
        Ok(Step::Complete)
    }
}
struct CallbackFactory(Arc<Callbacks>, Arc<Opaque>);
impl phenix_graph::process::ProcessFactory for CallbackFactory {
    fn spawn(&self) -> Box<dyn Process> {
        self.0.call();
        Box::new(CallbackProcess(self.0.clone()))
    }
    fn readiness(&self) -> Readiness {
        self.0.call();
        Readiness::new(self.1.clone())
    }
    fn correlation_lanes(&self) -> Option<NonZeroUsize> {
        self.0.call();
        None
    }
}
impl ProcessDispatcher for Callbacks {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        self.call();
        DispatchResult::Completed(job.run())
    }
}
impl phenix_graph::process::ProcessExecutionGuard for Callbacks {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, phenix_graph::Failure> {
        self.call();
        Ok(Box::new(()))
    }
}
struct CallbackPayload(Arc<Callbacks>);
impl std::fmt::Debug for CallbackPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.call();
        f.write_str("CallbackPayload")
    }
}
impl Drop for CallbackPayload {
    fn drop(&mut self) {
        assert!(!self.0.armed.load(Ordering::SeqCst));
        self.0.drops.fetch_add(1, Ordering::SeqCst);
    }
}
static CODEC_ARMED: AtomicBool = AtomicBool::new(false);
static CODEC_CALLS: AtomicUsize = AtomicUsize::new(0);
struct CallbackWire;
impl phenix_graph::ContractValue for CallbackWire {
    fn schema() -> phenix_schema::Schema {
        assert!(!CODEC_ARMED.load(Ordering::SeqCst));
        CODEC_CALLS.fetch_add(1, Ordering::SeqCst);
        <i64 as phenix_graph::ContractValue>::schema()
    }
    fn encode(&self) -> Vec<u8> {
        assert!(!CODEC_ARMED.load(Ordering::SeqCst));
        CODEC_CALLS.fetch_add(1, Ordering::SeqCst);
        0_i64.to_le_bytes().to_vec()
    }
    fn decode(_: &[u8]) -> Result<Self, phenix_graph::Failure> {
        assert!(!CODEC_ARMED.load(Ordering::SeqCst));
        CODEC_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(Self)
    }
}

#[test]
fn armed_factory_worker_dispatch_guard_codec_and_payload_are_never_touched_by_records() {
    let callbacks = Arc::new(Callbacks::default());
    let policy = Arc::new(Opaque {
        armed: AtomicBool::new(false),
        open: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        notified: true,
    });
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "p",
            Arc::new(
                Interface::new(
                    [
                        PortDef::local::<CallbackPayload>("a"),
                        PortDef::wire::<CallbackWire>("b").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(CallbackFactory(callbacks.clone(), policy.clone())),
        )
        .unwrap();
    b.expose_input("a", p.input::<CallbackPayload>("a").unwrap(), n(1))
        .unwrap();
    b.expose_input("b", p.wire_input::<CallbackWire>("b").unwrap(), n(1))
        .unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            dispatcher: callbacks.clone(),
            execution_guards: vec![callbacks.clone()],
            ..Default::default()
        });
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "a", Value::local(CallbackPayload(callbacks.clone())))
        .unwrap();
    s.inject_request(id, "b", Value::of(CallbackWire)).unwrap();
    s.close_request(id).unwrap();
    s.wake_handle("p").unwrap().wake();
    let callback_count = callbacks.calls.load(Ordering::SeqCst);
    let policy_count = policy.calls.load(Ordering::SeqCst);
    let codec_count = CODEC_CALLS.load(Ordering::SeqCst);
    callbacks.armed.store(true, Ordering::SeqCst);
    policy.armed.store(true, Ordering::SeqCst);
    CODEC_ARMED.store(true, Ordering::SeqCst);
    stable(&s, Some(id), Instant::now());
    assert!(s.explain_process(0, Instant::now()).unwrap().wake.pending);
    assert_eq!(callbacks.calls.load(Ordering::SeqCst), callback_count);
    assert_eq!(callbacks.drops.load(Ordering::SeqCst), 0);
    assert_eq!(policy.calls.load(Ordering::SeqCst), policy_count);
    assert_eq!(CODEC_CALLS.load(Ordering::SeqCst), codec_count);
    CODEC_ARMED.store(false, Ordering::SeqCst);
    policy.armed.store(false, Ordering::SeqCst);
    callbacks.armed.store(false, Ordering::SeqCst);
    drop(s);
    assert_eq!(callbacks.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn all_optional_cycle_observation_is_not_replayed_and_invalidated_ticket_inventory_is_exact() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "p",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("a").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Script {
                actions: VecDeque::new(),
                inputs: Inputs::default(),
            }),
        )
        .unwrap();
    b.expose_input("a", p.wire_input::<i64>("a").unwrap(), n(1))
        .unwrap();
    b.connect(
        p.wire_output::<i64>("out").unwrap(),
        p.wire_input::<i64>("a").unwrap(),
        n(1),
    )
    .unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            readiness: [("p".into(), Readiness::optional(vec![], vec!["a".into()]))].into(),
            request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
            ..Default::default()
        });
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "a", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    let (_, job) = s.next_job().unwrap();
    let ticket = job.ticket();
    s.complete_job(job.run()).unwrap();
    let now = Instant::now();
    let e = s.explain_process(0, now).unwrap();
    // The real data reservation observed a, so reachability alone owes nothing.
    assert!(!candidate(&e, Some(id)).inputs[0].fresh);
    assert!(
        s.explain_request(id, now)
            .unwrap()
            .closure_obligations
            .is_empty()
    );
    assert_eq!(
        s.request_status(id),
        Ok(RequestStatus::Settled { captures: 0 })
    );
    s.release_request(id).unwrap();
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "a", Value::of(2_i64)).unwrap();
    let (_, job) = s.next_job().unwrap();
    let invalidated = job.ticket();
    assert_ne!(ticket, invalidated);
    s.cancel_request(id).unwrap();
    let e = s.explain_request(id, now).unwrap();
    assert_eq!(e.status, Ok(RequestStatus::Cancelled));
    assert_eq!(e.terminal_origin, Some(TerminalOrigin::Local));
    assert_eq!(e.invalidated_tickets, [invalidated]);
    assert_eq!(e.disposal.handed_jobs, 1);
    assert!(e.owners.is_empty());
    assert!(s.explain_process(0, now).unwrap().job.is_none());
    stable(&s, Some(id), now);
    drop(job);
}

#[test]
fn custom_without_native_candidates_has_known_eos_fallback_without_evaluating_policy() {
    let custom = Arc::new(Opaque {
        armed: AtomicBool::new(false),
        open: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
        notified: true,
    });
    let mut s = pair(vec![], Inputs::default()).start_with_options(options(
        Readiness::new(custom.clone()),
        RequestQueuePolicy::SharedFifo,
    ));
    s.close_input("b").unwrap();
    // Native global EOS, not Custom.select, has already admitted the notification.
    let calls = custom.calls.load(Ordering::SeqCst);
    custom.armed.store(true, Ordering::SeqCst);
    let now = Instant::now();
    let e = s.explain_process(0, now).unwrap();
    assert_eq!(e.handoff, HandoffState::Ready);
    assert!(e.scheduled);
    assert!(e.candidates.iter().all(|c| !c.scheduler_candidate));
    assert!(matches!(e.readiness, ReadyAssessment::Blocked { .. }));
    let b = candidate(&e, None)
        .inputs
        .iter()
        .find(|input| input.port == port(&e, "b"))
        .unwrap();
    assert_eq!(b.closure, ClosureProof::GlobalEos);
    assert!(!b.fresh);
    stable(&s, None, now);
    assert_eq!(custom.calls.load(Ordering::SeqCst), calls);
    custom.armed.store(false, Ordering::SeqCst);
}
