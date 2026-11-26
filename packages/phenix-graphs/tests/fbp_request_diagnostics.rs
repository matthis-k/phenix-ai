use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{
        QueueEndpoint, QueueWait, Readiness, ReadinessPolicy, ReadinessSnapshot, RequestError,
        RequestQueuePolicy, RequestStatus, SessionOptions, SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Echo(Arc<AtomicUsize>);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(match cx.take("in") {
            Some(value) => Step::Emit(ProcessEmission::new("out", value)),
            None => Step::Complete,
        })
    }
}

struct Drain;
impl Process for Drain {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        cx.take("tight");
        cx.take("spare");
        Ok(Step::Complete)
    }
}

#[derive(Default)]
struct Gate {
    open: AtomicBool,
    calls: AtomicUsize,
}
impl ReadinessPolicy for Gate {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        snapshot
            .heads
            .iter()
            .filter(|head| self.open.load(Ordering::SeqCst) && head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

fn network(steps: &Arc<AtomicUsize>) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    for name in ["a", "b"] {
        let echo = builder
            .node(
                name,
                Arc::new(
                    Interface::new(
                        [PortDef::wire::<i64>("in").unwrap()],
                        [PortDef::wire::<i64>("out").unwrap()],
                    )
                    .unwrap(),
                ),
                {
                    let steps = steps.clone();
                    Arc::new(move || Echo(steps.clone()))
                },
            )
            .unwrap();
        let drain = builder
            .node(
                format!("{name}-drain"),
                Arc::new(
                    Interface::new(
                        ["tight", "spare"].map(|port| PortDef::wire::<i64>(port).unwrap()),
                        [],
                    )
                    .unwrap(),
                ),
                Arc::new(|| Drain),
            )
            .unwrap();
        builder
            .expose_input(name, echo.wire_input::<i64>("in").unwrap(), one())
            .unwrap();
        builder
            .expose_output(name, echo.wire_output::<i64>("out").unwrap())
            .unwrap();
        for (port, capacity) in [("tight", one()), ("spare", NonZeroUsize::new(2).unwrap())] {
            builder
                .connect(
                    echo.wire_output::<i64>("out").unwrap(),
                    drain.wire_input::<i64>(port).unwrap(),
                    capacity,
                )
                .unwrap();
        }
    }
    builder.build().unwrap().compile().unwrap()
}

fn options(policy: RequestQueuePolicy, gate: &Arc<Gate>) -> SessionOptions {
    let mut options = SessionOptions {
        request_queue_policy: policy,
        capture_capacity: one(),
        max_requests: NonZeroUsize::new(2).unwrap(),
        ..Default::default()
    };
    for name in ["a-drain", "b-drain"] {
        options
            .readiness
            .insert(name.into(), Readiness::new(gate.clone()));
    }
    options
}

fn assert_branch(waits: &[QueueWait], compiled: &CompiledNetwork, name: &str) {
    let index = |path: &str| {
        compiled
            .processes()
            .iter()
            .position(|process| process.path() == path)
            .unwrap()
    };
    assert_eq!(waits.len(), 2, "one full internal queue and one capture");
    assert_ne!(waits[0].queue, waits[1].queue);
    let mut capture = false;
    let mut internal = false;
    for wait in waits {
        assert_eq!((wait.occupancy, wait.capacity), (1, 1));
        assert!(matches!(
            &wait.producer,
            QueueEndpoint::Process { index: producer, port }
                if *producer == index(name) && port == "out"
        ));
        match &wait.consumer {
            QueueEndpoint::Capture(output) => {
                assert_eq!(output, name);
                capture = true;
            }
            QueueEndpoint::Process {
                index: consumer,
                port,
            } => {
                assert_eq!(*consumer, index(&format!("{name}-drain")));
                assert_eq!(port, "tight");
                internal = true;
            }
            other => panic!("unexpected publication target: {other:?}"),
        }
    }
    assert!(capture && internal);
}

#[test]
fn scoped_waits_report_only_full_pending_targets_and_clear_after_relief() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let steps = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Gate::default());
        let compiled = network(&steps);
        let mut session = compiled.start_with_options(options(policy, &gate));
        let a = session.begin_request(None).unwrap();
        let b = session.begin_request(None).unwrap();
        for (request, name) in [(a, "a"), (b, "b")] {
            session
                .inject_request(request, name, Value::of(1_i64))
                .unwrap();
        }
        assert_eq!(session.run_until_idle(32).publications, 2);
        // Full queues alone are not publication waits.
        assert!(session.blocked_queues().is_empty());
        assert!(session.request_blocked_queues(a).unwrap().is_empty());
        assert!(session.request_blocked_queues(b).unwrap().is_empty());
        for (request, name) in [(a, "a"), (b, "b")] {
            session
                .inject_request(request, name, Value::of(2_i64))
                .unwrap();
        }
        assert_eq!(session.run_until_idle(32).publications, 0);
        let a_waits = session.request_blocked_queues(a).unwrap();
        let b_waits = session.request_blocked_queues(b).unwrap();
        assert_branch(&a_waits, &compiled, "a");
        assert_branch(&b_waits, &compiled, "b");
        let mut scoped: Vec<_> = a_waits
            .iter()
            .chain(&b_waits)
            .map(|wait| wait.queue)
            .collect();
        let mut all: Vec<_> = session
            .blocked_queues()
            .iter()
            .map(|wait| wait.queue)
            .collect();
        scoped.sort_unstable();
        all.sort_unstable();
        assert_eq!(scoped, all);

        // A full ingress is not a target of the retained publication either.
        session.inject_request(a, "a", Value::of(3_i64)).unwrap();
        assert_branch(&session.request_blocked_queues(a).unwrap(), &compiled, "a");
        gate.open.store(true, Ordering::SeqCst);
        session.take_request_output(a, "a").unwrap().unwrap();
        session.run_until_idle(32);
        session.take_request_output(a, "a").unwrap().unwrap();
        session.run_until_idle(32);
        assert!(session.request_blocked_queues(a).unwrap().is_empty());
        session.take_request_output(a, "a").unwrap().unwrap();
        // Relieving A must not clear B's retained capture wait.
        let remaining = session.request_blocked_queues(b).unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(matches!(&remaining[0].consumer, QueueEndpoint::Capture(name) if name == "b"));
        session.take_request_output(b, "b").unwrap().unwrap();
        session.run_until_idle(32);
        assert!(session.request_blocked_queues(b).unwrap().is_empty());
        assert!(session.blocked_queues().is_empty());
        assert!(
            session
                .failure()
                .map(phenix_graph::Failure::summary)
                .is_none()
        );
    }
}

#[test]
fn shared_fifo_attributes_wait_to_a_publishing_behind_b_buffered_packets() {
    let steps = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Gate::default());
    let compiled = network(&steps);
    let mut session = compiled.start_with_options(options(RequestQueuePolicy::SharedFifo, &gate));
    let a = session.begin_request(None).unwrap();
    let b = session.begin_request(None).unwrap();
    session.inject_request(b, "a", Value::of(10_i64)).unwrap();
    assert_eq!(session.run_until_idle(32).publications, 1);
    session.inject_request(a, "a", Value::of(20_i64)).unwrap();
    assert_eq!(session.run_until_idle(32).publications, 0);
    assert_branch(&session.request_blocked_queues(a).unwrap(), &compiled, "a");
    assert!(session.request_blocked_queues(b).unwrap().is_empty());
    assert!(session.take_request_output(a, "a").unwrap().is_none());
    let buffered = session.take_request_output(b, "a").unwrap().unwrap();
    assert_eq!(buffered.metadata().unwrap().request(), Some(b));
    assert_eq!(buffered.value().get::<i64>().unwrap(), 10);
    gate.open.store(true, Ordering::SeqCst);
    session.run_until_idle(32);
    assert!(session.request_blocked_queues(a).unwrap().is_empty());
    let published = session.take_request_output(a, "a").unwrap().unwrap();
    assert_eq!(published.metadata().unwrap().request(), Some(a));
    assert_eq!(published.value().get::<i64>().unwrap(), 20);
}

#[test]
fn unscoped_publication_waits_remain_session_wide_only() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let steps = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Gate::default());
        let compiled = network(&steps);
        let mut session = compiled.start_with_options(options(policy, &gate));
        let request = session.begin_request(None).unwrap();
        for value in [1_i64, 2] {
            session.inject("a", Value::of(value)).unwrap();
            session.run_until_idle(32);
        }
        assert_branch(&session.blocked_queues(), &compiled, "a");
        assert!(session.request_blocked_queues(request).unwrap().is_empty());
    }
}

#[test]
fn foreign_and_released_request_ids_are_rejected() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let compiled = network(&Arc::new(AtomicUsize::new(0)));
        let gate = Arc::new(Gate::default());
        let mut session = compiled.start_with_options(options(policy, &gate));
        let mut other = compiled.start_with_options(options(policy, &gate));
        let request = session.begin_request(None).unwrap();
        let foreign = other.begin_request(None).unwrap();
        assert_eq!(
            session.request_blocked_queues(foreign).unwrap_err(),
            RequestError::Foreign
        );
        assert!(session.request_blocked_queues(request).unwrap().is_empty());
        session.close_request(request).unwrap();
        session.release_request(request).unwrap();
        let replacement = session.begin_request(None).unwrap();
        assert_eq!(
            session.request_blocked_queues(request).unwrap_err(),
            RequestError::Stale
        );
        assert!(
            session
                .request_blocked_queues(replacement)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn observation_does_not_call_readiness_execute_steps_or_supervise_overdue_jobs() {
    for policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let steps = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Gate::default());
        let compiled = network(&steps);
        let mut options = options(policy, &gate);
        options.deferred_timeout = Some(Duration::ZERO);
        let mut session = compiled.start_with_options(options);
        let a = session.begin_request(None).unwrap();
        let b = session.begin_request(None).unwrap();
        for value in [1_i64, 2] {
            session.inject_request(a, "a", Value::of(value)).unwrap();
            session.run_until_idle(32);
        }
        session.inject_request(b, "b", Value::of(3_i64)).unwrap();
        session.close_request(b).unwrap();
        let (index, job) = session.next_job().unwrap();
        let now = Instant::now();
        let overdue = session.overdue_jobs(now);
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].process, index);
        assert_eq!(overdue[0].ticket, job.ticket());
        let before = (
            gate.calls.load(Ordering::SeqCst),
            steps.load(Ordering::SeqCst),
            session.request_work(a).unwrap(),
            session.request_work(b).unwrap(),
            session.state(),
        );
        for _ in 0..3 {
            assert_branch(&session.request_blocked_queues(a).unwrap(), &compiled, "a");
            assert!(session.request_blocked_queues(b).unwrap().is_empty());
        }
        assert_eq!(
            before,
            (
                gate.calls.load(Ordering::SeqCst),
                steps.load(Ordering::SeqCst),
                session.request_work(a).unwrap(),
                session.request_work(b).unwrap(),
                session.state(),
            )
        );
        assert_eq!(session.request_status(b).unwrap(), RequestStatus::Pending);
        assert_eq!(session.overdue_jobs(now), overdue);
        assert!(
            session
                .failure()
                .map(phenix_graph::Failure::summary)
                .is_none()
        );
        assert_eq!(session.supervise_deferred(now), overdue);
        assert_eq!(session.state(), SessionState::Failed);
        assert_eq!(session.request_status(b).unwrap(), RequestStatus::Failed);
        drop(job);
    }
}
