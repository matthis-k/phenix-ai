use phenix_graph::{
    Value,
    connection::ConnectionError,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{
        Readiness, RequestError, RequestFailurePolicy, RequestQueuePolicy, RequestStatus,
        SessionOptions, SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

fn lane_options() -> SessionOptions {
    SessionOptions {
        max_requests: NonZeroUsize::new(2).unwrap(),
        capture_capacity: one(),
        request_queue_policy: RequestQueuePolicy::BoundedLanes,
        ..Default::default()
    }
}

#[test]
fn dirty_lane_capture_retry_and_eos_propagate_without_a_full_scan() {
    use phenix_graph::ComponentDef;

    let mut builder = NetworkBuilder::new();
    let first = builder
        .add("first", Arc::new(ComponentDef::map(|value: i64| value + 1)))
        .unwrap();
    let second = builder
        .add(
            "second",
            Arc::new(ComponentDef::map(|value: i64| value + 1)),
        )
        .unwrap();
    builder
        .expose_input("input", first.input::<i64>("input").unwrap(), one())
        .unwrap();
    builder
        .connect(
            first.output::<i64>("output").unwrap(),
            second.input::<i64>("input").unwrap(),
            one(),
        )
        .unwrap();
    builder
        .expose_output("output", second.output::<i64>("output").unwrap())
        .unwrap();
    for index in 0..8 {
        let idle = builder
            .add(
                format!("idle-{index}"),
                Arc::new(ComponentDef::sink(|_: &i64| {})),
            )
            .unwrap();
        builder
            .expose_input(
                format!("idle-{index}"),
                idle.input::<i64>("input").unwrap(),
                one(),
            )
            .unwrap();
    }
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(lane_options());
    let request = session.begin_request(None).unwrap();
    session
        .inject_request_local(request, "input", 10_i64)
        .unwrap();
    assert_eq!(session.run_until_idle(32).publications, 2);
    session
        .inject_request_local(request, "input", 20_i64)
        .unwrap();
    assert_eq!(session.run_until_idle(32).publications, 1);
    assert_eq!(
        session.request_work(request).unwrap().capture_backpressure,
        1
    );
    session.close_request(request).unwrap();
    session.close_input("input").unwrap();
    let first = session
        .take_request_output(request, "output")
        .unwrap()
        .unwrap();
    assert_eq!(first.value().borrow::<i64>(), Some(&12));
    assert_eq!(session.run_until_idle(32).publications, 1);
    let second = session
        .take_request_output(request, "output")
        .unwrap()
        .unwrap();
    assert_eq!(second.value().borrow::<i64>(), Some(&22));
    session.run_until_idle(32);
    assert!(session.output_closed("output").unwrap());
    assert_eq!(session.state(), SessionState::Quiescent);
    assert_eq!(
        session.request_status(request).unwrap(),
        RequestStatus::Settled { captures: 2 }
    );
    assert_eq!(session.request_work(request).unwrap().queued, 0);
    session.release_request(request).unwrap();
    assert!(session.failure().is_none());
}

struct Echo(bool);
impl Process for Echo {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.0 {
            self.0 = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        if value.borrow::<i64>() == Some(&-1) {
            return Err("deliberate request failure".into());
        }
        self.0 = true;
        Ok(Step::Emit(ProcessEmission::new("out", value)))
    }
}

fn echo_network() -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let echo = builder
        .node(
            "echo",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("in")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(|| Echo(false)),
        )
        .unwrap();
    builder
        .expose_input("input", echo.input::<i64>("in").unwrap(), one())
        .unwrap();
    builder
        .expose_output("output", echo.output::<i64>("out").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn finished_source_does_not_deliver_global_eos_ahead_of_a_scoped_all_join() {
    struct Source(bool);
    impl Process for Source {
        fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
            if self.0 {
                return Ok(Step::Finished);
            }
            self.0 = true;
            Ok(Step::Emit(ProcessEmission::new("a", Value::local(10_i64))))
        }
    }
    struct Join(bool);
    impl Process for Join {
        fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
            if self.0 {
                self.0 = false;
                return Ok(Step::Complete);
            }
            // Global closure notifications are valid once the scoped data drains.
            if cx.inputs().next().is_none() {
                return Ok(Step::Complete);
            }
            let a = cx.take("a").ok_or("join activated without a")?;
            let b = cx.take("b").ok_or("join activated without b")?;
            self.0 = true;
            Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::local(a.borrow::<i64>().unwrap() + b.borrow::<i64>().unwrap()),
            )))
        }
    }

    let mut builder = NetworkBuilder::new();
    let source = builder
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("a")]).unwrap()),
            Arc::new(|| Source(false)),
        )
        .unwrap();
    builder.start(&source).unwrap();
    let join = builder
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("a"), PortDef::local::<i64>("b")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(|| Join(false)),
        )
        .unwrap();
    builder
        .connect(
            source.output::<i64>("a").unwrap(),
            join.input::<i64>("a").unwrap(),
            one(),
        )
        .unwrap();
    builder
        .expose_input("b", join.input::<i64>("b").unwrap(), one())
        .unwrap();
    builder
        .expose_output("out", join.output::<i64>("out").unwrap())
        .unwrap();
    let mut options = lane_options();
    options.auto_start_sources = false;
    options.readiness.insert("join".into(), Readiness::All);
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);
    let request = session.begin_request(Some("out")).unwrap();
    session.start_request_source(request, "source").unwrap();

    let report = session.run_until_idle(16);
    assert_eq!(report.steps, 2, "only source Emit and Finished should run");
    assert_eq!(report.state, SessionState::Waiting);
    assert!(session.failure().is_none());
    assert!(!session.output_closed("out").unwrap());
    assert!(
        session
            .take_request_output(request, "out")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        session.request_status(request).unwrap(),
        RequestStatus::Open
    );
    assert_eq!(session.request_work(request).unwrap().queued, 1);
    assert_eq!(session.run_until_idle(16).steps, 0);

    session
        .inject_request(request, "b", Value::local(2_i64))
        .unwrap();
    session.close_request(request).unwrap();
    session.run_until_idle(16);
    let packet = session
        .take_request_output(request, "out")
        .unwrap()
        .unwrap();
    assert_eq!(packet.metadata().unwrap().request(), Some(request));
    assert_eq!(packet.value().borrow::<i64>(), Some(&12));
    assert_eq!(
        session.request_status(request).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert!(
        session
            .take_request_output(request, "out")
            .unwrap()
            .is_none()
    );
    assert!(session.failure().is_none());
    session.release_request(request).unwrap();
}

#[test]
fn close_input_applies_to_existing_and_later_allocated_lanes() {
    let mut session = echo_network().start_with_options(lane_options());
    let existing = session.begin_request(Some("output")).unwrap();
    session
        .inject_request(existing, "input", Value::local(7_i64))
        .unwrap();
    session.close_input("input").unwrap();
    let later = session.begin_request(None).unwrap();

    for (request, value) in [(existing, 8_i64), (later, 9)] {
        let error = session
            .inject_request(request, "input", Value::local(value))
            .unwrap_err();
        assert_eq!(
            error.reason,
            RequestError::Transport(ConnectionError::WriterClosed)
        );
        assert_eq!(error.value.borrow::<i64>(), Some(&value));
    }
    let error = session.inject("input", Value::local(10_i64)).unwrap_err();
    assert_eq!(error.reason, ConnectionError::WriterClosed);
    assert_eq!(error.value.borrow::<i64>(), Some(&10));
    session.close_request(existing).unwrap();
    session.close_request(later).unwrap();
    assert_ne!(session.run_until_idle(16).state, SessionState::Failed);
    let packet = session
        .take_request_output(existing, "output")
        .unwrap()
        .expect("closing a writer must preserve previously accepted input");
    assert_eq!(packet.value().borrow::<i64>(), Some(&7));
    assert_eq!(packet.metadata().unwrap().request(), Some(existing));
    assert!(
        session
            .take_request_output(later, "output")
            .unwrap()
            .is_none()
    );
    session.release_request(existing).unwrap();
    session.release_request(later).unwrap();
}

#[test]
fn finished_reader_stays_closed_in_existing_reused_and_new_lanes() {
    struct Finish(Arc<Mutex<usize>>);
    impl Process for Finish {
        fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
            assert_eq!(cx.take("in").unwrap().borrow::<i64>(), Some(&17));
            *self.0.lock().unwrap() += 1;
            Ok(Step::Finished)
        }
    }
    let calls = Arc::new(Mutex::new(0));
    let shared = calls.clone();
    let mut builder = NetworkBuilder::new();
    let finish = builder
        .node(
            "finish",
            Arc::new(Interface::new([PortDef::local::<i64>("in")], []).unwrap()),
            Arc::new(move || Finish(shared.clone())),
        )
        .unwrap();
    builder
        .expose_input("input", finish.input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(lane_options());
    let existing = session.begin_request(None).unwrap();
    session
        .inject_request(existing, "input", Value::local(17_i64))
        .unwrap();
    assert_eq!(session.run_until_idle(8).steps, 1);
    let error = session
        .inject_request(existing, "input", Value::local(18_i64))
        .unwrap_err();
    assert_eq!(
        error.reason,
        RequestError::Transport(ConnectionError::ReaderClosed)
    );
    assert_eq!(error.value.borrow::<i64>(), Some(&18));
    session.close_request(existing).unwrap();
    assert_eq!(
        session.request_status(existing).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    session.release_request(existing).unwrap();

    let reused = session.begin_request(None).unwrap();
    let new = session.begin_request(None).unwrap();
    assert_ne!(existing, reused);
    assert_ne!(reused, new);
    for (request, value) in [(reused, 19_i64), (new, 20)] {
        let error = session
            .inject_request(request, "input", Value::local(value))
            .unwrap_err();
        assert_eq!(
            error.reason,
            RequestError::Transport(ConnectionError::ReaderClosed)
        );
        assert_eq!(error.value.borrow::<i64>(), Some(&value));
        assert_eq!(session.request_work(request).unwrap().queued, 0);
        session.close_request(request).unwrap();
        session.release_request(request).unwrap();
    }
    assert_eq!(session.run_until_idle(8).steps, 0);
    assert_eq!(*calls.lock().unwrap(), 1);
    assert!(session.failure().is_none());
}

#[test]
fn fail_session_retains_another_requests_capture_until_scoped_drain() {
    let mut options = lane_options();
    options.request_failure_policy = RequestFailurePolicy::FailSession;
    let mut session = echo_network().start_with_options(options);
    let a = session.begin_request(Some("output")).unwrap();
    session
        .inject_request(a, "input", Value::local(42_i64))
        .unwrap();
    session.close_request(a).unwrap();
    session.run_until_idle(16);
    assert_eq!(session.request_status(a).unwrap(), RequestStatus::Pending);
    assert_eq!(session.request_work(a).unwrap().queued, 1);
    assert_eq!(session.release_request(a), Err(RequestError::NotSettled));

    let b = session.begin_request(None).unwrap();
    session
        .inject_request(b, "input", Value::local(-1_i64))
        .unwrap();
    session.close_request(b).unwrap();
    assert_eq!(session.run_until_idle(16).state, SessionState::Failed);
    assert!(
        session
            .failure()
            .unwrap()
            .contains("deliberate request failure")
    );
    assert_eq!(session.request_status(b).unwrap(), RequestStatus::Failed);
    assert_eq!(session.release_request(a), Err(RequestError::NotSettled));
    assert!(session.take_request_output(b, "output").unwrap().is_none());

    let packet = session.take_request_output(a, "output").unwrap().unwrap();
    assert_eq!(packet.metadata().unwrap().request(), Some(a));
    assert_eq!(packet.value().borrow::<i64>(), Some(&42));
    session.validate_capture(a, "output", &packet).unwrap();
    assert_eq!(
        session.validate_capture(b, "output", &packet),
        Err(RequestError::InvalidCapture)
    );
    assert!(session.take_request_output(a, "output").unwrap().is_none());
    session.release_request(a).unwrap();
    assert_eq!(session.request_status(a), Err(RequestError::Stale));
    session.release_request(b).unwrap();
}

type Records = Arc<Mutex<Vec<(Option<usize>, i64)>>>;

struct Record(Records);
impl Process for Record {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.0
            .lock()
            .unwrap()
            .push((cx.correlation_lane(), *value.borrow::<i64>().unwrap()));
        Ok(Step::Complete)
    }
}

fn record_network(names: &[&str], capacity: NonZeroUsize, records: Records) -> CompiledNetwork {
    let mut builder = NetworkBuilder::new();
    let record = builder
        .node(
            "record",
            Arc::new(Interface::new([PortDef::local::<i64>("in")], []).unwrap()),
            Arc::new(move || Record(records.clone())),
        )
        .unwrap();
    for name in names {
        builder
            .expose_input(*name, record.input::<i64>("in").unwrap(), capacity)
            .unwrap();
    }
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn shared_fifo_rotates_queues_on_the_same_port_across_requests() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let options = SessionOptions::default();
    assert_eq!(options.request_queue_policy, RequestQueuePolicy::SharedFifo);
    let mut session = record_network(
        &["left", "right"],
        NonZeroUsize::new(3).unwrap(),
        seen.clone(),
    )
    .start_with_options(options);
    let a = session.begin_request(None).unwrap();
    let b = session.begin_request(None).unwrap();
    for (request, input, values) in [(a, "left", [10_i64, 11, 12]), (b, "right", [20, 21, 22])] {
        for value in values {
            session
                .inject_request(request, input, Value::local(value))
                .unwrap();
        }
        session.close_request(request).unwrap();
    }
    assert_eq!(session.run_until_idle(6).steps, 6);
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (Some(0), 10),
            (Some(1), 20),
            (Some(0), 11),
            (Some(1), 21),
            (Some(0), 12),
            (Some(1), 22)
        ]
    );
    assert_eq!(session.run_until_idle(8).steps, 0);
    assert!(session.failure().is_none());
    for request in [a, b] {
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        session.release_request(request).unwrap();
    }
}

#[test]
fn bounded_lanes_round_robin_includes_unscoped_traffic_under_scoped_refill() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut session =
        record_network(&["input"], one(), seen.clone()).start_with_options(lane_options());
    let request = session.begin_request(None).unwrap();
    session
        .inject_request(request, "input", Value::local(10_i64))
        .unwrap();
    session
        .inject_request(request, "input", Value::local(11_i64))
        .unwrap();
    session.inject("input", Value::local(90_i64)).unwrap();

    let mut next = 12_i64;
    for _ in 0..8 {
        assert_eq!(session.run_until_idle(1).steps, 1);
        // Keep scoped work queued even when its next activation is already reserved.
        if session.request_work(request).unwrap().queued == 0 {
            session
                .inject_request(request, "input", Value::local(next))
                .unwrap();
            next += 1;
        }
    }
    assert_eq!(
        &seen.lock().unwrap()[..2],
        &[(Some(0), 10), (None, 90)],
        "unscoped traffic must get its turn despite a continuously refilled request"
    );
    session.close_request(request).unwrap();
    session.run_until_idle(16);
    let records = seen.lock().unwrap();
    assert_eq!(records.iter().filter(|(lane, _)| lane.is_none()).count(), 1);
    assert_eq!(
        records
            .iter()
            .filter_map(|(lane, value)| lane.map(|_| *value))
            .collect::<Vec<_>>(),
        (10..next).collect::<Vec<_>>()
    );
    assert!(session.failure().is_none());
    assert_eq!(
        session.request_status(request).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    session.release_request(request).unwrap();
}

#[test]
fn bounded_lanes_closure_only_request_gets_a_turn_under_data_refill() {
    struct Router(bool);
    impl Process for Router {
        fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
            if self.0 {
                self.0 = false;
                return Ok(Step::Complete);
            }
            let value = cx.take("in").unwrap();
            if value.borrow::<i64>() == Some(&-1) {
                return Ok(Step::Complete);
            }
            self.0 = true;
            Ok(Step::Emit(ProcessEmission::new("out", value)))
        }
    }
    type Observations = Arc<Mutex<Vec<(usize, Option<i64>)>>>;
    struct OptionalRecord {
        remaining: usize,
        seen: Observations,
    }
    impl Process for OptionalRecord {
        fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
            if self.remaining != 0 {
                self.remaining -= 1;
                return Ok(if self.remaining == 0 {
                    Step::Complete
                } else {
                    Step::Yield
                });
            }
            let value = cx.take("in").map(|value| *value.borrow::<i64>().unwrap());
            assert_eq!(cx.is_request_closed("in"), value.is_none());
            assert!(!cx.is_eos("in"));
            self.seen
                .lock()
                .unwrap()
                .push((cx.correlation_lane().unwrap(), value));
            if value.is_none() {
                return Ok(Step::Complete);
            }
            // Slower consumption keeps A queued at every subsequent selection.
            self.remaining = 4;
            Ok(Step::Yield)
        }
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let shared = seen.clone();
    let mut builder = NetworkBuilder::new();
    let router = builder
        .node(
            "router",
            Arc::new(
                Interface::new(
                    [PortDef::local::<i64>("in")],
                    [PortDef::local::<i64>("out")],
                )
                .unwrap(),
            ),
            Arc::new(|| Router(false)),
        )
        .unwrap();
    let sink = builder
        .node(
            "sink",
            Arc::new(Interface::new([PortDef::local::<i64>("in")], []).unwrap()),
            Arc::new(move || OptionalRecord {
                remaining: 0,
                seen: shared.clone(),
            }),
        )
        .unwrap();
    builder
        .expose_input("input", router.input::<i64>("in").unwrap(), one())
        .unwrap();
    builder
        .connect(
            router.output::<i64>("out").unwrap(),
            sink.input::<i64>("in").unwrap(),
            NonZeroUsize::new(8).unwrap(),
        )
        .unwrap();
    let mut options = lane_options();
    options.readiness.insert(
        "sink".into(),
        Readiness::Optional {
            required: vec![],
            optional: vec!["in".into()],
        },
    );
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);
    let a = session.begin_request(None).unwrap();
    let b = session.begin_request(None).unwrap();
    session
        .inject_request(a, "input", Value::local(0_i64))
        .unwrap();
    let mut next = 1_i64;
    for tick in 0..96 {
        if tick == 24 {
            assert!(seen.lock().unwrap().len() >= 2);
            assert!(
                session.request_work(a).unwrap().queued >= 2,
                "A must already have a backlog: {:?}",
                session.request_work(a).unwrap()
            );
            session
                .inject_request(b, "input", Value::local(-1_i64))
                .unwrap();
            session.close_request(b).unwrap();
            assert_eq!(session.request_work(b).unwrap().closure_pending, 1);
        }
        assert_eq!(session.run_until_idle(1).steps, 1);
        match session.inject_request(a, "input", Value::local(next)) {
            Ok(()) => next += 1,
            Err(error) => {
                assert_eq!(error.reason, RequestError::Transport(ConnectionError::Full));
                assert_eq!(error.value.borrow::<i64>(), Some(&next));
            }
        }
        if tick >= 24
            && session.request_status(b).unwrap() == (RequestStatus::Settled { captures: 0 })
        {
            break;
        }
    }
    assert_eq!(
        session.request_status(b).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    assert_eq!(session.request_work(b).unwrap().closure_pending, 0);
    assert_eq!(session.request_status(a).unwrap(), RequestStatus::Open);
    assert_eq!(session.state(), SessionState::Runnable);
    assert!(session.request_work(a).unwrap().queued > 0);
    session.release_request(b).unwrap();

    session.close_request(a).unwrap();
    session.run_until_idle(256);
    let records = seen.lock().unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|(lane, _)| *lane == 1)
            .copied()
            .collect::<Vec<_>>(),
        [(1, None)]
    );
    assert_eq!(
        records
            .iter()
            .filter_map(|(lane, value)| (*lane == 0).then_some(*value))
            .collect::<Vec<_>>(),
        (0..next).map(Some).collect::<Vec<_>>()
    );
    drop(records);
    assert_eq!(
        session.request_status(a).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    assert!(session.failure().is_none());
    assert_eq!(session.run_until_idle(8).steps, 0);
    session.release_request(a).unwrap();
}
