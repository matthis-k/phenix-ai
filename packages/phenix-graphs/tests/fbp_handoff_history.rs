//! Public ownership-return and transport complements to the private bounded
//! `ProcessHandoff` history envelope. Every job/completion below is real and
//! single-owner; no completion is cloned or reconstructed for a retry.
use phenix_graph::{
    CallbackPhase, Failure, FailureKind, Value,
    connection::{ConnectionError, ConnectionQueue, ConnectionSet, FanoutTarget, Packet},
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{
        CallbackPanicked, HandoffCancelError, HandoffError, HandoffState, PanicSummary, Process,
        ProcessCleanupError, ProcessContext, ProcessEmission, ProcessExecutionGuard,
        ProcessHandoff, Step,
    },
    runtime::{SessionOptions, SessionState},
};
use std::{
    any::Any,
    error::Error,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

struct EmitOnce {
    value: u64,
    emitted: bool,
    effects: Arc<AtomicUsize>,
}
impl EmitOnce {
    fn new(value: u64) -> (Self, Arc<AtomicUsize>) {
        let effects = Arc::new(AtomicUsize::new(0));
        (
            Self {
                value,
                emitted: false,
                effects: effects.clone(),
            },
            effects,
        )
    }
}
impl Process for EmitOnce {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        self.effects.fetch_add(1, Ordering::SeqCst);
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::of(self.value),
        )))
    }
}

fn admitted(process: Box<dyn Process>) -> ProcessHandoff {
    let mut handoff = ProcessHandoff::new(process);
    handoff.admit(ProcessContext::default()).unwrap();
    handoff
}

#[test]
fn foreign_completion_is_intact_until_its_rightful_owner_settles_it() {
    let (first, first_effects) = EmitOnce::new(11);
    let (second, second_effects) = EmitOnce::new(29);
    let mut first = admitted(Box::new(first));
    let mut second = admitted(Box::new(second));
    let first_job = first.next_job().unwrap();
    let second_job = second.next_job().unwrap();
    let first_ticket = first_job.ticket();
    let second_ticket = second_job.ticket();
    let mut returned = Box::new(second_job.run());

    // Repeated refusals move the *same* non-Clone completion back and forth.
    for _ in 0..3 {
        let (error, next) = first.complete(*returned).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        assert_eq!(next.ticket(), second_ticket);
        assert_eq!(first.outstanding(), Some(first_ticket));
        returned = next;
    }
    assert_eq!(first_effects.load(Ordering::SeqCst), 0);
    assert_eq!(second_effects.load(Ordering::SeqCst), 1);

    let Step::Emit(emission) = second.complete(*returned).unwrap().unwrap() else {
        panic!("rightful owner lost the foreign-returned emission")
    };
    assert_eq!(emission.value.get::<u64>().unwrap(), 29);
    assert_eq!(second.outstanding(), None);
    assert_eq!(second.state(), HandoffState::PublicationPending);
    assert_eq!(second_effects.load(Ordering::SeqCst), 1);
    let Step::Emit(first_emission) = first.complete(first_job.run()).unwrap().unwrap() else {
        panic!("first owner did not settle its own real job")
    };
    assert_eq!(first_emission.value.get::<u64>().unwrap(), 11);
    assert_eq!(first_effects.load(Ordering::SeqCst), 1);
}

#[test]
fn same_owner_stale_completion_survives_repeated_refusals_without_reexecution() {
    let (process, effects) = EmitOnce::new(47);
    let mut handoff = admitted(Box::new(process));
    let job = handoff.next_job().unwrap();
    let ticket = job.ticket();
    let mut returned = Box::new(job.run());
    assert_eq!(handoff.cancel().unwrap(), Some(ticket));
    assert_eq!(handoff.outstanding(), None);

    for _ in 0..3 {
        let (error, next) = handoff.complete(*returned).unwrap_err();
        assert_eq!(error, HandoffError::StaleCompletion);
        assert_eq!(next.ticket(), ticket);
        assert_eq!(handoff.state(), HandoffState::Cancelled);
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        returned = next;
    }
    assert_eq!(handoff.cancel().unwrap(), None);
    drop(returned);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}

#[test]
fn publication_commit_is_an_ack_not_an_idempotent_replay_permit() {
    let (process, effects) = EmitOnce::new(53);
    let mut handoff = admitted(Box::new(process));
    let job = handoff.next_job().unwrap();
    let Step::Emit(_) = handoff.complete(job.run()).unwrap().unwrap() else {
        panic!("missing emission")
    };
    handoff.publication_committed().unwrap();
    for _ in 0..2 {
        assert_eq!(
            handoff.publication_committed(),
            Err(HandoffError::InvalidState(HandoffState::Ready))
        );
    }
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    let continuation = handoff.next_job().unwrap();
    assert!(matches!(
        handoff.complete(continuation.run()).unwrap(),
        Ok(Step::Complete)
    ));
    assert_eq!(effects.load(Ordering::SeqCst), 2);
}

#[test]
fn retained_connection_full_and_session_backpressure_do_not_rerun_effects() {
    let (process, effects) = EmitOnce::new(73);
    let mut handoff = admitted(Box::new(process));
    let mut connections = ConnectionSet::new(vec![ConnectionQueue::new(
        NonZeroUsize::new(1).unwrap(),
        Value::of(0_u64).identity(),
    )]);
    let targets = [FanoutTarget::identity(0)];
    let initial = Packet::new(Value::of(0_u64));
    let mut initial_transaction = connections.prepare(&initial, &targets).unwrap();
    connections.commit(&mut initial_transaction).unwrap();

    let job = handoff.next_job().unwrap();
    let activation = job.ticket().activation();
    let Step::Emit(emission) = handoff.complete(job.run()).unwrap().unwrap() else {
        panic!("missing retained emission")
    };
    let packet = Packet::new(emission.value);
    let mut transaction = connections.prepare(&packet, &targets).unwrap();
    for _ in 0..4 {
        assert_eq!(
            connections.commit(&mut transaction),
            Err(ConnectionError::Full)
        );
        assert_eq!(handoff.state(), HandoffState::PublicationPending);
        assert_eq!(
            handoff.next_job().unwrap_err(),
            HandoffError::InvalidState(HandoffState::PublicationPending)
        );
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }
    assert_eq!(connections.pop(0).unwrap().value().get::<u64>().unwrap(), 0);
    connections.commit(&mut transaction).unwrap();
    assert_eq!(
        connections.commit(&mut transaction),
        Err(ConnectionError::AlreadyCommitted)
    );
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    handoff.publication_committed().unwrap();
    let continuation = handoff.next_job().unwrap();
    assert_eq!(continuation.ticket().activation(), activation);
    assert!(matches!(
        handoff.complete(continuation.run()).unwrap(),
        Ok(Step::Complete)
    ));
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert_eq!(
        connections.pop(0).unwrap().value().get::<u64>().unwrap(),
        73
    );
    assert!(connections.pop(0).is_none());

    struct Echo {
        emitted: bool,
        effects: Arc<AtomicUsize>,
    }
    impl Process for Echo {
        fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, Failure> {
            self.effects.fetch_add(1, Ordering::SeqCst);
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            let value = cx.take("in").expect("one request packet");
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new("out", value)))
        }
    }
    let effects = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let worker = builder
        .node(
            "worker",
            Arc::new(
                Interface::new(
                    [PortDef::wire::<u64>("in").unwrap()],
                    [PortDef::wire::<u64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new({
                let effects = effects.clone();
                move || Echo {
                    emitted: false,
                    effects: effects.clone(),
                }
            }),
        )
        .unwrap();
    builder
        .expose_input(
            "input",
            worker.wire_input::<u64>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("output", worker.wire_output::<u64>("out").unwrap())
        .unwrap();
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            capture_capacity: NonZeroUsize::new(1).unwrap(),
            ..SessionOptions::default()
        });
    session.inject("input", Value::of(101_u64)).unwrap();
    assert_eq!(session.run_until_idle(8).publications, 1);
    session.inject("input", Value::of(103_u64)).unwrap();
    let blocked = session.run_until_idle(8);
    assert_eq!((blocked.steps, blocked.publications), (1, 0));
    assert_eq!(blocked.state, SessionState::Waiting);
    for _ in 0..3 {
        let retry = session.run_until_idle(8);
        assert_eq!((retry.steps, retry.publications), (0, 0));
        assert_eq!(effects.load(Ordering::SeqCst), 3);
    }
    assert_eq!(
        session
            .take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<u64>()
            .unwrap(),
        101
    );
    let drained = session.run_until_idle(8);
    assert_eq!((drained.steps, drained.publications), (1, 1));
    assert_eq!(
        session
            .take_output("output")
            .unwrap()
            .unwrap()
            .value()
            .get::<u64>()
            .unwrap(),
        103
    );
    assert!(session.take_output("output").unwrap().is_none());
    assert_eq!(effects.load(Ordering::SeqCst), 4);
}

struct ContextLeaseProbe(Weak<()>, Arc<AtomicUsize>);
impl Drop for ContextLeaseProbe {
    fn drop(&mut self) {
        assert!(self.0.upgrade().is_some(), "context lost its code lease");
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}
struct Panics;
impl Process for Panics {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        panic!("deliberate compact step panic")
    }
}

#[test]
fn panic_is_an_accepted_typed_failure_with_compact_source_and_live_lease() {
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let context_drops = Arc::new(AtomicUsize::new(0));
    let mut handoff = ProcessHandoff::new(Box::new(Panics)).with_lease(lease);
    handoff
        .admit(ProcessContext::new(
            [(
                "context".into(),
                Value::local(ContextLeaseProbe(weak.clone(), context_drops.clone())),
            )]
            .into(),
            vec![],
        ))
        .unwrap();
    let completion = handoff.next_job().unwrap().run();
    let failure = handoff.complete(completion).unwrap().unwrap_err();
    assert_eq!(handoff.state(), HandoffState::Failed);
    assert_eq!(failure.kind(), FailureKind::Panic);
    assert_eq!(failure.context().phase, Some(CallbackPhase::Step));
    assert_eq!(
        failure
            .source()
            .and_then(|source| source.downcast_ref::<CallbackPanicked>())
            .map(|source| &source.payload),
        Some(&PanicSummary::String(
            "deliberate compact step panic".into()
        ))
    );
    assert_eq!(context_drops.load(Ordering::SeqCst), 1);
    assert!(
        weak.upgrade().is_some(),
        "returned Failure dropped its lease"
    );
    drop(failure);
    assert!(weak.upgrade().is_none());
}

struct ReverseGuard {
    name: &'static str,
    events: Arc<Mutex<Vec<&'static str>>>,
    panic: bool,
}
impl ProcessExecutionGuard for ReverseGuard {
    fn enter(&self) -> Result<Box<dyn Any>, Failure> {
        panic!("an unrun rejected job must not enter its guard")
    }
}
impl Drop for ReverseGuard {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.name);
        if self.panic {
            panic!("guard object cleanup panic")
        }
    }
}
struct Completes;
impl Process for Completes {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        Ok(Step::Complete)
    }
}

#[test]
fn accepted_reject_cleanup_failure_is_not_an_outer_refusal_and_guards_reverse() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut handoff = admitted(Box::new(Completes));
    let job = handoff
        .next_job()
        .unwrap()
        .with_guard(Arc::new(ReverseGuard {
            name: "first",
            events: events.clone(),
            panic: true,
        }))
        .with_guard(Arc::new(ReverseGuard {
            name: "second",
            events: events.clone(),
            panic: true,
        }));
    let failure = handoff.reject(job).unwrap().unwrap_err();
    assert_eq!(handoff.state(), HandoffState::Failed);
    assert_eq!(*events.lock().unwrap(), ["second", "first"]);
    assert_eq!(
        failure
            .source()
            .and_then(|source| source.downcast_ref::<ProcessCleanupError>()),
        Some(&ProcessCleanupError::GuardObject { guard_index: 1 })
    );
    assert_eq!(failure.cleanup().len(), 1);
    assert_eq!(
        failure.cleanup()[0]
            .source()
            .and_then(|source| source.downcast_ref::<ProcessCleanupError>()),
        Some(&ProcessCleanupError::GuardObject { guard_index: 0 })
    );
}

struct PanicProcessDrop {
    emit: bool,
    emitted: bool,
}
impl Process for PanicProcessDrop {
    fn step(&mut self, _: &mut ProcessContext) -> Result<Step, Failure> {
        if self.emit && !self.emitted {
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new("out", Value::of(1_u8))))
        } else {
            Ok(Step::Complete)
        }
    }
}
impl Drop for PanicProcessDrop {
    fn drop(&mut self) {
        panic!("deliberate process cleanup panic")
    }
}

#[test]
fn terminal_cleanup_errors_are_accepted_transitions_not_state_refusals() {
    let mut finish = ProcessHandoff::new(Box::new(PanicProcessDrop {
        emit: false,
        emitted: false,
    }));
    assert_eq!(finish.finish_idle(), Err(HandoffError::DestructorPanicked));
    assert_eq!(finish.state(), HandoffState::Finished);
    assert_eq!(
        finish.finish_idle(),
        Err(HandoffError::InvalidState(HandoffState::Finished))
    );

    let mut publication = admitted(Box::new(PanicProcessDrop {
        emit: true,
        emitted: false,
    }));
    let job = publication.next_job().unwrap();
    assert!(matches!(
        publication.complete(job.run()).unwrap(),
        Ok(Step::Emit(_))
    ));
    assert_eq!(
        publication.publication_failed(),
        Err(HandoffError::DestructorPanicked)
    );
    assert_eq!(publication.state(), HandoffState::Failed);
    assert_eq!(
        publication.publication_failed(),
        Err(HandoffError::InvalidState(HandoffState::Failed))
    );

    let mut cancel = ProcessHandoff::new(Box::new(PanicProcessDrop {
        emit: false,
        emitted: false,
    }));
    assert_eq!(
        cancel.cancel(),
        Err(HandoffCancelError::DestructorPanicked { ticket: None })
    );
    assert_eq!(cancel.state(), HandoffState::Cancelled);
    assert_eq!(cancel.cancel(), Ok(None));
}

#[test]
fn accepted_completion_cleanup_failure_retains_lease_through_context_disposal() {
    struct PanicInput(Weak<()>);
    impl Drop for PanicInput {
        fn drop(&mut self) {
            assert!(self.0.upgrade().is_some());
            panic!("deliberate context cleanup panic")
        }
    }
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let mut handoff = ProcessHandoff::new(Box::new(Completes)).with_lease(lease);
    handoff
        .admit(ProcessContext::new(
            [("input".into(), Value::local(PanicInput(weak.clone())))].into(),
            vec![],
        ))
        .unwrap();
    let job = handoff.next_job().unwrap();
    let failure = handoff.complete(job.run()).unwrap().unwrap_err();
    assert_eq!(handoff.state(), HandoffState::Failed);
    assert_eq!(failure.kind(), FailureKind::Cleanup);
    assert_eq!(
        failure
            .source()
            .and_then(|source| source.downcast_ref::<ProcessCleanupError>()),
        Some(&ProcessCleanupError::OwnedState)
    );
    assert!(weak.upgrade().is_some());
    drop(failure);
    assert!(weak.upgrade().is_none());
}
