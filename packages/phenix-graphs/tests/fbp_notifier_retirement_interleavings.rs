//! Q5 adversarial interleavings at request recovery / process-index reuse.
//!
//! Each round uses the actual compiled FBP network, owned WaitWake handoff,
//! concurrent public notification handles and RestartAffectedProcess. The
//! three schedules cover pre-retirement, post-retirement and racing calls;
//! this is bounded threaded evidence, not exhaustive model checking.
use phenix_graph::{
    Value,
    interface::{Interface, PortDef},
    network::NetworkBuilder,
    process::{Process, ProcessContext, Step},
    runtime::{RequestFailurePolicy, RequestStatus, SessionOptions, SessionState},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

struct WaitThenComplete {
    waiting: bool,
    completed: Arc<AtomicUsize>,
}
impl Process for WaitThenComplete {
    fn step(&mut self, cx: &mut ProcessContext) -> Result<Step, String> {
        if !self.waiting {
            cx.take("in")
                .ok_or("missing input for wait")?
                .get::<u64>()?;
            self.waiting = true;
            Ok(Step::WaitWake)
        } else {
            self.waiting = false;
            self.completed.fetch_add(1, Ordering::SeqCst);
            Ok(Step::Complete)
        }
    }
}

#[test]
fn concurrent_old_wakes_never_resume_replacement_after_request_restart() {
    let completed = Arc::new(AtomicUsize::new(0));
    let mut builder = NetworkBuilder::new();
    let worker = builder
        .node(
            "worker",
            Arc::new(Interface::new([PortDef::wire::<u64>("in").unwrap()], []).unwrap()),
            {
                let completed = completed.clone();
                Arc::new(move || WaitThenComplete {
                    waiting: false,
                    completed: completed.clone(),
                })
            },
        )
        .unwrap();
    builder
        .expose_input(
            "input",
            worker.wire_input::<u64>("in").unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    let mut session = builder
        .build()
        .unwrap()
        .compile()
        .unwrap()
        .start_with_options(SessionOptions {
            request_failure_policy: RequestFailurePolicy::RestartAffectedProcess,
            ..SessionOptions::default()
        });

    // Three controlled schedules repeated with fresh request and wake-slot
    // identities but the same compiled process index and persistent session.
    for round in 0..96 {
        let cancelled = session.begin_request(None).unwrap();
        session
            .inject_request(cancelled, "input", Value::of(round as u64))
            .unwrap();
        session.close_request(cancelled).unwrap();
        assert_eq!(session.run_until_idle(16).steps, 1);
        assert_eq!(
            session.request_status(cancelled).unwrap(),
            RequestStatus::Pending
        );

        let old_notifier = session.process_notifier("worker").unwrap();
        let old_wake = session.wake_handle("worker").unwrap();
        match round % 3 {
            0 => {
                // Both operations linearize before retirement.
                let result = thread::spawn({
                    let notifier = old_notifier.clone();
                    let wake = old_wake.clone();
                    move || (notifier.notify(), wake.wake())
                })
                .join()
                .unwrap();
                assert_eq!(result, (true, true));
                session.cancel_request(cancelled).unwrap();
            }
            1 => {
                // Both operations linearize after retirement.
                session.cancel_request(cancelled).unwrap();
                let result = thread::spawn({
                    let notifier = old_notifier.clone();
                    let wake = old_wake.clone();
                    move || (notifier.notify(), wake.wake())
                })
                .join()
                .unwrap();
                assert_eq!(result, (false, false));
            }
            _ => {
                // Race both registrations against recovery's retirement and
                // replacement, without imposing a winner on scheduler order.
                let barrier = Arc::new(Barrier::new(2));
                let sender = thread::spawn({
                    let notifier = old_notifier.clone();
                    let wake = old_wake.clone();
                    let barrier = barrier.clone();
                    move || {
                        barrier.wait();
                        (notifier.notify(), wake.wake())
                    }
                });
                barrier.wait();
                session.cancel_request(cancelled).unwrap();
                let _ = sender.join().unwrap();
            }
        }

        assert_eq!(
            session.request_status(cancelled).unwrap(),
            RequestStatus::Cancelled
        );
        assert!(!old_notifier.notify(), "round {round}: stale notifier");
        assert!(!old_wake.wake(), "round {round}: stale wake");
        assert!(session.failure().is_none());

        // A fresh request reuses the same process index with its own WakeSlot.
        // Previous wake() may have returned true, but must not resume the
        // replacement's WaitWake continuation without a fresh wake().
        let fresh_notifier = session.process_notifier("worker").unwrap();
        let fresh_wake = session.wake_handle("worker").unwrap();
        let survivor = session.begin_request(None).unwrap();
        session
            .inject_request(survivor, "input", Value::of((round + 10_000) as u64))
            .unwrap();
        session.close_request(survivor).unwrap();
        assert_eq!(session.run_until_idle(16).steps, 1);
        assert_eq!(
            session.request_status(survivor).unwrap(),
            RequestStatus::Pending
        );
        assert!(fresh_notifier.notify());
        assert_eq!(session.run_until_idle(16).steps, 0);
        assert_eq!(completed.load(Ordering::SeqCst), round);
        assert!(fresh_wake.wake());
        assert_eq!(session.run_until_idle(16).steps, 1);
        assert_eq!(completed.load(Ordering::SeqCst), round + 1);
        assert_eq!(
            session.request_status(survivor).unwrap(),
            RequestStatus::Settled { captures: 0 }
        );
        assert_eq!(session.state(), SessionState::Quiescent);
        session.release_request(cancelled).unwrap();
        session.release_request(survivor).unwrap();
    }
}
