use super::*;
use crate::{
    ComponentDef, NetworkBuilder,
    interface::{Interface, PortDef},
    process::{Process, ProcessEmission},
};
use std::sync::atomic::AtomicUsize;

struct Heads {
    notified: bool,
    registrations: AtomicUsize,
    selections: AtomicUsize,
}

impl Heads {
    fn new(notified: bool) -> Arc<Self> {
        Arc::new(Self {
            notified,
            registrations: AtomicUsize::new(0),
            selections: AtomicUsize::new(0),
        })
    }
}

impl ReadinessPolicy for Heads {
    fn notification_driven(&self) -> bool {
        assert_eq!(self.registrations.fetch_add(1, Ordering::Relaxed), 0);
        self.notified
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.selections.fetch_add(1, Ordering::Relaxed);
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

struct Stage {
    group: Option<Readiness>,
    waiting: bool,
}

impl Process for Stage {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        if self.waiting {
            assert_eq!(cx.take("next").unwrap().get::<u64>()?, 22);
            self.waiting = false;
            Ok(Step::Complete)
        } else if cx.is_eos("start") {
            Ok(Step::Finished)
        } else {
            assert!(cx.take("start").is_some());
            self.waiting = true;
            Ok(match &self.group {
                Some(policy) => Step::WaitInputs(policy.clone()),
                None => Step::WaitInput("next".into()),
            })
        }
    }
}

fn stages(group: Option<Readiness>) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    for name in ["stage", "other", "idle"] {
        let group = group.clone();
        let node = builder
            .node(
                name,
                Arc::new(
                    Interface::new(
                        ["start", "next"].map(|port| PortDef::wire::<u64>(port).unwrap()),
                        [],
                    )
                    .unwrap(),
                ),
                Arc::new(move || Stage {
                    group: group.clone(),
                    waiting: false,
                }),
            )
            .unwrap();
        for port in ["start", "next"] {
            builder
                .expose_input(
                    format!("{name}-{port}"),
                    node.wire_input::<u64>(port).unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
        }
    }
    builder.build().unwrap().compile().unwrap()
}

fn step(session: &mut NetworkSession, expected: usize) {
    let (index, job) = session.next_job().expect("expected cooperative step");
    assert_eq!(index, expected);
    session.complete(index, job.run()).unwrap();
    assert!(session.failure().is_none(), "{:?}", session.failure());
}

#[test]
fn unresolved_waits_are_polled_but_successful_reservations_remove_historical_membership() {
    let custom = Heads::new(true);
    for group in [
        None,
        Some(Readiness::Any),
        Some(Readiness::Custom(custom.clone())),
    ] {
        for queue_policy in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            let mut session = stages(group.clone()).start_with_options(SessionOptions {
                request_queue_policy: queue_policy,
                ..Default::default()
            });
            let index = session.compiled.physical_process_index("stage").unwrap();
            assert!(session.always_poll.is_empty());
            let request = session.begin_request(None).unwrap();
            session
                .inject_request(request, "stage-start", Value::of(11_u64))
                .unwrap();
            step(&mut session, index);
            assert_eq!(
                session.processes[index].handoff.state(),
                HandoffState::WaitingInput
            );
            assert_eq!(session.always_poll, BTreeSet::from([index]));
            assert!(session.dirty_processes.is_empty());
            let selections = custom.selections.load(Ordering::Relaxed);
            for _ in 0..2 {
                assert!(session.next_job().is_none());
                assert_eq!(session.always_poll, BTreeSet::from([index]));
            }
            if matches!(&group, Some(Readiness::Custom(_))) {
                assert!(custom.selections.load(Ordering::Relaxed) >= selections + 2);
            }
            session
                .inject_request(request, "stage-next", Value::of(22_u64))
                .unwrap();
            // Removal must happen at reservation, not at the later Complete step.
            assert_eq!(
                session.processes[index].handoff.state(),
                HandoffState::Ready
            );
            assert!(session.processes[index].waiting.is_none());
            assert!(session.processes[index].waiting_policy.is_none());
            assert!(
                session.always_poll.is_empty(),
                "resumed wait retained historical membership"
            );
            step(&mut session, index);
            assert!(session.next_job().is_none());
            session.close_request(request).unwrap();
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Settled { captures: 0 }
            );
            session.release_request(request).unwrap();
            assert!(session.always_poll.is_empty());
        }
    }
}

#[test]
fn base_polling_survives_resume_but_finish_overrides_every_poll_reason() {
    for notified in [false, true] {
        let policy = Heads::new(notified);
        let mut session = stages(None).start_with_options(SessionOptions {
            readiness: [("stage".into(), Readiness::Custom(policy.clone()))].into(),
            ..Default::default()
        });
        let index = session.compiled.physical_process_index("stage").unwrap();
        session.inject("stage-start", Value::of(11_u64)).unwrap();
        step(&mut session, index);
        session.inject("stage-next", Value::of(22_u64)).unwrap();
        assert_eq!(session.always_poll.contains(&index), !notified);
        step(&mut session, index);
        session.close_input("stage-start").unwrap();
        session.close_input("stage-next").unwrap();
        step(&mut session, index);
        assert!(session.processes[index].done);
        assert!(!session.always_poll.contains(&index));
        assert_eq!(policy.registrations.load(Ordering::Relaxed), 1);
    }

    // Source declarations cannot legally have inputs. Force just that base
    // classification to exercise the resume branch independently of Custom.
    let mut session = stages(None).start();
    let index = session.compiled.physical_process_index("stage").unwrap();
    session.inject("stage-start", Value::of(11_u64)).unwrap();
    step(&mut session, index);
    session.processes[index].source = true;
    session.inject("stage-next", Value::of(22_u64)).unwrap();
    assert!(session.always_poll.contains(&index));
    step(&mut session, index);
    session.finish(index);
    assert!(!session.always_poll.contains(&index));

    // An unstarted inputless node finishes on the initial refresh. Pin a live
    // continuation instead, isolating the inputless reason from source/Custom.
    let mut session = stages(None).start();
    let index = session.compiled.physical_process_index("stage").unwrap();
    session.inject("stage-start", Value::of(11_u64)).unwrap();
    step(&mut session, index);
    session.inject("stage-next", Value::of(22_u64)).unwrap();
    let inputs = std::mem::take(&mut session.processes[index].inputs);
    session.sync_poll(index);
    assert_eq!(session.always_poll, BTreeSet::from([index]));
    session.processes[index].inputs = inputs;
    step(&mut session, index);
    // A retained historical wait must not defeat the done check either.
    session.processes[index].waiting = Some("historical".into());
    session.processes[index].waiting_policy = Some(Readiness::Any);
    session.finish(index);
    assert!(session.processes[index].done);
    assert!(session.always_poll.is_empty());
}

#[test]
fn cancellation_detaches_staged_membership_under_both_failure_and_queue_policies() {
    for failure_policy in [
        RequestFailurePolicy::FailSession,
        RequestFailurePolicy::RestartAffectedProcess,
    ] {
        for queue_policy in [
            RequestQueuePolicy::SharedFifo,
            RequestQueuePolicy::BoundedLanes,
        ] {
            for group in [None, Some(Readiness::Any)] {
                let mut session = stages(group).start_with_options(SessionOptions {
                    request_failure_policy: failure_policy,
                    request_queue_policy: queue_policy,
                    ..Default::default()
                });
                let index = session.compiled.physical_process_index("stage").unwrap();
                let request = session.begin_request(None).unwrap();
                session
                    .inject_request(request, "stage-start", Value::of(11_u64))
                    .unwrap();
                step(&mut session, index);
                assert_eq!(session.always_poll, BTreeSet::from([index]));
                assert!(session.cancel_request(request).unwrap().is_empty());
                assert!(
                    session.always_poll.is_empty(),
                    "cancelled wait retained historical membership"
                );
                assert!(session.processes[index].waiting.is_none());
                assert!(session.processes[index].waiting_policy.is_none());
                assert!(session.processes[index].request.is_none());
                assert_eq!(
                    session.request_status(request).unwrap(),
                    RequestStatus::Cancelled
                );
                session.release_request(request).unwrap();
                if failure_policy == RequestFailurePolicy::RestartAffectedProcess {
                    let next = session.begin_request(None).unwrap();
                    session
                        .inject_request(next, "stage-start", Value::of(11_u64))
                        .unwrap();
                    step(&mut session, index);
                    session
                        .inject_request(next, "stage-next", Value::of(22_u64))
                        .unwrap();
                    step(&mut session, index);
                    session.close_request(next).unwrap();
                    session.release_request(next).unwrap();
                    assert!(session.always_poll.is_empty());
                } else {
                    assert_eq!(session.state(), SessionState::Failed);
                }
            }
        }
    }
}

struct TwoEmissions(u64);

impl Process for TwoEmissions {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
        if self.0 == 0 {
            assert!(cx.take("input").is_some());
        }
        self.0 += 1;
        Ok(if self.0 <= 2 {
            Step::Emit(ProcessEmission::new("output", Value::of(22_u64)))
        } else {
            Step::Complete
        })
    }
}

#[test]
fn full_scan_resume_preserves_suppressed_capacity_event_for_an_earlier_pending_producer() {
    let one = NonZeroUsize::new(1).unwrap();
    let mut builder = NetworkBuilder::new();
    let producer = builder
        .node(
            "a-producer",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<u64>("input").unwrap()],
                    [PortDef::wire::<u64>("output").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| TwoEmissions(0)),
        )
        .unwrap();
    builder
        .expose_input("produce", producer.wire_input::<u64>("input").unwrap(), one)
        .unwrap();
    for name in ["b-stage", "c-stage"] {
        let node = builder
            .node(
                name,
                Arc::new(
                    Interface::new(
                        ["start", "next"].map(|port| PortDef::wire::<u64>(port).unwrap()),
                        [],
                    )
                    .unwrap(),
                ),
                Arc::new(|| Stage {
                    group: None,
                    waiting: false,
                }),
            )
            .unwrap();
        builder
            .expose_input(name, node.wire_input::<u64>("start").unwrap(), one)
            .unwrap();
        if name == "b-stage" {
            builder
                .connect(
                    producer.wire_output::<u64>("output").unwrap(),
                    node.wire_input::<u64>("next").unwrap(),
                    one,
                )
                .unwrap();
        } else {
            builder
                .expose_input("unused-next", node.wire_input::<u64>("next").unwrap(), one)
                .unwrap();
        }
    }
    let mut session = builder.build().unwrap().compile().unwrap().start();
    assert_eq!(
        session.compiled.physical_process_index("a-producer"),
        Some(0)
    );
    session.inject("b-stage", Value::of(11_u64)).unwrap();
    let (consumer, held) = session.next_job().unwrap();
    assert_eq!(consumer, 1);
    session.inject("c-stage", Value::of(11_u64)).unwrap();
    step(&mut session, 2);
    session.inject("produce", Value::of(1_u64)).unwrap();
    step(&mut session, 0);
    // next_job commits the first packet before handing out the second emit.
    step(&mut session, 0);
    assert!(session.next_job().is_none());
    assert!(session.processes[0].pending.is_some());
    assert_eq!(session.blocked_queues().len(), 1);
    assert!(!session.processes[0].queued);
    session.force_full_refresh = false;
    session.dirty_processes.clear();

    // Completing this held job adds the second mandatory waiter. In the full
    // scan, producer 0 is checked while blocked, then consumer 1 frees its queue
    // before sync_poll shrinks the set. That capacity event was suppressed.
    session.complete(consumer, held.run()).unwrap();
    assert!(!session.prefer_full_refresh());
    assert_eq!(session.always_poll, BTreeSet::from([2]));
    assert!(session.dirty_processes.is_empty());
    assert!(!session.processes[0].queued);
    assert!(session.blocked_queues().is_empty());
    assert!(
        session.force_full_refresh,
        "full-scan removal lost resynchronization latch"
    );
    session.refresh();
    assert!(!session.force_full_refresh);
    assert!(
        session.processes[0].queued,
        "suppressed capacity event stranded publication"
    );
    step(&mut session, 1);
    step(&mut session, 0);
    assert!(session.processes[0].pending.is_none());
    assert!(session.failure().is_none());
}

#[test]
fn last_scoped_watcher_release_alone_resynchronizes_suppressed_publication_events() {
    for notified in [false, true] {
        let one = NonZeroUsize::new(1).unwrap();
        let mut builder = NetworkBuilder::new();
        let producer = builder
            .node(
                "a-producer",
                Arc::new(
                    Interface::new(
                        [PortDef::wire::<u64>("input").unwrap()],
                        [PortDef::wire::<u64>("output").unwrap()],
                    )
                    .unwrap(),
                ),
                Arc::new(|| TwoEmissions(0)),
            )
            .unwrap();
        builder
            .expose_input("produce", producer.wire_input::<u64>("input").unwrap(), one)
            .unwrap();
        builder
            .expose_output("output", producer.wire_output::<u64>("output").unwrap())
            .unwrap();
        let mut options = SessionOptions {
            capture_capacity: one,
            ..Default::default()
        };
        for name in ["b-watch", "c-watch"] {
            let node = builder
                .add(name, Arc::new(ComponentDef::sink(|_: &u64| {})))
                .unwrap();
            builder
                .expose_input(name, node.input::<u64>("input").unwrap(), one)
                .unwrap();
            options.readiness.insert(
                name.into(),
                if notified {
                    Readiness::Custom(Heads::new(true))
                } else {
                    Readiness::Optional {
                        required: vec!["input".into()],
                        optional: vec![],
                    }
                },
            );
        }
        let mut session = builder
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .start_with_options(options);
        let mut requests = Vec::new();
        for _ in 0..2 {
            let request = session.begin_request(None).unwrap();
            for name in ["b-watch", "c-watch"] {
                session
                    .inject_request(request, name, Value::local(1_u64))
                    .unwrap();
            }
            assert_eq!(session.run_until_idle(8).steps, 2);
            session.close_request(request).unwrap();
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Settled { captures: 0 }
            );
            requests.push(request);
        }
        assert!(session.always_poll.is_empty());
        assert_eq!(
            session.scoped_readiness_poll,
            BTreeMap::from([(1, 2), (2, 2)])
        );
        assert!(session.prefer_full_refresh());
        session.inject("produce", Value::of(1_u64)).unwrap();
        step(&mut session, 0);
        step(&mut session, 0);
        assert!(session.next_job().is_none());
        assert!(session.processes[0].pending.is_some());
        assert_eq!(session.blocked_queues().len(), 1);
        session.force_full_refresh = false;
        session.dirty_processes.clear();

        // Split capture consumption from refresh to pin the adversarial boundary:
        // capacity is released in full mode, then scopes retire before a drive.
        let capture = session.captures["output"];
        assert_eq!(
            session
                .transport
                .pop(capture)
                .unwrap()
                .value()
                .get::<u64>()
                .unwrap(),
            22
        );
        session.dirty_queue(capture);
        assert!(session.dirty_processes.is_empty());
        session.release_request(requests[0]).unwrap();
        assert!(session.prefer_full_refresh());
        assert!(!session.force_full_refresh);
        assert_eq!(
            session.scoped_readiness_poll,
            BTreeMap::from([(1, 1), (2, 1)])
        );
        session.release_request(requests[1]).unwrap();
        assert!(session.always_poll.is_empty());
        assert!(session.scoped_readiness_poll.is_empty());
        assert!(!session.prefer_full_refresh());
        assert!(
            session.force_full_refresh,
            "watcher-only crossover lost resynchronization latch"
        );
        // No staged membership removal, notification or new queue event can
        // rescue this publication. Only unwatch_scoped_readiness's latch can.
        step(&mut session, 0);
        assert!(session.processes[0].pending.is_none());
        assert!(!session.force_full_refresh);
        assert_eq!(
            session
                .take_output("output")
                .unwrap()
                .unwrap()
                .value()
                .get::<u64>()
                .unwrap(),
            22
        );
        assert!(session.next_job().is_none());
    }
}
