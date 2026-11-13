use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::{CompiledNetwork, NetworkBuilder},
    process::{Process, ProcessContext, ProcessEmission, Step},
    runtime::{
        NetworkSession, Readiness, ReadinessPolicy, ReadinessSnapshot, SessionOptions, SessionState,
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
struct Counter {
    sum: i64,
    emitted: bool,
    effects: Arc<AtomicUsize>,
}
impl Process for Counter {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let Some(value) = cx.take("in") else {
            return Ok(Step::Finished);
        };
        self.effects.fetch_add(1, Ordering::SeqCst);
        self.sum += value.get::<i64>()?;
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(self.sum))))
    }
}
fn counter(effects: Arc<AtomicUsize>) -> CompiledNetwork {
    let mut b = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let p = b
        .node(
            "counter",
            interface,
            Arc::new(move || Counter {
                sum: 0,
                emitted: false,
                effects: effects.clone(),
            }),
        )
        .unwrap();
    b.expose_input("input", p.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.expose_output("output", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    b.build().unwrap().compile().unwrap()
}
fn options() -> SessionOptions {
    SessionOptions {
        capture_capacity: one(),
        ..Default::default()
    }
}

struct ReadinessCalls(Arc<AtomicUsize>, Arc<AtomicBool>);
impl ReadinessPolicy for ReadinessCalls {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if !self.1.load(Ordering::SeqCst) {
            return Vec::new();
        }
        snapshot
            .heads
            .iter()
            .filter(|head| head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}

#[test]
fn session_state_is_read_only_and_does_not_execute_custom_readiness() {
    let effects = Arc::new(AtomicUsize::new(0));
    let compiled = counter(effects.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let allowed = Arc::new(AtomicBool::new(false));
    let mut config = options();
    config.readiness.insert(
        "counter".into(),
        Readiness::Custom(Arc::new(ReadinessCalls(calls.clone(), allowed.clone()))),
    );
    let mut session = compiled.start_with_options(config);
    let before_query = calls.load(Ordering::SeqCst);
    // Startup has no input heads, so the custom policy need not be invoked.
    assert_eq!(session.state(), SessionState::Quiescent);
    assert_eq!(calls.load(Ordering::SeqCst), before_query);
    assert_eq!(effects.load(Ordering::SeqCst), 0);

    session.inject("input", Value::of(2_i64)).unwrap();
    let after_admission = calls.load(Ordering::SeqCst);
    assert!(after_admission > before_query, "admission still schedules");
    let snapshot: &NetworkSession = &session;
    for _ in 0..4 {
        assert_eq!(snapshot.state(), SessionState::Waiting);
    }
    assert_eq!(calls.load(Ordering::SeqCst), after_admission);

    allowed.store(true, Ordering::SeqCst);
    // A pure observation does not drive a newly enabled readiness policy.
    assert_eq!(snapshot.state(), SessionState::Waiting);
    assert_eq!(calls.load(Ordering::SeqCst), after_admission);
    assert!(session.run_until_idle(64).steps > 0);
    assert_eq!(
        session
            .take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        2
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn deferred_job_supervision_invalidates_ticket_without_reclaiming_owned_process() {
    use phenix_graph::process::HandoffError;
    use std::time::{Duration, Instant};

    let compiled = counter(Arc::new(AtomicUsize::new(0)));
    let mut config = options();
    config.deferred_timeout = Some(Duration::from_secs(1));
    let mut session = compiled.start_with_options(config);
    session.inject("input", Value::of(7_i64)).unwrap();
    let (_, job) = session.next_job().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let overdue = session.overdue_jobs(deadline);
    assert_eq!(overdue.len(), 1);
    assert_eq!(overdue[0].ticket, job.ticket());
    assert_eq!(session.state(), SessionState::Waiting);
    assert_eq!(session.supervise_deferred(deadline), overdue);
    assert_eq!(session.state(), SessionState::Failed);
    assert!(session.failure().unwrap().contains("deferred job timeout"));
    assert!(session.overdue_jobs(deadline).is_empty());
    let (reason, _returned) = session.complete_job(job.run()).unwrap_err();
    assert_eq!(reason, HandoffError::StaleCompletion);

    let mut unrestricted = compiled.start();
    unrestricted.inject("input", Value::of(9_i64)).unwrap();
    let (index, job) = unrestricted.next_job().unwrap();
    assert!(unrestricted.overdue_jobs(deadline).is_empty());
    unrestricted.complete(index, job.run()).unwrap();
    assert_ne!(unrestricted.state(), SessionState::Failed);
}

#[test]
fn persistent_injection_capacity_recovery_and_independent_sessions() {
    let compiled = counter(Arc::new(AtomicUsize::new(0)));
    let mut a = compiled.start_with_options(options());
    let mut b = compiled.start();
    a.inject("input", Value::of(2_i64)).unwrap();
    a.run_until_idle(100);
    assert_eq!(
        a.take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        2
    );
    a.inject("input", Value::of(3_i64)).unwrap();
    a.run_until_idle(100);
    assert_eq!(
        a.take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        5
    );
    assert_eq!(a.state(), SessionState::Quiescent);
    b.inject("input", Value::of(3_i64)).unwrap();
    b.run_until_idle(100);
    assert_eq!(
        b.take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        3
    );
    a.close_input("input").unwrap();
    assert_eq!(a.run_until_idle(100).state, SessionState::Completed);
    assert!(a.output_closed("output").unwrap());
}

#[test]
fn slow_capture_retains_emission_without_repeating_effects() {
    let effects = Arc::new(AtomicUsize::new(0));
    let mut s = counter(effects.clone()).start_with_options(options());
    s.inject("input", Value::of(1_i64)).unwrap();
    s.run_until_idle(100);
    s.inject("input", Value::of(2_i64)).unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    s.inject("input", Value::of(4_i64)).unwrap();
    let error = s.inject("input", Value::of(8_i64)).unwrap_err();
    assert_eq!(
        error.reason,
        phenix_graph::connection::ConnectionError::Full
    );
    assert_eq!(error.value.get::<i64>().unwrap(), 8);
    assert_eq!(s.blocked_queues().len(), 1);
    for _ in 0..5 {
        s.run_until_idle(100);
    }
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert_eq!(
        s.take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        1
    );
    s.run_until_idle(100);
    assert_eq!(
        s.take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        3
    );
    s.run_until_idle(100);
    assert_eq!(
        s.take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        7
    );
}

#[test]
fn delayed_completion_wrong_owner_and_cancellation() {
    let compiled = counter(Arc::new(AtomicUsize::new(0)));
    let mut a = compiled.start();
    let mut b = compiled.start();
    a.inject("input", Value::of(1_i64)).unwrap();
    let (index, job) = a.next_job().unwrap();
    assert_eq!(a.state(), SessionState::Waiting);
    let completion = std::thread::spawn(move || job.run()).join().unwrap();
    let (_, returned) = b.complete(index, completion).unwrap_err();
    a.complete(index, *returned).unwrap();
    a.run_until_idle(100);
    assert!(a.take_output("output").unwrap().is_some());
    a.inject("input", Value::of(2_i64)).unwrap();
    let (_, job) = a.next_job().unwrap();
    a.cancel();
    let (error, returned) = a.complete_job(job.run()).unwrap_err();
    assert_eq!(error, phenix_graph::process::HandoffError::StaleCompletion);
    drop(returned);
    assert_eq!(a.state(), SessionState::Cancelled);
    assert!(a.take_output("output").unwrap().is_none());
}

struct Split {
    pending: Option<i64>,
    phase: usize,
}
impl Process for Split {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.phase == 0 {
            self.pending = cx.take("in").map(|v| v.get()).transpose()?;
            if self.pending.is_none() {
                return Ok(Step::Finished);
            }
        }
        self.phase += 1;
        match self.phase {
            1 => Ok(Step::Emit(ProcessEmission::new(
                "a",
                Value::of(self.pending.unwrap()),
            ))),
            2 => Ok(Step::Emit(ProcessEmission::new(
                "b",
                Value::of(self.pending.unwrap() * 2),
            ))),
            _ => {
                self.phase = 0;
                Ok(Step::Complete)
            }
        }
    }
}
struct Join;
impl Process for Join {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if let (Some(a), Some(b)) = (cx.take("a"), cx.take("b")) {
            assert_eq!(b.get::<i64>()?, a.get::<i64>()? * 2);
            Ok(Step::Complete)
        } else {
            Ok(Step::Finished)
        }
    }
}
#[test]
fn split_join_multiple_outputs_and_zero_output_sink() {
    let mut b = NetworkBuilder::new();
    let split = b
        .node(
            "split",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                )
                .unwrap(),
            ),
            Arc::new(|| Split {
                pending: None,
                phase: 0,
            }),
        )
        .unwrap();
    let join = b
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(|| Join),
        )
        .unwrap();
    b.connect(
        split.wire_output::<i64>("a").unwrap(),
        join.wire_input::<i64>("a").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        split.wire_output::<i64>("b").unwrap(),
        join.wire_input::<i64>("b").unwrap(),
        one(),
    )
    .unwrap();
    b.expose_input("input", split.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    let mut opts = options();
    opts.readiness.insert("join".into(), Readiness::All);
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    for n in 0..4_i64 {
        s.inject("input", Value::of(n)).unwrap();
        assert_eq!(s.run_until_idle(100).state, SessionState::Quiescent);
    }
    s.close_input("input").unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Completed);
}

struct Source {
    emitted: bool,
}
impl Process for Source {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            Ok(Step::Complete)
        } else {
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::local(42_i64),
            )))
        }
    }
}
#[test]
fn explicit_source_wakes_later_and_local_type_validation() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| Source { emitted: false }),
        )
        .unwrap();
    b.start(&p).unwrap();
    b.expose_output("out", p.output::<i64>("out").unwrap())
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    let wake = s.wake_handle("source").unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    assert_eq!(
        s.take_output("out")
            .unwrap()
            .unwrap()
            .value()
            .borrow::<i64>(),
        Some(&42)
    );
    wake.wake();
    wake.wake();
    s.run_until_idle(100);
    assert!(s.take_output("out").unwrap().is_some());
    assert!(s.take_output("out").unwrap().is_none());
    let mut wire = counter(Arc::new(AtomicUsize::new(0))).start();
    assert_eq!(
        wire.inject_local("input", 1_i64).unwrap_err().reason,
        phenix_graph::connection::ConnectionError::WrongType
    );
}

#[test]
fn capture_aliases_are_one_atomic_fanout() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| Source { emitted: false }),
        )
        .unwrap();
    b.start(&p).unwrap();
    b.expose_output("a", p.output::<i64>("out").unwrap())
        .unwrap();
    b.expose_output("b", p.output::<i64>("out").unwrap())
        .unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options());
    s.run_until_idle(100);
    s.take_output("a").unwrap().unwrap();
    s.wake_handle("source").unwrap().wake();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    assert!(s.take_output("a").unwrap().is_none());
    s.take_output("b").unwrap().unwrap();
    s.run_until_idle(100);
    assert!(s.take_output("a").unwrap().is_some());
    assert!(s.take_output("b").unwrap().is_some());
}

struct Waiter {
    phase: usize,
    total: Arc<AtomicUsize>,
}
impl Process for Waiter {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        match self.phase {
            0 => {
                cx.take("a").unwrap();
                self.phase = 1;
                Ok(Step::WaitInput("b".into()))
            }
            _ => {
                if let Some(value) = cx.take("b") {
                    self.total
                        .fetch_add(value.get::<i64>()? as usize, Ordering::SeqCst);
                    Ok(Step::WaitInput("b".into()))
                } else if cx.is_closed("b") {
                    Ok(Step::Finished)
                } else {
                    Err("resumed without data or EOS".into())
                }
            }
        }
    }
}
#[test]
fn wait_input_resumes_same_activation_for_values_and_eos() {
    let total = Arc::new(AtomicUsize::new(0));
    let shared = total.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "wait",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || Waiter {
                phase: 0,
                total: shared.clone(),
            }),
        )
        .unwrap();
    for port in ["a", "b"] {
        b.expose_input(port, p.wire_input::<i64>(port).unwrap(), one())
            .unwrap();
    }
    let mut s = b.build().unwrap().compile().unwrap().start();
    s.inject("a", Value::of(1_i64)).unwrap();
    assert_eq!(s.run_until_idle(10).state, SessionState::Waiting);
    let activation = s.last_step().unwrap().activation;
    for n in [2_i64, 3] {
        s.inject("b", Value::of(n)).unwrap();
        s.run_until_idle(10);
        assert_eq!(s.last_step().unwrap().activation, activation);
    }
    assert_eq!(total.load(Ordering::SeqCst), 5);
    s.close_input("b").unwrap();
    assert_ne!(s.run_until_idle(10).state, SessionState::Completed);
    s.close_input("a").unwrap();
    assert_eq!(s.state(), SessionState::Completed);
}

#[test]
fn ingress_fanout_is_atomic_when_one_branch_is_slow() {
    let mut b = NetworkBuilder::new();
    let mut effects = vec![];
    for name in ["a", "b"] {
        let effect = Arc::new(AtomicUsize::new(0));
        effects.push(effect.clone());
        let p = b
            .node(
                name,
                Arc::new(
                    Interface::new(
                        [PortDef::wire::<i64>("in").unwrap()],
                        [PortDef::wire::<i64>("out").unwrap()],
                    )
                    .unwrap(),
                ),
                Arc::new(move || Counter {
                    sum: 0,
                    emitted: false,
                    effects: effect.clone(),
                }),
            )
            .unwrap();
        b.expose_input("in", p.wire_input::<i64>("in").unwrap(), one())
            .unwrap();
        b.expose_output(name, p.wire_output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options());
    for n in [1_i64, 2] {
        s.inject("in", Value::of(n)).unwrap();
        s.run_until_idle(100);
    }
    s.take_output("a").unwrap().unwrap();
    s.run_until_idle(100);
    s.inject("in", Value::of(3_i64)).unwrap();
    s.run_until_idle(100);
    s.take_output("a").unwrap().unwrap();
    s.run_until_idle(100);
    assert_eq!(
        s.inject("in", Value::of(4_i64)).unwrap_err().reason,
        phenix_graph::connection::ConnectionError::Full
    );
    s.take_output("a").unwrap().unwrap();
    s.run_until_idle(100);
    assert_eq!(effects[0].load(Ordering::SeqCst), 3);
    assert!(s.take_output("a").unwrap().is_none());
    s.take_output("b").unwrap().unwrap();
    s.run_until_idle(100);
    s.inject("in", Value::of(4_i64)).unwrap();
    s.run_until_idle(100);
    assert_eq!(effects[0].load(Ordering::SeqCst), 4);
}

struct Inspect {
    expected: usize,
    seen: Arc<AtomicUsize>,
}
impl Process for Inspect {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        assert_eq!(cx.inputs().count(), self.expected);
        self.seen.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}
#[test]
fn at_least_policy_requires_distinct_ports_and_reserves_exactly_n() {
    let mut b = NetworkBuilder::new();
    let seen = Arc::new(AtomicUsize::new(0));
    let shared = seen.clone();
    let p = b
        .node(
            "n",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                        PortDef::wire::<i64>("c").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || Inspect {
                expected: 2,
                seen: shared.clone(),
            }),
        )
        .unwrap();
    for port in ["a", "b", "c"] {
        b.expose_input(port, p.wire_input::<i64>(port).unwrap(), one())
            .unwrap();
    }
    let mut options = options();
    options.readiness.insert(
        "n".into(),
        Readiness::AtLeast(NonZeroUsize::new(2).unwrap()),
    );
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options);
    s.inject("a", Value::of(1_i64)).unwrap();
    assert_eq!(s.run_until_idle(10).steps, 0);
    s.inject("b", Value::of(2_i64)).unwrap();
    s.run_until_idle(10);
    assert_eq!(seen.load(Ordering::SeqCst), 1);
}

#[test]
fn step_budget_and_rejected_jobs_preserve_reservation() {
    let effects = Arc::new(AtomicUsize::new(0));
    let mut s = counter(effects.clone()).start();
    s.inject("input", Value::of(1_i64)).unwrap();
    assert!(s.run_until_idle(0).budget_exhausted);
    let (index, job) = s.next_job().unwrap();
    let first = job.ticket();
    s.reject(index, job).unwrap();
    let (index, job) = s.next_job().unwrap();
    assert_eq!(job.ticket().activation(), first.activation());
    assert_ne!(job.ticket().sequence(), first.sequence());
    s.complete(index, job.run()).unwrap();
    let report = s.run_until_idle(1);
    assert_eq!(report.steps, 1);
    assert_eq!(report.publications, 1);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn capture_and_step_metadata_use_actual_compiled_producer_origin() {
    use phenix_graph::{GraphOrigin, component::ComponentDef};
    let origin = GraphOrigin::default();
    let interface = Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap());
    let definition = Arc::new(
        ComponentDef::primitive(interface, Arc::new(|| Source { emitted: false }))
            .with_origin(origin.clone()),
    );
    let mut b = NetworkBuilder::new();
    let p = b.add("source", definition).unwrap();
    b.start(&p).unwrap();
    b.expose_output("out", p.output::<i64>("out").unwrap())
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    let (index, job) = s.next_job().unwrap();
    let ticket = job.ticket();
    s.complete(index, job.run()).unwrap();
    let metadata = s.last_step().unwrap();
    assert_eq!(metadata.origin.as_ref(), Some(&origin));
    assert_eq!(metadata.path, "source");
    assert_eq!(metadata.output.as_deref(), Some("out"));
    assert_eq!(metadata.activation, ticket.activation());
    assert_eq!(s.run_until_idle(10).publications, 1);
    assert_eq!(s.take_output("out").unwrap().unwrap().origin, Some(origin));
}

struct BadEmission;
impl Process for BadEmission {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(42_i64))))
    }
}
#[test]
fn wrong_emitted_schema_fails_even_with_no_output_consumers() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "bad",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| BadEmission),
        )
        .unwrap();
    b.start(&p).unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    assert_eq!(s.run_until_idle(10).state, SessionState::Failed);
    assert!(s.failure().unwrap().contains("invalid emission"));
}

struct FairInput(Arc<std::sync::Mutex<Vec<String>>>);
impl Process for FairInput {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        let names: Vec<_> = cx.inputs().map(|(name, _)| name.to_owned()).collect();
        assert_eq!(names.len(), 1);
        self.0.lock().unwrap().extend(names);
        Ok(Step::Complete)
    }
}
#[test]
fn default_any_rotates_between_available_ports() {
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let shared = log.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "fair",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                        PortDef::wire::<i64>("c").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || FairInput(shared.clone())),
        )
        .unwrap();
    for port in ["a", "b", "c"] {
        b.expose_input(port, p.wire_input::<i64>(port).unwrap(), one())
            .unwrap();
    }
    let mut s = b.build().unwrap().compile().unwrap().start();
    for port in ["a", "a", "b", "c"] {
        s.inject(port, Value::of(1_i64)).unwrap();
    }
    s.run_until_idle(10);
    assert_eq!(*log.lock().unwrap(), ["a", "b", "c", "a"]);
}

struct DropPayload(Arc<AtomicUsize>);
impl Drop for DropPayload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct Hold;
impl Process for Hold {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Yield)
    }
}
#[test]
fn cancellation_disposes_owned_queues_but_does_not_claim_to_preempt_runner() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "hold",
            Arc::new(Interface::new([PortDef::local::<DropPayload>("in")], []).unwrap()),
            Arc::new(|| Hold),
        )
        .unwrap();
    b.expose_input("in", p.input::<DropPayload>("in").unwrap(), one())
        .unwrap();
    let mut s = b.build().unwrap().compile().unwrap().start();
    s.inject_local("in", DropPayload(drops.clone())).unwrap();
    s.inject_local("in", DropPayload(drops.clone())).unwrap();
    let (index, job) = s.next_job().unwrap();
    s.cancel();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    drop(s.complete(index, job.run()).unwrap_err());
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn completion_requires_closed_ingress_and_drained_capture() {
    let mut s = counter(Arc::new(AtomicUsize::new(0))).start();
    s.inject("input", Value::of(1_i64)).unwrap();
    s.close_input("input").unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Waiting);
    assert!(!s.output_closed("output").unwrap());
    s.take_output("output").unwrap().unwrap();
    assert_eq!(s.state(), SessionState::Completed);
    assert!(s.next_job().is_none());
}

#[test]
fn request_accounting_spans_reservation_job_publication_and_capture() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let compiled = counter(Arc::new(AtomicUsize::new(0)));
    let mut opts = options();
    opts.max_requests = one();
    let mut s = compiled.start_with_options(opts);
    let mut previous = 0;
    for n in [2_i64, 3] {
        let id = s.begin_request(Some("output")).unwrap();
        assert!(id.sequence() > previous);
        previous = id.sequence();
        assert_eq!(s.begin_request(None).unwrap_err(), RequestError::Full);
        s.inject_request(id, "input", Value::of(n)).unwrap();
        s.close_request(id).unwrap();
        assert_eq!(s.request_work(id).unwrap().ready, 1);
        assert_eq!(
            s.inject_request(id, "input", Value::of(4_i64))
                .unwrap_err()
                .reason,
            RequestError::Sealed
        );
        let (index, job) = s.next_job().unwrap();
        assert_eq!(s.request_work(id).unwrap().dispatched, 1);
        s.complete(index, job.run()).unwrap();
        assert_eq!(s.request_work(id).unwrap().publication_pending, 1);
        s.run_until_idle(100);
        assert_eq!(s.request_work(id).unwrap().queued, 1);
        assert_eq!(s.request_status(id).unwrap(), RequestStatus::Pending);
        let packet = s.take_request_output(id, "output").unwrap().unwrap();
        s.validate_capture(id, "output", &packet).unwrap();
        let metadata = packet.metadata().unwrap();
        assert_eq!(metadata.request(), Some(id));
        assert_eq!(metadata.process(), Some(0));
        assert_eq!(metadata.output(), "out");
        assert!(metadata.activation() > 0);
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        assert_eq!(s.state(), SessionState::Quiescent);
        let mut foreign = compiled.start();
        assert_eq!(
            foreign
                .inject_request(id, "input", Value::of(9_i64))
                .unwrap_err()
                .reason,
            RequestError::Foreign
        );
        assert_eq!(
            foreign.validate_capture(id, "output", &packet).unwrap_err(),
            RequestError::Foreign
        );
        s.release_request(id).unwrap();
        assert_eq!(s.request_status(id).unwrap_err(), RequestError::Stale);
    }
}

struct CorrelatedJoin {
    emitted: bool,
}
impl Process for CorrelatedJoin {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let a = cx.take("a").unwrap().get::<i64>()?;
        let b = cx.take("b").unwrap().get::<i64>()?;
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(a + b))))
    }
}
fn correlated_join_with(policy: Readiness) -> phenix_graph::runtime::NetworkSession {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| CorrelatedJoin { emitted: false }),
        )
        .unwrap();
    for port in ["a", "b"] {
        b.expose_input(
            port,
            p.wire_input::<i64>(port).unwrap(),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    }
    b.expose_output("out", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    let mut options = options();
    options.capture_capacity = NonZeroUsize::new(2).unwrap();
    options.readiness.insert("join".into(), policy);
    b.build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options)
}
fn correlated_join() -> phenix_graph::runtime::NetworkSession {
    correlated_join_with(Readiness::All)
}

#[test]
fn correlated_all_crossed_heads_settle_in_one_bounded_shared_session() {
    use phenix_graph::runtime::RequestStatus;

    let mut session = correlated_join_with(Readiness::CorrelatedAll);
    let a = session.begin_request(Some("out")).unwrap();
    let b = session.begin_request(Some("out")).unwrap();
    for (request, port, value) in [(a, "a", 1_i64), (b, "b", 20), (b, "a", 10), (a, "b", 2)] {
        session
            .inject_request(request, port, Value::of(value))
            .unwrap();
    }
    session.close_request(a).unwrap();
    session.close_request(b).unwrap();
    assert_eq!(session.run_until_idle(100).state, SessionState::Waiting);
    // Captures retain their own global FIFO. The eagerly selectable B
    // request may publish first; read FIFO and verify authenticated metadata.
    let mut captured = Vec::new();
    for _ in 0..2 {
        let packet = session.take_output("out").unwrap().unwrap();
        captured.push((
            packet.metadata().unwrap().request().unwrap(),
            packet.value().get::<i64>().unwrap(),
        ));
    }
    captured.sort_by_key(|(request, _)| *request);
    assert_eq!(captured, vec![(a, 3), (b, 30)]);
    assert_eq!(
        session.request_status(a).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert_eq!(
        session.request_status(b).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert!(session.correlation_blocks().is_empty());
}

#[test]
fn correlated_all_cancelled_request_releases_only_its_own_packets() {
    use phenix_graph::runtime::RequestStatus;

    let mut session = correlated_join_with(Readiness::CorrelatedAll);
    let a = session.begin_request(Some("out")).unwrap();
    let b = session.begin_request(Some("out")).unwrap();
    session.inject_request(a, "a", Value::of(1_i64)).unwrap();
    session.inject_request(b, "b", Value::of(20_i64)).unwrap();
    session.inject_request(b, "a", Value::of(10_i64)).unwrap();
    session.close_request(a).unwrap();
    session.close_request(b).unwrap();
    // A incomplete request must not consume B's packets or prevent an
    // ordinary request-local cancellation from disposing only A's queue.
    session.cancel_request(a).unwrap();
    assert_eq!(session.request_status(a).unwrap(), RequestStatus::Cancelled);
    session.run_until_idle(100);
    assert_eq!(
        session
            .take_request_output(b, "out")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        30
    );
    assert_eq!(
        session.request_status(b).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
}

#[test]
fn all_join_keeps_requests_separate_and_scoped_capture_reads_preserve_ownership() {
    use phenix_graph::runtime::RequestStatus;
    let mut s = correlated_join();
    let a = s.begin_request(Some("out")).unwrap();
    let b = s.begin_request(Some("out")).unwrap();
    for (id, port, value) in [(a, "a", 1_i64), (b, "a", 10), (a, "b", 2), (b, "b", 20)] {
        s.inject_request(id, port, Value::of(value)).unwrap();
    }
    s.close_request(a).unwrap();
    s.close_request(b).unwrap();
    assert!(s.correlation_blocks().is_empty());
    s.run_until_idle(100);
    // A request-scoped B read selects its own buffered packet past A.
    // The later A read still receives A, without mixing requests.
    assert_eq!(
        s.take_request_output(b, "out")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        30
    );
    assert_eq!(
        s.take_request_output(a, "out")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        3
    );
    assert_eq!(
        s.request_status(a).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert_eq!(
        s.request_status(b).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    let mut blocked = correlated_join();
    let a = blocked.begin_request(None).unwrap();
    let b = blocked.begin_request(None).unwrap();
    for (id, port) in [(a, "a"), (b, "b"), (b, "a"), (a, "b")] {
        blocked.inject_request(id, port, Value::of(1_i64)).unwrap();
    }
    blocked.close_request(a).unwrap();
    blocked.close_request(b).unwrap();
    let mismatch = blocked.correlation_blocks();
    assert_eq!(mismatch.len(), 1);
    assert_eq!(mismatch[0].path, "join");
    assert_eq!(
        mismatch[0].heads,
        vec![("a".into(), vec![a]), ("b".into(), vec![b])]
    );
    // Read-only diagnosis does not reserve, dequeue or discard any packet.
    assert_eq!(blocked.correlation_blocks(), mismatch);
    assert_eq!(blocked.run_until_idle(100).steps, 0);
    assert_eq!(blocked.state(), SessionState::Waiting);
    assert_eq!(blocked.correlation_blocks(), mismatch);
    assert_eq!(blocked.request_status(a).unwrap(), RequestStatus::Pending);
}

struct Duplicate {
    phase: usize,
}
impl Process for Duplicate {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.phase == 2 {
            self.phase = 0;
            return Ok(Step::Complete);
        }
        if self.phase == 0 && cx.take("in").unwrap().get::<i64>()? == 0 {
            return Ok(Step::Complete);
        }
        self.phase += 1;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(1_i64))))
    }
}
#[test]
fn exact_capture_does_not_settle_after_first_of_two_emissions() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "duplicate",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Duplicate { phase: 0 }),
        )
        .unwrap();
    b.expose_input("in", p.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.expose_output("out", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(options());
    let id = s.begin_request(Some("out")).unwrap();
    s.inject_request(id, "in", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    s.run_until_idle(100);
    s.take_request_output(id, "out").unwrap().unwrap();
    assert_eq!(s.request_status(id).unwrap(), RequestStatus::Pending);
    s.run_until_idle(100);
    s.take_request_output(id, "out").unwrap().unwrap();
    assert_eq!(
        s.request_status(id).unwrap_err(),
        RequestError::CaptureCount(2)
    );
    s.release_request(id).unwrap();
    let empty = s.begin_request(Some("out")).unwrap();
    s.inject_request(empty, "in", Value::of(0_i64)).unwrap();
    s.close_request(empty).unwrap();
    s.run_until_idle(100);
    assert_eq!(
        s.request_status(empty).unwrap_err(),
        RequestError::CaptureCount(0)
    );
    s.release_request(empty).unwrap();
}

#[test]
fn capture_authentication_distinguishes_export_aliases_and_rejects_stale_receipts() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "counter",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Counter {
                sum: 0,
                emitted: false,
                effects: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .unwrap();
    b.expose_input("in", p.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    for name in ["a", "b"] {
        b.expose_output(name, p.wire_output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut s = b.build().unwrap().compile().unwrap().start();
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "in", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    s.run_until_idle(100);
    let packet = s.take_request_output(id, "a").unwrap().unwrap();
    assert_eq!(packet.metadata().unwrap().capture(), Some("a"));
    assert_eq!(
        s.validate_capture(id, "b", &packet).unwrap_err(),
        RequestError::InvalidCapture
    );
    assert_eq!(
        s.validate_capture(
            id,
            "a",
            &phenix_graph::connection::Packet::new(Value::of(1_i64))
        )
        .unwrap_err(),
        RequestError::InvalidCapture
    );
    s.take_request_output(id, "b").unwrap().unwrap();
    assert_eq!(
        s.request_status(id).unwrap(),
        RequestStatus::Settled { captures: 2 }
    );
    s.cancel();
    assert_eq!(
        s.validate_capture(id, "a", &packet).unwrap_err(),
        RequestError::Stale
    );
}

#[test]
fn wake_latch_notifies_before_wait_cross_thread_and_invalidates() {
    use std::time::Duration;
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| Source { emitted: false }),
        )
        .unwrap();
    b.start(&p).unwrap();
    let mut opts = options();
    opts.discard_outputs.insert(("source".into(), "out".into()));
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    s.run_until_idle(100);
    let mut listener = s.wake_listener();
    let wake = s.wake_handle("source").unwrap();
    let other = wake.clone();
    assert!(std::thread::spawn(move || other.wake()).join().unwrap());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(s.state(), SessionState::Runnable);
    s.run_until_idle(100);
    assert!(!listener.wait_timeout(Duration::ZERO));
    let notifier = s.notifier();
    assert!(notifier.notify());
    assert!(listener.wait_timeout(Duration::ZERO));
    let mut sleeping = s.wake_listener();
    let (ready, started) = std::sync::mpsc::channel();
    let host = std::thread::spawn(move || {
        ready.send(()).unwrap();
        sleeping.wait_timeout(Duration::from_secs(2))
    });
    started.recv().unwrap();
    assert!(notifier.notify());
    assert!(host.join().unwrap());
    s.cancel();
    assert!(!wake.wake());
    assert!(!notifier.notify());
    assert!(listener.wait_timeout(Duration::ZERO));
    assert!(s.next_job().is_none());
}

struct WakeSource {
    phase: usize,
}
impl Process for WakeSource {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        match self.phase {
            0 => {
                self.phase = 1;
                Ok(Step::WaitWake)
            }
            1 => {
                self.phase = 2;
                Ok(Step::Emit(ProcessEmission::new(
                    "out",
                    Value::local(42_i64),
                )))
            }
            _ => {
                self.phase = 0;
                Ok(Step::Complete)
            }
        }
    }
}
#[test]
fn request_source_wake_preserves_scope_and_activation_without_spinning() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "async",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| WakeSource { phase: 0 }),
        )
        .unwrap();
    b.start(&p).unwrap();
    b.expose_output("out", p.output::<i64>("out").unwrap())
        .unwrap();
    let mut opts = options();
    opts.auto_start_sources = false;
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    assert_eq!(s.run_until_idle(100).steps, 0);
    let wake = s.wake_handle("async").unwrap();
    for _ in 0..2 {
        let id = s.begin_request(Some("out")).unwrap();
        s.start_request_source(id, "async").unwrap();
        assert_eq!(
            s.start_request_source(id, "async").unwrap_err(),
            RequestError::Busy
        );
        s.close_request(id).unwrap();
        let report = s.run_until_idle(100);
        assert_eq!(report.steps, 1);
        assert_eq!(report.state, SessionState::Waiting);
        assert_eq!(s.request_work(id).unwrap().waiting_wake, 1);
        assert_eq!(s.run_until_idle(100).steps, 0);
        let activation = s.last_step().unwrap().activation;
        let other = wake.clone();
        assert!(std::thread::spawn(move || other.wake()).join().unwrap());
        s.run_until_idle(100);
        let packet = s.take_request_output(id, "out").unwrap().unwrap();
        assert_eq!(packet.metadata().unwrap().activation(), activation);
        assert_eq!(packet.value().borrow::<i64>(), Some(&42));
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        s.release_request(id).unwrap();
        assert_eq!(s.state(), SessionState::Quiescent);
    }
}

struct ResourceUser;
impl Process for ResourceUser {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        assert_eq!(cx.read::<i64, _>("value", |v| *v)?, 7);
        assert!(cx.write::<i64, _>("value", |v| *v = 8).is_err());
        assert!(cx.read::<i64, _>("secret", |v| *v).is_err());
        Ok(Step::Finished)
    }
}
#[test]
fn resources_reference_existing_store_and_require_explicit_per_process_grants() {
    use phenix_graph::{Store, process::ResourceGrant};
    let mut store = Store::new();
    store.insert("value", 7_i64);
    store.insert("secret", 9_i64);
    let store = Arc::new(store);
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "user",
            Arc::new(Interface::new([], []).unwrap()),
            Arc::new(|| ResourceUser),
        )
        .unwrap();
    b.start(&p).unwrap();
    let c = b.build().unwrap().compile().unwrap();
    let mut opts = options();
    opts.resources.insert(
        "user".into(),
        vec![ResourceGrant::read(store.clone(), "value")],
    );
    assert_eq!(
        c.start_with_options(opts).run_until_idle(10).state,
        SessionState::Completed
    );
    assert_eq!(c.start().run_until_idle(10).state, SessionState::Failed);
    let cx = ProcessContext::default()
        .with_resources(vec![ResourceGrant::write(store.clone(), "value")]);
    cx.write::<i64, _>("value", |v| *v = 10).unwrap();
    assert_eq!(cx.read::<i64, _>("value", |v| *v).unwrap(), 10);
}

struct NativeLease(Arc<AtomicUsize>);
impl Drop for NativeLease {
    fn drop(&mut self) {
        self.0.store(0, Ordering::SeqCst);
    }
}
struct NativePayload(Arc<AtomicUsize>);
impl Drop for NativePayload {
    fn drop(&mut self) {
        assert_eq!(
            self.0.load(Ordering::SeqCst),
            1,
            "payload outlived native code"
        );
    }
}
struct NativeSource {
    alive: Arc<AtomicUsize>,
    emitted: bool,
}
impl Drop for NativeSource {
    fn drop(&mut self) {
        assert_eq!(
            self.alive.load(Ordering::SeqCst),
            1,
            "process outlived native code"
        );
    }
}
impl Process for NativeSource {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            Ok(Step::Finished)
        } else {
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "out",
                Value::local(NativePayload(self.alive.clone())),
            )))
        }
    }
}
fn native_source(alive: Arc<AtomicUsize>) -> CompiledNetwork {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "native",
            Arc::new(Interface::new([], [PortDef::local::<NativePayload>("out")]).unwrap()),
            Arc::new(move || NativeSource {
                alive: alive.clone(),
                emitted: false,
            }),
        )
        .unwrap();
    b.start(&p).unwrap();
    b.expose_output("out", p.output::<NativePayload>("out").unwrap())
        .unwrap();
    b.build().unwrap().compile().unwrap()
}
#[test]
fn session_lease_outlives_capture_and_late_completion_payloads() {
    let alive = Arc::new(AtomicUsize::new(1));
    let c = native_source(alive.clone());
    let mut opts = options();
    opts.lease = Some(Arc::new(NativeLease(alive.clone())));
    let mut s = c.start_with_options(opts);
    s.run_until_idle(100);
    let packet = s.take_output("out").unwrap().unwrap();
    drop(s);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(packet);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
    alive.store(1, Ordering::SeqCst);
    let mut opts = options();
    opts.lease = Some(Arc::new(NativeLease(alive.clone())));
    let mut s = c.start_with_options(opts);
    let (_, job) = s.next_job().unwrap();
    s.cancel();
    drop(s);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    let completion = job.run();
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(completion);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}

thread_local! { static IN_NATIVE_GUARD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
struct WorkerGuard(Arc<AtomicUsize>);
struct EnteredGuard;
impl Drop for EnteredGuard {
    fn drop(&mut self) {
        IN_NATIVE_GUARD.set(false);
    }
}
impl phenix_graph::process::ProcessExecutionGuard for WorkerGuard {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, String> {
        assert!(!IN_NATIVE_GUARD.get());
        IN_NATIVE_GUARD.set(true);
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(EnteredGuard))
    }
}
struct Guarded;
impl Process for Guarded {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        assert!(IN_NATIVE_GUARD.get());
        Ok(Step::Finished)
    }
}
struct Delayed(Arc<std::sync::Mutex<Option<phenix_graph::process::ActivationJob>>>);
impl phenix_graph::process::ProcessDispatcher for Delayed {
    fn dispatch(
        &self,
        job: phenix_graph::process::ActivationJob,
    ) -> phenix_graph::process::DispatchResult {
        assert!(!IN_NATIVE_GUARD.get());
        *self.0.lock().unwrap() = Some(job);
        phenix_graph::process::DispatchResult::Deferred
    }
}
#[test]
fn dispatcher_and_execution_guard_run_on_actual_worker_and_return_same_handoff() {
    let pending = Arc::new(std::sync::Mutex::new(None));
    let enters = Arc::new(AtomicUsize::new(0));
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "guarded",
            Arc::new(Interface::new([], []).unwrap()),
            Arc::new(|| Guarded),
        )
        .unwrap();
    b.start(&p).unwrap();
    let mut opts = options();
    opts.dispatcher = Arc::new(Delayed(pending.clone()));
    opts.execution_guard = Some(Arc::new(WorkerGuard(enters.clone())));
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let report = s.run_until_idle(100);
    assert_eq!(report.steps, 1);
    assert_eq!(report.state, SessionState::Waiting);
    assert_eq!(enters.load(Ordering::SeqCst), 0);
    let job = pending.lock().unwrap().take().unwrap();
    let notify = s.notifier();
    let mut listener = s.wake_listener();
    let completion = std::thread::spawn(move || {
        let completion = job.run();
        assert!(!IN_NATIVE_GUARD.get());
        notify.notify();
        completion
    })
    .join()
    .unwrap();
    assert!(listener.wait_timeout(std::time::Duration::ZERO));
    assert_eq!(enters.load(Ordering::SeqCst), 1);
    assert!(!IN_NATIVE_GUARD.get());
    s.complete_job(completion).unwrap();
    assert_eq!(s.state(), SessionState::Completed);
}

struct Staged {
    phase: usize,
    total: Arc<AtomicUsize>,
}
impl Process for Staged {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        match self.phase {
            0 => {
                cx.take("start").unwrap();
                self.phase = 1;
                Ok(Step::WaitInputs(Readiness::AtLeast(
                    NonZeroUsize::new(2).unwrap(),
                )))
            }
            _ => {
                assert_eq!(cx.inputs().count(), 2);
                self.total.fetch_add(1, Ordering::SeqCst);
                Ok(Step::Complete)
            }
        }
    }
}
struct StartOnly;
impl phenix_graph::runtime::ReadinessPolicy for StartOnly {
    fn select(&self, s: &phenix_graph::runtime::ReadinessSnapshot) -> Vec<String> {
        s.heads
            .iter()
            .filter(|h| h.port == "start" && h.available)
            .map(|h| h.port.clone())
            .collect()
    }
}
#[test]
fn custom_initial_policy_and_staged_reservation_keep_request_activation() {
    use phenix_graph::runtime::RequestStatus;
    let total = Arc::new(AtomicUsize::new(0));
    let shared = total.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "stage",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("start").unwrap(),
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || Staged {
                phase: 0,
                total: shared.clone(),
            }),
        )
        .unwrap();
    for name in ["start", "a", "b"] {
        b.expose_input(name, p.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    let mut opts = options();
    opts.readiness
        .insert("stage".into(), Readiness::Custom(Arc::new(StartOnly)));
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "start", Value::of(1_i64)).unwrap();
    s.run_until_idle(100);
    let activation = s.last_step().unwrap().activation;
    assert_eq!(s.request_work(id).unwrap().waiting_input, 1);
    s.inject_request(id, "a", Value::of(2_i64)).unwrap();
    assert_eq!(s.run_until_idle(100).steps, 0);
    s.inject_request(id, "b", Value::of(3_i64)).unwrap();
    s.close_request(id).unwrap();
    s.run_until_idle(100);
    assert_eq!(s.last_step().unwrap().activation, activation);
    assert_eq!(s.last_step().unwrap().request, Some(id));
    assert_eq!(total.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.request_status(id).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
}

struct AlwaysSelectMissing;
impl ReadinessPolicy for AlwaysSelectMissing {
    fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
        assert!(
            snapshot
                .heads
                .iter()
                .any(|head| head.port == "start" && head.available)
        );
        assert!(
            snapshot
                .heads
                .iter()
                .any(|head| head.port == "missing" && !head.available && !head.closed)
        );
        vec!["missing".into()]
    }
}

#[test]
fn custom_readiness_rejects_unavailable_open_port_from_same_snapshot() {
    // The selected-port guard must reject an open missing port, even though
    // a packet on another port admitted the request's readiness candidate.
    let mut builder = NetworkBuilder::new();
    let stage = builder
        .node(
            "stage",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("start").unwrap(),
                        PortDef::wire::<i64>("missing").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(|| Omit),
        )
        .unwrap();
    for name in ["start", "missing"] {
        builder
            .expose_input(name, stage.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    let mut opts = options();
    opts.readiness.insert(
        "stage".into(),
        Readiness::Custom(Arc::new(AlwaysSelectMissing)),
    );
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let id = session.begin_request(None).unwrap();
    session
        .inject_request(id, "start", Value::of(1_i64))
        .unwrap();
    assert_eq!(session.state(), SessionState::Failed);
    assert!(
        session
            .failure()
            .unwrap()
            .contains("invalid readiness selection for missing")
    );
}

struct OptionalSink(Arc<AtomicUsize>);
impl Process for OptionalSink {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if let Some(value) = cx.take("required") {
            assert!(cx.is_closed("optional"));
            self.0
                .fetch_add(value.get::<i64>()? as usize, Ordering::SeqCst);
            Ok(Step::Complete)
        } else {
            Ok(Step::Complete)
        }
    }
}
#[test]
fn optional_input_is_omitted_only_at_real_eos_not_open_idle() {
    let total = Arc::new(AtomicUsize::new(0));
    let shared = total.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "optional",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("required").unwrap(),
                        PortDef::wire::<i64>("optional").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || OptionalSink(shared.clone())),
        )
        .unwrap();
    for name in ["required", "optional"] {
        b.expose_input(name, p.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    let mut opts = options();
    opts.readiness.insert(
        "optional".into(),
        Readiness::Optional {
            required: vec!["required".into()],
            optional: vec!["optional".into()],
        },
    );
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    s.inject("required", Value::of(4_i64)).unwrap();
    assert_eq!(s.run_until_idle(100).steps, 0);
    s.close_input("optional").unwrap();
    s.run_until_idle(100);
    assert_eq!(total.load(Ordering::SeqCst), 4);
    assert_ne!(s.state(), SessionState::Completed);
    s.close_input("required").unwrap();
    assert_eq!(s.run_until_idle(100).state, SessionState::Completed);
}

struct ClosureJoin {
    finish: bool,
    emitted: bool,
}
impl Process for ClosureJoin {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        match (cx.take("a"), cx.take("b")) {
            (Some(a), Some(b)) => {
                self.emitted = true;
                Ok(Step::Emit(ProcessEmission::new(
                    "out",
                    Value::of(a.get::<i64>()? + b.get::<i64>()?),
                )))
            }
            _ if self.finish => Ok(Step::Finished),
            _ => Ok(Step::Complete),
        }
    }
}
fn closure_join(finish: bool) -> phenix_graph::runtime::NetworkSession {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(move || ClosureJoin {
                finish,
                emitted: false,
            }),
        )
        .unwrap();
    for name in ["a", "b"] {
        b.expose_input(name, p.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    b.expose_output("out", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    let mut opts = options();
    opts.readiness.insert("join".into(), Readiness::All);
    b.build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts)
}

#[test]
fn closure_finished_and_complete_report_undelivered_input_without_capture_rollback() {
    for finish in [true, false] {
        let mut s = closure_join(finish);
        s.inject("a", Value::of(1_i64)).unwrap();
        s.inject("b", Value::of(2_i64)).unwrap();
        assert_eq!(s.run_until_idle(20).publications, 1);
        s.inject("a", Value::of(77_i64)).unwrap();
        assert_eq!(s.run_until_idle(20).steps, 0);
        s.close_input("a").unwrap();
        s.close_input("b").unwrap();
        let report = s.run_until_idle(20);
        assert_eq!(report.state, SessionState::Failed);
        assert_eq!(report.disposed_packets, 1);
        assert_eq!(
            s.stranded_inputs()
                .iter()
                .map(|q| q.occupancy)
                .sum::<usize>(),
            1
        );
        assert!(
            s.failure()
                .unwrap()
                .contains(if finish { "abandoned" } else { "stranded" })
        );
        assert!(s.failure().unwrap().contains("a=1"));
        assert_eq!(
            s.take_output("out")
                .unwrap()
                .unwrap()
                .value()
                .get::<i64>()
                .unwrap(),
            3
        );
        assert!(s.take_output("out").unwrap().is_none());
        assert_eq!(s.state(), SessionState::Failed);
    }
}

#[test]
fn correlation_diagnostics_ignore_unscoped_inputs_and_incomplete_joins() {
    let mut incomplete = correlated_join();
    incomplete.inject("a", Value::of(1_i64)).unwrap();
    assert!(incomplete.correlation_blocks().is_empty());

    let mut unscoped = correlated_join();
    unscoped.inject("a", Value::of(1_i64)).unwrap();
    unscoped.inject("b", Value::of(2_i64)).unwrap();
    assert!(unscoped.correlation_blocks().is_empty());
}

#[test]
fn open_join_and_correlated_fifo_head_blocking_are_not_stranded_eos() {
    let mut s = closure_join(false);
    s.inject("a", Value::of(77_i64)).unwrap();
    assert_eq!(s.run_until_idle(20).state, SessionState::Waiting);
    s.close_input("b").unwrap();
    assert_eq!(s.run_until_idle(20).state, SessionState::Waiting);
    assert!(s.failure().is_none());
    assert_eq!(s.disposed_packets(), 0);
    let mut blocked = correlated_join();
    let a = blocked.begin_request(None).unwrap();
    let b = blocked.begin_request(None).unwrap();
    for (id, port) in [(a, "a"), (b, "b"), (b, "a"), (a, "b")] {
        blocked.inject_request(id, port, Value::of(1_i64)).unwrap();
    }
    blocked.close_request(a).unwrap();
    blocked.close_request(b).unwrap();
    blocked.close_input("a").unwrap();
    blocked.close_input("b").unwrap();
    assert_eq!(blocked.run_until_idle(20).state, SessionState::Waiting);
    assert!(blocked.failure().is_none());
}

struct ObserveClosure(Arc<AtomicUsize>);
impl Process for ObserveClosure {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        assert!(cx.is_closed("opt"));
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Step::Complete)
    }
}
#[test]
fn optional_excluded_input_does_not_replay_eos_only_activations() {
    let count = Arc::new(AtomicUsize::new(0));
    let shared = count.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "optional",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("unused").unwrap(),
                        PortDef::wire::<i64>("opt").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || ObserveClosure(shared.clone())),
        )
        .unwrap();
    for name in ["unused", "opt"] {
        b.expose_input(name, p.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    let mut opts = options();
    opts.readiness.insert(
        "optional".into(),
        Readiness::Optional {
            required: vec![],
            optional: vec!["opt".into()],
        },
    );
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    s.inject("unused", Value::of(77_i64)).unwrap();
    s.close_input("opt").unwrap();
    let report = s.run_until_idle(20);
    assert_eq!(report.steps, 1);
    assert!(!report.budget_exhausted);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(s.run_until_idle(20).steps, 0);
    s.close_input("unused").unwrap();
    assert_eq!(s.state(), SessionState::Failed);
    assert_eq!(s.disposed_packets(), 1);
}

#[test]
fn unconnected_output_requires_explicit_physical_discard_permission() {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "source",
            Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
            Arc::new(|| Source { emitted: false }),
        )
        .unwrap();
    b.start(&p).unwrap();
    let compiled = b.build().unwrap().compile().unwrap();
    let mut rejected = compiled.start();
    let report = rejected.run_until_idle(20);
    assert_eq!(report.state, SessionState::Failed);
    assert_eq!(report.publications, 0);
    assert_eq!(report.discarded_emissions, 0);
    assert!(rejected.failure().unwrap().contains("source.out"));
    let mut opts = options();
    opts.discard_outputs.insert(("source".into(), "out".into()));
    let mut allowed = compiled.start_with_options(opts);
    let report = allowed.run_until_idle(20);
    assert_eq!(report.state, SessionState::Quiescent);
    assert_eq!(report.publications, 1);
    assert_eq!(report.discarded_emissions, 1);
    let mut bad = options();
    bad.discard_outputs.insert(("source".into(), "typo".into()));
    assert_eq!(
        compiled.start_with_options(bad).state(),
        SessionState::Failed
    );
}

struct DelayedOptional {
    phase: usize,
    value: i64,
}
impl Process for DelayedOptional {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        match self.phase {
            0 => {
                self.value = cx.take("in").unwrap().get()?;
                self.phase = 1;
                Ok(Step::WaitWake)
            }
            1 if self.value != 0 => {
                self.phase = 2;
                Ok(Step::Emit(ProcessEmission::new(
                    "out",
                    Value::of(self.value),
                )))
            }
            _ => {
                self.phase = 0;
                Ok(Step::Complete)
            }
        }
    }
}
type OptionalValues = (i64, Option<i64>);
struct ScopedOptional(Arc<std::sync::Mutex<Vec<OptionalValues>>>);
impl Process for ScopedOptional {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        let required = cx.take("required").unwrap().get::<i64>()?;
        let optional = cx.take("optional").map(|v| v.get::<i64>()).transpose()?;
        if optional.is_none() {
            assert!(cx.is_request_closed("optional"));
            assert!(!cx.is_eos("optional"));
        }
        self.0.lock().unwrap().push((required, optional));
        Ok(Step::Complete)
    }
}
#[test]
fn scoped_optional_uses_real_upstream_work_and_retires_absence_between_requests() {
    use phenix_graph::runtime::RequestStatus;
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let shared = log.clone();
    let mut b = NetworkBuilder::new();
    let iface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let required = b
        .node(
            "a_required",
            iface.clone(),
            Arc::new(|| Counter {
                sum: 0,
                emitted: false,
                effects: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .unwrap();
    let optional = b
        .node(
            "b_optional",
            iface,
            Arc::new(|| DelayedOptional { phase: 0, value: 0 }),
        )
        .unwrap();
    let join = b
        .node(
            "z_join",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("required").unwrap(),
                        PortDef::wire::<i64>("optional").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || ScopedOptional(shared.clone())),
        )
        .unwrap();
    for p in [&required, &optional] {
        b.expose_input("in", p.wire_input::<i64>("in").unwrap(), one())
            .unwrap();
    }
    b.connect(
        required.wire_output::<i64>("out").unwrap(),
        join.wire_input::<i64>("required").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        optional.wire_output::<i64>("out").unwrap(),
        join.wire_input::<i64>("optional").unwrap(),
        one(),
    )
    .unwrap();
    let mut opts = options();
    opts.readiness.insert(
        "z_join".into(),
        Readiness::Optional {
            required: vec!["required".into()],
            optional: vec!["optional".into()],
        },
    );
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let wake = s.wake_handle("b_optional").unwrap();
    for (iteration, value) in [0_i64, 5].into_iter().enumerate() {
        let id = s.begin_request(None).unwrap();
        s.inject_request(id, "in", Value::of(value)).unwrap();
        s.close_request(id).unwrap();
        s.run_until_idle(30);
        assert_eq!(log.lock().unwrap().len(), iteration);
        assert_eq!(s.request_work(id).unwrap().waiting_wake, 1);
        assert_eq!(s.request_status(id).unwrap(), RequestStatus::Pending);
        wake.wake();
        let (index, job) = s.next_job().unwrap();
        s.complete(index, job.run()).unwrap();
        if value != 0 {
            assert_eq!(s.request_work(id).unwrap().publication_pending, 1);
            assert_eq!(log.lock().unwrap().len(), iteration);
        }
        s.run_until_idle(30);
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        s.release_request(id).unwrap();
        assert_eq!(s.state(), SessionState::Quiescent);
    }
    assert_eq!(*log.lock().unwrap(), vec![(0, None), (5, Some(5))]);
}

struct Omit;
impl Process for Omit {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        cx.take("in").unwrap();
        Ok(Step::Complete)
    }
}
struct ClosureRelay {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<&'static str>>>,
}
impl Process for ClosureRelay {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        assert!(cx.is_request_closed("opt"));
        assert!(!cx.is_eos("opt"));
        self.log.lock().unwrap().push(self.name);
        Ok(Step::Complete)
    }
}
#[test]
fn all_optional_closure_obligations_settle_in_upstream_order_without_new_packets() {
    use phenix_graph::runtime::RequestStatus;
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let mut b = NetworkBuilder::new();
    let root = b
        .node(
            "z_root",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Omit),
        )
        .unwrap();
    let mut refs = vec![];
    for name in ["m_first", "a_last"] {
        let shared = log.clone();
        refs.push(
            b.node(
                name,
                Arc::new(
                    Interface::new(
                        [PortDef::wire::<i64>("opt").unwrap()],
                        [PortDef::wire::<i64>("out").unwrap()],
                    )
                    .unwrap(),
                ),
                Arc::new(move || ClosureRelay {
                    name,
                    log: shared.clone(),
                }),
            )
            .unwrap(),
        );
    }
    b.expose_input("in", root.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.connect(
        root.wire_output::<i64>("out").unwrap(),
        refs[0].wire_input::<i64>("opt").unwrap(),
        one(),
    )
    .unwrap();
    b.connect(
        refs[0].wire_output::<i64>("out").unwrap(),
        refs[1].wire_input::<i64>("opt").unwrap(),
        one(),
    )
    .unwrap();
    let mut opts = options();
    for name in ["m_first", "a_last"] {
        opts.readiness.insert(
            name.into(),
            Readiness::Optional {
                required: vec![],
                optional: vec!["opt".into()],
            },
        );
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    for _ in 0..2 {
        let id = s.begin_request(None).unwrap();
        s.inject_request(id, "in", Value::of(1_i64)).unwrap();
        s.close_request(id).unwrap();
        assert_eq!(s.request_work(id).unwrap().closure_pending, 2);
        assert_eq!(s.run_until_idle(30).steps, 3);
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        assert_eq!(s.run_until_idle(30).steps, 0);
        s.release_request(id).unwrap();
    }
    assert_eq!(
        *log.lock().unwrap(),
        vec!["m_first", "a_last", "m_first", "a_last"]
    );
}

struct ScopedStage {
    phase: usize,
    log: Arc<std::sync::Mutex<Vec<Option<i64>>>>,
}
impl Process for ScopedStage {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.phase == 0 {
            cx.take("start").unwrap();
            self.phase = 1;
            return Ok(Step::WaitInput("late".into()));
        }
        let late = cx.take("late").map(|v| v.get::<i64>()).transpose()?;
        if late.is_none() {
            assert!(cx.is_request_closed("late"));
            assert!(!cx.is_eos("late"));
        } else {
            assert!(!cx.is_request_closed("late"));
        }
        self.log.lock().unwrap().push(late);
        self.phase = 0;
        Ok(Step::Complete)
    }
}
#[test]
fn scoped_staged_closure_preserves_activation_and_does_not_leak_history() {
    use phenix_graph::runtime::RequestStatus;
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let shared = log.clone();
    let mut b = NetworkBuilder::new();
    let stage = b
        .node(
            "stage",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("start").unwrap(),
                        PortDef::wire::<i64>("late").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || ScopedStage {
                phase: 0,
                log: shared.clone(),
            }),
        )
        .unwrap();
    let branch = b
        .node(
            "branch",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| DelayedOptional { phase: 0, value: 0 }),
        )
        .unwrap();
    b.expose_input("in", stage.wire_input::<i64>("start").unwrap(), one())
        .unwrap();
    b.expose_input("in", branch.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.connect(
        branch.wire_output::<i64>("out").unwrap(),
        stage.wire_input::<i64>("late").unwrap(),
        one(),
    )
    .unwrap();
    let mut opts = options();
    opts.readiness
        .insert("stage".into(), Readiness::Custom(Arc::new(StartOnly)));
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let wake = s.wake_handle("branch").unwrap();
    for value in [0_i64, 5] {
        let id = s.begin_request(None).unwrap();
        s.inject_request(id, "in", Value::of(value)).unwrap();
        s.close_request(id).unwrap();
        s.run_until_idle(30);
        let activation = s.last_step().unwrap().activation;
        assert_eq!(s.request_work(id).unwrap().waiting_input, 1);
        assert_eq!(s.request_work(id).unwrap().waiting_wake, 1);
        assert!(log.lock().unwrap().len() < 2);
        wake.wake();
        s.run_until_idle(30);
        assert_eq!(s.last_step().unwrap().activation, activation);
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        s.release_request(id).unwrap();
    }
    assert_eq!(*log.lock().unwrap(), vec![None, Some(5)]);
}

#[test]
fn optional_scope_cannot_settle_behind_another_requests_handed_job() {
    use phenix_graph::runtime::RequestStatus;
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let mut b = NetworkBuilder::new();
    let root = b
        .node(
            "root",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Omit),
        )
        .unwrap();
    let mut consumer = None;
    for name in ["consumer", "unrelated"] {
        let shared = log.clone();
        let p = b
            .node(
                name,
                Arc::new(Interface::new([PortDef::wire::<i64>("opt").unwrap()], []).unwrap()),
                Arc::new(move || ClosureRelay {
                    name,
                    log: shared.clone(),
                }),
            )
            .unwrap();
        if name == "consumer" {
            consumer = Some(p);
        } else {
            b.expose_input("unrelated", p.wire_input::<i64>("opt").unwrap(), one())
                .unwrap();
        }
    }
    b.expose_input("in", root.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.connect(
        root.wire_output::<i64>("out").unwrap(),
        consumer.unwrap().wire_input::<i64>("opt").unwrap(),
        one(),
    )
    .unwrap();
    let mut opts = options();
    for name in ["consumer", "unrelated"] {
        opts.readiness.insert(
            name.into(),
            Readiness::Optional {
                required: vec![],
                optional: vec!["opt".into()],
            },
        );
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let a = s.begin_request(None).unwrap();
    s.inject_request(a, "in", Value::of(1_i64)).unwrap();
    s.close_request(a).unwrap();
    assert_eq!(s.run_until_idle(1).steps, 1);
    let (_, held) = s.next_job().unwrap();
    assert_eq!(held.metadata().unwrap().path, "consumer");
    let b = s.begin_request(None).unwrap();
    s.inject_request(b, "in", Value::of(2_i64)).unwrap();
    s.close_request(b).unwrap();
    s.run_until_idle(20);
    let work = s.request_work(b).unwrap();
    assert_eq!(work.queued, 0);
    assert_eq!(work.closure_pending, 1);
    assert_eq!(s.request_status(b).unwrap(), RequestStatus::Pending);
    s.complete_job(held.run()).unwrap();
    s.run_until_idle(20);
    for id in [a, b] {
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        s.release_request(id).unwrap();
    }
    assert_eq!(*log.lock().unwrap(), vec!["consumer", "consumer"]);
}

struct ObserveOptionalData(Arc<std::sync::Mutex<Vec<Option<i64>>>>);
impl Process for ObserveOptionalData {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        let value = cx.take("opt").map(|v| v.get::<i64>()).transpose()?;
        self.0.lock().unwrap().push(value);
        Ok(Step::Complete)
    }
}
#[test]
fn all_optional_data_does_not_replay_as_a_missing_branch_after_settlement() {
    use phenix_graph::runtime::RequestStatus;
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let shared = log.clone();
    let mut b = NetworkBuilder::new();
    let root = b
        .node(
            "root",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(|| Counter {
                sum: 0,
                emitted: false,
                effects: Arc::new(AtomicUsize::new(0)),
            }),
        )
        .unwrap();
    let sink = b
        .node(
            "sink",
            Arc::new(Interface::new([PortDef::wire::<i64>("opt").unwrap()], []).unwrap()),
            Arc::new(move || ObserveOptionalData(shared.clone())),
        )
        .unwrap();
    b.expose_input("in", root.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.connect(
        root.wire_output::<i64>("out").unwrap(),
        sink.wire_input::<i64>("opt").unwrap(),
        one(),
    )
    .unwrap();
    let mut opts = options();
    opts.readiness.insert(
        "sink".into(),
        Readiness::Optional {
            required: vec![],
            optional: vec!["opt".into()],
        },
    );
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "in", Value::of(7_i64)).unwrap();
    s.close_request(id).unwrap();
    assert_eq!(s.run_until_idle(30).steps, 3);
    assert_eq!(*log.lock().unwrap(), vec![Some(7)]);
    assert_eq!(
        s.request_status(id).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
    assert_eq!(s.run_until_idle(30).steps, 0);
}

fn cancellable_counter(
    effects: Arc<AtomicUsize>,
    spawns: Arc<AtomicUsize>,
    capacity: usize,
) -> CompiledNetwork {
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "counter",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(move || {
                spawns.fetch_add(1, Ordering::SeqCst);
                Counter {
                    sum: 0,
                    emitted: false,
                    effects: effects.clone(),
                }
            }),
        )
        .unwrap();
    b.expose_input(
        "input",
        p.wire_input::<i64>("in").unwrap(),
        NonZeroUsize::new(capacity).unwrap(),
    )
    .unwrap();
    b.expose_output("output", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    b.build().unwrap().compile().unwrap()
}
fn restarting_options() -> SessionOptions {
    SessionOptions {
        request_failure_policy: phenix_graph::runtime::RequestFailurePolicy::RestartAffectedProcess,
        ..options()
    }
}

#[test]
fn queued_request_cancellation_preserves_other_requests_fifo_and_process_state() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let effects = Arc::new(AtomicUsize::new(0));
    let spawns = Arc::new(AtomicUsize::new(0));
    // Queued-only cancellation is local even with the default FailSession policy.
    let mut s = cancellable_counter(effects.clone(), spawns.clone(), 4).start();
    let held = s.begin_request(Some("output")).unwrap();
    s.inject_request(held, "input", Value::of(1_i64)).unwrap();
    s.close_request(held).unwrap();
    let (_, job) = s.next_job().unwrap();
    let cancelled = s.begin_request(None).unwrap();
    let c = s.begin_request(Some("output")).unwrap();
    let d = s.begin_request(Some("output")).unwrap();
    for (id, value) in [(cancelled, 9_i64), (c, 2), (d, 3), (cancelled, 10)] {
        s.inject_request(id, "input", Value::of(value)).unwrap();
    }
    for id in [cancelled, c, d] {
        s.close_request(id).unwrap();
    }
    assert!(s.cancel_request(cancelled).unwrap().is_empty());
    assert_eq!(
        s.request_status(cancelled).unwrap(),
        RequestStatus::Cancelled
    );
    assert_eq!(s.request_disposal(cancelled).unwrap().queued_packets, 2);
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    assert_eq!(s.request_work(held).unwrap().dispatched, 1);
    s.release_request(cancelled).unwrap();
    assert_eq!(
        s.release_request(cancelled).unwrap_err(),
        RequestError::Stale
    );
    assert_eq!(
        s.cancel_request(cancelled).unwrap_err(),
        RequestError::Stale
    );
    s.complete_job(job.run()).unwrap();
    s.run_until_idle(50);
    // D may now be read before earlier capture heads without consuming them.
    // Its request metadata and the retained process state remain authoritative.
    let d_packet = s.take_request_output(d, "output").unwrap().unwrap();
    assert_eq!(d_packet.value().get::<i64>().unwrap(), 6);
    assert_eq!(d_packet.metadata().unwrap().request(), Some(d));
    assert_eq!(
        s.request_status(d).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    s.release_request(d).unwrap();
    for (id, expected) in [(held, 1_i64), (c, 3)] {
        let packet = s.take_request_output(id, "output").unwrap().unwrap();
        assert_eq!(packet.value().get::<i64>().unwrap(), expected);
        assert_eq!(
            s.request_status(id).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        s.release_request(id).unwrap();
    }
    assert_eq!(effects.load(Ordering::SeqCst), 3);
    assert_eq!(s.state(), SessionState::Quiescent);
}

#[test]
fn cancelling_pending_atomic_fanout_preserves_older_captures_and_restarts_only_active_state() {
    use phenix_graph::runtime::RequestStatus;
    let spawns = Arc::new(AtomicUsize::new(0));
    let count = spawns.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "counter",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Counter {
                    sum: 0,
                    emitted: false,
                    effects: Arc::new(AtomicUsize::new(0)),
                }
            }),
        )
        .unwrap();
    b.expose_input(
        "input",
        p.wire_input::<i64>("in").unwrap(),
        NonZeroUsize::new(2).unwrap(),
    )
    .unwrap();
    for name in ["a", "b"] {
        b.expose_output(name, p.wire_output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(restarting_options());
    let old = s.begin_request(None).unwrap();
    s.inject_request(old, "input", Value::of(1_i64)).unwrap();
    s.close_request(old).unwrap();
    assert_eq!(s.run_until_idle(30).publications, 1);
    let cancelled = s.begin_request(None).unwrap();
    s.inject_request(cancelled, "input", Value::of(2_i64))
        .unwrap();
    s.close_request(cancelled).unwrap();
    assert_eq!(s.run_until_idle(30).state, SessionState::Waiting);
    assert_eq!(s.request_work(cancelled).unwrap().publication_pending, 1);
    let next = s.begin_request(None).unwrap();
    s.inject_request(next, "input", Value::of(4_i64)).unwrap();
    s.close_request(next).unwrap();
    assert!(s.cancel_request(cancelled).unwrap().is_empty());
    let disposal = s.request_disposal(cancelled).unwrap();
    assert_eq!(disposal.prepared_packets, 2);
    assert_eq!(disposal.queued_packets, 0);
    assert_eq!(disposal.activations, 1);
    assert_eq!(spawns.load(Ordering::SeqCst), 2);
    for name in ["a", "b"] {
        let packet = s.take_request_output(old, name).unwrap().unwrap();
        s.validate_capture(old, name, &packet).unwrap();
        assert_eq!(packet.value().get::<i64>().unwrap(), 1);
    }
    assert_eq!(
        s.request_status(old).unwrap(),
        RequestStatus::Settled { captures: 2 }
    );
    assert_eq!(s.run_until_idle(30).publications, 1);
    for name in ["a", "b"] {
        assert_eq!(
            s.take_request_output(next, name)
                .unwrap()
                .unwrap()
                .value()
                .get::<i64>()
                .unwrap(),
            4
        );
    }
    assert_eq!(
        s.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 2 }
    );
    s.release_request(cancelled).unwrap();
}

struct FaultyCounter {
    sum: i64,
    emitted: bool,
}
impl Process for FaultyCounter {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let value = cx.take("in").unwrap().get::<i64>()?;
        self.sum += value;
        if value == -1 {
            panic!("request process panicked after mutating state");
        }
        if value == -2 {
            return Err("request process rejected input after mutation".into());
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(self.sum))))
    }
}
#[test]
fn tagged_panic_and_error_recover_with_fresh_state_without_resetting_unrelated_instance() {
    use phenix_graph::runtime::RequestStatus;
    let spawns = Arc::new(AtomicUsize::new(0));
    let shared = spawns.clone();
    let mut b = NetworkBuilder::new();
    let iface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let bad = b
        .node(
            "faulty",
            iface.clone(),
            Arc::new(move || {
                shared.fetch_add(1, Ordering::SeqCst);
                FaultyCounter {
                    sum: 0,
                    emitted: false,
                }
            }),
        )
        .unwrap();
    let shared = spawns.clone();
    let stable = b
        .node(
            "stable",
            iface,
            Arc::new(move || {
                shared.fetch_add(1, Ordering::SeqCst);
                Counter {
                    sum: 0,
                    emitted: false,
                    effects: Arc::new(AtomicUsize::new(0)),
                }
            }),
        )
        .unwrap();
    for (name, p) in [("faulty", bad), ("stable", stable)] {
        b.expose_input(name, p.wire_input::<i64>("in").unwrap(), one())
            .unwrap();
        b.expose_output(name, p.wire_output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut opts = restarting_options();
    opts.capture_capacity = NonZeroUsize::new(4).unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    s.inject("stable", Value::of(10_i64)).unwrap();
    s.run_until_idle(30);
    s.take_output("stable").unwrap().unwrap();
    for (iteration, value) in [-1_i64, -2].into_iter().enumerate() {
        let failed = s.begin_request(Some("faulty")).unwrap();
        let good = s.begin_request(Some("stable")).unwrap();
        s.inject_request(failed, "faulty", Value::of(value))
            .unwrap();
        s.inject_request(good, "stable", Value::of(3_i64)).unwrap();
        s.close_request(failed).unwrap();
        s.close_request(good).unwrap();
        assert_ne!(s.run_until_idle(50).state, SessionState::Failed);
        assert_eq!(s.request_status(failed).unwrap(), RequestStatus::Failed);
        assert!(
            s.request_failure(failed)
                .unwrap()
                .unwrap()
                .contains(if value == -1 { "panicked" } else { "rejected" })
        );
        assert_eq!(s.request_disposal(failed).unwrap().activations, 1);
        assert_eq!(spawns.load(Ordering::SeqCst), 3 + iteration);
        assert_eq!(
            s.take_request_output(good, "stable")
                .unwrap()
                .unwrap()
                .value()
                .get::<i64>()
                .unwrap(),
            13 + iteration as i64 * 3
        );
        let next = s.begin_request(Some("faulty")).unwrap();
        s.inject_request(next, "faulty", Value::of(7_i64)).unwrap();
        s.close_request(next).unwrap();
        s.run_until_idle(30);
        assert_eq!(
            s.take_request_output(next, "faulty")
                .unwrap()
                .unwrap()
                .value()
                .get::<i64>()
                .unwrap(),
            7
        );
        for id in [failed, good, next] {
            s.release_request(id).unwrap();
        }
    }
    s.inject("faulty", Value::of(-1_i64)).unwrap();
    assert_eq!(s.run_until_idle(30).state, SessionState::Failed);
}

#[test]
fn cancelled_handed_job_rejects_late_emission_and_retains_code_lease_after_scope_release() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let alive = Arc::new(AtomicUsize::new(1));
    let compiled = native_source(alive.clone());
    let mut opts = restarting_options();
    opts.auto_start_sources = false;
    opts.lease = Some(Arc::new(NativeLease(alive.clone())));
    let mut s = compiled.start_with_options(opts);
    let old_wake = s.wake_handle("native").unwrap();
    let notifier = s.notifier();
    let id = s.begin_request(Some("out")).unwrap();
    s.start_request_source(id, "native").unwrap();
    s.close_request(id).unwrap();
    let (_, job) = s.next_job().unwrap();
    let ticket = job.ticket();
    assert_eq!(s.cancel_request(id).unwrap(), vec![ticket]);
    assert_eq!(s.request_cancelled_jobs(id).unwrap(), &[ticket]);
    assert_eq!(s.request_disposal(id).unwrap().handed_jobs, 1);
    assert_eq!(s.request_status(id).unwrap(), RequestStatus::Cancelled);
    assert!(!old_wake.wake());
    assert!(notifier.notify());
    s.release_request(id).unwrap();
    assert_eq!(s.request_status(id).unwrap_err(), RequestError::Stale);
    let returned = s.complete_job(job.run()).unwrap_err().1;
    assert!(s.take_output("out").unwrap().is_none());
    let next = s.begin_request(Some("out")).unwrap();
    s.start_request_source(next, "native").unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    let packet = s.take_request_output(next, "out").unwrap().unwrap();
    assert!(packet.metadata().unwrap().activation() > ticket.activation());
    assert_eq!(
        s.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    drop(s);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(packet);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(returned);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}

#[test]
fn budget_exhaustion_is_resumable_and_explicit_cancellation_allows_followup_request() {
    use phenix_graph::runtime::RequestStatus;
    let mut s = cancellable_counter(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        one().get(),
    )
    .start_with_options(restarting_options());
    let id = s.begin_request(Some("output")).unwrap();
    s.inject_request(id, "input", Value::of(1_i64)).unwrap();
    s.close_request(id).unwrap();
    assert!(s.run_until_idle(1).budget_exhausted);
    s.run_until_idle(1);
    s.take_request_output(id, "output").unwrap().unwrap();
    assert_eq!(
        s.request_status(id).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    let cancelled = s.begin_request(Some("output")).unwrap();
    s.inject_request(cancelled, "input", Value::of(2_i64))
        .unwrap();
    s.close_request(cancelled).unwrap();
    assert!(s.run_until_idle(1).budget_exhausted);
    s.cancel_request(cancelled).unwrap();
    assert_eq!(s.request_disposal(cancelled).unwrap().prepared_packets, 1);
    s.release_request(cancelled).unwrap();
    let next = s.begin_request(Some("output")).unwrap();
    s.inject_request(next, "input", Value::of(3_i64)).unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    assert_eq!(
        s.take_request_output(next, "output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        3
    );
}

#[test]
fn factory_panics_fail_session_at_start_and_during_recovery() {
    let mut b = NetworkBuilder::new();
    b.node(
        "bad",
        Arc::new(Interface::new([], []).unwrap()),
        Arc::new(|| -> FaultyCounter {
            panic!("startup factory failed");
        }),
    )
    .unwrap();
    let failed = b.build().unwrap().compile().unwrap().start();
    assert_eq!(failed.state(), SessionState::Failed);
    assert!(failed.failure().unwrap().contains("factory panicked"));
    let spawns = Arc::new(AtomicUsize::new(0));
    let shared = spawns.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "faulty",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<i64>("in").unwrap()],
                    [PortDef::wire::<i64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new(move || {
                assert_eq!(
                    shared.fetch_add(1, Ordering::SeqCst),
                    0,
                    "restart factory failed"
                );
                FaultyCounter {
                    sum: 0,
                    emitted: false,
                }
            }),
        )
        .unwrap();
    b.expose_input("input", p.wire_input::<i64>("in").unwrap(), one())
        .unwrap();
    b.expose_output("output", p.wire_output::<i64>("out").unwrap())
        .unwrap();
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(restarting_options());
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "input", Value::of(-1_i64)).unwrap();
    s.close_request(id).unwrap();
    assert_eq!(s.run_until_idle(30).state, SessionState::Failed);
    assert!(
        s.failure()
            .unwrap()
            .contains("factory panicked during request recovery")
    );
    s.release_request(id).unwrap();
}

#[test]
fn cancelling_committed_capture_is_explicit_disposal_not_state_or_publication_rollback() {
    use phenix_graph::runtime::RequestStatus;
    let spawns = Arc::new(AtomicUsize::new(0));
    let mut s = cancellable_counter(Arc::new(AtomicUsize::new(0)), spawns.clone(), 3).start();
    let mut scopes = vec![];
    for value in [1_i64, 2, 3] {
        let id = s.begin_request(Some("output")).unwrap();
        s.inject_request(id, "input", Value::of(value)).unwrap();
        s.close_request(id).unwrap();
        assert_eq!(s.run_until_idle(30).publications, 1);
        scopes.push(id);
    }
    let cancelled = scopes[1];
    assert!(s.cancel_request(cancelled).unwrap().is_empty());
    assert_eq!(s.request_disposal(cancelled).unwrap().queued_packets, 1);
    assert_eq!(s.request_disposal(cancelled).unwrap().activations, 0);
    assert_eq!(spawns.load(Ordering::SeqCst), 1);
    // Scope 2's committed capture remains accessible despite scope 0's
    // older buffered capture; cancellation only disposed scope 1.
    let latest = s.take_request_output(scopes[2], "output").unwrap().unwrap();
    assert_eq!(latest.value().get::<i64>().unwrap(), 6);
    assert_eq!(latest.metadata().unwrap().request(), Some(scopes[2]));
    assert_eq!(
        s.request_status(scopes[2]).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    let first = s.take_request_output(scopes[0], "output").unwrap().unwrap();
    assert_eq!(first.value().get::<i64>().unwrap(), 1);
    assert_eq!(first.metadata().unwrap().request(), Some(scopes[0]));
    assert_eq!(
        s.request_status(scopes[0]).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    assert_eq!(s.run_until_idle(30).publications, 0);
    assert_eq!(s.disposed_packets(), 1);
    let next = s.begin_request(Some("output")).unwrap();
    s.inject_request(next, "input", Value::of(4_i64)).unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    assert_eq!(
        s.take_request_output(next, "output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        10
    );
}

#[test]
fn cancelling_wake_wait_invalidates_only_affected_registration_and_scope() {
    use phenix_graph::runtime::RequestStatus;
    let mut b = NetworkBuilder::new();
    for name in ["a", "b"] {
        let p = b
            .node(
                name,
                Arc::new(Interface::new([], [PortDef::local::<i64>("out")]).unwrap()),
                Arc::new(|| WakeSource { phase: 0 }),
            )
            .unwrap();
        b.start(&p).unwrap();
        b.expose_output(name, p.output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut opts = restarting_options();
    opts.auto_start_sources = false;
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts);
    let old_a = s.wake_handle("a").unwrap();
    let wake_b = s.wake_handle("b").unwrap();
    let a = s.begin_request(Some("a")).unwrap();
    let other = s.begin_request(Some("b")).unwrap();
    for (id, path) in [(a, "a"), (other, "b")] {
        s.start_request_source(id, path).unwrap();
        s.close_request(id).unwrap();
    }
    s.run_until_idle(30);
    assert_eq!(s.request_work(a).unwrap().waiting_wake, 1);
    assert_eq!(s.request_work(other).unwrap().waiting_wake, 1);
    s.cancel_request(a).unwrap();
    s.release_request(a).unwrap();
    assert!(!old_a.wake());
    assert!(wake_b.wake());
    s.run_until_idle(30);
    assert_eq!(
        s.take_request_output(other, "b")
            .unwrap()
            .unwrap()
            .value()
            .borrow::<i64>(),
        Some(&42)
    );
    assert_eq!(
        s.request_status(other).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    let next = s.begin_request(Some("a")).unwrap();
    s.start_request_source(next, "a").unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    assert!(s.wake_handle("a").unwrap().wake());
    s.run_until_idle(30);
    assert!(s.take_request_output(next, "a").unwrap().is_some());
    assert_eq!(
        s.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
}

#[test]
fn scoped_failure_exposes_sibling_runner_inventory_and_rejects_late_publication() {
    use phenix_graph::runtime::RequestStatus;
    let effects = Arc::new(AtomicUsize::new(0));
    let shared = effects.clone();
    let mut b = NetworkBuilder::new();
    let iface = Arc::new(
        Interface::new(
            [PortDef::wire::<i64>("in").unwrap()],
            [PortDef::wire::<i64>("out").unwrap()],
        )
        .unwrap(),
    );
    let bad = b
        .node(
            "bad",
            iface.clone(),
            Arc::new(|| FaultyCounter {
                sum: 0,
                emitted: false,
            }),
        )
        .unwrap();
    let sibling = b
        .node(
            "sibling",
            iface,
            Arc::new(move || Counter {
                sum: 0,
                emitted: false,
                effects: shared.clone(),
            }),
        )
        .unwrap();
    for (name, p) in [("bad", bad), ("sibling", sibling)] {
        b.expose_input("input", p.wire_input::<i64>("in").unwrap(), one())
            .unwrap();
        b.expose_output(name, p.wire_output::<i64>("out").unwrap())
            .unwrap();
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(restarting_options());
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "input", Value::of(-1_i64)).unwrap();
    s.close_request(id).unwrap();
    let (_, bad) = s.next_job().unwrap();
    let (_, late) = s.next_job().unwrap();
    assert_eq!(late.metadata().unwrap().path, "sibling");
    let ticket = late.ticket();
    s.complete_job(bad.run()).unwrap();
    assert_eq!(s.request_status(id).unwrap(), RequestStatus::Failed);
    assert_eq!(s.request_cancelled_jobs(id).unwrap(), &[ticket]);
    assert_eq!(s.request_disposal(id).unwrap().handed_jobs, 1);
    assert_eq!(s.request_disposal(id).unwrap().activations, 2);
    assert_eq!(s.cancel_request(id).unwrap(), vec![ticket]);
    s.release_request(id).unwrap();
    drop(s.complete_job(late.run()).unwrap_err());
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert!(s.take_output("sibling").unwrap().is_none());
    let next = s.begin_request(None).unwrap();
    s.inject_request(next, "input", Value::of(7_i64)).unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    for name in ["bad", "sibling"] {
        assert_eq!(
            s.take_request_output(next, name)
                .unwrap()
                .unwrap()
                .value()
                .get::<i64>()
                .unwrap(),
            7
        );
    }
    assert_eq!(
        s.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 2 }
    );
    assert_eq!(effects.load(Ordering::SeqCst), 2);
}

#[test]
fn request_cancellation_restarts_input_continuation_without_reusing_staged_state() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let total = Arc::new(AtomicUsize::new(0));
    let shared = total.clone();
    let mut b = NetworkBuilder::new();
    let p = b
        .node(
            "waiter",
            Arc::new(
                Interface::new(
                    [
                        PortDef::wire::<i64>("a").unwrap(),
                        PortDef::wire::<i64>("b").unwrap(),
                    ],
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || Waiter {
                phase: 0,
                total: shared.clone(),
            }),
        )
        .unwrap();
    for name in ["a", "b"] {
        b.expose_input(name, p.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    let mut s = b
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(restarting_options());
    let id = s.begin_request(None).unwrap();
    s.inject_request(id, "a", Value::of(1_i64)).unwrap();
    s.run_until_idle(30);
    assert_eq!(s.request_work(id).unwrap().waiting_input, 1);
    s.cancel_request(id).unwrap();
    assert_eq!(
        s.inject_request(id, "b", Value::of(99_i64))
            .unwrap_err()
            .reason,
        RequestError::Stale
    );
    s.release_request(id).unwrap();
    let next = s.begin_request(None).unwrap();
    s.inject_request(next, "a", Value::of(2_i64)).unwrap();
    s.run_until_idle(1);
    s.inject_request(next, "b", Value::of(3_i64)).unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    assert_eq!(total.load(Ordering::SeqCst), 3);
    assert_eq!(
        s.request_status(next).unwrap(),
        RequestStatus::Settled { captures: 0 }
    );
}

#[test]
fn active_cancellation_defaults_to_fail_session_and_host_failure_can_opt_into_restart() {
    use phenix_graph::runtime::{RequestError, RequestStatus};
    let compiled = cancellable_counter(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        1,
    );
    let mut default = compiled.start();
    let id = default.begin_request(None).unwrap();
    default
        .inject_request(id, "input", Value::of(1_i64))
        .unwrap();
    default.cancel_request(id).unwrap();
    assert_eq!(default.state(), SessionState::Failed);
    assert_eq!(
        default.request_status(id).unwrap(),
        RequestStatus::Cancelled
    );
    default.release_request(id).unwrap();
    let mut s = compiled.start_with_options(restarting_options());
    let failed = s.begin_request(None).unwrap();
    s.inject_request(failed, "input", Value::of(99_i64))
        .unwrap();
    s.fail_request(failed, "host validation failed").unwrap();
    assert_eq!(s.request_status(failed).unwrap(), RequestStatus::Failed);
    assert_eq!(
        s.request_failure(failed).unwrap(),
        Some("host validation failed")
    );
    assert_eq!(s.state(), SessionState::Quiescent);
    s.release_request(failed).unwrap();
    assert_eq!(
        s.fail_request(failed, "again").unwrap_err(),
        RequestError::Stale
    );
    let next = s.begin_request(Some("output")).unwrap();
    s.inject_request(next, "input", Value::of(5_i64)).unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(20);
    assert_eq!(
        s.take_request_output(next, "output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        5
    );
}

struct PanicOnceDispatcher(std::sync::atomic::AtomicBool);
impl phenix_graph::process::ProcessDispatcher for PanicOnceDispatcher {
    fn dispatch(
        &self,
        job: phenix_graph::process::ActivationJob,
    ) -> phenix_graph::process::DispatchResult {
        assert!(
            !self.0.swap(false, Ordering::SeqCst),
            "dispatcher panicked after accepting ownership"
        );
        phenix_graph::process::DispatchResult::Completed(job.run())
    }
}
#[test]
fn tagged_dispatcher_panic_invalidates_owned_job_and_allows_followup_under_restart_policy() {
    use phenix_graph::runtime::RequestStatus;
    let mut opts = restarting_options();
    opts.dispatcher = Arc::new(PanicOnceDispatcher(std::sync::atomic::AtomicBool::new(
        true,
    )));
    let mut s = cancellable_counter(
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        1,
    )
    .start_with_options(opts);
    let id = s.begin_request(Some("output")).unwrap();
    s.inject_request(id, "input", Value::of(99_i64)).unwrap();
    s.close_request(id).unwrap();
    assert_ne!(s.run_until_idle(30).state, SessionState::Failed);
    assert_eq!(s.request_status(id).unwrap(), RequestStatus::Failed);
    assert!(
        s.request_failure(id)
            .unwrap()
            .unwrap()
            .contains("dispatcher panicked")
    );
    assert_eq!(s.request_cancelled_jobs(id).unwrap().len(), 1);
    s.release_request(id).unwrap();
    let next = s.begin_request(Some("output")).unwrap();
    s.inject_request(next, "input", Value::of(5_i64)).unwrap();
    s.close_request(next).unwrap();
    s.run_until_idle(30);
    assert_eq!(
        s.take_request_output(next, "output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        5
    );
}

struct ForeignReturnDispatcher {
    returned: std::sync::Mutex<Option<phenix_graph::process::DispatchResult>>,
    accepted: std::sync::Mutex<Option<phenix_graph::process::ActivationJob>>,
    calls: AtomicUsize,
}
impl phenix_graph::process::ProcessDispatcher for ForeignReturnDispatcher {
    fn dispatch(
        &self,
        job: phenix_graph::process::ActivationJob,
    ) -> phenix_graph::process::DispatchResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.accepted.lock().unwrap() = Some(job);
        self.returned.lock().unwrap().take().unwrap()
    }
}
#[test]
fn driver_retains_foreign_completion_or_rejected_job_without_panic_publication_or_rerun() {
    use phenix_graph::{
        process::{DispatchResult, HandoffError, ProcessHandoff},
        runtime::DispatchReturn,
    };
    for completed in [true, false] {
        let alive = Arc::new(AtomicUsize::new(1));
        let foreign_effects = Arc::new(AtomicUsize::new(0));
        let own_effects = Arc::new(AtomicUsize::new(0));
        let mut foreign = ProcessHandoff::new(Box::new(Counter {
            sum: 0,
            emitted: false,
            effects: foreign_effects.clone(),
        }))
        .with_lease(Arc::new(NativeLease(alive.clone())));
        foreign
            .admit(ProcessContext::new(
                std::collections::BTreeMap::from([("in".into(), Value::of(7_i64))]),
                vec![],
            ))
            .unwrap();
        let foreign_job = foreign.next_job().unwrap();
        let ticket = foreign_job.ticket();
        let returned = if completed {
            DispatchResult::Completed(foreign_job.run())
        } else {
            DispatchResult::Rejected(foreign_job)
        };
        let dispatcher = Arc::new(ForeignReturnDispatcher {
            returned: std::sync::Mutex::new(Some(returned)),
            accepted: std::sync::Mutex::new(None),
            calls: AtomicUsize::new(0),
        });
        let mut opts = restarting_options();
        opts.dispatcher = dispatcher.clone();
        let mut session = counter(own_effects.clone()).start_with_options(opts);
        let id = session.begin_request(Some("output")).unwrap();
        session
            .inject_request(id, "input", Value::of(1_i64))
            .unwrap();
        session.close_request(id).unwrap();
        let report = session.run_until_idle(20);
        assert_eq!(report.state, SessionState::Failed);
        assert_eq!(report.steps, 1);
        assert_eq!(report.publications, 0);
        assert_eq!(own_effects.load(Ordering::SeqCst), 0);
        assert!(session.failure().unwrap().contains(if completed {
            "invalid completion"
        } else {
            "invalid rejected job"
        }));
        assert!(session.take_output("output").unwrap().is_none());
        assert_eq!(session.run_until_idle(20).steps, 0);
        assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
        let refused = session.take_dispatch_return().unwrap();
        assert!(session.take_dispatch_return().is_none());
        drop(dispatcher.accepted.lock().unwrap().take());
        drop(session);
        assert_eq!(alive.load(Ordering::SeqCst), 1);
        let result = match refused {
            DispatchReturn::Completed { error, completion } => {
                assert!(completed);
                assert_eq!(error, HandoffError::ForeignCompletion);
                assert_eq!(completion.ticket(), ticket);
                foreign.complete(*completion).unwrap().unwrap()
            }
            DispatchReturn::Rejected { error, job } => {
                assert!(!completed);
                assert_eq!(error, HandoffError::ForeignCompletion);
                assert_eq!(job.ticket(), ticket);
                assert_eq!(foreign_effects.load(Ordering::SeqCst), 0);
                foreign.reject(job).unwrap();
                let job = foreign.next_job().unwrap();
                assert_eq!(job.ticket().activation(), ticket.activation());
                foreign.complete(job.run()).unwrap().unwrap()
            }
        };
        let Step::Emit(emission) = result else {
            panic!("foreign emission was not retained");
        };
        assert_eq!(foreign_effects.load(Ordering::SeqCst), 1);
        assert_eq!(own_effects.load(Ordering::SeqCst), 0);
        let typed = emission.value.decoded::<i64>().unwrap();
        drop(foreign);
        drop(emission);
        assert_eq!(*typed, 7);
        assert_eq!(alive.load(Ordering::SeqCst), 1);
        drop(typed);
        assert_eq!(alive.load(Ordering::SeqCst), 0);
    }
}

struct RejectOnceDispatcher(std::sync::atomic::AtomicBool);
impl phenix_graph::process::ProcessDispatcher for RejectOnceDispatcher {
    fn dispatch(
        &self,
        job: phenix_graph::process::ActivationJob,
    ) -> phenix_graph::process::DispatchResult {
        if self.0.swap(false, Ordering::SeqCst) {
            phenix_graph::process::DispatchResult::Rejected(job)
        } else {
            phenix_graph::process::DispatchResult::Completed(job.run())
        }
    }
}
#[test]
fn driver_accepts_ordinary_rejection_without_retaining_a_protocol_error() {
    let effects = Arc::new(AtomicUsize::new(0));
    let mut opts = options();
    opts.dispatcher = Arc::new(RejectOnceDispatcher(std::sync::atomic::AtomicBool::new(
        true,
    )));
    let mut session = counter(effects.clone()).start_with_options(opts);
    session.inject("input", Value::of(3_i64)).unwrap();
    assert_eq!(session.run_until_idle(20).state, SessionState::Runnable);
    assert!(session.take_dispatch_return().is_none());
    assert!(session.failure().is_none());
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    session.run_until_idle(20);
    assert_eq!(
        session
            .take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<i64>()
            .unwrap(),
        3
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert!(session.take_dispatch_return().is_none());
}

struct SelectRequiredB;
impl phenix_graph::runtime::ReadinessPolicy for SelectRequiredB {
    fn select(&self, snapshot: &phenix_graph::runtime::ReadinessSnapshot) -> Vec<String> {
        snapshot
            .heads
            .iter()
            .filter(|head| head.port == "b" && head.available)
            .map(|head| head.port.clone())
            .collect()
    }
}
struct RequiredStage {
    phase: bool,
    policy: Readiness,
    resumed: Arc<AtomicUsize>,
    scoped: bool,
}
impl Process for RequiredStage {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if !self.phase {
            if cx.take("a").is_none() {
                assert!(cx.is_eos("a"));
                return Ok(Step::Finished);
            }
            self.phase = true;
            return Ok(Step::WaitInputs(self.policy.clone()));
        }
        self.resumed.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            cx.take("b")
                .expect("resumed without required b")
                .get::<i64>()?,
            22
        );
        if matches!(self.policy, Readiness::Optional { .. }) {
            assert!(cx.is_closed("c"));
            assert_eq!(cx.is_request_closed("c"), self.scoped);
            assert_eq!(cx.is_eos("c"), !self.scoped);
        }
        self.phase = false;
        Ok(Step::Complete)
    }
}
struct RequiredBSource(u8);
impl Process for RequiredBSource {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        self.0 += 1;
        Ok(match self.0 {
            1 => Step::WaitWake,
            2 => Step::Emit(ProcessEmission::new("out", Value::of(22_i64))),
            _ => Step::Complete,
        })
    }
}
fn required_stage_session(
    policy: Readiness,
    live_b: bool,
    scoped: bool,
    resumed: Arc<AtomicUsize>,
) -> phenix_graph::runtime::NetworkSession {
    let mut b = NetworkBuilder::new();
    let stage = b
        .node(
            "stage",
            Arc::new(
                Interface::new(
                    ["a", "b", "c"].map(|name| PortDef::wire::<i64>(name).unwrap()),
                    [],
                )
                .unwrap(),
            ),
            Arc::new(move || RequiredStage {
                phase: false,
                policy: policy.clone(),
                resumed: resumed.clone(),
                scoped,
            }),
        )
        .unwrap();
    for name in ["a", "c"] {
        b.expose_input(name, stage.wire_input::<i64>(name).unwrap(), one())
            .unwrap();
    }
    if live_b {
        let source = b
            .node(
                "source",
                Arc::new(Interface::new([], [PortDef::wire::<i64>("out").unwrap()]).unwrap()),
                Arc::new(|| RequiredBSource(0)),
            )
            .unwrap();
        b.start(&source).unwrap();
        b.connect(
            source.wire_output::<i64>("out").unwrap(),
            stage.wire_input::<i64>("b").unwrap(),
            one(),
        )
        .unwrap();
    } else {
        b.expose_input("b", stage.wire_input::<i64>("b").unwrap(), one())
            .unwrap();
    }
    let mut opts = restarting_options();
    opts.auto_start_sources = false;
    b.build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(opts)
}
fn required_b_optional_c() -> Readiness {
    Readiness::Optional {
        required: vec!["b".into()],
        optional: vec!["c".into()],
    }
}

#[test]
fn staged_optional_and_custom_groups_wait_for_live_required_producer_not_optional_closure() {
    use phenix_graph::runtime::RequestStatus;
    for policy in [
        required_b_optional_c(),
        Readiness::Custom(Arc::new(SelectRequiredB)),
    ] {
        let resumed = Arc::new(AtomicUsize::new(0));
        let mut session = required_stage_session(policy, true, true, resumed.clone());
        let request = session.begin_request(None).unwrap();
        session.start_request_source(request, "source").unwrap();
        session
            .inject_request(request, "a", Value::of(1_i64))
            .unwrap();
        session.close_request(request).unwrap();
        let report = session.run_until_idle(20);
        assert_eq!(report.state, SessionState::Waiting);
        assert_eq!(resumed.load(Ordering::SeqCst), 0);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Pending
        );
        assert_eq!(session.request_work(request).unwrap().waiting_input, 1);
        assert_eq!(session.request_work(request).unwrap().waiting_wake, 1);
        assert_eq!(session.run_until_idle(20).steps, 0);
        session.wake_handle("source").unwrap().wake();
        session.run_until_idle(20);
        assert_eq!(resumed.load(Ordering::SeqCst), 1);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
    }
}

#[test]
fn staged_missing_required_groups_fail_request_without_successful_continuation() {
    use phenix_graph::runtime::RequestStatus;
    for policy in [
        required_b_optional_c(),
        Readiness::Custom(Arc::new(SelectRequiredB)),
        Readiness::All,
        Readiness::AtLeast(NonZeroUsize::new(2).unwrap()),
    ] {
        let resumed = Arc::new(AtomicUsize::new(0));
        let mut session = required_stage_session(policy, false, true, resumed.clone());
        let request = session.begin_request(None).unwrap();
        session
            .inject_request(request, "a", Value::of(1_i64))
            .unwrap();
        session.close_request(request).unwrap();
        session.run_until_idle(20);
        assert_eq!(resumed.load(Ordering::SeqCst), 0);
        assert_eq!(
            session.request_status(request).unwrap(),
            RequestStatus::Failed
        );
        assert!(
            session
                .request_failure(request)
                .unwrap()
                .unwrap()
                .contains("required")
        );
        assert_eq!(session.request_work(request).unwrap().waiting_input, 0);
        session.release_request(request).unwrap();
    }
}

#[test]
fn staged_global_optional_eos_waits_for_required_data_and_exhausted_required_eos_fails() {
    let resumed = Arc::new(AtomicUsize::new(0));
    let mut session =
        required_stage_session(required_b_optional_c(), false, false, resumed.clone());
    session.inject("a", Value::of(1_i64)).unwrap();
    session.run_until_idle(20);
    session.close_input("c").unwrap();
    assert_eq!(session.run_until_idle(20).steps, 0);
    assert_eq!(resumed.load(Ordering::SeqCst), 0);
    session.inject("b", Value::of(22_i64)).unwrap();
    session.run_until_idle(20);
    assert_eq!(resumed.load(Ordering::SeqCst), 1);
    assert!(session.failure().is_none());
    session.close_input("a").unwrap();
    session.close_input("b").unwrap();
    assert_eq!(session.run_until_idle(20).state, SessionState::Completed);
    let resumed = Arc::new(AtomicUsize::new(0));
    let mut missing =
        required_stage_session(required_b_optional_c(), false, false, resumed.clone());
    missing.inject("a", Value::of(1_i64)).unwrap();
    missing.run_until_idle(20);
    missing.close_input("c").unwrap();
    missing.close_input("b").unwrap();
    assert_eq!(missing.state(), SessionState::Failed);
    assert_eq!(resumed.load(Ordering::SeqCst), 0);
    assert!(missing.failure().unwrap().contains("required input b"));
}

#[test]
fn idle_input_prefilter_preserves_eos_and_custom_policy_rechecks() {
    use phenix_graph::component::ComponentDef;

    let mut builder = NetworkBuilder::new();
    let idle = builder
        .add("idle", Arc::new(ComponentDef::sink(|_: &i64| {})))
        .unwrap();
    builder
        .expose_input("input", idle.input::<i64>("input").unwrap(), one())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();

    let mut session = compiled.start();
    for _ in 0..3 {
        let report = session.run_until_idle(5);
        assert_eq!(report.steps, 0);
        assert_eq!(report.state, SessionState::Quiescent);
    }
    session.close_input("input").unwrap();
    assert_eq!(session.run_until_idle(16).state, SessionState::Completed);

    // Custom readiness only runs when an actual packet/request candidate
    // exists. Keep a packet pending under a closed policy, then open the
    // policy without admitting another packet: the next driver pass retries.
    struct CountCalls(Arc<AtomicUsize>, Arc<AtomicBool>);
    impl ReadinessPolicy for CountCalls {
        fn select(&self, snapshot: &ReadinessSnapshot) -> Vec<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            if !self.1.load(Ordering::SeqCst) {
                return vec![];
            }
            snapshot
                .heads
                .iter()
                .filter(|head| head.available)
                .map(|head| head.port.clone())
                .collect()
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let enabled = Arc::new(AtomicBool::new(false));
    let mut options = SessionOptions::default();
    options.readiness.insert(
        "idle".into(),
        Readiness::Custom(Arc::new(CountCalls(calls.clone(), enabled.clone()))),
    );
    let mut custom = compiled.start_with_options(options);
    custom.inject_local("input", 17_i64).unwrap();
    let before = calls.load(Ordering::SeqCst);
    let blocked = custom.run_until_idle(5);
    assert_eq!(blocked.steps, 0);
    assert_eq!(blocked.state, SessionState::Waiting);
    assert!(calls.load(Ordering::SeqCst) > before);
    let before = calls.load(Ordering::SeqCst);
    enabled.store(true, Ordering::SeqCst);
    let resumed = custom.run_until_idle(5);
    assert!(resumed.steps > 0);
    assert_eq!(resumed.state, SessionState::Quiescent);
    assert!(calls.load(Ordering::SeqCst) > before);
}
