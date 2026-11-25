//! Q5: an old delegated job can keep running after request cancellation.
//! Its worker-owned side effect cannot be undone, but its late completion
//! must never be accepted by a replacement with the same compiled index.
//! Three deterministic handoff orders exercise actual worker execution.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{HandoffError, Process, ProcessContext, ProcessEmission, Step},
    runtime::{RequestFailurePolicy, RequestStatus, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Barrier,
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
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if self.emitted {
            self.emitted = false;
            return Ok(Step::Complete);
        }
        let value = cx
            .take("in")
            .ok_or("missing request packet")?
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
}
