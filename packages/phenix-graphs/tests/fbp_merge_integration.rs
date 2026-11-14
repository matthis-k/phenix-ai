use phenix_graph::{
    ComponentDef, Interface, NetworkBuilder, PortDef, Process, ProcessContext, ProcessEmission,
    ProcessFactory, Step, Value,
    correlation::{CorrelationBounds, bounded},
    network::CompiledNetwork,
    runtime::{
        Readiness, ReadinessPolicy, ReadinessSnapshot, RequestError, RequestQueuePolicy,
        RequestStatus, SessionOptions, SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn one() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}

struct Consume;
impl Process for Consume {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        cx.take("in");
        Ok(Step::Complete)
    }
}

struct PolicyFactory(Readiness);
impl ProcessFactory for PolicyFactory {
    fn spawn(&self) -> Box<dyn Process> {
        Box::new(Consume)
    }

    fn readiness(&self) -> Readiness {
        self.0.clone()
    }
}

#[test]
fn resolved_factory_readiness_is_validated_before_scheduling() {
    for policy in [
        Readiness::Optional {
            required: vec![],
            optional: vec!["missing".into()],
        },
        Readiness::Optional {
            required: vec![],
            optional: vec![],
        },
        Readiness::AtLeast(NonZeroUsize::new(2).unwrap()),
    ] {
        let mut builder = NetworkBuilder::new();
        let root = builder
            .add("root", Arc::new(ComponentDef::map(|value: i64| value)))
            .unwrap();
        let sink = builder
            .node(
                "sink",
                Arc::new(Interface::new([PortDef::local::<i64>("in")], []).unwrap()),
                Arc::new(PolicyFactory(policy)),
            )
            .unwrap();
        builder
            .expose_input("input", root.input::<i64>("input").unwrap(), one())
            .unwrap();
        builder
            .connect(
                root.output::<i64>("output").unwrap(),
                sink.input::<i64>("in").unwrap(),
                one(),
            )
            .unwrap();
        let mut session = builder.build().unwrap().compile().unwrap().start();
        assert_eq!(session.state(), SessionState::Failed);
        assert!(session.failure().unwrap().contains("process sink:"));
        assert_eq!(session.begin_request(None), Err(RequestError::Stale));
        assert_eq!(session.run_until_idle(8).steps, 0);
    }
}

struct Join {
    phase: usize,
    staged: bool,
}
impl Process for Join {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.phase == 0 && self.staged {
            cx.take("a").ok_or("missing initial a")?;
            self.phase = 1;
            return Ok(Step::WaitInputs(Readiness::CorrelatedAll));
        }
        if self.phase == 2 {
            self.phase = 0;
            return Ok(Step::Complete);
        }
        let a = cx.take("a").ok_or("missing a")?;
        let b = cx.take("b").ok_or("missing b")?;
        self.phase = 2;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::local(a.borrow::<i64>().unwrap() + b.borrow::<i64>().unwrap()),
        )))
    }
}

fn bounded_join(staged: bool) -> CompiledNetwork {
    let two = NonZeroUsize::new(2).unwrap();
    let definition = bounded(
        Arc::new(
            Interface::new(
                [PortDef::local::<i64>("a"), PortDef::local::<i64>("b")],
                [PortDef::local::<i64>("out")],
            )
            .unwrap(),
        ),
        Arc::new(move || Join { phase: 0, staged }),
        if staged {
            Readiness::Any
        } else {
            Readiness::CorrelatedAll
        },
        CorrelationBounds {
            max_live: two,
            distinct_scope_skew: two,
            ingress_capacity: two,
            lane_capacity: one(),
        },
    )
    .unwrap();
    let mut builder = NetworkBuilder::new();
    let join = builder.add("bounded", Arc::new(definition)).unwrap();
    for name in ["a", "b"] {
        builder
            .expose_input(name, join.input::<i64>(name).unwrap(), two)
            .unwrap();
    }
    builder
        .expose_output("out", join.output::<i64>("out").unwrap())
        .unwrap();
    builder.build().unwrap().compile().unwrap()
}

#[test]
fn bounded_correlated_all_joins_crossed_requests_in_both_queue_modes() {
    for request_queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session = bounded_join(false).start_with_options(SessionOptions {
            max_requests: NonZeroUsize::new(2).unwrap(),
            request_queue_policy,
            ..Default::default()
        });
        let a = session.begin_request(Some("out")).unwrap();
        let b = session.begin_request(Some("out")).unwrap();
        for (request, port, value) in [(a, "a", 1_i64), (b, "b", 20), (b, "a", 10), (a, "b", 2)] {
            session.inject_request_local(request, port, value).unwrap();
        }
        for request in [a, b] {
            session.close_request(request).unwrap();
        }
        session.run_until_idle(64);
        for (request, expected) in [(a, 3), (b, 30)] {
            let packet = session
                .take_request_output(request, "out")
                .unwrap()
                .unwrap();
            assert_eq!(packet.value().borrow::<i64>(), Some(&expected));
            assert_eq!(packet.metadata().unwrap().request(), Some(request));
            assert_eq!(
                session.request_status(request).unwrap(),
                RequestStatus::Settled { captures: 1 }
            );
            session.release_request(request).unwrap();
        }
        assert!(session.failure().is_none());
    }
}

#[test]
fn bounded_correlated_all_staged_wait_requires_the_whole_lane_group() {
    for request_queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let mut session = bounded_join(true).start_with_options(SessionOptions {
            max_requests: NonZeroUsize::new(2).unwrap(),
            request_queue_policy,
            ..Default::default()
        });
        let spare = session.begin_request(None).unwrap();
        let request = session.begin_request(Some("out")).unwrap();
        session.inject_request_local(request, "a", 0_i64).unwrap();
        session.run_until_idle(32);
        let activation = session.last_step().unwrap().activation;
        assert_eq!(session.request_work(request).unwrap().waiting_input, 1);
        session.inject_request_local(request, "b", 20_i64).unwrap();
        session.run_until_idle(32);
        assert_eq!(session.request_work(request).unwrap().waiting_input, 1);
        assert!(
            session
                .take_request_output(request, "out")
                .unwrap()
                .is_none()
        );
        session.inject_request_local(request, "a", 10_i64).unwrap();
        session.close_request(request).unwrap();
        session.run_until_idle(32);
        let packet = session
            .take_request_output(request, "out")
            .unwrap()
            .unwrap();
        assert_eq!(packet.value().borrow::<i64>(), Some(&30));
        assert_eq!(packet.metadata().unwrap().activation(), activation);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        session.release_request(request).unwrap();
        session.close_request(spare).unwrap();
        session.release_request(spare).unwrap();
        assert!(session.failure().is_none());
    }
}

struct NotifiedPolicy {
    calls: Arc<AtomicUsize>,
    enabled: Arc<AtomicBool>,
}
impl ReadinessPolicy for NotifiedPolicy {
    fn notification_driven(&self) -> bool {
        true
    }

    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.enabled.load(Ordering::SeqCst) {
            snapshot
                .heads
                .iter()
                .filter(|head| head.available)
                .map(|head| head.port.clone())
                .collect()
        } else {
            vec![]
        }
    }
}

#[test]
fn leased_factory_readiness_keeps_targeted_notification_contract() {
    for leased in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let enabled = Arc::new(AtomicBool::new(false));
        let mut builder = NetworkBuilder::new();
        let sink = builder
            .node(
                "sink",
                Arc::new(Interface::new([PortDef::local::<i64>("in")], []).unwrap()),
                Arc::new(PolicyFactory(Readiness::Custom(Arc::new(NotifiedPolicy {
                    calls: calls.clone(),
                    enabled: enabled.clone(),
                })))),
            )
            .unwrap();
        builder
            .expose_input("input", sink.input::<i64>("in").unwrap(), one())
            .unwrap();
        for index in 0..4 {
            let name = format!("idle-{index}");
            let idle = builder
                .add(&name, Arc::new(ComponentDef::sink(|_: &i64| {})))
                .unwrap();
            builder
                .expose_input(name, idle.input::<i64>("input").unwrap(), one())
                .unwrap();
        }
        let compiled = builder.build().unwrap().compile().unwrap();
        let compiled = if leased {
            compiled.with_lease(Arc::new(()))
        } else {
            compiled
        };
        let mut session = compiled.start();
        let notifier = session.process_notifier("sink").unwrap();
        session.inject_local("input", 1_i64).unwrap();
        session.run_until_idle(1);
        let before = calls.load(Ordering::SeqCst);
        assert!(before > 0);
        for _ in 0..10 {
            assert_eq!(session.run_until_idle(1).steps, 0);
        }
        assert_eq!(calls.load(Ordering::SeqCst), before, "leased={leased}");
        enabled.store(true, Ordering::SeqCst);
        assert!(notifier.notify());
        assert_eq!(session.run_until_idle(8).steps, 1);
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        assert_eq!(session.state(), SessionState::Quiescent);
        assert!(session.failure().is_none());
    }
}
