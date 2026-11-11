//! Production owned-step invariants. These do not substitute for scheduler,
//! correlation, topology, or downstream dispatch-guard integration tests.
use phenix_graph::Value;
use phenix_graph::process::*;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counter {
    value: Cell<i64>,
    emitted: bool,
}
impl Process for Counter {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let increment = context.take("increment").unwrap().get::<i64>()?;
        self.value.set(self.value.get() + increment);
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "sum",
            Value::of(self.value.get()),
        )))
    }
}
fn counter() -> Counter {
    Counter {
        value: Cell::new(0),
        emitted: false,
    }
}
fn inputs(value: i64) -> ProcessContext {
    ProcessContext::new(
        BTreeMap::from([("increment".into(), Value::of(value))]),
        vec![],
    )
}
fn step(handoff: &mut ProcessHandoff) -> Step {
    let job = handoff.next_job().unwrap();
    handoff.complete(job.run()).unwrap().unwrap()
}
fn emission_value(step: Step) -> i64 {
    let Step::Emit(emission) = step else {
        panic!("expected emission")
    };
    assert_eq!(emission.output, "sum");
    emission.value.get().unwrap()
}

#[test]
fn send_not_sync_process_and_owned_arguments_cross_worker_thread() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(3)).unwrap();
    let job = handoff.next_job().unwrap();
    let completion = std::thread::spawn(move || job.run()).join().unwrap();
    assert_eq!(
        emission_value(handoff.complete(completion).unwrap().unwrap()),
        3
    );
    assert_eq!(handoff.state(), HandoffState::PublicationPending);
}

#[test]
fn emission_and_logical_completion_are_distinct_and_state_is_persistent() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(3)).unwrap();
    let job = handoff.next_job().unwrap();
    let activation = job.ticket().activation();
    assert_eq!(
        emission_value(handoff.complete(job.run()).unwrap().unwrap()),
        3
    );
    assert!(handoff.admit(inputs(100)).is_err());
    assert!(handoff.next_job().is_err());
    handoff.publication_committed().unwrap();
    let job = handoff.next_job().unwrap();
    assert_eq!(job.ticket().activation(), activation);
    assert!(matches!(
        handoff.complete(job.run()).unwrap().unwrap(),
        Step::Complete
    ));
    handoff.admit(inputs(5)).unwrap();
    let job = handoff.next_job().unwrap();
    assert_ne!(job.ticket().activation(), activation);
    assert_eq!(
        emission_value(handoff.complete(job.run()).unwrap().unwrap()),
        8
    );
}

#[test]
fn factory_spawns_are_isolated_across_instances_and_session_owners() {
    let factory: Arc<dyn ProcessFactory> = Arc::new(counter);
    let mut handoffs: Vec<_> = (0..4)
        .map(|_| ProcessHandoff::new(factory.spawn()))
        .collect();
    for (index, handoff) in handoffs.iter_mut().enumerate() {
        handoff.admit(inputs(index as i64 + 1)).unwrap();
        assert_eq!(emission_value(step(handoff)), index as i64 + 1);
        handoff.publication_committed().unwrap();
        assert!(matches!(step(handoff), Step::Complete));
    }
    for (index, handoff) in handoffs.iter_mut().enumerate() {
        handoff.admit(inputs(10)).unwrap();
        assert_eq!(emission_value(step(handoff)), index as i64 + 11);
    }
}

#[test]
fn only_one_job_and_one_activation_can_be_outstanding() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    assert!(handoff.next_job().is_err());
    handoff.admit(inputs(1)).unwrap();
    let job = handoff.next_job().unwrap();
    assert_eq!(handoff.outstanding(), Some(job.ticket()));
    assert!(handoff.next_job().is_err());
    assert!(handoff.admit(inputs(2)).is_err());
    handoff.complete(job.run()).unwrap().unwrap();
    assert!(handoff.outstanding().is_none());
    assert!(handoff.next_job().is_err());
}

#[test]
fn different_processes_can_complete_out_of_dispatch_order() {
    let mut first = ProcessHandoff::new(Box::new(counter()));
    let mut second = ProcessHandoff::new(Box::new(counter()));
    first.admit(inputs(7)).unwrap();
    second.admit(inputs(11)).unwrap();
    let a = first.next_job().unwrap();
    let b = second.next_job().unwrap();
    assert_eq!(
        emission_value(second.complete(b.run()).unwrap().unwrap()),
        11
    );
    assert_eq!(first.state(), HandoffState::Dispatched);
    assert_eq!(emission_value(first.complete(a.run()).unwrap().unwrap()), 7);
}

#[test]
fn foreign_completion_is_returned_intact_and_does_not_replace_state() {
    let mut first = ProcessHandoff::new(Box::new(counter()));
    let mut second = ProcessHandoff::new(Box::new(counter()));
    first.admit(inputs(2)).unwrap();
    second.admit(inputs(9)).unwrap();
    let a = first.next_job().unwrap();
    let b = second.next_job().unwrap();
    let (error, completion) = first.complete(b.run()).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(first.outstanding(), Some(a.ticket()));
    assert_eq!(
        emission_value(second.complete(*completion).unwrap().unwrap()),
        9
    );
    assert_eq!(emission_value(first.complete(a.run()).unwrap().unwrap()), 2);
}

#[test]
fn rejected_dispatch_keeps_reservation_and_process_and_renews_step_ticket() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(4)).unwrap();
    let first = handoff.next_job().unwrap();
    let ticket = first.ticket();
    handoff.reject(first).unwrap();
    let second = handoff.next_job().unwrap();
    assert_eq!(second.ticket().activation(), ticket.activation());
    assert_ne!(second.ticket().sequence(), ticket.sequence());
    assert_eq!(
        emission_value(handoff.complete(second.run()).unwrap().unwrap()),
        4
    );
}

#[test]
fn foreign_dispatch_rejection_does_not_destroy_job() {
    let mut first = ProcessHandoff::new(Box::new(counter()));
    let mut second = ProcessHandoff::new(Box::new(counter()));
    second.admit(inputs(4)).unwrap();
    let job = second.next_job().unwrap();
    let (error, job) = first.reject(job).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    second.reject(job).unwrap();
    assert_eq!(emission_value(step(&mut second)), 4);
}

struct DropProbe(Arc<AtomicUsize>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct ProbedProcess {
    _probe: DropProbe,
    panic: bool,
}
impl Process for ProbedProcess {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        assert!(!self.panic, "step panic");
        Ok(Step::Finished)
    }
}

#[test]
fn cancellation_invalidates_before_late_completion_and_preserves_runner_inventory() {
    let drops = Arc::new(AtomicUsize::new(0));
    let process = ProbedProcess {
        _probe: DropProbe(drops.clone()),
        panic: false,
    };
    let mut handoff = ProcessHandoff::new(Box::new(process));
    handoff.admit(ProcessContext::default()).unwrap();
    let job = handoff.next_job().unwrap();
    assert_eq!(handoff.cancel(), Some(job.ticket()));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let (error, completion) = handoff.complete(job.run()).unwrap_err();
    assert_eq!(error, HandoffError::StaleCompletion);
    assert_eq!(handoff.state(), HandoffState::Cancelled);
    assert!(handoff.admit(inputs(1)).is_err());
    drop(completion);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(handoff.cancel().is_none());
}

#[test]
fn panic_settles_failure_and_drops_process_and_owned_inputs_once() {
    let process_drops = Arc::new(AtomicUsize::new(0));
    let input_drops = Arc::new(AtomicUsize::new(0));
    let process = ProbedProcess {
        _probe: DropProbe(process_drops.clone()),
        panic: true,
    };
    let mut handoff = ProcessHandoff::new(Box::new(process));
    handoff
        .admit(ProcessContext::new(
            BTreeMap::from([(
                "move-only".into(),
                Value::local(DropProbe(input_drops.clone())),
            )]),
            vec![],
        ))
        .unwrap();
    let job = handoff.next_job().unwrap();
    assert_eq!(
        handoff.complete(job.run()).unwrap().unwrap_err(),
        "process step panicked"
    );
    assert_eq!(handoff.state(), HandoffState::Failed);
    assert_eq!(process_drops.load(Ordering::SeqCst), 1);
    assert_eq!(input_drops.load(Ordering::SeqCst), 1);
    assert!(handoff.next_job().is_err());
}

#[test]
fn abandoned_job_drops_process_arguments_and_lease_once() {
    let process_drops = Arc::new(AtomicUsize::new(0));
    let input_drops = Arc::new(AtomicUsize::new(0));
    let lease_drops = Arc::new(AtomicUsize::new(0));
    let process = ProbedProcess {
        _probe: DropProbe(process_drops.clone()),
        panic: false,
    };
    let mut handoff =
        ProcessHandoff::new(Box::new(process)).with_lease(Arc::new(DropProbe(lease_drops.clone())));
    handoff
        .admit(ProcessContext::new(
            BTreeMap::from([(
                "move-only".into(),
                Value::local(DropProbe(input_drops.clone())),
            )]),
            vec![],
        ))
        .unwrap();
    let job = handoff.next_job().unwrap();
    drop(handoff);
    assert_eq!(lease_drops.load(Ordering::SeqCst), 0);
    drop(job);
    assert_eq!(process_drops.load(Ordering::SeqCst), 1);
    assert_eq!(input_drops.load(Ordering::SeqCst), 1);
    assert_eq!(lease_drops.load(Ordering::SeqCst), 1);
}

struct LeaseOrder {
    alive: Arc<AtomicUsize>,
    is_process: bool,
}
impl Drop for LeaseOrder {
    fn drop(&mut self) {
        if self.is_process {
            assert_eq!(
                self.alive.load(Ordering::SeqCst),
                1,
                "code lease dropped before process"
            );
        } else {
            self.alive.store(0, Ordering::SeqCst);
        }
    }
}
impl Process for LeaseOrder {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Finished)
    }
}

#[test]
fn late_completion_drops_process_before_code_lease() {
    let alive = Arc::new(AtomicUsize::new(1));
    let mut handoff = ProcessHandoff::new(Box::new(LeaseOrder {
        alive: alive.clone(),
        is_process: true,
    }))
    .with_lease(Arc::new(LeaseOrder {
        alive: alive.clone(),
        is_process: false,
    }));
    handoff.admit(ProcessContext::default()).unwrap();
    let job = handoff.next_job().unwrap();
    handoff.cancel();
    let (_, completion) = handoff.complete(job.run()).unwrap_err();
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(completion);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}

struct Continuation(u8);
impl Process for Continuation {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        self.0 += 1;
        Ok(match self.0 {
            1 => Step::Yield,
            2 => Step::WaitInput("later".into()),
            3 => {
                assert_eq!(context.take("later").unwrap().get::<i64>()?, 10);
                assert!(context.is_closed("later"));
                Step::Emit(ProcessEmission::new("first", Value::of(1_i64)))
            }
            4 => Step::Emit(ProcessEmission::new("second", Value::of(2_i64))),
            5 => Step::Complete,
            _ => Step::Finished,
        })
    }
}

#[test]
fn yields_input_waits_and_multiple_outputs_preserve_one_activation() {
    let mut handoff = ProcessHandoff::new(Box::new(Continuation(0)));
    handoff.admit(ProcessContext::default()).unwrap();
    let first = handoff.next_job().unwrap();
    let activation = first.ticket().activation();
    assert!(matches!(
        handoff.complete(first.run()).unwrap().unwrap(),
        Step::Yield
    ));
    assert!(matches!(step(&mut handoff), Step::WaitInput(name) if name == "later"));
    assert!(handoff.next_job().is_err());
    handoff
        .resume_input(ProcessContext::new(
            BTreeMap::from([("later".into(), Value::of(10_i64))]),
            vec!["later".into()],
        ))
        .unwrap();
    for output in ["first", "second"] {
        let job = handoff.next_job().unwrap();
        assert_eq!(job.ticket().activation(), activation);
        assert!(
            matches!(handoff.complete(job.run()).unwrap().unwrap(), Step::Emit(emission) if emission.output == output)
        );
        assert!(handoff.next_job().is_err());
        handoff.publication_committed().unwrap();
    }
    assert!(matches!(step(&mut handoff), Step::Complete));
    assert_eq!(handoff.state(), HandoffState::Idle);
    handoff.admit(ProcessContext::default()).unwrap();
    assert!(matches!(step(&mut handoff), Step::Finished));
    assert_eq!(handoff.state(), HandoffState::Finished);
}

#[test]
fn publication_failure_is_terminal_and_never_reexecutes_action() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(1)).unwrap();
    assert_eq!(emission_value(step(&mut handoff)), 1);
    handoff.publication_failed().unwrap();
    assert_eq!(handoff.state(), HandoffState::Failed);
    assert!(handoff.publication_committed().is_err());
    assert!(handoff.next_job().is_err());
}

#[test]
fn local_dispatcher_uses_the_same_owned_job() {
    let dispatcher: &dyn ProcessDispatcher = &LocalProcessDispatcher;
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(6)).unwrap();
    let DispatchResult::Completed(completion) = dispatcher.dispatch(handoff.next_job().unwrap())
    else {
        panic!("local dispatch was not completed");
    };
    assert_eq!(
        emission_value(handoff.complete(completion).unwrap().unwrap()),
        6
    );
}

#[test]
fn idle_process_cannot_run_without_explicit_admission() {
    let mut handoff = ProcessHandoff::new(Box::new(Continuation(0)));
    assert_eq!(handoff.state(), HandoffState::Idle);
    assert!(handoff.next_job().is_err());
    assert!(handoff.resume_input(ProcessContext::default()).is_err());
    assert!(handoff.publication_committed().is_err());
    assert!(handoff.publication_failed().is_err());
}

#[test]
fn failed_admission_returns_owned_arguments_without_touching_live_activation() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(1)).unwrap();
    let (error, returned) = handoff.admit(inputs(99)).unwrap_err();
    assert_eq!(error, HandoffError::InvalidState(HandoffState::Ready));
    assert_eq!(
        returned.input("increment").unwrap().get::<i64>().unwrap(),
        99
    );
    assert_eq!(emission_value(step(&mut handoff)), 1);
}

#[test]
fn failed_input_resume_returns_owned_arguments_without_overwriting_reservation() {
    let mut handoff = ProcessHandoff::new(Box::new(Continuation(0)));
    let (_, returned) = handoff.resume_input(inputs(7)).unwrap_err();
    assert_eq!(
        returned.input("increment").unwrap().get::<i64>().unwrap(),
        7
    );
    handoff.admit(inputs(8)).unwrap();
    assert!(matches!(step(&mut handoff), Step::Yield));
    assert!(matches!(step(&mut handoff), Step::WaitInput(_)));
    let (error, returned) = handoff.resume_input(inputs(9)).unwrap_err();
    assert_eq!(
        error,
        HandoffError::InvalidState(HandoffState::WaitingInput)
    );
    assert_eq!(
        returned.input("increment").unwrap().get::<i64>().unwrap(),
        9
    );
    assert_eq!(handoff.state(), HandoffState::WaitingInput);
    handoff
        .resume_input(ProcessContext::new(
            BTreeMap::from([("later".into(), Value::of(10_i64))]),
            vec!["later".into()],
        ))
        .unwrap();
    assert!(matches!(step(&mut handoff), Step::Emit(_)));
}

struct EmitsLeaseGuard {
    alive: Arc<AtomicUsize>,
}
impl Process for EmitsLeaseGuard {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, String> {
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::local(LeaseOrder {
                alive: self.alive.clone(),
                is_process: true,
            }),
        )))
    }
}

#[test]
fn late_emission_payload_drops_before_its_job_releases_code_lease() {
    let alive = Arc::new(AtomicUsize::new(1));
    let mut handoff = ProcessHandoff::new(Box::new(EmitsLeaseGuard {
        alive: alive.clone(),
    }))
    .with_lease(Arc::new(LeaseOrder {
        alive: alive.clone(),
        is_process: false,
    }));
    handoff.admit(ProcessContext::default()).unwrap();
    let completion = handoff.next_job().unwrap().run();
    handoff.cancel();
    let (_, completion) = handoff.complete(completion).unwrap_err();
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(completion);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}

#[test]
fn capacity_one_stall_and_resume_preserve_effect_and_activation_exactly_once() {
    use phenix_graph::connection::*;
    use std::num::NonZeroUsize;

    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    let mut connections = ConnectionSet::new(vec![ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        Value::of(0_i64).identity(),
    )]);
    let targets = [FanoutTarget::identity(0)];
    handoff.admit(inputs(1)).unwrap();
    let Step::Emit(first) = step(&mut handoff) else {
        panic!("missing first emission")
    };
    let mut first = connections
        .prepare(&Packet::new(first.value), &targets)
        .unwrap();
    connections.commit(&mut first).unwrap();
    handoff.publication_committed().unwrap();
    assert!(matches!(step(&mut handoff), Step::Complete));

    handoff.admit(inputs(2)).unwrap();
    let second_job = handoff.next_job().unwrap();
    let activation = second_job.ticket().activation();
    let Step::Emit(second) = handoff.complete(second_job.run()).unwrap().unwrap() else {
        panic!("missing second emission");
    };
    let mut second = connections
        .prepare(&Packet::new(second.value), &targets)
        .unwrap();
    for _ in 0..32 {
        assert_eq!(connections.commit(&mut second), Err(ConnectionError::Full));
        assert!(handoff.next_job().is_err());
        assert!(handoff.admit(inputs(100)).is_err());
    }
    assert_eq!(connections.pop(0).unwrap().value().get::<i64>().unwrap(), 1);
    connections.commit(&mut second).unwrap();
    assert_eq!(
        connections
            .pop(0)
            .unwrap()
            .into_value()
            .get::<i64>()
            .unwrap(),
        3
    );
    handoff.publication_committed().unwrap();
    let completion_job = handoff.next_job().unwrap();
    assert_eq!(completion_job.ticket().activation(), activation);
    assert!(matches!(
        handoff.complete(completion_job.run()).unwrap().unwrap(),
        Step::Complete
    ));
    assert_eq!(handoff.state(), HandoffState::Idle);
}

struct DeniedGuard;
impl ProcessExecutionGuard for DeniedGuard {
    fn enter(&self) -> Result<Box<dyn std::any::Any>, String> {
        Err("guard rejected execution".into())
    }
}
#[test]
fn execution_guard_failure_never_calls_process_and_settles_same_job() {
    let mut handoff = ProcessHandoff::new(Box::new(counter()));
    handoff.admit(inputs(3)).unwrap();
    let job = handoff
        .next_job()
        .unwrap()
        .with_guard(Arc::new(DeniedGuard));
    let ticket = job.ticket();
    let completion = std::thread::spawn(move || job.run()).join().unwrap();
    assert_eq!(completion.ticket(), ticket);
    assert_eq!(
        handoff.complete(completion).unwrap().unwrap_err(),
        "guard rejected execution"
    );
    assert_eq!(handoff.state(), HandoffState::Failed);
}

#[test]
fn successful_handoff_emission_retains_lease_through_value_and_typed_clones() {
    let alive = Arc::new(AtomicUsize::new(1));
    let mut handoff = ProcessHandoff::new(Box::new(EmitsLeaseGuard {
        alive: alive.clone(),
    }))
    .with_lease(Arc::new(LeaseOrder {
        alive: alive.clone(),
        is_process: false,
    }));
    handoff.admit(ProcessContext::default()).unwrap();
    let Step::Emit(emission) = step(&mut handoff) else {
        panic!("missing local emission");
    };
    let value = emission.value;
    let cloned = value.clone();
    let typed = value.leased_shared::<LeaseOrder>().unwrap();
    let typed_clone = typed.clone();
    drop(handoff);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    assert!(value.shared::<LeaseOrder>().is_err());
    assert!(value.clone().into_shared::<LeaseOrder>().is_err());
    drop(value);
    drop(typed);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(cloned);
    assert_eq!(typed_clone.alive.load(Ordering::SeqCst), 1);
    drop(typed_clone);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}

#[test]
fn successful_handoff_wire_emission_requires_lease_aware_decoding_after_owner_drop() {
    let alive = Arc::new(AtomicUsize::new(1));
    let mut handoff = ProcessHandoff::new(Box::new(counter())).with_lease(Arc::new(LeaseOrder {
        alive: alive.clone(),
        is_process: false,
    }));
    handoff.admit(inputs(7)).unwrap();
    let Step::Emit(emission) = step(&mut handoff) else {
        panic!("missing wire emission");
    };
    let value = emission.value;
    let typed = value.decoded::<i64>().unwrap();
    let cloned = typed.clone();
    drop(handoff);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    assert!(value.get::<i64>().is_err());
    assert_eq!(*typed, 7);
    drop(value);
    drop(typed);
    assert_eq!(*cloned, 7);
    assert_eq!(alive.load(Ordering::SeqCst), 1);
    drop(cloned);
    assert_eq!(alive.load(Ordering::SeqCst), 0);
}
