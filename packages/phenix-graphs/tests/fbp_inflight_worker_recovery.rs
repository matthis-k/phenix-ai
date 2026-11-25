//! Q5: an old delegated job can keep running after request cancellation.
//! Its worker-owned side effect cannot be undone, but its late completion
//! must never be accepted by a replacement with the same compiled index.
//! Five deterministic handoff orders exercise actual worker execution.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{
        CompletedStep, HandoffError, Process, ProcessContext, ProcessEmission, Step, StepTicket,
    },
    runtime::{
        ActivationMetadata, RequestFailurePolicy, RequestQueuePolicy, RequestStatus,
        SessionOptions, SessionState,
    },
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Barrier, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

struct PausedWorker {
    emitted: bool,
    entered: Arc<Barrier>,
    released: Arc<Barrier>,
    old_effects: Arc<AtomicUsize>,
    new_effects: Arc<AtomicUsize>,
}

impl Process for PausedWorker {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let value = cx
            .take("in")
            .ok_or_else(|| phenix_graph::Failure::application_message("missing request packet"))?
            .get::<u64>()?;
        if value % 2 == 1 {
            // Signal that the worker is *inside* Process::step, not simply
            // holding a job that the host has not dispatched yet.
            self.entered.wait();
            self.released.wait();
            self.old_effects.fetch_add(1, Ordering::SeqCst);
        } else {
            self.new_effects.fetch_add(1, Ordering::SeqCst);
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(value))))
    }
}

#[test]
fn in_flight_worker_effect_survives_cancel_but_its_completion_cannot_cross_recovery() {
    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(Barrier::new(2));
    let old_effects = Arc::new(AtomicUsize::new(0));
    let new_effects = Arc::new(AtomicUsize::new(0));

    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<u64>("in").unwrap()],
            [PortDef::wire::<u64>("out").unwrap()],
        )
        .unwrap(),
    );
    let node = builder
        .node("worker", interface, {
            let entered = entered.clone();
            let released = released.clone();
            let old_effects = old_effects.clone();
            let new_effects = new_effects.clone();
            Arc::new(move || PausedWorker {
                emitted: false,
                entered: entered.clone(),
                released: released.clone(),
                old_effects: old_effects.clone(),
                new_effects: new_effects.clone(),
            })
        })
        .unwrap();
    builder
        .expose_input(
            "input",
            node.wire_input::<u64>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("output", node.wire_output::<u64>("out").unwrap())
        .unwrap();
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
            capture_capacity: NonZeroUsize::new(1).unwrap(),
            ..SessionOptions::default()
        });

    // Repeat three concrete orderings 16 times each in the *same session*.
    // Reused process IDs must never mean reused generation or step tickets.
    for round in 0..48_usize {
        let old = session.begin_request(Some("output")).unwrap();
        let old_value = (round as u64) * 2 + 1;
        session
            .inject_request(old, "input", Value::of(old_value))
            .unwrap();
        session.close_request(old).unwrap();
        let (index, held) = session.next_job().unwrap();
        let old_ticket = held.ticket();
        let old_wake = session.wake_handle("worker").unwrap();
        let old_notifier = session.process_notifier("worker").unwrap();
        let notifier = session.notifier();
        let mut listener = session.wake_listener();
        let thread = thread::spawn(move || {
            let completion = held.run();
            assert!(notifier.notify());
            completion
        });
        entered.wait();
        assert_eq!(session.request_status(old).unwrap(), RequestStatus::Pending);

        // The session must be able to cancel a process whose worker holds
        // its state and is blocked inside an external side-effect boundary.
        assert_eq!(session.cancel_request(old).unwrap(), vec![old_ticket]);
        assert_eq!(
            session.request_status(old).unwrap(),
            RequestStatus::Cancelled
        );
        assert_eq!(session.request_disposal(old).unwrap().handed_jobs, 1);
        assert!(!old_wake.wake());
        assert!(!old_notifier.notify());

        let new = session.begin_request(Some("output")).unwrap();
        let new_value = old_value + 1;
        session
            .inject_request(new, "input", Value::of(new_value))
            .unwrap();
        session.close_request(new).unwrap();

        match round % 3 {
            0 => {
                // Replacement fully settles before the old worker returns.
                assert_eq!(session.run_until_idle(16).steps, 2);
                released.wait();
            }
            1 => {
                // Old worker returns before replacement's first step runs.
                released.wait();
            }
            _ => {
                // Replacement is already dispatched, then the old completion
                // returns before the fresh dispatched completion is admitted.
                let (new_index, fresh_job) = session.next_job().unwrap();
                assert_eq!(new_index, index);
                released.wait();
                let late = thread.join().unwrap();
                assert!(listener.wait_timeout(Duration::ZERO));
                let (error, returned) = session.complete_job(late).unwrap_err();
                assert_eq!(error, HandoffError::ForeignCompletion);
                drop(returned);
                session.complete_job(fresh_job.run()).unwrap();
                assert_eq!(session.run_until_idle(16).steps, 1);
                assert!(!old_wake.wake());
                assert!(!old_notifier.notify());
                let packet = session.take_request_output(new, "output").unwrap().unwrap();
                assert_eq!(packet.value().get::<u64>().unwrap(), new_value);
                assert_eq!(packet.metadata().unwrap().request(), Some(new));
                assert_eq!(
                    session.request_status(new).unwrap(),
                    RequestStatus::Settled { captures: 1 }
                );
                assert!(session.take_output("output").unwrap().is_none());
                assert_eq!(session.state(), SessionState::Quiescent);
                session.release_request(old).unwrap();
                session.release_request(new).unwrap();
                assert_eq!(old_effects.load(Ordering::SeqCst), round + 1);
                assert_eq!(new_effects.load(Ordering::SeqCst), round + 1);
                continue;
            }
        }

        let late = thread.join().unwrap();
        assert!(listener.wait_timeout(Duration::ZERO));
        let (error, returned) = session.complete_job(late).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        drop(returned);
        if round % 3 == 1 {
            assert_eq!(session.run_until_idle(16).steps, 2);
        }
        assert!(!old_wake.wake());
        assert!(!old_notifier.notify());
        let packet = session.take_request_output(new, "output").unwrap().unwrap();
        assert_eq!(packet.value().get::<u64>().unwrap(), new_value);
        assert_eq!(packet.metadata().unwrap().request(), Some(new));
        assert_eq!(
            session.request_status(new).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        assert!(session.take_output("output").unwrap().is_none());
        assert_eq!(session.state(), SessionState::Quiescent);
        session.release_request(old).unwrap();
        session.release_request(new).unwrap();
        // Cancellation does not undo real external effects. The late result
        // is discarded, but the side effect is performed exactly once.
        assert_eq!(old_effects.load(Ordering::SeqCst), round + 1);
        assert_eq!(new_effects.load(Ordering::SeqCst), round + 1);
    }
    eprintln!(
        "Q5 legacy worker frames: replacement-first=16 old-return-first=16 fresh-dispatched=16 total=48 rounds"
    );
}

struct CodeLease;
#[derive(Default)]
struct LeaseProbes {
    old_effects: AtomicUsize,
    new_effects: AtomicUsize,
    spawned: AtomicUsize,
    process_drops: Mutex<Vec<usize>>,
    context_drops: Mutex<Vec<usize>>,
}
struct LeasedInput {
    source: usize,
    value: u64,
    lease: Weak<CodeLease>,
    probes: Arc<LeaseProbes>,
}
impl Drop for LeasedInput {
    fn drop(&mut self) {
        assert!(self.lease.upgrade().is_some(), "input lost its code lease");
        self.probes.context_drops.lock().unwrap().push(self.source);
    }
}
struct LeasedPausedWorker {
    source: usize,
    emitted: bool,
    entered: Arc<Barrier>,
    released: Arc<Barrier>,
    lease: Weak<CodeLease>,
    probes: Arc<LeaseProbes>,
}
impl Drop for LeasedPausedWorker {
    fn drop(&mut self) {
        assert!(
            self.lease.upgrade().is_some(),
            "process lost its code lease"
        );
        self.probes.process_drops.lock().unwrap().push(self.source);
    }
}
impl Process for LeasedPausedWorker {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        assert!(self.lease.upgrade().is_some());
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        // Borrow rather than take: the real input must remain in the returned
        // completion's ProcessContext after the foreign refusal.
        let value = cx
            .input("in")
            .and_then(|value| value.borrow::<LeasedInput>())
            .expect("leased request packet")
            .value;
        if value % 2 == 1 {
            self.entered.wait();
            self.released.wait();
            self.probes.old_effects.fetch_add(1, Ordering::SeqCst);
        } else {
            self.probes.new_effects.fetch_add(1, Ordering::SeqCst);
        }
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(value))))
    }
}

fn emission_publication_recovery_order(publication_before_late_return: bool) {
    let entered = Arc::new(Barrier::new(2));
    let released = Arc::new(Barrier::new(2));
    let probes = Arc::new(LeaseProbes::default());
    let code_lease = Arc::new(CodeLease);
    let code_weak = Arc::downgrade(&code_lease);

    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::local::<LeasedInput>("in")],
            [PortDef::wire::<u64>("out").unwrap()],
        )
        .unwrap(),
    );
    let node = builder
        .node("worker", interface, {
            let entered = entered.clone();
            let released = released.clone();
            let probes = probes.clone();
            let code_weak = code_weak.clone();
            Arc::new(move || LeasedPausedWorker {
                source: probes.spawned.fetch_add(1, Ordering::SeqCst),
                emitted: false,
                entered: entered.clone(),
                released: released.clone(),
                lease: code_weak.clone(),
                probes: probes.clone(),
            })
        })
        .unwrap();
    builder
        .expose_input(
            "input",
            node.input::<LeasedInput>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("output", node.wire_output::<u64>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut session = compiled.start_with_options(SessionOptions {
        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
        capture_capacity: NonZeroUsize::new(1).unwrap(),
        lease: Some(code_lease),
        ..SessionOptions::default()
    });

    let old = session.begin_request(Some("output")).unwrap();
    session
        .inject_request(
            old,
            "input",
            Value::local(LeasedInput {
                source: 0,
                value: 41,
                lease: code_weak.clone(),
                probes: probes.clone(),
            }),
        )
        .unwrap();
    session.close_request(old).unwrap();
    let (index, old_job) = session.next_job().unwrap();
    let old_ticket = old_job.ticket();
    let old_notifier = session.process_notifier("worker").unwrap();
    let global_notifier = session.notifier();
    let mut listener = session.wake_listener();
    let worker = thread::spawn(move || {
        let completion = old_job.run();
        assert!(global_notifier.notify());
        completion
    });
    entered.wait();
    assert_eq!(session.cancel_request(old).unwrap(), vec![old_ticket]);
    assert!(!old_notifier.notify());

    let fresh_notifier = session.process_notifier("worker").unwrap();
    let new = session.begin_request(Some("output")).unwrap();
    session
        .inject_request(
            new,
            "input",
            Value::local(LeasedInput {
                source: 1,
                value: 42,
                lease: code_weak.clone(),
                probes: probes.clone(),
            }),
        )
        .unwrap();
    session.close_request(new).unwrap();

    // E: the old external effect and old emission have really happened, but the
    // CompletedStep has not yet been offered to the replacement session.
    released.wait();
    let late = worker.join().unwrap();
    assert!(listener.wait_timeout(Duration::ZERO));
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);

    let (new_index, fresh_emission) = session.next_job().unwrap();
    assert_eq!(new_index, index);
    session.complete_job(fresh_emission.run()).unwrap();
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 1);
    // The real emission is accepted and retained, but transport has not committed.
    assert!(
        session
            .take_request_output(new, "output")
            .unwrap()
            .is_none()
    );

    let finish_job;
    let old_returned;
    if publication_before_late_return {
        // E < P < R < F. next_job commits the retained transport transaction
        // before returning the fresh logical-completion job.
        let (_, job) = session.next_job().unwrap();
        finish_job = job;
        let (error, returned) = session.complete_job(late).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        old_returned = returned;
    } else {
        // E < R < P < F. A foreign old return cannot consume or publish the
        // fresh retained transaction before the fresh transport commit.
        let (error, returned) = session.complete_job(late).unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        old_returned = returned;
        let (_, job) = session.next_job().unwrap();
        finish_job = job;
    }
    assert!(!old_notifier.notify());
    assert!(fresh_notifier.notify());
    assert_eq!(old_returned.ticket(), old_ticket);
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 1);
    assert!(probes.process_drops.lock().unwrap().is_empty());
    assert!(probes.context_drops.lock().unwrap().is_empty());

    let packet = session
        .take_request_output(new, "output")
        .unwrap()
        .expect("fresh transport publication");
    assert_eq!(*packet.value().decoded::<u64>().unwrap(), 42);
    assert_eq!(packet.metadata().unwrap().request(), Some(new));
    // F: only the fresh logical completion settles the fresh publication.
    session.complete_job(finish_job.run()).unwrap();
    assert_eq!(
        session.request_status(new).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    let report = session.run_until_idle(8);
    assert_eq!((report.steps, report.publications), (0, 0));
    assert!(session.take_output("output").unwrap().is_none());
    assert_eq!(session.state(), SessionState::Quiescent);
    assert_eq!(*probes.context_drops.lock().unwrap(), [1]);
    assert!(probes.process_drops.lock().unwrap().is_empty());
    assert!(code_weak.upgrade().is_some());

    drop(packet);
    session.release_request(old).unwrap();
    session.release_request(new).unwrap();
    drop(session);
    drop(compiled);
    drop((old_notifier, fresh_notifier, listener));
    assert_eq!(*probes.process_drops.lock().unwrap(), [1]);
    assert_eq!(*probes.context_drops.lock().unwrap(), [1]);
    // The returned old completion still owns its process, context, and lease
    // after every session/replacement-owned owner has gone away.
    assert!(code_weak.upgrade().is_some());
    drop(old_returned);
    assert_eq!(*probes.process_drops.lock().unwrap(), [1, 0]);
    assert_eq!(*probes.context_drops.lock().unwrap(), [1, 0]);
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 1);
    assert!(code_weak.upgrade().is_none());
    eprintln!(
        "Q5 worker frame {}: rounds=1 old_effects=1 new_effects=1 fresh_publications=1 foreign_returns=1",
        if publication_before_late_return {
            "E<P<R<F"
        } else {
            "E<R<P<F"
        }
    );
}

#[test]
fn old_return_precedes_fresh_transport_publication_e_r_p_f() {
    emission_publication_recovery_order(false);
}

#[test]
fn old_return_follows_fresh_transport_publication_e_p_r_f() {
    emission_publication_recovery_order(true);
}

struct DirectEcho {
    emitted: bool,
    effect: Arc<AtomicUsize>,
}
impl Process for DirectEcho {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let value = cx
            .take("in")
            .ok_or_else(|| phenix_graph::Failure::application_message("missing echo packet"))?;
        self.effect.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", value)))
    }
}

#[test]
fn same_session_wrong_index_preserves_both_equal_numbered_real_jobs() {
    let effects = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
    let mut builder = NetworkBuilder::new();
    let interface = Arc::new(
        Interface::new(
            [PortDef::wire::<u64>("in").unwrap()],
            [PortDef::wire::<u64>("out").unwrap()],
        )
        .unwrap(),
    );
    for (position, name) in ["a", "b"].into_iter().enumerate() {
        let node = builder
            .node(name, interface.clone(), {
                let effect = effects[position].clone();
                Arc::new(move || DirectEcho {
                    effect: effect.clone(),
                    emitted: false,
                })
            })
            .unwrap();
        builder
            .expose_input(
                format!("input-{name}"),
                node.wire_input::<u64>("in").unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        builder
            .expose_output(
                format!("output-{name}"),
                node.wire_output::<u64>("out").unwrap(),
            )
            .unwrap();
    }
    let mut session = builder.build().unwrap().compile().unwrap().start();
    session.inject("input-a", Value::of(11_u64)).unwrap();
    session.inject("input-b", Value::of(29_u64)).unwrap();
    let (a_index, a_job) = session.next_job().unwrap();
    let (b_index, b_job) = session.next_job().unwrap();
    assert_ne!(a_index, b_index);
    let a_ticket = a_job.ticket();
    let b_ticket = b_job.ticket();
    assert_eq!(a_ticket.activation(), b_ticket.activation());
    assert_eq!(a_ticket.sequence(), b_ticket.sequence());
    assert_ne!(a_ticket, b_ticket);

    let a_completion = a_job.run();
    assert_eq!(effects[0].load(Ordering::SeqCst), 1);
    assert_eq!(effects[1].load(Ordering::SeqCst), 0);
    // B's numeric index is not authority for A's owner, despite equal visible
    // activation and sequence numbers.
    let (error, returned) = session.complete(b_index, a_completion).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(returned.ticket(), a_ticket);
    assert_eq!(effects[0].load(Ordering::SeqCst), 1);
    assert_eq!(effects[1].load(Ordering::SeqCst), 0);

    session.complete(a_index, *returned).unwrap();
    session.complete(b_index, b_job.run()).unwrap();
    let report = session.run_until_idle(8);
    assert_eq!((report.steps, report.publications), (2, 2));
    assert_eq!(effects[0].load(Ordering::SeqCst), 1);
    assert_eq!(effects[1].load(Ordering::SeqCst), 1);
    assert_eq!(
        session
            .take_output("output-a")
            .unwrap()
            .unwrap()
            .value()
            .get::<u64>()
            .unwrap(),
        11
    );
    assert_eq!(
        session
            .take_output("output-b")
            .unwrap()
            .unwrap()
            .value()
            .get::<u64>()
            .unwrap(),
        29
    );
    eprintln!(
        "Q5 same-session identity: wrong_index_foreign_returns=1 rightful_completions=2 equal_activation_sequence_pairs=1"
    );
}

struct WakeEcho {
    stage: u8,
    effects: Arc<AtomicUsize>,
}
impl Process for WakeEcho {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        match self.stage {
            0 => {
                self.stage = 1;
                Ok(Step::WaitWake)
            }
            1 => {
                let value = cx.take("in").ok_or_else(|| {
                    phenix_graph::Failure::application_message("missing wake echo packet")
                })?;
                self.effects.fetch_add(1, Ordering::SeqCst);
                self.stage = 2;
                Ok(Step::Emit(ProcessEmission::new("out", value)))
            }
            _ => Ok(Step::Complete),
        }
    }
}

#[test]
fn same_compiled_path_in_two_sessions_keeps_completion_wake_and_notifier_local() {
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
                move || WakeEcho {
                    stage: 0,
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
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut a = compiled.start();
    let mut b = compiled.start();
    a.inject("input", Value::of(41_u64)).unwrap();
    b.inject("input", Value::of(43_u64)).unwrap();

    let (_, a_wait) = a.next_job().unwrap();
    a.complete_job(a_wait.run()).unwrap();
    let (_, b_wait) = b.next_job().unwrap();
    b.complete_job(b_wait.run()).unwrap();
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    let mut b_listener = b.wake_listener();
    let a_wake = a.wake_handle("worker").unwrap();
    let a_notifier = a.process_notifier("worker").unwrap();
    // Same path and index in A gives no authority to wake or notify B.
    assert!(a_wake.wake());
    assert!(a_notifier.notify());
    let b_report = b.run_until_idle(8);
    assert_eq!((b_report.steps, b_report.publications), (0, 0));
    assert!(!b_listener.wait_timeout(Duration::ZERO));

    let (a_index, a_emit) = a.next_job().unwrap();
    assert!(b.wake_handle("worker").unwrap().wake());
    let (b_index, b_emit) = b.next_job().unwrap();
    assert_eq!(a_index, b_index);
    let a_ticket = a_emit.ticket();
    let b_ticket = b_emit.ticket();
    assert_eq!(a_ticket.activation(), b_ticket.activation());
    assert_eq!(a_ticket.sequence(), b_ticket.sequence());
    assert_ne!(a_ticket, b_ticket);
    let a_completion = a_emit.run();
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    let (error, returned) = b.complete_job(a_completion).unwrap_err();
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(returned.ticket(), a_ticket);
    assert_eq!(effects.load(Ordering::SeqCst), 1);

    a.complete_job(*returned).unwrap();
    b.complete_job(b_emit.run()).unwrap();
    assert_eq!(a.run_until_idle(8).publications, 1);
    assert_eq!(b.run_until_idle(8).publications, 1);
    let a_packet = a.take_output("output").unwrap().unwrap();
    let b_packet = b.take_output("output").unwrap().unwrap();
    assert_eq!(a_packet.value().get::<u64>().unwrap(), 41);
    assert_eq!(b_packet.value().get::<u64>().unwrap(), 43);
    assert_eq!(a_packet.metadata().unwrap().process(), Some(a_index));
    assert_eq!(b_packet.metadata().unwrap().process(), Some(b_index));
    assert_ne!(
        a_packet.metadata().unwrap().session(),
        b_packet.metadata().unwrap().session()
    );
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    eprintln!(
        "Q5 cross-session identity: wrong_session_foreign_returns=1 rightful_completions=2 isolated_wake_calls=1 isolated_notifier_calls=1"
    );
}

struct PreparedPayload {
    value: u64,
    drops: Arc<AtomicUsize>,
}
impl Drop for PreparedPayload {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
struct PreparedWorker {
    emitted: bool,
    old_effects: Arc<AtomicUsize>,
    new_effects: Arc<AtomicUsize>,
    old_drops: Arc<AtomicUsize>,
    new_drops: Arc<AtomicUsize>,
}
impl Process for PreparedWorker {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let value = cx
            .take("in")
            .ok_or_else(|| phenix_graph::Failure::application_message("missing prepared input"))?
            .get::<u64>()?;
        let (effects, drops) = if value % 2 == 1 {
            (&self.old_effects, &self.old_drops)
        } else {
            (&self.new_effects, &self.new_drops)
        };
        effects.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new(
            "out",
            Value::local(PreparedPayload {
                value,
                drops: drops.clone(),
            }),
        )))
    }
}

#[test]
fn cancelled_publication_pending_disposes_prepared_old_output_for_each_queue_policy() {
    for queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        let old_effects = Arc::new(AtomicUsize::new(0));
        let new_effects = Arc::new(AtomicUsize::new(0));
        let old_drops = Arc::new(AtomicUsize::new(0));
        let new_drops = Arc::new(AtomicUsize::new(0));
        let mut builder = NetworkBuilder::new();
        let worker = builder
            .node(
                "worker",
                Arc::new(
                    Interface::new(
                        [PortDef::wire::<u64>("in").unwrap()],
                        [PortDef::local::<PreparedPayload>("out")],
                    )
                    .unwrap(),
                ),
                Arc::new({
                    let old_effects = old_effects.clone();
                    let new_effects = new_effects.clone();
                    let old_drops = old_drops.clone();
                    let new_drops = new_drops.clone();
                    move || PreparedWorker {
                        emitted: false,
                        old_effects: old_effects.clone(),
                        new_effects: new_effects.clone(),
                        old_drops: old_drops.clone(),
                        new_drops: new_drops.clone(),
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
            .expose_output("output", worker.output::<PreparedPayload>("out").unwrap())
            .unwrap();
        let mut session = builder
            .build()
            .unwrap()
            .compile()
            .unwrap()
            .start_with_options(SessionOptions {
                request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
                request_queue_policy: queue_policy,
                capture_capacity: NonZeroUsize::new(1).unwrap(),
                ..SessionOptions::default()
            });

        let old = session.begin_request(Some("output")).unwrap();
        session
            .inject_request(old, "input", Value::of(51_u64))
            .unwrap();
        session.close_request(old).unwrap();
        let (index, old_job) = session.next_job().unwrap();
        session.complete_job(old_job.run()).unwrap();
        assert_eq!(old_effects.load(Ordering::SeqCst), 1);
        assert_eq!(old_drops.load(Ordering::SeqCst), 0);
        assert_eq!(session.request_work(old).unwrap().publication_pending, 1);
        assert!(
            session
                .take_request_output(old, "output")
                .unwrap()
                .is_none()
        );
        let old_notifier = session.process_notifier("worker").unwrap();

        // Cancellation owns and disposes the retained prepared value; no job is
        // outstanding in PublicationPending and no old publication can commit.
        assert!(session.cancel_request(old).unwrap().is_empty());
        let disposal = session.request_disposal(old).unwrap();
        assert_eq!(disposal.prepared_packets, 1);
        assert_eq!(disposal.handed_jobs, 0);
        assert!(!old_notifier.notify());
        assert_eq!(old_drops.load(Ordering::SeqCst), 1);
        assert_eq!(old_effects.load(Ordering::SeqCst), 1);

        let new = session.begin_request(Some("output")).unwrap();
        session
            .inject_request(new, "input", Value::of(52_u64))
            .unwrap();
        session.close_request(new).unwrap();
        let (new_index, new_emission) = session.next_job().unwrap();
        assert_eq!(new_index, index);
        session.complete_job(new_emission.run()).unwrap();
        let report = session.run_until_idle(8);
        assert_eq!((report.steps, report.publications), (1, 1));
        assert_eq!(new_effects.load(Ordering::SeqCst), 1);
        let packet = session
            .take_request_output(new, "output")
            .unwrap()
            .expect("only the fresh publication");
        assert_eq!(
            packet.value().borrow::<PreparedPayload>().unwrap().value,
            52
        );
        assert_eq!(packet.metadata().unwrap().request(), Some(new));
        assert_eq!(
            session.request_status(new).unwrap(),
            RequestStatus::Settled { captures: 1 }
        );
        assert!(session.take_output("output").unwrap().is_none());
        drop(packet);
        assert_eq!(new_drops.load(Ordering::SeqCst), 1);
        assert_eq!(old_effects.load(Ordering::SeqCst), 1);
        assert_eq!(new_effects.load(Ordering::SeqCst), 1);
        session.release_request(old).unwrap();
        session.release_request(new).unwrap();
        eprintln!(
            "Q5 publication-pending recovery {queue_policy:?}: old_prepared_disposed=1 old_publications=0 old_effects=1 fresh_publications=1 fresh_effects=1 handed_jobs=0"
        );
    }
}

struct LateCompleteWorker {
    source: usize,
    emitted: bool,
    lease: Weak<CodeLease>,
    probes: Arc<LeaseProbes>,
}
impl Drop for LateCompleteWorker {
    fn drop(&mut self) {
        assert!(
            self.lease.upgrade().is_some(),
            "late Complete dropped its process code lease"
        );
        self.probes.process_drops.lock().unwrap().push(self.source);
    }
}
impl Process for LateCompleteWorker {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, phenix_graph::Failure> {
        assert!(self.lease.upgrade().is_some());
        // The borrowed input stays in this job's ProcessContext, including for
        // the old non-emitting Complete result retained across every refusal.
        let value = cx
            .input("in")
            .and_then(|value| value.borrow::<LeasedInput>())
            .expect("late complete request input")
            .value;
        if self.source == 0 {
            self.probes.old_effects.fetch_add(1, Ordering::SeqCst);
            return Ok(Step::Complete);
        }
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        self.probes.new_effects.fetch_add(1, Ordering::SeqCst);
        self.emitted = true;
        Ok(Step::Emit(ProcessEmission::new("out", Value::of(value))))
    }
}

#[allow(clippy::too_many_arguments)]
fn foreign_complete_at_checkpoint(
    session: &mut phenix_graph::NetworkSession,
    index: usize,
    by_index: bool,
    completion: CompletedStep,
    old_ticket: StepTicket,
    old_metadata: &ActivationMetadata,
    probes: &LeaseProbes,
    code_weak: &Weak<CodeLease>,
) -> Box<CompletedStep> {
    let (error, returned) = if by_index {
        session.complete(index, completion).unwrap_err()
    } else {
        session.complete_job(completion).unwrap_err()
    };
    assert_eq!(error, HandoffError::ForeignCompletion);
    assert_eq!(returned.ticket(), old_ticket);
    let metadata = returned.metadata().expect("old completion metadata");
    assert_eq!(metadata.session, old_metadata.session);
    assert_eq!(metadata.generation, old_metadata.generation);
    assert_eq!(metadata.request, old_metadata.request);
    assert_eq!(metadata.process, old_metadata.process);
    assert_eq!(metadata.path, old_metadata.path);
    assert_eq!(metadata.activation, old_metadata.activation);
    assert_eq!(metadata.step, old_metadata.step);
    assert_eq!(metadata.output, old_metadata.output);
    assert_eq!(metadata.origin, old_metadata.origin);
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);
    assert!(probes.process_drops.lock().unwrap().is_empty());
    assert!(
        probes
            .context_drops
            .lock()
            .unwrap()
            .iter()
            .all(|source| *source != 0),
        "foreign return released the old Complete context"
    );
    assert!(code_weak.upgrade().is_some());
    returned
}

fn late_complete_checkpoint_history(queue_policy: RequestQueuePolicy) {
    let probes = Arc::new(LeaseProbes::default());
    let code_lease = Arc::new(CodeLease);
    let code_weak = Arc::downgrade(&code_lease);
    let mut builder = NetworkBuilder::new();
    let worker = builder
        .node(
            "worker",
            Arc::new(
                Interface::new(
                    [PortDef::local::<LeasedInput>("in")],
                    [PortDef::wire::<u64>("out").unwrap()],
                )
                .unwrap(),
            ),
            Arc::new({
                let probes = probes.clone();
                let code_weak = code_weak.clone();
                move || LateCompleteWorker {
                    source: probes.spawned.fetch_add(1, Ordering::SeqCst),
                    emitted: false,
                    lease: code_weak.clone(),
                    probes: probes.clone(),
                }
            }),
        )
        .unwrap();
    builder
        .expose_input(
            "input",
            worker.input::<LeasedInput>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    builder
        .expose_output("output", worker.wire_output::<u64>("out").unwrap())
        .unwrap();
    let compiled = builder.build().unwrap().compile().unwrap();
    let mut session = compiled.start_with_options(SessionOptions {
        request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
        request_queue_policy: queue_policy,
        capture_capacity: NonZeroUsize::new(1).unwrap(),
        lease: Some(code_lease),
        ..SessionOptions::default()
    });

    let old = session.begin_request(Some("output")).unwrap();
    session
        .inject_request(
            old,
            "input",
            Value::local(LeasedInput {
                source: 0,
                value: 61,
                lease: code_weak.clone(),
                probes: probes.clone(),
            }),
        )
        .unwrap();
    session.close_request(old).unwrap();
    let (index, old_job) = session.next_job().unwrap();
    let old_ticket = old_job.ticket();
    let old_metadata = old_job.metadata().unwrap().clone();
    let mut late = Box::new(old_job.run());
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 0);
    let old_notifier = session.process_notifier("worker").unwrap();
    // The non-emitting old Complete really ran, but remains runner-owned.
    assert_eq!(session.cancel_request(old).unwrap(), vec![old_ticket]);
    assert_eq!(session.request_disposal(old).unwrap().handed_jobs, 1);
    assert_eq!(session.request_disposal(old).unwrap().prepared_packets, 0);
    assert!(!old_notifier.notify());

    let mut checkpoints = 0;
    // 1/5: before any fresh admission.
    late = foreign_complete_at_checkpoint(
        &mut session,
        index,
        true,
        *late,
        old_ticket,
        &old_metadata,
        &probes,
        &code_weak,
    );
    checkpoints += 1;

    let new = session.begin_request(Some("output")).unwrap();
    session
        .inject_request(
            new,
            "input",
            Value::local(LeasedInput {
                source: 1,
                value: 62,
                lease: code_weak.clone(),
                probes: probes.clone(),
            }),
        )
        .unwrap();
    session.close_request(new).unwrap();
    let (new_index, fresh_emission) = session.next_job().unwrap();
    assert_eq!(new_index, index);
    let fresh_ticket = fresh_emission.ticket();
    // 2/5: the fresh job is dispatched but has not run.
    late = foreign_complete_at_checkpoint(
        &mut session,
        index,
        false,
        *late,
        old_ticket,
        &old_metadata,
        &probes,
        &code_weak,
    );
    checkpoints += 1;
    assert_eq!(fresh_emission.ticket(), fresh_ticket);
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 0);

    session.complete_job(fresh_emission.run()).unwrap();
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 1);
    assert!(
        session
            .take_request_output(new, "output")
            .unwrap()
            .is_none()
    );
    // 3/5: fresh Emit accepted, transport retained but not yet committed.
    late = foreign_complete_at_checkpoint(
        &mut session,
        index,
        true,
        *late,
        old_ticket,
        &old_metadata,
        &probes,
        &code_weak,
    );
    checkpoints += 1;
    assert!(
        session
            .take_request_output(new, "output")
            .unwrap()
            .is_none()
    );

    let (finish_index, fresh_finish) = session.next_job().unwrap();
    assert_eq!(finish_index, index);
    assert_eq!(
        fresh_finish.ticket().activation(),
        fresh_ticket.activation()
    );
    assert!(fresh_finish.ticket().sequence() > fresh_ticket.sequence());
    let packet = session
        .take_request_output(new, "output")
        .unwrap()
        .expect("fresh publication before logical completion");
    assert_eq!(*packet.value().decoded::<u64>().unwrap(), 62);
    assert_eq!(packet.metadata().unwrap().request(), Some(new));
    // 4/5: fresh transport publication exists, but its activation is not settled.
    late = foreign_complete_at_checkpoint(
        &mut session,
        index,
        false,
        *late,
        old_ticket,
        &old_metadata,
        &probes,
        &code_weak,
    );
    checkpoints += 1;
    assert_eq!(session.request_status(new).unwrap(), RequestStatus::Pending);

    session.complete_job(fresh_finish.run()).unwrap();
    assert_eq!(
        session.request_status(new).unwrap(),
        RequestStatus::Settled { captures: 1 }
    );
    // 5/5: fresh publication and logical completion have both settled.
    late = foreign_complete_at_checkpoint(
        &mut session,
        index,
        true,
        *late,
        old_ticket,
        &old_metadata,
        &probes,
        &code_weak,
    );
    checkpoints += 1;
    assert_eq!(checkpoints, 5);
    assert_eq!(*probes.context_drops.lock().unwrap(), [1]);
    assert!(probes.process_drops.lock().unwrap().is_empty());
    let report = session.run_until_idle(8);
    assert_eq!((report.steps, report.publications), (0, 0));
    assert!(session.take_output("output").unwrap().is_none());
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 1);

    drop(packet);
    session.release_request(old).unwrap();
    session.release_request(new).unwrap();
    drop(session);
    drop(compiled);
    drop(old_notifier);
    assert_eq!(*probes.process_drops.lock().unwrap(), [1]);
    assert_eq!(*probes.context_drops.lock().unwrap(), [1]);
    assert!(code_weak.upgrade().is_some());
    drop(late);
    assert_eq!(*probes.process_drops.lock().unwrap(), [1, 0]);
    assert_eq!(*probes.context_drops.lock().unwrap(), [1, 0]);
    assert_eq!(probes.old_effects.load(Ordering::SeqCst), 1);
    assert_eq!(probes.new_effects.load(Ordering::SeqCst), 1);
    assert!(code_weak.upgrade().is_none());
    eprintln!(
        "Q5 late Complete history {queue_policy:?}: checkpoints=5 complete_calls=3 complete_job_calls=2 foreign_returns=5 old_effects=1 fresh_publications=1 fresh_effects=1"
    );
}

#[test]
fn one_non_emitting_old_completion_stays_foreign_through_all_five_fresh_boundaries() {
    for queue_policy in [
        RequestQueuePolicy::SharedFifo,
        RequestQueuePolicy::BoundedLanes,
    ] {
        late_complete_checkpoint_history(queue_policy);
    }
}
